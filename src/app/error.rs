use std::fmt;

use crate::agent::ask::AskError;
use crate::ingest::import::ImportError;

#[derive(Debug)]
pub enum Error {
    NotFound(String),
    Invalid(String),
    Unprocessable(String),
    Unavailable(String),
    /// A metadata source the user asked for directly could not be reached.
    LookupFailed(anyhow::Error),
    Import(ImportError),
    Internal(anyhow::Error),
}

impl Error {
    pub(crate) fn not_found() -> Self {
        Error::NotFound("not found".into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotFound(m)
            | Error::Invalid(m)
            | Error::Unprocessable(m)
            | Error::Unavailable(m) => f.write_str(m),
            Error::LookupFailed(_) => f.write_str("lookup failed"),
            Error::Import(e) => fmt::Display::fmt(e, f),
            Error::Internal(e) => fmt::Display::fmt(e, f),
        }
    }
}

impl From<anyhow::Error> for Error {
    fn from(e: anyhow::Error) -> Self {
        Error::Internal(e)
    }
}

impl From<ImportError> for Error {
    fn from(e: ImportError) -> Self {
        Error::Import(e)
    }
}

impl From<Error> for anyhow::Error {
    fn from(e: Error) -> Self {
        match e {
            Error::Internal(e) => e,
            Error::LookupFailed(e) => e.context("lookup failed"),
            Error::Import(e) => e.into(),
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
