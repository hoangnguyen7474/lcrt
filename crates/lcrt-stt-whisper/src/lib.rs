//! Bounded local streaming speech-to-text powered by whisper.cpp.

mod backend;
mod config;
mod error;
mod resample;
mod window;

pub use backend::WhisperTranscriber;
pub use config::WhisperConfig;
pub use error::WhisperBackendError;
