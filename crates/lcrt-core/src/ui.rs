//! UI adapter boundary.

use std::{error::Error, fmt};

use crate::CaptionSnapshot;

/// Actionable error reported by a native UI adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptionSinkError {
    message: String,
}

impl CaptionSinkError {
    /// Creates a UI adapter error with user-actionable context.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for CaptionSinkError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for CaptionSinkError {}

/// Receives immutable caption snapshots for native presentation.
pub trait CaptionSink: Send {
    /// Publishes the newest caption state without blocking on expensive work.
    fn publish(&mut self, snapshot: CaptionSnapshot) -> Result<(), CaptionSinkError>;
}
