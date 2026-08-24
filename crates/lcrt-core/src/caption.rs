//! Caption domain state.

use std::{error::Error, fmt};

use crate::transcription::TranscriptUpdate;

/// Whether a caption may still change or has been finalized by transcription.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptionStatus {
    /// The caption contains an incremental transcription result.
    Partial,
    /// The caption contains a completed transcription result.
    Final,
}

/// A platform-independent caption produced by the transcription pipeline.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Caption {
    text: String,
    status: CaptionStatus,
}

impl Caption {
    /// Creates a caption from an incremental transcription result.
    pub fn partial(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            status: CaptionStatus::Partial,
        }
    }

    /// Returns the caption text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns whether the caption is partial or final.
    pub fn status(&self) -> CaptionStatus {
        self.status
    }

    /// Marks the caption as final while preserving its text.
    pub fn finalize(&mut self) {
        self.status = CaptionStatus::Final;
    }
}

/// Immutable caption state sent to a UI adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptionSnapshot {
    revision: u64,
    caption: Caption,
}

impl CaptionSnapshot {
    /// Returns a monotonically increasing state revision.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Returns the current caption.
    pub fn caption(&self) -> &Caption {
        &self.caption
    }
}

/// Mutable caption state owned by the application pipeline.
#[derive(Clone, Debug, Default)]
pub struct CaptionState {
    revision: u64,
    current: Option<Caption>,
}

impl CaptionState {
    /// Creates empty caption state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Applies one transcription update and returns the new UI snapshot.
    pub fn apply(
        &mut self,
        update: TranscriptUpdate,
    ) -> Result<CaptionSnapshot, CaptionStateError> {
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or(CaptionStateError::RevisionOverflow)?;
        let status = update.status();
        let caption = Caption {
            text: update.into_text(),
            status,
        };
        self.current = Some(caption.clone());
        Ok(CaptionSnapshot {
            revision: self.revision,
            caption,
        })
    }

    /// Returns the current state without changing its revision.
    pub fn snapshot(&self) -> Option<CaptionSnapshot> {
        self.current.clone().map(|caption| CaptionSnapshot {
            revision: self.revision,
            caption,
        })
    }
}

/// Failure while updating caption state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptionStateError {
    /// The monotonically increasing revision counter was exhausted.
    RevisionOverflow,
}

impl fmt::Display for CaptionStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("caption revision counter overflowed")
    }
}

impl Error for CaptionStateError {}

#[cfg(test)]
mod tests {
    use super::{Caption, CaptionState, CaptionStatus};
    use crate::TranscriptUpdate;

    #[test]
    fn finalizing_a_partial_caption_preserves_its_text() {
        let mut caption = Caption::partial("low-latency captions");
        assert_eq!(caption.status(), CaptionStatus::Partial);
        caption.finalize();
        assert_eq!(caption.text(), "low-latency captions");
        assert_eq!(caption.status(), CaptionStatus::Final);
    }

    #[test]
    fn applying_updates_advances_revision_and_replaces_current_caption() {
        let mut state = CaptionState::new();
        let partial = state
            .apply(TranscriptUpdate::partial("hello").unwrap())
            .unwrap();
        let final_caption = state
            .apply(TranscriptUpdate::finalized("hello world").unwrap())
            .unwrap();
        assert_eq!(partial.revision(), 1);
        assert_eq!(partial.caption().status(), CaptionStatus::Partial);
        assert_eq!(final_caption.revision(), 2);
        assert_eq!(final_caption.caption().text(), "hello world");
        assert_eq!(final_caption.caption().status(), CaptionStatus::Final);
        assert_eq!(state.snapshot(), Some(final_caption));
    }
}
