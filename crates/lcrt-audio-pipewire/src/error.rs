use std::{error::Error, fmt, time::Duration};

/// Failure while connecting to or operating a PipeWire audio adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PipeWireError {
    /// A caller supplied a zero or otherwise unusable limit.
    InvalidConfiguration(&'static str),
    /// PipeWire rejected setup or a server operation.
    PipeWire(String),
    /// The specifically selected source is no longer available.
    SelectedSourceUnavailable,
    /// PipeWire negotiated an audio format this adapter cannot safely consume.
    InvalidNegotiatedFormat(String),
    /// PipeWire supplied an invalid or unsupported mapped audio buffer.
    MalformedBuffer(String),
    /// A running PipeWire stream entered a terminal error state.
    StreamFailure(String),
    /// Source enumeration did not complete within the caller's bound.
    EnumerationTimeout(Duration),
    /// Capture did not reach a streaming state within the caller's bound.
    StartupTimeout(Duration),
    /// The capture worker terminated before reporting its state.
    WorkerStopped,
    /// The capture worker could not be created or joined cleanly.
    Worker(String),
}

impl fmt::Display for PipeWireError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfiguration(message) => {
                write!(
                    formatter,
                    "invalid PipeWire capture configuration: {message}"
                )
            }
            Self::PipeWire(message) => write!(formatter, "PipeWire operation failed: {message}"),
            Self::SelectedSourceUnavailable => formatter.write_str(
                "the selected PipeWire audio source disconnected or is unavailable; select another source to continue",
            ),
            Self::InvalidNegotiatedFormat(message) => {
                write!(formatter, "PipeWire negotiated an unsupported audio format: {message}")
            }
            Self::MalformedBuffer(message) => {
                write!(formatter, "PipeWire supplied a malformed audio buffer: {message}")
            }
            Self::StreamFailure(message) => {
                write!(formatter, "PipeWire capture stream failed: {message}")
            }
            Self::EnumerationTimeout(timeout) => write!(
                formatter,
                "PipeWire source enumeration did not finish within {timeout:?}"
            ),
            Self::StartupTimeout(timeout) => write!(
                formatter,
                "PipeWire capture did not start within {timeout:?}; check that the selected source is available"
            ),
            Self::WorkerStopped => {
                formatter.write_str("PipeWire capture worker stopped unexpectedly")
            }
            Self::Worker(message) => write!(formatter, "PipeWire capture worker failed: {message}"),
        }
    }
}

impl Error for PipeWireError {}
