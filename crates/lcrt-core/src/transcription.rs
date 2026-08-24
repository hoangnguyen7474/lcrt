//! Speech-to-text adapter boundary and incremental update types.

use std::{error::Error, fmt};

use crate::{AudioChunk, CaptionStatus};

/// An incremental or final speech-to-text result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TranscriptUpdate {
    text: String,
    status: CaptionStatus,
}

impl TranscriptUpdate {
    /// Creates an incremental result that may be superseded.
    pub fn partial(text: impl Into<String>) -> Result<Self, TranscriptUpdateError> {
        Self::new(text, CaptionStatus::Partial)
    }

    /// Creates a finalized result.
    pub fn finalized(text: impl Into<String>) -> Result<Self, TranscriptUpdateError> {
        Self::new(text, CaptionStatus::Final)
    }

    fn new(text: impl Into<String>, status: CaptionStatus) -> Result<Self, TranscriptUpdateError> {
        let text = text.into();
        if text.trim().is_empty() {
            return Err(TranscriptUpdateError::EmptyText);
        }
        Ok(Self { text, status })
    }

    /// Returns the update text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Consumes the update and returns its text.
    pub(crate) fn into_text(self) -> String {
        self.text
    }

    /// Returns whether this update is partial or final.
    pub fn status(&self) -> CaptionStatus {
        self.status
    }
}

/// Invalid speech-to-text update.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TranscriptUpdateError {
    /// Empty updates are not meaningful caption state.
    EmptyText,
}

impl fmt::Display for TranscriptUpdateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("transcript update text must not be empty")
    }
}

impl Error for TranscriptUpdateError {}

/// Actionable error reported by a speech-to-text adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TranscriptionError {
    message: String,
}

impl TranscriptionError {
    /// Creates a backend error with user-actionable context.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for TranscriptionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for TranscriptionError {}

/// A local or remote-replaceable speech-to-text backend.
pub trait Transcriber: Send {
    /// Consumes one bounded audio chunk and returns zero or more caption updates.
    fn push_audio(
        &mut self,
        chunk: &AudioChunk,
    ) -> Result<Vec<TranscriptUpdate>, TranscriptionError>;
    /// Flushes any buffered speech when capture stops.
    fn finish(&mut self) -> Result<Vec<TranscriptUpdate>, TranscriptionError>;
}

#[cfg(test)]
mod tests {
    use super::{TranscriptUpdate, TranscriptUpdateError};

    #[test]
    fn transcript_update_rejects_whitespace_only_text() {
        assert_eq!(
            TranscriptUpdate::partial("   ").unwrap_err(),
            TranscriptUpdateError::EmptyText
        );
    }
}
