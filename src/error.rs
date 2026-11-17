//! What can go wrong, each with what it was doing.

use std::io;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A request that cannot be done as asked: the client's to fix.
    #[error("{0}")]
    InvalidInput(String),
    /// What serve answered when it would not do a request.
    #[error("serve refused: {0}")]
    Refused(String),
    #[error("serve is not running: nobody answers at {}: {source}", .path.display())]
    NotServing {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    /// serve took the request and ended before it answered: what was asked may be done or not,
    /// and `get` from the next serve shows which.
    #[error("serve hung up without answering")]
    HungUp,
    #[error("another serve already holds {}", .0.display())]
    AlreadyServing(PathBuf),
    #[error("{what} {}: {source}", .path.display())]
    File {
        what: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{what}: {source}")]
    Io {
        what: &'static str,
        #[source]
        source: io::Error,
    },
    /// A line of the store that does not read as an event: serve will not start over it, so that
    /// no event is dropped unseen.
    #[error("{} line {line}: {source}", .path.display())]
    Store {
        path: PathBuf,
        line: usize,
        #[source]
        source: serde_json::Error,
    },
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("no home: none of EVENTS_HOME, XDG_DATA_HOME and HOME is set")]
    NoHome,
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
