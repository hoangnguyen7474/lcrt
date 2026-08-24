use std::{
    cell::Cell,
    mem,
    rc::Rc,
    sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError, sync_channel},
    thread::{self, JoinHandle},
    time::Duration,
};

use lcrt_core::{
    AudioCapture, AudioCaptureError, AudioChunk, AudioInputEvent, AudioSourceDescriptor,
    AudioSourceKind,
};
use pipewire as pw;
use pw::{properties::properties, spa};
use spa::{
    param::audio::{AudioFormat, AudioInfoRaw},
    param::format::{MediaSubtype, MediaType},
    param::format_utils,
    pod::Pod,
};
use tracing::{info, warn};

use crate::PipeWireError;

/// Bounds PipeWire capture startup and buffering.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PipeWireCaptureConfig {
    /// Maximum number of PCM chunks waiting for the application pipeline.
    pub queue_capacity: usize,
    /// Maximum time to wait for PipeWire to enter the streaming state.
    pub startup_timeout: Duration,
}

impl Default for PipeWireCaptureConfig {
    fn default() -> Self {
        Self {
            queue_capacity: 8,
            startup_timeout: Duration::from_secs(5),
        }
    }
}

impl PipeWireCaptureConfig {
    fn validate(&self) -> Result<(), PipeWireError> {
        if self.queue_capacity == 0 {
            return Err(PipeWireError::InvalidConfiguration(
                "queue capacity must be greater than zero",
            ));
        }
        if self.startup_timeout.is_zero() {
            return Err(PipeWireError::InvalidConfiguration(
                "startup timeout must be greater than zero",
            ));
        }
        Ok(())
    }
}

enum WorkerEvent {
    Audio(AudioChunk),
    Failure(String),
    Stopped,
}

/// A live PipeWire session implementing the portable [`AudioCapture`] port.
pub struct PipeWireCapture {
    source: AudioSourceDescriptor,
    events: Receiver<WorkerEvent>,
    stop_sender: Option<pw::channel::Sender<()>>,
    worker: Option<JoinHandle<Result<(), PipeWireError>>>,
    stopped: bool,
}

impl PipeWireCapture {
    /// Starts capture from an enumerated microphone or system-output source.
    pub fn start(
        source: AudioSourceDescriptor,
        config: PipeWireCaptureConfig,
    ) -> Result<Self, PipeWireError> {
        config.validate()?;
        let (event_sender, events) = sync_channel(config.queue_capacity);
        let (startup_sender, startup_receiver) = sync_channel(1);
        let (stop_sender, stop_receiver) = pw::channel::channel();
        let worker_source = source.clone();
        let worker = thread::Builder::new()
            .name("lcrt-pipewire-capture".to_owned())
            .spawn(move || {
                let result = run_worker(
                    worker_source,
                    event_sender.clone(),
                    startup_sender.clone(),
                    stop_receiver,
                );
                if let Err(error) = &result {
                    let message = error.to_string();
                    let _ = startup_sender.try_send(Err(message.clone()));
                    let _ = event_sender.try_send(WorkerEvent::Failure(message));
                }
                let _ = event_sender.try_send(WorkerEvent::Stopped);
                result
            })
            .map_err(|error| PipeWireError::Worker(error.to_string()))?;

        match startup_receiver.recv_timeout(config.startup_timeout) {
            Ok(Ok(())) => {
                info!(source_id = source.id(), "PipeWire capture streaming");
                Ok(Self {
                    source,
                    events,
                    stop_sender: Some(stop_sender),
                    worker: Some(worker),
                    stopped: false,
                })
            }
            Ok(Err(message)) => {
                let _ = stop_sender.send(());
                let _ = worker.join();
                Err(PipeWireError::PipeWire(message))
            }
            Err(RecvTimeoutError::Timeout) => {
                let _ = stop_sender.send(());
                // The stop message is attached to the PipeWire loop. Detach the
                // handle so an unresponsive server cannot make this bounded
                // constructor wait beyond its documented timeout.
                drop(worker);
                Err(PipeWireError::StartupTimeout(config.startup_timeout))
            }
            Err(RecvTimeoutError::Disconnected) => {
                let _ = worker.join();
                Err(PipeWireError::WorkerStopped)
            }
        }
    }

    fn shutdown(&mut self) -> Result<(), AudioCaptureError> {
        if self.stopped {
            return Ok(());
        }
        self.stopped = true;
        if let Some(sender) = self.stop_sender.take() {
            let _ = sender.send(());
        }
        let Some(worker) = self.worker.take() else {
            return Ok(());
        };
        match worker.join() {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(AudioCaptureError::new(error.to_string())),
            Err(_) => Err(AudioCaptureError::new(
                "PipeWire capture worker panicked during shutdown",
            )),
        }
    }
}

impl AudioCapture for PipeWireCapture {
    fn source(&self) -> &AudioSourceDescriptor {
        &self.source
    }

    fn next_event(&mut self, timeout: Duration) -> Result<AudioInputEvent, AudioCaptureError> {
        if self.stopped {
            return Ok(AudioInputEvent::EndOfStream);
        }
        match self.events.recv_timeout(timeout) {
            Ok(WorkerEvent::Audio(chunk)) => Ok(AudioInputEvent::Chunk(chunk)),
            Ok(WorkerEvent::Failure(message)) => Err(AudioCaptureError::new(message)),
            Ok(WorkerEvent::Stopped) => Err(AudioCaptureError::new(
                "PipeWire capture stopped before the application requested shutdown",
            )),
            Err(RecvTimeoutError::Timeout) => Ok(AudioInputEvent::Idle),
            Err(RecvTimeoutError::Disconnected) => Err(AudioCaptureError::new(
                "PipeWire capture channel disconnected unexpectedly",
            )),
        }
    }

    fn stop(&mut self) -> Result<(), AudioCaptureError> {
        self.shutdown()
    }
}

impl Drop for PipeWireCapture {
    fn drop(&mut self) {
        if let Err(error) = self.shutdown() {
            warn!(%error, "failed to stop PipeWire capture while dropping adapter");
        }
    }
}

struct WorkerData {
    format: Option<NegotiatedFormat>,
    events: SyncSender<WorkerEvent>,
    startup: Option<SyncSender<Result<(), String>>>,
    mainloop: pw::main_loop::MainLoopWeak,
    dropped_chunks: Rc<Cell<u64>>,
}

#[derive(Clone, Copy)]
struct NegotiatedFormat {
    sample_rate_hz: u32,
    channels: u16,
}

fn run_worker(
    source: AudioSourceDescriptor,
    events: SyncSender<WorkerEvent>,
    startup: SyncSender<Result<(), String>>,
    stop_receiver: pw::channel::Receiver<()>,
) -> Result<(), PipeWireError> {
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None).map_err(pipewire_error)?;
    let context = pw::context::ContextRc::new(&mainloop, None).map_err(pipewire_error)?;
    let core = context.connect_rc(None).map_err(pipewire_error)?;
    let _stop_listener = stop_receiver.attach(mainloop.loop_(), {
        let mainloop = mainloop.clone();
        move |_| mainloop.quit()
    });

    let mut stream_properties = properties! {
        *pw::keys::APP_NAME => "LCRT",
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Capture",
        *pw::keys::MEDIA_ROLE => "Communication",
        *pw::keys::TARGET_OBJECT => source.id(),
    };
    if source.kind() == AudioSourceKind::SystemOutput {
        stream_properties.insert(*pw::keys::STREAM_CAPTURE_SINK, "true");
    }
    let stream = pw::stream::StreamBox::new(&core, "lcrt-audio-capture", stream_properties)
        .map_err(pipewire_error)?;
    let dropped_chunks = Rc::new(Cell::new(0));
    let worker_data = WorkerData {
        format: None,
        events,
        startup: Some(startup),
        mainloop: mainloop.downgrade(),
        dropped_chunks: Rc::clone(&dropped_chunks),
    };

    let _listener = stream
        .add_local_listener_with_user_data(worker_data)
        .state_changed(|_, data, _, new_state| match new_state {
            pw::stream::StreamState::Streaming => {
                if let Some(startup) = data.startup.take() {
                    let _ = startup.try_send(Ok(()));
                }
            }
            pw::stream::StreamState::Error(message) => {
                report_fatal(
                    data,
                    format!("capture stream entered an error state: {message}"),
                );
            }
            _ => {}
        })
        .param_changed(|_, data, id, parameter| {
            if id != spa::param::ParamType::Format.as_raw() {
                return;
            }
            let Some(parameter) = parameter else {
                data.format = None;
                return;
            };
            let (media_type, subtype) = match format_utils::parse_format(parameter) {
                Ok(format) => format,
                Err(error) => {
                    report_fatal(
                        data,
                        format!("failed to identify negotiated PipeWire format: {error}"),
                    );
                    return;
                }
            };
            if media_type != MediaType::Audio || subtype != MediaSubtype::Raw {
                report_fatal(
                    data,
                    "PipeWire negotiated a non-raw-audio format".to_owned(),
                );
                return;
            }
            let mut info = AudioInfoRaw::new();
            if let Err(error) = info.parse(parameter) {
                report_fatal(
                    data,
                    format!("failed to parse negotiated PipeWire format: {error}"),
                );
                return;
            }
            match info.format() {
                AudioFormat::F32LE => match u16::try_from(info.channels()) {
                    Ok(channels) if channels > 0 && info.rate() > 0 => {
                        data.format = Some(NegotiatedFormat {
                            sample_rate_hz: info.rate(),
                            channels,
                        });
                    }
                    _ => report_fatal(
                        data,
                        "PipeWire negotiated an invalid channel count".to_owned(),
                    ),
                },
                _ => report_fatal(
                    data,
                    "PipeWire negotiated an unsupported non-F32LE format".to_owned(),
                ),
            }
        })
        .process(|stream, data| {
            let Some(format) = data.format else {
                return;
            };
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let Some(plane) = buffer.datas_mut().first_mut() else {
                return;
            };
            let offset = usize::try_from(plane.chunk().offset()).unwrap_or(usize::MAX);
            let size = usize::try_from(plane.chunk().size()).unwrap_or(usize::MAX);
            let Some(bytes) = plane.data() else {
                return;
            };
            let Some(end) = offset.checked_add(size) else {
                report_fatal(
                    data,
                    "PipeWire returned an overflowing buffer range".to_owned(),
                );
                return;
            };
            let Some(bytes) = bytes.get(offset..end) else {
                report_fatal(
                    data,
                    "PipeWire returned an out-of-bounds buffer range".to_owned(),
                );
                return;
            };
            let chunk = match decode_f32le(bytes, format) {
                Ok(chunk) => chunk,
                Err(error) => {
                    report_fatal(data, format!("PipeWire produced invalid PCM: {error}"));
                    return;
                }
            };
            match data.events.try_send(WorkerEvent::Audio(chunk)) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => data
                    .dropped_chunks
                    .set(data.dropped_chunks.get().saturating_add(1)),
                Err(TrySendError::Disconnected(_)) => {
                    if let Some(mainloop) = data.mainloop.upgrade() {
                        mainloop.quit();
                    }
                }
            }
        })
        .register()
        .map_err(pipewire_error)?;

    let mut audio_info = AudioInfoRaw::new();
    audio_info.set_format(AudioFormat::F32LE);
    let object = spa::pod::Object {
        type_: spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: spa::param::ParamType::EnumFormat.as_raw(),
        properties: audio_info.into(),
    };
    let values = spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(object),
    )
    .map_err(|error| PipeWireError::PipeWire(format!("failed to serialize audio format: {error}")))?
    .0
    .into_inner();
    let mut parameters = [Pod::from_bytes(&values).ok_or_else(|| {
        PipeWireError::PipeWire("serialized audio format was not a valid SPA pod".to_owned())
    })?];
    stream
        .connect(
            spa::utils::Direction::Input,
            None,
            pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
            &mut parameters,
        )
        .map_err(pipewire_error)?;

    mainloop.run();
    let dropped_chunks = dropped_chunks.get();
    if dropped_chunks > 0 {
        warn!(
            dropped_chunks,
            "dropped PipeWire audio chunks because the bounded queue was full"
        );
    }
    Ok(())
}

fn report_fatal(data: &mut WorkerData, message: String) {
    if let Some(startup) = data.startup.take() {
        let _ = startup.try_send(Err(message.clone()));
    }
    let _ = data.events.try_send(WorkerEvent::Failure(message));
    if let Some(mainloop) = data.mainloop.upgrade() {
        mainloop.quit();
    }
}

fn decode_f32le(bytes: &[u8], format: NegotiatedFormat) -> Result<AudioChunk, String> {
    if bytes.len() % mem::size_of::<f32>() != 0 {
        return Err("PipeWire returned a partial F32 sample".to_owned());
    }
    let samples = bytes
        .chunks_exact(mem::size_of::<f32>())
        .map(|sample| f32::from_le_bytes([sample[0], sample[1], sample[2], sample[3]]))
        .collect();
    AudioChunk::new(samples, format.sample_rate_hz, format.channels)
        .map_err(|error| format!("PipeWire produced invalid PCM: {error}"))
}

fn pipewire_error(error: pw::Error) -> PipeWireError {
    PipeWireError::PipeWire(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{NegotiatedFormat, PipeWireCaptureConfig, decode_f32le};
    use crate::PipeWireError;

    #[test]
    fn config_rejects_an_unbounded_zero_capacity_queue() {
        let config = PipeWireCaptureConfig {
            queue_capacity: 0,
            ..PipeWireCaptureConfig::default()
        };
        assert_eq!(
            config.validate(),
            Err(PipeWireError::InvalidConfiguration(
                "queue capacity must be greater than zero"
            ))
        );
    }

    #[test]
    fn config_rejects_zero_startup_timeout() {
        let config = PipeWireCaptureConfig {
            startup_timeout: Duration::ZERO,
            ..PipeWireCaptureConfig::default()
        };
        assert_eq!(
            config.validate(),
            Err(PipeWireError::InvalidConfiguration(
                "startup timeout must be greater than zero"
            ))
        );
    }

    #[test]
    fn decoder_preserves_f32le_samples_and_format() {
        let bytes = [0.25_f32.to_le_bytes(), (-0.5_f32).to_le_bytes()].concat();
        let chunk = decode_f32le(
            &bytes,
            NegotiatedFormat {
                sample_rate_hz: 48_000,
                channels: 1,
            },
        )
        .unwrap();

        assert_eq!(chunk.samples(), &[0.25, -0.5]);
        assert_eq!(chunk.sample_rate_hz(), 48_000);
        assert_eq!(chunk.channels(), 1);
    }

    #[test]
    fn decoder_rejects_partial_f32_sample() {
        let error = decode_f32le(
            &[0, 1, 2],
            NegotiatedFormat {
                sample_rate_hz: 48_000,
                channels: 1,
            },
        )
        .unwrap_err();

        assert_eq!(error, "PipeWire returned a partial F32 sample");
    }
}
