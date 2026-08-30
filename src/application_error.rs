//! Transport-independent application failure categories.

use std::fmt;

/// Stable application failure categories independent of wording or transport.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ApplicationErrorKind {
    /// The decoded command or selected structural view is invalid.
    InvalidInput,
    /// The explicitly selected resource does not exist.
    NotFound,
    /// Current state conflicts with the requested mutation.
    Conflict,
    /// A bounded application input exceeds its supported size.
    InputTooLarge,
    /// Another single-process management action owns the mutation boundary.
    Busy,
    /// The Service failed independently of the submitted command.
    Internal,
}

/// One application error carrying an explicit category and context message.
#[derive(Debug)]
pub(crate) struct ApplicationError {
    kind: ApplicationErrorKind,
    message: String,
}

impl ApplicationError {
    /// Construct an explicitly classified application failure.
    pub(crate) fn new(kind: ApplicationErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// Return the explicit category found in an anyhow error chain.
    pub(crate) fn kind(error: &anyhow::Error) -> Option<ApplicationErrorKind> {
        error.downcast_ref::<Self>().map(|error| error.kind)
    }
}

impl fmt::Display for ApplicationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ApplicationError {}

/// Create a classified anyhow error without coupling domains to a transport.
pub(crate) fn application_error(
    kind: ApplicationErrorKind,
    message: impl Into<String>,
) -> anyhow::Error {
    ApplicationError::new(kind, message).into()
}

#[cfg(test)]
#[path = "application_error_tests.rs"]
mod tests;
