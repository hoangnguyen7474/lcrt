use lcrt_core::TranscriptUpdate;

use crate::{WhisperBackendError, window::InferenceKind};

const TRUNCATION_MARKER: &str = "…";

/// Bounded transcript state for one utterance.
///
/// `committed` contains words that disappeared from the front of an advancing
/// rolling audio window. `partial` is the current window hypothesis and remains
/// replaceable. Both strings share one explicit byte budget.
pub(crate) struct TranscriptAssembler {
    committed: String,
    partial: String,
    max_bytes: usize,
}

impl TranscriptAssembler {
    pub(crate) fn new(max_bytes: usize) -> Self {
        Self {
            committed: String::new(),
            partial: String::new(),
            max_bytes,
        }
    }

    pub(crate) fn apply(
        &mut self,
        kind: InferenceKind,
        text: String,
        window_rolled: bool,
    ) -> Result<Option<TranscriptUpdate>, WhisperBackendError> {
        let text = text.trim();
        match kind {
            InferenceKind::Partial if text.is_empty() => Ok(None),
            InferenceKind::Partial => {
                let previous = self.joined();
                if window_rolled {
                    self.commit_prefix_not_in(text);
                }
                self.partial.clear();
                self.partial.push_str(text);
                self.enforce_bound();
                if self.joined() == previous {
                    return Ok(None);
                }
                TranscriptUpdate::incremental(&self.committed, &self.partial)
                    .map(Some)
                    .map_err(|error| WhisperBackendError::Whisper(error.to_string()))
            }
            InferenceKind::Final => {
                if !text.is_empty() {
                    if window_rolled {
                        self.commit_prefix_not_in(text);
                    }
                    self.partial.clear();
                    self.partial.push_str(text);
                }
                self.enforce_bound();
                let finalized = self.joined();
                self.committed.clear();
                self.partial.clear();
                if finalized.is_empty() {
                    Ok(None)
                } else {
                    TranscriptUpdate::finalized(finalized)
                        .map(Some)
                        .map_err(|error| WhisperBackendError::Whisper(error.to_string()))
                }
            }
        }
    }

    fn commit_prefix_not_in(&mut self, next_partial: &str) {
        let prefix = non_overlapping_prefix(&self.partial, next_partial);
        if prefix.is_empty() {
            return;
        }
        if !self.committed.is_empty() {
            self.committed.push(' ');
        }
        self.committed.push_str(&prefix);
    }

    fn enforce_bound(&mut self) {
        let separator_bytes = usize::from(!self.committed.is_empty() && !self.partial.is_empty());
        if self.partial.len().saturating_add(separator_bytes) >= self.max_bytes {
            self.committed.clear();
            self.partial = truncate_front(&self.partial, self.max_bytes);
            return;
        }

        let committed_budget = self
            .max_bytes
            .saturating_sub(self.partial.len())
            .saturating_sub(separator_bytes);
        self.committed = truncate_front(&self.committed, committed_budget);
    }

    fn joined(&self) -> String {
        match (self.committed.is_empty(), self.partial.is_empty()) {
            (true, true) => String::new(),
            (true, false) => self.partial.clone(),
            (false, true) => self.committed.clone(),
            (false, false) => format!("{} {}", self.committed, self.partial),
        }
    }
}

fn non_overlapping_prefix(previous: &str, current: &str) -> String {
    let previous_words = previous.split_whitespace().collect::<Vec<_>>();
    let current_words = current.split_whitespace().collect::<Vec<_>>();
    let maximum = previous_words.len().min(current_words.len());
    let overlap = (1..=maximum)
        .rev()
        .find(|&count| {
            previous_words[previous_words.len() - count..]
                .iter()
                .zip(&current_words[..count])
                .all(|(left, right)| normalized_word(left) == normalized_word(right))
        })
        .unwrap_or(0);
    previous_words[..previous_words.len() - overlap].join(" ")
}

fn normalized_word(word: &str) -> String {
    word.trim_matches(|character: char| !character.is_alphanumeric())
        .to_lowercase()
}

fn truncate_front(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_owned();
    }
    if max_bytes < TRUNCATION_MARKER.len() {
        return String::new();
    }
    if max_bytes == TRUNCATION_MARKER.len() {
        return TRUNCATION_MARKER.to_owned();
    }

    let suffix_budget = max_bytes - TRUNCATION_MARKER.len() - 1;
    let mut start = text.len().saturating_sub(suffix_budget);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    let suffix = text[start..].trim_start();
    format!("{TRUNCATION_MARKER} {suffix}")
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use lcrt_core::CaptionStatus;

    use super::{TRUNCATION_MARKER, TranscriptAssembler};
    use crate::{
        WhisperConfig,
        window::{InferenceKind, StreamingWindow},
    };

    fn short_window_config() -> WhisperConfig {
        let mut config = WhisperConfig::new(std::env::temp_dir().join("unused-test-model.bin"));
        config.window_duration = Duration::from_secs(2);
        config.partial_step = Duration::from_millis(500);
        config.minimum_speech = Duration::from_millis(250);
        config.final_silence = Duration::from_millis(300);
        config.speech_rms_threshold = 0.01;
        config
    }

    #[test]
    fn rolling_windows_commit_disappearing_prefix_without_duplicates() {
        let mut transcript = TranscriptAssembler::new(256);

        transcript
            .apply(
                InferenceKind::Partial,
                "alpha beta gamma delta".to_owned(),
                false,
            )
            .unwrap();
        let second = transcript
            .apply(
                InferenceKind::Partial,
                "gamma delta epsilon zeta".to_owned(),
                true,
            )
            .unwrap()
            .unwrap();
        let third = transcript
            .apply(
                InferenceKind::Partial,
                "epsilon zeta eta theta".to_owned(),
                true,
            )
            .unwrap()
            .unwrap();

        assert_eq!(second.text(), "alpha beta gamma delta epsilon zeta");
        assert_eq!(second.stable_text(), "alpha beta");
        assert_eq!(
            third.text(),
            "alpha beta gamma delta epsilon zeta eta theta"
        );
        assert_eq!(third.stable_text(), "alpha beta gamma delta");
    }

    #[test]
    fn continuous_speech_beyond_window_preserves_accepted_beginning() {
        let config = short_window_config();
        let mut window = StreamingWindow::new(&config).unwrap();
        let mut transcript = TranscriptAssembler::new(config.max_transcript_bytes);
        let hypotheses = [
            "zero",
            "zero one",
            "zero one two",
            "zero one two three",
            "one two three four",
            "two three four five",
        ];
        let mut last = None;

        for hypothesis in hypotheses {
            let kind = window.push(&vec![0.1; 8_000]).unwrap();
            let rolled = window.rolled_since_inference();
            window.mark_inferred(kind);
            last = transcript
                .apply(kind, hypothesis.to_owned(), rolled)
                .unwrap()
                .or(last);
        }

        let last = last.unwrap();
        assert_eq!(window.samples().len(), 32_000);
        assert_eq!(last.text(), "zero one two three four five");
        assert_eq!(last.stable_text(), "zero one");
    }

    #[test]
    fn empty_final_preserves_and_finalizes_last_valid_partial() {
        let mut transcript = TranscriptAssembler::new(256);
        transcript
            .apply(
                InferenceKind::Partial,
                "keep this caption".to_owned(),
                false,
            )
            .unwrap();

        let final_update = transcript
            .apply(InferenceKind::Final, String::new(), false)
            .unwrap()
            .unwrap();

        assert_eq!(final_update.text(), "keep this caption");
        assert_eq!(final_update.status(), CaptionStatus::Final);
    }

    #[test]
    fn transcript_memory_and_visible_text_stay_bounded() {
        let mut transcript = TranscriptAssembler::new(48);
        transcript
            .apply(
                InferenceKind::Partial,
                "one two three four five six seven eight".to_owned(),
                false,
            )
            .unwrap();
        let update = transcript
            .apply(
                InferenceKind::Partial,
                "seven eight nine ten eleven twelve thirteen".to_owned(),
                true,
            )
            .unwrap()
            .unwrap();

        assert!(update.text().len() <= 48);
        assert!(update.text().starts_with(TRUNCATION_MARKER));
    }
}
