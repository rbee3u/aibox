//! Foreground Service composition and its Console-internal HTTP boundary.

mod control;
mod http;
mod runtime;
mod state;
#[cfg(test)]
mod testutil;

use http::router;
pub(crate) use runtime::{ConsoleCommand, dispatch};

fn control_router() -> axum::Router<state::ServiceState> {
    control::router()
}
