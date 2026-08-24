use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError, sync_channel},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use lcrt_core::{AudioChunk, Transcriber, TranscriptUpdate, TranscriptionError};
use tracing::{debug, info};
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState,
};

use crate::{
    WhisperBackendError, WhisperConfig,
    resample::AudioConverter,
    window::{InferenceKind, StreamingWindow},
};

enum WorkerCommand {
    Audio(AudioChunk),
    Finish,
}

enum WorkerEvent {
    Update(TranscriptUpdate),
    Failure(String),
    Done,
}

/// A non-blocking [`Transcriber`] adapter backed by a dedicated whisper.cpp worker.
pub struct WhisperTranscriber {
    commands: Option<SyncSender<WorkerCommand>>,
    events: Receiver<WorkerEvent>,
    worker: Option<JoinHandle<Result<(), WhisperBackendError>>>,
    cancel: Arc<AtomicBool>,
    input_queue_capacity: usize,
    finish_timeout: Duration,
    finished: bool,
}

impl WhisperTranscriber {
    /// Loads a local model on a worker thread and waits for bounded readiness.
    pub fn new(config: WhisperConfig) -> Result<Self, WhisperBackendError> {
        config.validate()?;
        let input_queue_capacity = config.input_queue_capacity;
        let finish_timeout = config.finish_timeout;
        let startup_timeout = config.startup_timeout;
        let (commands, command_receiver) = sync_channel(input_queue_capacity);
        let (event_sender, events) = sync_channel(8);
        let (startup_sender, startup_receiver) = sync_channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        let worker = thread::Builder::new()
            .name("lcrt-whisper-stt".to_owned())
            .spawn(move || {
                let result = run_worker(
                    config,
                    command_receiver,
                    event_sender.clone(),
                    startup_sender.clone(),
                    worker_cancel,
                );
                if let Err(error) = &result {
                    let message = error.to_string();
                    let _ = startup_sender.try_send(Err(message.clone()));
                    let _ = event_sender.send(WorkerEvent::Failure(message));
                }
                let _ = event_sender.send(WorkerEvent::Done);
                result
            })
            .map_err(|error| WhisperBackendError::Worker(error.to_string()))?;

        match startup_receiver.recv_timeout(startup_timeout) {
            Ok(Ok(())) => Ok(Self {
                commands: Some(commands),
                events,
                worker: Some(worker),
                cancel,
                input_queue_capacity,
                finish_timeout,
                finished: false,
            }),
            Ok(Err(message)) => {
                drop(commands);
                let _ = worker.join();
                Err(WhisperBackendError::Whisper(message))
            }
            Err(RecvTimeoutError::Timeout) => {
                cancel.store(true, Ordering::Release);
                drop(commands);
                drop(worker);
                Err(WhisperBackendError::StartupTimeout(startup_timeout))
            }
            Err(RecvTimeoutError::Disconnected) => {
                drop(commands);
                let _ = worker.join();
                Err(WhisperBackendError::Worker(
                    "worker stopped while loading the model".to_owned(),
                ))
            }
        }
    }

    fn collect_available(&mut self) -> Result<Vec<TranscriptUpdate>, WhisperBackendError> {
        let mut updates = Vec::new();
        loop {
            match self.events.try_recv() {
                Ok(WorkerEvent::Update(update)) => updates.push(update),
                Ok(WorkerEvent::Failure(message)) => {
                    self.finished = true;
                    self.join_worker()?;
                    return Err(WhisperBackendError::Whisper(message));
                }
                Ok(WorkerEvent::Done) => {
                    self.finished = true;
                    self.join_worker()?;
                    break;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) if self.finished => break,
                Err(TryRecvError::Disconnected) => {
                    return Err(WhisperBackendError::Worker(
                        "result channel disconnected unexpectedly".to_owned(),
                    ));
                }
            }
        }
        Ok(updates)
    }

    fn join_worker(&mut self) -> Result<(), WhisperBackendError> {
        let Some(worker) = self.worker.take() else {
            return Ok(());
        };
        match worker.join() {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(error),
            Err(_) => Err(WhisperBackendError::Worker(
                "worker panicked during shutdown".to_owned(),
            )),
        }
    }

    fn send_finish_until(&mut self, deadline: Instant) -> Result<(), WhisperBackendError> {
        let Some(sender) = self.commands.as_ref() else {
            return Ok(());
        };
        let mut command = WorkerCommand::Finish;
        loop {
            match sender.try_send(command) {
                Ok(()) => return Ok(()),
                Err(TrySendError::Full(returned)) if Instant::now() < deadline => {
                    command = returned;
                    thread::sleep(Duration::from_millis(5));
                }
                Err(TrySendError::Full(_)) => {
                    return Err(WhisperBackendError::FinishTimeout(self.finish_timeout));
                }
                Err(TrySendError::Disconnected(_)) => {
                    return Err(WhisperBackendError::Worker(
                        "command channel disconnected before flush".to_owned(),
                    ));
                }
            }
        }
    }

    fn finish_worker(&mut self) -> Result<Vec<TranscriptUpdate>, WhisperBackendError> {
        if self.finished {
            let updates = self.collect_available()?;
            self.join_worker()?;
            return Ok(updates);
        }
        let deadline = Instant::now() + self.finish_timeout;
        self.send_finish_until(deadline)?;
        self.commands.take();
        let mut updates = Vec::new();
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(WhisperBackendError::FinishTimeout(self.finish_timeout));
            }
            match self.events.recv_timeout(remaining) {
                Ok(WorkerEvent::Update(update)) => updates.push(update),
                Ok(WorkerEvent::Failure(message)) => {
                    self.finished = true;
                    self.join_worker()?;
                    return Err(WhisperBackendError::Whisper(message));
                }
                Ok(WorkerEvent::Done) => {
                    self.finished = true;
                    break;
                }
                Err(RecvTimeoutError::Timeout) => {
                    return Err(WhisperBackendError::FinishTimeout(self.finish_timeout));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(WhisperBackendError::Worker(
                        "result channel disconnected before flush completed".to_owned(),
                    ));
                }
            }
        }
        self.join_worker()?;
        Ok(updates)
    }
}

impl Transcriber for WhisperTranscriber {
    fn push_audio(
        &mut self,
        chunk: &AudioChunk,
    ) -> Result<Vec<TranscriptUpdate>, TranscriptionError> {
        if self.finished {
            return Err(TranscriptionError::new(
                "Whisper transcriber received audio after it was finished",
            ));
        }
        let Some(commands) = self.commands.as_ref() else {
            return Err(TranscriptionError::new(
                "Whisper worker command channel is unavailable",
            ));
        };
        match commands.try_send(WorkerCommand::Audio(chunk.clone())) {
            Ok(()) => self
                .collect_available()
                .map_err(|error| TranscriptionError::new(error.to_string())),
            Err(TrySendError::Full(_)) => Err(TranscriptionError::new(
                WhisperBackendError::InputQueueFull(self.input_queue_capacity).to_string(),
            )),
            Err(TrySendError::Disconnected(_)) => Err(TranscriptionError::new(
                "Whisper worker command channel disconnected unexpectedly",
            )),
        }
    }

    fn finish(&mut self) -> Result<Vec<TranscriptUpdate>, TranscriptionError> {
        self.finish_worker()
            .map_err(|error| TranscriptionError::new(error.to_string()))
    }
}

impl Drop for WhisperTranscriber {
    fn drop(&mut self) {
        if self.finished {
            let _ = self.join_worker();
            return;
        }
        self.cancel.store(true, Ordering::Release);
        self.commands.take();
        if self.worker.as_ref().is_some_and(JoinHandle::is_finished) {
            let Some(worker) = self.worker.take() else {
                return;
            };
            let _ = worker.join();
        }
    }
}

fn run_worker(
    config: WhisperConfig,
    commands: Receiver<WorkerCommand>,
    events: SyncSender<WorkerEvent>,
    startup: SyncSender<Result<(), String>>,
    cancel: Arc<AtomicBool>,
) -> Result<(), WhisperBackendError> {
    whisper_rs::install_logging_hooks();
    let model_path = config.model_path.to_str().ok_or_else(|| {
        WhisperBackendError::InvalidConfiguration(
            "whisper-rs 0.15 requires a UTF-8 model path".to_owned(),
        )
    })?;
    let context = WhisperContext::new_with_params(model_path, WhisperContextParameters::default())
        .map_err(|error| WhisperBackendError::Whisper(error.to_string()))?;
    let mut state = context
        .create_state()
        .map_err(|error| WhisperBackendError::Whisper(error.to_string()))?;
    let mut window = StreamingWindow::new(&config)?;
    let mut converter = None;
    let mut last_partial = String::new();
    startup
        .send(Ok(()))
        .map_err(|_| WhisperBackendError::Worker("startup receiver disconnected".to_owned()))?;
    info!(model_path = %config.model_path.display(), "local Whisper model loaded");

    while let Ok(command) = commands.recv() {
        if cancel.load(Ordering::Acquire) {
            return Ok(());
        }
        match command {
            WorkerCommand::Audio(chunk) => {
                let converter = match converter.as_mut() {
                    Some(converter) => converter,
                    None => converter.insert(AudioConverter::new(&chunk)?),
                };
                let mono = converter.push(&chunk)?;
                process_mono(
                    &mono,
                    &mut window,
                    &mut state,
                    &config,
                    &events,
                    &mut last_partial,
                )?;
            }
            WorkerCommand::Finish => {
                if let Some(converter) = converter.as_mut() {
                    let tail = converter.finish()?;
                    process_mono(
                        &tail,
                        &mut window,
                        &mut state,
                        &config,
                        &events,
                        &mut last_partial,
                    )?;
                }
                if let Some(kind) = window.finish_kind() {
                    infer_and_publish(
                        kind,
                        &mut window,
                        &mut state,
                        &config,
                        &events,
                        &mut last_partial,
                    )?;
                }
                return Ok(());
            }
        }
    }
    Ok(())
}

fn process_mono(
    mono: &[f32],
    window: &mut StreamingWindow,
    state: &mut WhisperState,
    config: &WhisperConfig,
    events: &SyncSender<WorkerEvent>,
    last_partial: &mut String,
) -> Result<(), WhisperBackendError> {
    if let Some(kind) = window.push(mono) {
        infer_and_publish(kind, window, state, config, events, last_partial)?;
    }
    Ok(())
}

fn infer_and_publish(
    kind: InferenceKind,
    window: &mut StreamingWindow,
    state: &mut WhisperState,
    config: &WhisperConfig,
    events: &SyncSender<WorkerEvent>,
    last_partial: &mut String,
) -> Result<(), WhisperBackendError> {
    let started = Instant::now();
    let text = transcribe_window(state, window.samples(), config)?;
    debug!(
        ?kind,
        audio_samples = window.samples().len(),
        inference_ms = started.elapsed().as_millis(),
        "Whisper inference completed"
    );
    window.mark_inferred(kind);
    if text.is_empty() {
        if kind == InferenceKind::Final {
            last_partial.clear();
        }
        return Ok(());
    }
    if kind == InferenceKind::Partial && text == *last_partial {
        return Ok(());
    }
    let update = match kind {
        InferenceKind::Partial => {
            *last_partial = text.clone();
            TranscriptUpdate::partial(text)
        }
        InferenceKind::Final => {
            last_partial.clear();
            TranscriptUpdate::finalized(text)
        }
    }
    .map_err(|error| WhisperBackendError::Whisper(error.to_string()))?;
    events
        .send(WorkerEvent::Update(update))
        .map_err(|_| WhisperBackendError::Worker("result receiver disconnected".to_owned()))
}

fn transcribe_window(
    state: &mut WhisperState,
    samples: &[f32],
    config: &WhisperConfig,
) -> Result<String, WhisperBackendError> {
    let mut parameters = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    parameters.set_n_threads(i32::from(config.inference_threads));
    parameters.set_language(config.language.as_deref());
    parameters.set_translate(false);
    parameters.set_no_context(true);
    parameters.set_no_timestamps(true);
    parameters.set_print_special(false);
    parameters.set_print_progress(false);
    parameters.set_print_realtime(false);
    parameters.set_print_timestamps(false);
    parameters.set_suppress_blank(true);
    parameters.set_suppress_nst(true);
    state
        .full(parameters, samples)
        .map_err(|error| WhisperBackendError::Whisper(error.to_string()))?;

    let mut text = String::new();
    for segment in state.as_iter() {
        let segment = segment
            .to_str_lossy()
            .map_err(|error| WhisperBackendError::Whisper(error.to_string()))?;
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(segment);
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::WhisperTranscriber;
    use crate::{WhisperBackendError, WhisperConfig};

    #[test]
    fn missing_model_fails_before_worker_start() {
        let config = WhisperConfig::new(PathBuf::from("/definitely/missing/lcrt-model.bin"));

        let error = match WhisperTranscriber::new(config) {
            Ok(_) => panic!("missing model unexpectedly loaded"),
            Err(error) => error,
        };
        assert_eq!(
            error,
            WhisperBackendError::ModelUnavailable(PathBuf::from(
                "/definitely/missing/lcrt-model.bin"
            ))
        );
    }
}
