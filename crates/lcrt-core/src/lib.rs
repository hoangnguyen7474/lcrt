//! Portable domain types and logic shared by LCRT platform implementations.

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

#[cfg(test)]
mod tests {
    use super::{Caption, CaptionStatus};

    #[test]
    fn finalizing_a_partial_caption_preserves_its_text() {
        let mut caption = Caption::partial("low-latency captions");

        assert_eq!(caption.status(), CaptionStatus::Partial);
        caption.finalize();

        assert_eq!(caption.text(), "low-latency captions");
        assert_eq!(caption.status(), CaptionStatus::Final);
    }
}
