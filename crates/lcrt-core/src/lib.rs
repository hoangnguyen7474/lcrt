//! Portable application core for LCRT.
//!
//! This crate owns platform-independent audio and transcription ports, caption
//! state, runtime configuration, and pipeline orchestration. Platform adapters
//! (PipeWire, whisper.cpp, and native UI implementations) live outside it.

pub mod audio;
pub mod caption;
pub mod config;
pub mod pipeline;
pub mod transcription;
pub mod ui;

pub use audio::{
    AudioCapture, AudioCaptureError, AudioChunk, AudioChunkError, AudioInputEvent,
    AudioSourceDescriptor, AudioSourceKind,
};
pub use caption::{Caption, CaptionSnapshot, CaptionState, CaptionStateError, CaptionStatus};
pub use config::{RuntimeConfig, RuntimeConfigError};
pub use pipeline::{CaptionPipeline, PipelineError, RunSummary};
pub use transcription::{Transcriber, TranscriptUpdate, TranscriptUpdateError, TranscriptionError};
pub use ui::{CaptionSink, CaptionSinkError};
