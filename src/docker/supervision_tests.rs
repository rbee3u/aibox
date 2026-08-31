use super::*;
use std::cell::RefCell;
use std::collections::VecDeque;

#[derive(Debug, PartialEq, Eq)]
enum CleanupEvent {
    Wait(Duration),
    Stop { signal: i32, cid: String },
    SignalChild(i32),
}

#[derive(Debug, PartialEq, Eq)]
enum ContainerStopEvent {
    Signal { name: String, cid: String },
    Inspect(String),
    CheckGrace,
    Wait(Duration),
    Kill(String),
}

fn active_run_cleanup_events(
    cids: impl IntoIterator<Item = Option<&'static str>>,
) -> Vec<CleanupEvent> {
    let cids = RefCell::new(cids.into_iter().collect::<VecDeque<_>>());
    let events = RefCell::new(Vec::new());
    let signal = signal_hook::consts::SIGTERM;

    stop_active_run_with(
        signal,
        |timeout| {
            events.borrow_mut().push(CleanupEvent::Wait(timeout));
            cids.borrow_mut()
                .pop_front()
                .expect("script one cid result per wait")
                .map(str::to_owned)
        },
        |signal, cid| {
            events.borrow_mut().push(CleanupEvent::Stop {
                signal,
                cid: cid.to_owned(),
            });
        },
        |signal| {
            events.borrow_mut().push(CleanupEvent::SignalChild(signal));
        },
    );

    events.into_inner()
}

fn container_stop_events(
    signal: i32,
    states: impl IntoIterator<Item = ContainerState>,
    grace_decisions: impl IntoIterator<Item = bool>,
) -> Vec<ContainerStopEvent> {
    let states = RefCell::new(states.into_iter().collect::<VecDeque<_>>());
    let grace_decisions = RefCell::new(grace_decisions.into_iter().collect::<VecDeque<_>>());
    let events = RefCell::new(Vec::new());

    stop_container_id_with(
        signal,
        "test-container",
        |name, cid| {
            events.borrow_mut().push(ContainerStopEvent::Signal {
                name: name.to_owned(),
                cid: cid.to_owned(),
            });
        },
        |cid| {
            events
                .borrow_mut()
                .push(ContainerStopEvent::Inspect(cid.to_owned()));
            states
                .borrow_mut()
                .pop_front()
                .expect("script one state per inspection")
        },
        || {
            events.borrow_mut().push(ContainerStopEvent::CheckGrace);
            grace_decisions
                .borrow_mut()
                .pop_front()
                .expect("script one decision per grace check")
        },
        |duration| {
            events.borrow_mut().push(ContainerStopEvent::Wait(duration));
        },
        |cid| {
            events
                .borrow_mut()
                .push(ContainerStopEvent::Kill(cid.to_owned()));
        },
    );

    events.into_inner()
}

#[test]
fn active_run_cleanup_orders_early_late_and_missing_cid_paths() {
    let signal = signal_hook::consts::SIGTERM;
    assert_eq!(
        active_run_cleanup_events([Some("early-container")]),
        vec![
            CleanupEvent::Wait(CIDFILE_WAIT),
            CleanupEvent::Stop {
                signal,
                cid: "early-container".into(),
            },
            CleanupEvent::SignalChild(signal),
        ]
    );
    assert_eq!(
        active_run_cleanup_events([None, Some("late-container")]),
        vec![
            CleanupEvent::Wait(CIDFILE_WAIT),
            CleanupEvent::SignalChild(signal),
            CleanupEvent::Wait(LATE_CIDFILE_WAIT),
            CleanupEvent::Stop {
                signal,
                cid: "late-container".into(),
            },
        ]
    );
    assert_eq!(
        active_run_cleanup_events([None, None]),
        vec![
            CleanupEvent::Wait(CIDFILE_WAIT),
            CleanupEvent::SignalChild(signal),
            CleanupEvent::Wait(LATE_CIDFILE_WAIT),
        ]
    );
}

#[test]
fn lingering_container_cleanup_kills_only_running_or_unknown_state() {
    let killed = RefCell::new(Vec::new());
    assert!(!stop_lingering_container_with(
        "stopped-container",
        ContainerState::Stopped,
        |cid| killed.borrow_mut().push(cid.to_owned()),
    ));
    assert!(killed.borrow().is_empty());

    for (cid, state) in [
        ("running-container", ContainerState::Running),
        ("unknown-container", ContainerState::Unknown),
    ] {
        assert!(stop_lingering_container_with(cid, state, |cid| {
            killed.borrow_mut().push(cid.to_owned());
        }));
    }
    assert_eq!(
        killed.into_inner(),
        ["running-container", "unknown-container"]
    );
}

#[test]
fn container_stop_maps_signals_and_skips_grace_after_a_clean_exit() {
    for (signal, name) in [
        (signal_hook::consts::SIGINT, "INT"),
        (signal_hook::consts::SIGHUP, "HUP"),
        (signal_hook::consts::SIGTERM, "TERM"),
    ] {
        assert_eq!(
            container_stop_events(signal, [ContainerState::Stopped], []),
            vec![
                ContainerStopEvent::Signal {
                    name: name.into(),
                    cid: "test-container".into(),
                },
                ContainerStopEvent::Inspect("test-container".into()),
            ]
        );
    }
}

#[test]
fn container_stop_observes_graceful_exit_without_sigkill() {
    assert_eq!(
        container_stop_events(
            signal_hook::consts::SIGTERM,
            [ContainerState::Running, ContainerState::Stopped],
            [true],
        ),
        vec![
            ContainerStopEvent::Signal {
                name: "TERM".into(),
                cid: "test-container".into(),
            },
            ContainerStopEvent::Inspect("test-container".into()),
            ContainerStopEvent::CheckGrace,
            ContainerStopEvent::Wait(CONTAINER_POLL_INTERVAL),
            ContainerStopEvent::Inspect("test-container".into()),
        ]
    );
}

#[test]
fn container_stop_escalates_immediately_after_a_second_signal() {
    for state in [ContainerState::Running, ContainerState::Unknown] {
        assert_eq!(
            container_stop_events(signal_hook::consts::SIGINT, [state], [false]),
            vec![
                ContainerStopEvent::Signal {
                    name: "INT".into(),
                    cid: "test-container".into(),
                },
                ContainerStopEvent::Inspect("test-container".into()),
                ContainerStopEvent::CheckGrace,
                ContainerStopEvent::Kill("test-container".into()),
            ]
        );
    }
}

#[test]
fn container_grace_ends_on_a_second_signal_or_the_deadline() {
    assert!(continue_container_grace(
        1,
        CONTAINER_GRACE - CONTAINER_POLL_INTERVAL,
    ));
    assert!(!continue_container_grace(2, Duration::ZERO));
    assert!(!continue_container_grace(1, CONTAINER_GRACE));
}
