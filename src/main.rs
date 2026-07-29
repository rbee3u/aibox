//! `aibox` binary entry point — a thin shell over [`aibox`] the library.
//!
//! Its only jobs: split argv at the first `--` (so agent pass-through args never
//! reach clap; see [`aibox::cli::split_passthrough`]), let clap parse the left
//! half, and hand off to [`aibox::run_os`]. All real logic lives in the library so
//! it can be unit-tested without spawning a process.

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
