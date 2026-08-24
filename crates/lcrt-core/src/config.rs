//! Explicit runtime configuration shared by application frontends.

use std::{error::Error, fmt, time::Duration};

/// Portable pipeline runtime configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeConfig {
    /// Maximum time one audio read may block before checking for shutdown.
    pub audio_poll_timeout: Duration,
    /// Maximum number of interleaved samples accepted in one adapter chunk.
    pub max_audio_chunk_samples: usize,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            audio_poll_timeout: Duration::from_millis(50),
            max_audio_chunk_samples: 48_000,
        }
    }
}

impl RuntimeConfig {
    /// Validates runtime limits before capture starts.
    pub fn validate(&self) -> Result<(), RuntimeConfigError> {
        if self.audio_poll_timeout.is_zero() {
            return Err(RuntimeConfigError::ZeroAudioPollTimeout);
        }
        if self.max_audio_chunk_samples == 0 {
            return Err(RuntimeConfigError::ZeroMaxAudioChunkSamples);
        }
        Ok(())
    }
}

/// Invalid runtime configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeConfigError {
    /// Audio reads would never wait, causing a busy loop.
    ZeroAudioPollTimeout,
    /// Every non-empty audio chunk would be rejected.
    ZeroMaxAudioChunkSamples,
}

impl fmt::Display for RuntimeConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroAudioPollTimeout => {
                formatter.write_str("audio poll timeout must be greater than zero")
            }
            Self::ZeroMaxAudioChunkSamples => {
                formatter.write_str("maximum audio chunk size must be greater than zero")
            }
        }
    }
}

impl Error for RuntimeConfigError {}

#[cfg(test)]
mod tests {
    use super::{RuntimeConfig, RuntimeConfigError};
    use std::time::Duration;

    #[test]
    fn config_rejects_zero_poll_timeout() {
        let config = RuntimeConfig {
            audio_poll_timeout: Duration::ZERO,
            ..RuntimeConfig::default()
        };
        assert_eq!(
            config.validate(),
            Err(RuntimeConfigError::ZeroAudioPollTimeout)
        );
    }
}
