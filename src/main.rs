//! `aibox` binary entry point — a thin shell over [`aibox`] the library.
//!
//! It first handles callbacks from generated completion scripts. Ordinary
//! invocations are then split at the first `--` (so agent pass-through args
//! never reach clap; see [`aibox::cli::split_passthrough`]), parsed on the left,
//! and handed to [`aibox::run_os`]. All real logic lives in the library so it
//! can be unit-tested without spawning a process.

use aibox::cli::{split_passthrough, Cli};
use std::process::ExitCode;

fn main() -> ExitCode {
    aibox::handle_completion();

    let (left, passthrough) = split_passthrough(std::env::args_os().collect());

    let cli = Cli::parse_from(left);

    match aibox::run_os(cli, passthrough) {
        Ok(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
        Err(e) => {
            eprintln!("!! {e:#}");
            ExitCode::from(1)
        }
    }
}
