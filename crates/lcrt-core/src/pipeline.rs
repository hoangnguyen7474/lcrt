//! Portable audio-to-caption orchestration.

use std::sync::atomic::{AtomicBool, Ordering};

use tracing::{debug, info, warn};

use crate::{
    AudioCapture, AudioCaptureError, AudioInputEvent, CaptionSink, CaptionSinkError, CaptionState,
    CaptionStateError, RuntimeConfig, RuntimeConfigError, Transcriber, TranscriptUpdate,
    TranscriptionError,
};

/// Counts useful work completed by one bounded pipeline run.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RunSummary {
    /// Number of audio chunks passed to transcription.
    pub audio_chunks: u64,
    /// Number of caption snapshots published to the UI.
    pub caption_updates: u64,
}

/// Portable caption application pipeline.
pub struct CaptionPipeline<A, T, U> {
    audio: A,
    transcriber: T,
    sink: U,
    captions: CaptionState,
    config: RuntimeConfig,
}

impl<A, T, U> CaptionPipeline<A, T, U>
where
    A: AudioCapture,
    T: Transcriber,
    U: CaptionSink,
{
    /// Creates a validated pipeline from concrete platform adapters.
    pub fn new(
        audio: A,
        transcriber: T,
        sink: U,
        config: RuntimeConfig,
    ) -> Result<Self, PipelineError> {
        config.validate().map_err(PipelineError::Configuration)?;
        Ok(Self {
            audio,
            transcriber,
            sink,
            captions: CaptionState::new(),
            config,
        })
    }

    /// Runs until cancellation, clean end-of-stream, or an adapter error.
    ///
    /// The audio adapter is stopped before transcription is flushed, and is
    /// still stopped when capture, transcription, or UI publication fails. If
    /// shutdown also fails, the original processing error is kept.
    pub fn run(mut self, cancelled: &AtomicBool) -> Result<RunSummary, PipelineError> {
        let source_id = self.audio.source().id().to_owned();
        info!(source_id, "caption pipeline started");
        let capture_result = self.capture_until_stopped(cancelled);
        let stop_result = self.audio.stop().map_err(PipelineError::Audio);
        match (capture_result, stop_result) {
            (Ok(mut summary), Ok(())) => {
                let final_updates = self
                    .transcriber
                    .finish()
                    .map_err(PipelineError::Transcription)?;
                self.publish_updates(final_updates, &mut summary)?;
                info!(
                    source_id,
                    audio_chunks = summary.audio_chunks,
                    caption_updates = summary.caption_updates,
                    "caption pipeline stopped"
                );
                Ok(summary)
            }
            (Err(error), Ok(())) => Err(error),
            (Ok(_), Err(stop_error)) => Err(stop_error),
            (Err(error), Err(stop_error)) => {
                warn!(%stop_error, "audio shutdown also failed after pipeline error");
                Err(error)
            }
        }
    }

    fn capture_until_stopped(
        &mut self,
        cancelled: &AtomicBool,
    ) -> Result<RunSummary, PipelineError> {
        let mut summary = RunSummary::default();
        while !cancelled.load(Ordering::Acquire) {
            match self
                .audio
                .next_event(self.config.audio_poll_timeout)
                .map_err(PipelineError::Audio)?
            {
                AudioInputEvent::Chunk(chunk) => {
                    if chunk.samples().len() > self.config.max_audio_chunk_samples {
                        return Err(PipelineError::AudioChunkTooLarge {
                            actual_samples: chunk.samples().len(),
                            maximum_samples: self.config.max_audio_chunk_samples,
                        });
                    }
                    summary.audio_chunks += 1;
                    let updates = self
                        .transcriber
                        .push_audio(&chunk)
                        .map_err(PipelineError::Transcription)?;
                    self.publish_updates(updates, &mut summary)?;
                }
                AudioInputEvent::Idle => debug!("audio poll completed without samples"),
                AudioInputEvent::EndOfStream => break,
            }
        }

        Ok(summary)
    }

    fn publish_updates(
        &mut self,
        updates: Vec<TranscriptUpdate>,
        summary: &mut RunSummary,
    ) -> Result<(), PipelineError> {
        for update in updates {
            let snapshot = self
                .captions
                .apply(update)
                .map_err(PipelineError::CaptionState)?;
            self.sink
                .publish(snapshot)
                .map_err(PipelineError::CaptionSink)?;
            summary.caption_updates += 1;
        }
        Ok(())
    }
}

/// Typed failure from a pipeline boundary.
#[derive(Debug)]
pub enum PipelineError {
    /// Runtime limits were invalid.
    Configuration(RuntimeConfigError),
    /// The audio adapter failed.
    Audio(AudioCaptureError),
    /// An adapter violated the configured per-chunk bound.
    AudioChunkTooLarge {
        actual_samples: usize,
        maximum_samples: usize,
    },
    /// The speech-to-text backend failed.
    Transcription(TranscriptionError),
    /// Caption state could not be advanced.
    CaptionState(CaptionStateError),
    /// The native UI sink failed.
    CaptionSink(CaptionSinkError),
}

impl std::fmt::Display for PipelineError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Configuration(error) => {
                write!(formatter, "invalid runtime configuration: {error}")
            }
            Self::Audio(error) => write!(formatter, "audio capture failed: {error}"),
            Self::AudioChunkTooLarge {
                actual_samples,
                maximum_samples,
            } => write!(
                formatter,
                "audio adapter produced {actual_samples} samples; configured maximum is {maximum_samples}"
            ),
            Self::Transcription(error) => write!(formatter, "transcription failed: {error}"),
            Self::CaptionState(error) => write!(formatter, "caption state failed: {error}"),
            Self::CaptionSink(error) => write!(formatter, "caption UI failed: {error}"),
        }
    }
}

impl std::error::Error for PipelineError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Configuration(error) => Some(error),
            Self::Audio(error) => Some(error),
            Self::AudioChunkTooLarge { .. } => None,
            Self::Transcription(error) => Some(error),
            Self::CaptionState(error) => Some(error),
            Self::CaptionSink(error) => Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::Duration,
    };

    use crate::{
        AudioCapture, AudioCaptureError, AudioChunk, AudioInputEvent, AudioSourceDescriptor,
        AudioSourceKind, CaptionSink, CaptionSinkError, CaptionSnapshot, RuntimeConfig,
        Transcriber, TranscriptUpdate, TranscriptionError,
    };

    use super::{CaptionPipeline, PipelineError, RunSummary};

    struct FakeAudio {
        source: AudioSourceDescriptor,
        events: VecDeque<AudioInputEvent>,
        stopped: Arc<AtomicBool>,
    }

    impl AudioCapture for FakeAudio {
        fn source(&self) -> &AudioSourceDescriptor {
            &self.source
        }

        fn next_event(&mut self, _timeout: Duration) -> Result<AudioInputEvent, AudioCaptureError> {
            Ok(self
                .events
                .pop_front()
                .unwrap_or(AudioInputEvent::EndOfStream))
        }

        fn stop(&mut self) -> Result<(), AudioCaptureError> {
            self.stopped.store(true, Ordering::Release);
            Ok(())
        }
    }

    struct FakeTranscriber {
        audio_stopped: Arc<AtomicBool>,
    }

    impl Transcriber for FakeTranscriber {
        fn push_audio(
            &mut self,
            _chunk: &AudioChunk,
        ) -> Result<Vec<TranscriptUpdate>, TranscriptionError> {
            Ok(vec![TranscriptUpdate::partial("hello").unwrap()])
        }

        fn finish(&mut self) -> Result<Vec<TranscriptUpdate>, TranscriptionError> {
            if !self.audio_stopped.load(Ordering::Acquire) {
                return Err(TranscriptionError::new(
                    "audio was not stopped before transcription flush",
                ));
            }
            Ok(vec![TranscriptUpdate::finalized("hello world").unwrap()])
        }
    }

    #[derive(Default)]
    struct CollectingSink {
        snapshots: Vec<CaptionSnapshot>,
    }

    impl CaptionSink for CollectingSink {
        fn publish(&mut self, snapshot: CaptionSnapshot) -> Result<(), CaptionSinkError> {
            self.snapshots.push(snapshot);
            Ok(())
        }
    }

    fn audio_with(events: Vec<AudioInputEvent>, stopped: Arc<AtomicBool>) -> FakeAudio {
        FakeAudio {
            source: AudioSourceDescriptor::new(
                "test-mic",
                "Test microphone",
                AudioSourceKind::Microphone,
            ),
            events: events.into(),
            stopped,
        }
    }

    #[test]
    fn pipeline_publishes_incremental_and_final_updates_then_stops_audio() {
        let stopped = Arc::new(AtomicBool::new(false));
        let audio = audio_with(
            vec![
                AudioInputEvent::Chunk(AudioChunk::new(vec![0.0; 160], 16_000, 1).unwrap()),
                AudioInputEvent::EndOfStream,
            ],
            Arc::clone(&stopped),
        );
        let pipeline = CaptionPipeline::new(
            audio,
            FakeTranscriber {
                audio_stopped: Arc::clone(&stopped),
            },
            CollectingSink::default(),
            RuntimeConfig::default(),
        )
        .unwrap();
        let summary = pipeline.run(&AtomicBool::new(false)).unwrap();
        assert_eq!(
            summary,
            RunSummary {
                audio_chunks: 1,
                caption_updates: 2
            }
        );
        assert!(stopped.load(Ordering::Acquire));
    }

    #[test]
    fn pipeline_enforces_chunk_bound_and_still_stops_audio() {
        let stopped = Arc::new(AtomicBool::new(false));
        let audio = audio_with(
            vec![AudioInputEvent::Chunk(
                AudioChunk::new(vec![0.0; 5], 16_000, 1).unwrap(),
            )],
            Arc::clone(&stopped),
        );
        let config = RuntimeConfig {
            max_audio_chunk_samples: 4,
            ..RuntimeConfig::default()
        };
        let pipeline = CaptionPipeline::new(
            audio,
            FakeTranscriber {
                audio_stopped: Arc::clone(&stopped),
            },
            CollectingSink::default(),
            config,
        )
        .unwrap();
        let error = pipeline.run(&AtomicBool::new(false)).unwrap_err();
        assert!(matches!(
            error,
            PipelineError::AudioChunkTooLarge {
                actual_samples: 5,
                maximum_samples: 4
            }
        ));
        assert!(stopped.load(Ordering::Acquire));
    }
}
