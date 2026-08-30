//! Docker child process execution and output capture.

use super::LogCallback;
use super::supervision::RunRegistration;
use anyhow::{Context, Result};
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Child, ExitStatus};
use std::time::{Duration, Instant};

const CONTAINER_CREATE_WAIT: Duration = Duration::from_secs(1);
const CONTAINER_CREATE_POLL_INTERVAL: Duration = Duration::from_millis(20);

pub(super) struct RegisteredRun {
    child: Child,
    registration: RunRegistration,
    pub(super) finished: bool,
    output_threads: Vec<std::thread::JoinHandle<()>>,
}

impl RegisteredRun {
    #[cfg(test)]
    pub(super) fn new(child: Child) -> Self {
        Self::with_registration(child, RunRegistration::detached())
    }

    pub(super) fn with_registration(child: Child, registration: RunRegistration) -> Self {
        registration.attach(child.id());
        Self {
            child,
            registration,
            finished: false,
            output_threads: Vec::new(),
        }
    }

    pub(super) fn capture_output(&mut self, log: LogCallback) -> Result<()> {
        let stdout = self
            .child
            .stdout
            .take()
            .context("capture docker run stdout")?;
        let stderr = self
            .child
            .stderr
            .take()
            .context("capture docker run stderr")?;
        let stdout_log = log.clone();
        self.output_threads.push(std::thread::spawn(move || {
            forward_lines(stdout, stdout_log)
        }));
        self.output_threads
            .push(std::thread::spawn(move || forward_lines(stderr, log)));
        Ok(())
    }

    pub(super) fn child_mut(&mut self) -> &mut Child {
        &mut self.child
    }

    pub(super) fn finish(&mut self) -> bool {
        let stopped_lingering_container = self.registration.finish();
        self.finish_output();
        self.finished = true;
        stopped_lingering_container
    }

    fn finish_output(&mut self) {
        for thread in self.output_threads.drain(..) {
            let _ = thread.join();
        }
    }

    pub(super) fn finish_after_wait(
        &mut self,
        waited: Result<ExitStatus>,
    ) -> Result<(ExitStatus, bool)> {
        let status = waited.context("wait for docker run")?;
        Ok((status, self.finish()))
    }
}

impl Drop for RegisteredRun {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = self.registration.finish();
        self.finish_output();
    }
}

pub(super) fn forward_lines(reader: impl Read, log: LogCallback) {
    let mut reader = BufReader::new(reader);
    let mut bytes = Vec::new();
    loop {
        bytes.clear();
        match reader.read_until(b'\n', &mut bytes) {
            Ok(0) | Err(_) => break,
            Ok(_) => log(String::from_utf8_lossy(&bytes).trim_end().to_string()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ContainerCreate {
    Created,
    ChildExited(ExitStatus),
    TimedOut,
}

pub(super) fn wait_for_container_create(
    child: &mut Child,
    cid_path: &Path,
) -> Result<ContainerCreate> {
    let started = Instant::now();
    loop {
        if super::supervision::cidfile_has_id(cid_path) {
            return Ok(ContainerCreate::Created);
        }
        if let Some(status) = child
            .try_wait()
            .context("poll docker run before container create")?
        {
            return Ok(ContainerCreate::ChildExited(status));
        }
        if started.elapsed() >= CONTAINER_CREATE_WAIT {
            return Ok(ContainerCreate::TimedOut);
        }
        std::thread::sleep(CONTAINER_CREATE_POLL_INTERVAL);
    }
}

pub(super) fn wait_with_delayed_container_create<F: FnOnce()>(
    child: &mut Child,
    cid_path: &Path,
    after_container_created: &mut Option<F>,
) -> Result<ExitStatus> {
    loop {
        if super::supervision::cidfile_has_id(cid_path) {
            if let Some(callback) = after_container_created.take() {
                callback();
            }
            return child.wait().context("wait for docker run");
        }
        if let Some(status) = child
            .try_wait()
            .context("poll docker run after delayed container create")?
        {
            return Ok(status);
        }
        std::thread::sleep(CONTAINER_CREATE_POLL_INTERVAL);
    }
}

/// Map a child status to the shell's conventional exit code.
pub(super) fn exit_code(status: ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = status.signal() {
            return 128 + sig;
        }
    }
    1
}
