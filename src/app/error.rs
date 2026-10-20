use std::fmt;

use crate::agent::ask::AskError;

#[derive(Debug)]
pub enum Error {
    NotFound(String),
    Invalid(String),
    Unprocessable(String),
    Unavailable(String),
    Internal(anyhow::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotFound(m)
            | Error::Invalid(m)
            | Error::Unprocessable(m)
            | Error::Unavailable(m) => f.write_str(m),
            Error::Internal(e) => fmt::Display::fmt(e, f),
        }
    }
}

impl From<anyhow::Error> for Error {
    fn from(e: anyhow::Error) -> Self {
        Error::Internal(e)
    }
}

impl From<Error> for anyhow::Error {
    fn from(e: Error) -> Self {
        match e {
            Error::Internal(e) => e,
            e => anyhow::Error::msg(e.to_string()),
        }
    }
}

impl From<AskError> for Error {
    fn from(e: AskError) -> Self {
        match e {
            AskError::Invalid(m) => Error::Invalid(m.into()),
            AskError::Internal(e) => Error::Internal(e),
        }
    }
}
