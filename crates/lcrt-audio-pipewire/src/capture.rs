use std::{
    borrow::Cow,
    cell::Cell,
    mem,
    rc::Rc,
    sync::mpsc::{
        Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError, sync_channel,
    },
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
    /// Maximum time to wait for the worker to stop after capture is stopped.
    pub shutdown_timeout: Duration,
}

impl Default for PipeWireCaptureConfig {
    fn default() -> Self {
        Self {
            queue_capacity: 8,
            startup_timeout: Duration::from_secs(5),
            shutdown_timeout: Duration::from_secs(5),
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
        if self.shutdown_timeout.is_zero() {
            return Err(PipeWireError::InvalidConfiguration(
                "shutdown timeout must be greater than zero",
            ));
        }
        Ok(())
    }
}

enum AudioEvent {
    Audio(AudioChunk),
}

enum ControlEvent {
    Failure(String),
    Stopped,
}

/// A live PipeWire session implementing the portable [`AudioCapture`] port.
pub struct PipeWireCapture {
    source: AudioSourceDescriptor,
    audio_events: Receiver<AudioEvent>,
    control_events: Receiver<ControlEvent>,
    stop_sender: Option<pw::channel::Sender<()>>,
    worker_done: Receiver<()>,
    worker: Option<JoinHandle<Result<(), PipeWireError>>>,
    shutdown_timeout: Duration,
    stopped: bool,
    terminal: bool,
}

impl PipeWireCapture {
    /// Starts capture from an enumerated microphone or system-output source.
    pub fn start(
        source: AudioSourceDescriptor,
        config: PipeWireCaptureConfig,
    ) -> Result<Self, PipeWireError> {
        config.validate()?;
        let (audio_sender, audio_events) = sync_channel(config.queue_capacity);
        // Control events have their own one-item bounded channel. Audio may be
        // dropped under backpressure, but a terminal failure can never be
        // displaced by queued PCM.
        let (control_sender, control_events) = sync_channel(1);
        let (startup_sender, startup_receiver) = sync_channel(1);
        let (worker_done_sender, worker_done) = sync_channel(1);
        let (stop_sender, stop_receiver) = pw::channel::channel();
        let worker_source = source.clone();
        let worker = thread::Builder::new()
            .name("lcrt-pipewire-capture".to_owned())
            .spawn(move || {
                let result = run_worker(
                    worker_source,
                    audio_sender,
                    control_sender.clone(),
                    startup_sender.clone(),
                    stop_receiver,
                );
                if let Err(error) = &result {
                    let message = error.to_string();
                    let _ = startup_sender.try_send(Err(message.clone()));
                    let _ = control_sender.try_send(ControlEvent::Failure(message));
                }
                let _ = worker_done_sender.try_send(());
                result
            })
            .map_err(|error| PipeWireError::Worker(error.to_string()))?;

        match startup_receiver.recv_timeout(config.startup_timeout) {
            Ok(Ok(())) => {
                info!(source_id = source.id(), "PipeWire capture streaming");
                Ok(Self {
                    source,
                    audio_events,
                    control_events,
                    stop_sender: Some(stop_sender),
                    worker_done,
                    worker: Some(worker),
                    shutdown_timeout: config.shutdown_timeout,
                    stopped: false,
                    terminal: false,
                })
            }
            Ok(Err(message)) => {
                let _ = stop_sender.send(());
                wait_for_worker(&worker_done, config.shutdown_timeout);
                Err(PipeWireError::PipeWire(message))
            }
            Err(RecvTimeoutError::Timeout) => {
                let _ = stop_sender.send(());
                wait_for_worker(&worker_done, config.shutdown_timeout);
                Err(PipeWireError::StartupTimeout(config.startup_timeout))
            }
            Err(RecvTimeoutError::Disconnected) => {
                wait_for_worker(&worker_done, config.shutdown_timeout);
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
        match self.worker_done.recv_timeout(self.shutdown_timeout) {
            Ok(()) => {}
            Err(RecvTimeoutError::Timeout) => {
                drop(worker);
                return Err(AudioCaptureError::new(format!(
                    "PipeWire capture worker did not stop within {:?}",
                    self.shutdown_timeout
                )));
            }
            Err(RecvTimeoutError::Disconnected) => {
                // The worker may have panicked before it could acknowledge
                // completion; joining now is non-blocking because its sender
                // has been dropped.
            }
        }
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
        if self.stopped || self.terminal {
            return Ok(AudioInputEvent::EndOfStream);
        }
        match self.control_events.try_recv() {
            Ok(ControlEvent::Failure(message)) => {
                self.terminal = true;
                return Err(AudioCaptureError::new(message));
            }
            Ok(ControlEvent::Stopped) => {
                self.terminal = true;
                return Err(AudioCaptureError::new(
                    "PipeWire capture stopped before the application requested shutdown",
                ));
            }
            Err(TryRecvError::Disconnected) => {
                return Err(AudioCaptureError::new(
                    "PipeWire capture control channel disconnected unexpectedly",
                ));
            }
            Err(TryRecvError::Empty) => {}
        }
        match self.audio_events.recv_timeout(timeout) {
            Ok(AudioEvent::Audio(chunk)) => Ok(AudioInputEvent::Chunk(chunk)),
            Err(RecvTimeoutError::Timeout) => Ok(AudioInputEvent::Idle),
            Err(RecvTimeoutError::Disconnected) => Err(AudioCaptureError::new(
                "PipeWire capture audio channel disconnected unexpectedly",
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
    audio_events: SyncSender<AudioEvent>,
    startup: Option<SyncSender<Result<(), String>>>,
    mainloop: pw::main_loop::MainLoopWeak,
    dropped_chunks: Rc<Cell<u64>>,
    terminal_sent: Rc<Cell<bool>>,
    control_events: SyncSender<ControlEvent>,
    stop_requested: Rc<Cell<bool>>,
}

#[derive(Clone, Copy)]
struct NegotiatedFormat {
    sample_rate_hz: u32,
    channels: u16,
}

fn run_worker(
    source: AudioSourceDescriptor,
    audio_events: SyncSender<AudioEvent>,
    control_events: SyncSender<ControlEvent>,
    startup: SyncSender<Result<(), String>>,
    stop_receiver: pw::channel::Receiver<()>,
) -> Result<(), PipeWireError> {
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None).map_err(pipewire_error)?;
    let context = pw::context::ContextRc::new(&mainloop, None).map_err(pipewire_error)?;
    let core = context.connect_rc(None).map_err(pipewire_error)?;
    let stop_requested = Rc::new(Cell::new(false));
    let _stop_listener = stop_receiver.attach(mainloop.loop_(), {
        let mainloop = mainloop.clone();
        let stop_requested = Rc::clone(&stop_requested);
        move |_| {
            stop_requested.set(true);
            mainloop.quit();
        }
    });

    let stream_properties = stream_properties(&source);
    let stream = pw::stream::StreamBox::new(&core, "lcrt-audio-capture", stream_properties)
        .map_err(pipewire_error)?;
    let dropped_chunks = Rc::new(Cell::new(0));
    let terminal_sent = Rc::new(Cell::new(false));
    let worker_data = WorkerData {
        format: None,
        audio_events,
        startup: Some(startup),
        mainloop: mainloop.downgrade(),
        dropped_chunks: Rc::clone(&dropped_chunks),
        terminal_sent: Rc::clone(&terminal_sent),
        control_events: control_events.clone(),
        stop_requested: Rc::clone(&stop_requested),
    };

    let _listener = stream
        .add_local_listener_with_user_data(worker_data)
        .state_changed(|_, data, old_state, new_state| match new_state {
            pw::stream::StreamState::Streaming => {
                if let Some(startup) = data.startup.take() {
                    let _ = startup.try_send(Ok(()));
                }
            }
            pw::stream::StreamState::Error(message) => {
                report_fatal(data, PipeWireError::StreamFailure(message));
            }
            pw::stream::StreamState::Unconnected
                if !data.stop_requested.get()
                    && !matches!(old_state, pw::stream::StreamState::Unconnected) =>
            {
                report_fatal(data, PipeWireError::SelectedSourceUnavailable);
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
                        PipeWireError::InvalidNegotiatedFormat(format!(
                            "failed to identify the format: {error}"
                        )),
                    );
                    return;
                }
            };
            if media_type != MediaType::Audio || subtype != MediaSubtype::Raw {
                report_fatal(
                    data,
                    PipeWireError::InvalidNegotiatedFormat("expected raw audio".to_owned()),
                );
                return;
            }
            let mut info = AudioInfoRaw::new();
            if let Err(error) = info.parse(parameter) {
                report_fatal(
                    data,
                    PipeWireError::InvalidNegotiatedFormat(format!(
                        "failed to parse the format: {error}"
                    )),
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
                        PipeWireError::InvalidNegotiatedFormat(
                            "channel count or sample rate was zero or too large".to_owned(),
                        ),
                    ),
                },
                _ => report_fatal(
                    data,
                    PipeWireError::InvalidNegotiatedFormat(
                        "only interleaved F32LE PCM is supported".to_owned(),
                    ),
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
            let offset = plane.chunk().offset();
            let size = plane.chunk().size();
            let flags = plane.chunk().flags().bits();
            let Some(bytes) = plane.data() else {
                report_fatal(
                    data,
                    PipeWireError::MalformedBuffer("mapped PCM data was unavailable".to_owned()),
                );
                return;
            };
            let bytes = match normalized_chunk_bytes(bytes, offset, size, flags) {
                Ok(Some(bytes)) => bytes,
                // SPA_CHUNK_FLAG_EMPTY has no reliable payload duration in this
                // adapter. Skipping it avoids decoding stale mapped storage as
                // audio while preserving bounded realtime behavior.
                Ok(None) => return,
                Err(error) => {
                    report_fatal(data, PipeWireError::MalformedBuffer(error));
                    return;
                }
            };
            let chunk = match decode_f32le(&bytes, format) {
                Ok(chunk) => chunk,
                Err(error) => {
                    report_fatal(data, PipeWireError::MalformedBuffer(error));
                    return;
                }
            };
            match data.audio_events.try_send(AudioEvent::Audio(chunk)) {
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
            capture_stream_flags(),
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
    if !terminal_sent.get() && !stop_requested.get() {
        let _ = control_events.try_send(ControlEvent::Stopped);
    }
    Ok(())
}

fn report_fatal(data: &mut WorkerData, error: PipeWireError) {
    if let Some(startup) = data.startup.take() {
        let _ = startup.try_send(Err(error.to_string()));
    }
    try_send_terminal_failure(&data.control_events, &data.terminal_sent, error.to_string());
    if let Some(mainloop) = data.mainloop.upgrade() {
        mainloop.quit();
    }
}

fn try_send_terminal_failure(
    control_events: &SyncSender<ControlEvent>,
    terminal_sent: &Cell<bool>,
    message: String,
) {
    if !terminal_sent.replace(true) {
        // This control channel is distinct from the lossy bounded audio queue.
        // A single worker emits at most one terminal event, so its capacity is
        // sufficient without blocking a PipeWire callback.
        let _ = control_events.try_send(ControlEvent::Failure(message));
    }
}

fn stream_properties(source: &AudioSourceDescriptor) -> pw::properties::PropertiesBox {
    let mut properties = properties! {
        *pw::keys::APP_NAME => "LCRT",
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Capture",
        *pw::keys::MEDIA_ROLE => "Communication",
        *pw::keys::TARGET_OBJECT => source.id(),
        // PipeWire documents this as destroying the node when its target is
        // removed instead of reconnecting it to a default compatible source.
        *pw::keys::NODE_DONT_RECONNECT => "true",
    };
    if source.kind() == AudioSourceKind::SystemOutput {
        properties.insert(*pw::keys::STREAM_CAPTURE_SINK, "true");
    }
    properties
}

fn capture_stream_flags() -> pw::stream::StreamFlags {
    pw::stream::StreamFlags::AUTOCONNECT
        | pw::stream::StreamFlags::DONT_RECONNECT
        | pw::stream::StreamFlags::MAP_BUFFERS
}

fn wait_for_worker(worker_done: &Receiver<()>, timeout: Duration) {
    // A failed startup must not turn an externally bounded start call into an
    // unbounded join. Once the completion signal arrives, dropping the handle
    // is safe because the worker has already exited.
    let _ = worker_done.recv_timeout(timeout);
}

const SPA_CHUNK_FLAG_EMPTY: i32 = 1 << 1;

fn normalized_chunk_bytes(
    backing: &[u8],
    offset: u32,
    size: u32,
    flags: i32,
) -> Result<Option<Cow<'_, [u8]>>, String> {
    if flags & spa::buffer::ChunkFlags::CORRUPTED.bits() != 0 {
        return Err("PipeWire marked the chunk as corrupted".to_owned());
    }
    // libspa 0.10 exposes raw chunk flags but does not name EMPTY. The
    // supported SPA headers define bit 1 as SPA_CHUNK_FLAG_EMPTY: its backing
    // bytes are media-specific neutral data and must not be decoded as PCM.
    if flags & SPA_CHUNK_FLAG_EMPTY != 0 || size == 0 {
        return Ok(None);
    }
    if backing.is_empty() {
        return Err("chunk contains data but its mapped backing buffer is empty".to_owned());
    }

    let backing_len = backing.len();
    let offset = usize::try_from(offset)
        .map_err(|_| "chunk offset cannot be represented on this platform".to_owned())?;
    let size = usize::try_from(size)
        .map_err(|_| "chunk size cannot be represented on this platform".to_owned())?;
    if size > backing_len {
        return Err(format!(
            "chunk size {size} exceeds mapped backing buffer size {backing_len}"
        ));
    }

    // SPA defines offset modulo data.maxsize. `Data::data` exposes precisely
    // that mapped max-size range, so normalize before forming a Rust slice.
    let start = offset % backing_len;
    let end = start
        .checked_add(size)
        .ok_or_else(|| "normalized chunk range overflowed".to_owned())?;
    if end <= backing_len {
        return Ok(Some(Cow::Borrowed(&backing[start..end])));
    }

    let wrapped_end = end - backing_len;
    let mut normalized = Vec::with_capacity(size);
    normalized.extend_from_slice(&backing[start..]);
    normalized.extend_from_slice(&backing[..wrapped_end]);
    Ok(Some(Cow::Owned(normalized)))
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
    use std::{borrow::Cow, cell::Cell, sync::mpsc::sync_channel, time::Duration};

    use super::{
        AudioEvent, ControlEvent, NegotiatedFormat, PipeWireCaptureConfig, SPA_CHUNK_FLAG_EMPTY,
        capture_stream_flags, decode_f32le, normalized_chunk_bytes, stream_properties,
        try_send_terminal_failure,
    };
    use crate::PipeWireError;
    use lcrt_core::{AudioChunk, AudioSourceDescriptor, AudioSourceKind};

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
    fn config_rejects_zero_shutdown_timeout() {
        let config = PipeWireCaptureConfig {
            shutdown_timeout: Duration::ZERO,
            ..PipeWireCaptureConfig::default()
        };
        assert_eq!(
            config.validate(),
            Err(PipeWireError::InvalidConfiguration(
                "shutdown timeout must be greater than zero"
            ))
        );
    }

    #[test]
    fn selected_source_is_pinned_and_never_reconnected() {
        let source = AudioSourceDescriptor::new(
            "alsa_input.test",
            "Test microphone",
            AudioSourceKind::Microphone,
        );
        let properties = stream_properties(&source);

        assert_eq!(
            properties.get(*pipewire::keys::TARGET_OBJECT),
            Some("alsa_input.test")
        );
        assert_eq!(
            properties.get(*pipewire::keys::NODE_DONT_RECONNECT),
            Some("true")
        );
        assert!(capture_stream_flags().contains(pipewire::stream::StreamFlags::DONT_RECONNECT));
        assert!(
            PipeWireError::SelectedSourceUnavailable
                .to_string()
                .contains("select another source")
        );
    }

    #[test]
    fn normalizes_a_contiguous_chunk_range() {
        let bytes = normalized_chunk_bytes(&[0, 1, 2, 3], 1, 2, 0).unwrap();
        assert_eq!(bytes, Some(Cow::Borrowed(&[1, 2][..])));
    }

    #[test]
    fn normalizes_offset_and_reconstructs_wrapped_chunk_range() {
        let bytes = normalized_chunk_bytes(&[0, 1, 2, 3], 7, 3, 0).unwrap();
        assert_eq!(bytes, Some(Cow::Owned(vec![3, 0, 1])));
    }

    #[test]
    fn skips_zero_length_and_empty_chunks_without_reading_backing_storage() {
        assert_eq!(normalized_chunk_bytes(&[], 0, 0, 0).unwrap(), None);
        assert_eq!(
            normalized_chunk_bytes(
                &[255, 255, 255, 255],
                u32::MAX,
                u32::MAX,
                SPA_CHUNK_FLAG_EMPTY
            )
            .unwrap(),
            None
        );
    }

    #[test]
    fn rejects_oversized_and_unusable_chunk_ranges() {
        assert_eq!(
            normalized_chunk_bytes(&[0, 1, 2, 3], 0, 5, 0),
            Err("chunk size 5 exceeds mapped backing buffer size 4".to_owned())
        );
        assert_eq!(
            normalized_chunk_bytes(&[], 0, 1, 0),
            Err("chunk contains data but its mapped backing buffer is empty".to_owned())
        );
    }

    #[test]
    fn rejects_chunks_marked_corrupted() {
        assert_eq!(
            normalized_chunk_bytes(
                &[0, 1, 2, 3],
                0,
                4,
                pipewire::spa::buffer::ChunkFlags::CORRUPTED.bits(),
            ),
            Err("PipeWire marked the chunk as corrupted".to_owned())
        );
    }

    #[test]
    fn fatal_control_event_survives_a_full_audio_queue() {
        let (audio_sender, _audio_receiver) = sync_channel(1);
        let (control_sender, control_receiver) = sync_channel(1);
        audio_sender
            .try_send(AudioEvent::Audio(
                AudioChunk::new(vec![0.0], 48_000, 1).unwrap(),
            ))
            .unwrap();
        control_sender
            .try_send(ControlEvent::Failure(
                "selected source disconnected".to_owned(),
            ))
            .unwrap();

        match control_receiver.try_recv().unwrap() {
            ControlEvent::Failure(message) => assert_eq!(message, "selected source disconnected"),
            ControlEvent::Stopped => panic!("expected fatal failure"),
        }
    }

    #[test]
    fn terminal_failure_is_emitted_exactly_once() {
        let (control_sender, control_receiver) = sync_channel(1);
        let terminal_sent = Cell::new(false);

        try_send_terminal_failure(&control_sender, &terminal_sent, "first failure".to_owned());
        try_send_terminal_failure(&control_sender, &terminal_sent, "second failure".to_owned());

        match control_receiver.try_recv().unwrap() {
            ControlEvent::Failure(message) => assert_eq!(message, "first failure"),
            ControlEvent::Stopped => panic!("expected fatal failure"),
        }
        assert!(control_receiver.try_recv().is_err());
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
