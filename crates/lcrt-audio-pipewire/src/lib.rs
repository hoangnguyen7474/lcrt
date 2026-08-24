//! Linux PipeWire audio source enumeration and bounded capture adapter.
//!
//! The crate has no PipeWire dependency on non-Linux targets. Linux exports are
//! intentionally isolated so the portable LCRT core remains OS-independent.

#[cfg(target_os = "linux")]
mod capture;
mod error;
#[cfg(target_os = "linux")]
mod sources;

#[cfg(target_os = "linux")]
pub use capture::{PipeWireCapture, PipeWireCaptureConfig};
pub use error::PipeWireError;
#[cfg(target_os = "linux")]
pub use sources::enumerate_audio_sources;
