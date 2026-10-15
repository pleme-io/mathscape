use mathscape_core::bootstrap::SpecExecutionError;
use mathscape_core::snapshot::SnapshotError;
use std::fmt;

#[derive(Debug)]
pub enum StreamError {
    Io(std::io::Error),
    Snapshot(SnapshotError),
    Corrupt { object: String, reason: String },
    Missing { object: String },
    BadHead { content: String },
    BadMetadata { object: String, key: &'static str },
    SeedMismatch { head: u64, config: u64 },
    Motor(String),
}

impl fmt::Display for StreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "io: {e}"),
            Self::Snapshot(e) => write!(f, "snapshot: {e}"),
            Self::Corrupt { object, reason } => write!(f, "object {object} is corrupt: {reason}"),
            Self::Missing { object } => write!(f, "object {object} is named but not stored"),
            Self::BadHead { content } => write!(f, "HEAD does not name an object: {content:?}"),
            Self::BadMetadata { object, key } => write!(f, "snapshot {object} lacks a readable `{key}`"),
            Self::SeedMismatch { head, config } => write!(
                f,
                "HEAD was grown from seed {head} but the config says seed {config}; a stream never changes seed, start a new data dir instead"
            ),
            Self::Motor(e) => write!(f, "motor: {e}"),
        }
    }
}

impl std::error::Error for StreamError {}

impl From<std::io::Error> for StreamError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<SnapshotError> for StreamError {
    fn from(e: SnapshotError) -> Self {
        Self::Snapshot(e)
    }
}

impl From<SpecExecutionError> for StreamError {
    fn from(e: SpecExecutionError) -> Self {
        Self::Motor(format!("{e:?}"))
    }
}
