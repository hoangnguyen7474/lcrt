use std::{
    env,
    ffi::OsString,
    path::PathBuf,
    process::ExitCode,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, RecvTimeoutError, SyncSender, TryRecvError, sync_channel},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use lcrt_audio_pipewire::{PipeWireCapture, PipeWireCaptureConfig, enumerate_audio_sources};
use lcrt_core::{AudioSourceDescriptor, CaptionPipeline, RunSummary, RuntimeConfig};
use lcrt_stt_whisper::{WhisperConfig, WhisperTranscriber};
use lcrt_ui_gtk::{CaptionUiAction, CaptionUiOptions, GtkCaptionSink, run_caption_ui};
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

const SOURCE_ENUMERATION_TIMEOUT: Duration = Duration::from_secs(3);
const CONTROLLER_POLL_INTERVAL: Duration = Duration::from_millis(50);
const UI_EVENT_CAPACITY: usize = 64;
const UI_ACTION_CAPACITY: usize = 8;

#[derive(Clone, Debug, Eq, PartialEq)]
struct AppConfig {
    model_path: Option<PathBuf>,
    language: Option<String>,
    list_sources: bool,
    smoke: Option<SmokeConfig>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SmokeConfig {
    source_id: String,
    duration: Duration,
}

enum ParsedCommand {
    Run(AppConfig),
    Help,
}

struct PipelineSession {
    cancelled: Arc<AtomicBool>,
    result: Receiver<Result<RunSummary, String>>,
    worker: JoinHandle<()>,
}

fn main() -> ExitCode {
    configure_logging();
    let command = match parse_arguments(env::args_os().skip(1)) {
        Ok(command) => command,
        Err(message) => {
            eprintln!("error: {message}\n\n{}", usage());
            return ExitCode::FAILURE;
        }
    };
    let ParsedCommand::Run(config) = command else {
        println!("{}", usage());
        return ExitCode::SUCCESS;
    };

    let sources = match enumerate_audio_sources(SOURCE_ENUMERATION_TIMEOUT) {
        Ok(sources) => sources,
        Err(error) if config.list_sources => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
        Err(error) => {
            return run_ui_with_startup_error(
                config,
                format!("Audio source discovery failed: {error}"),
            );
        }
    };
    if config.list_sources {
        for source in sources {
            println!("{:?}\t{}\t{}", source.kind(), source.id(), source.name());
        }
        return ExitCode::SUCCESS;
    }
    run_application(config, sources, None)
}

fn configure_logging() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(
            "lcrt=info,lcrt_core=info,lcrt_audio_pipewire=info,lcrt_stt_whisper=info,whisper_rs=warn",
        )
    });
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

fn run_ui_with_startup_error(config: AppConfig, message: String) -> ExitCode {
    run_application(config, Vec::new(), Some(message))
}

fn run_application(
    config: AppConfig,
    sources: Vec<AudioSourceDescriptor>,
    startup_error: Option<String>,
) -> ExitCode {
    let (sink, events) = match GtkCaptionSink::channel(UI_EVENT_CAPACITY) {
        Ok(channel) => channel,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    let mut initial_errors = Vec::new();
    if let Some(error) = startup_error {
        initial_errors.push(error);
    }
    if config.model_path.is_none() {
        initial_errors.push(
            "No local Whisper model is configured. Pass --model PATH or set LCRT_MODEL_PATH."
                .to_owned(),
        );
    }
    if !initial_errors.is_empty() {
        let _ = sink.show_error(initial_errors.join("\n"));
    }

    let (actions, action_receiver) = sync_channel(UI_ACTION_CAPACITY);
    let controller_sink = sink.clone();
    let controller_sources = sources.clone();
    let controller_config = config.clone();
    let controller = thread::Builder::new()
        .name("lcrt-application-controller".to_owned())
        .spawn(move || {
            run_controller(
                controller_config,
                controller_sources,
                controller_sink,
                action_receiver,
            )
        });
    let controller = match controller {
        Ok(controller) => controller,
        Err(error) => {
            let _ = sink.show_error(format!("Could not start the caption controller: {error}"));
            return ExitCode::FAILURE;
        }
    };

    if let Some(smoke) = config.smoke.clone() {
        spawn_smoke_actions(actions.clone(), smoke);
    }
    let options = CaptionUiOptions {
        sources,
        ..CaptionUiOptions::default()
    };
    let status = run_caption_ui(events, actions, options);
    let controller_ok = controller.join().is_ok();
    if status == gtk::glib::ExitCode::SUCCESS && controller_ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn run_controller(
    config: AppConfig,
    sources: Vec<AudioSourceDescriptor>,
    sink: GtkCaptionSink,
    actions: Receiver<CaptionUiAction>,
) {
    let mut session: Option<PipelineSession> = None;
    loop {
        if let Some(completed) = take_completed_session(&mut session) {
            publish_completion(&sink, completed);
            if config.smoke.is_some() {
                let _ = sink.quit();
                break;
            }
        }

        match actions.recv_timeout(CONTROLLER_POLL_INTERVAL) {
            Ok(CaptionUiAction::Start { source_id }) => {
                if session.is_some() {
                    let _ = sink.show_error("Captioning is already running.");
                    continue;
                }
                let Some(source) = sources
                    .iter()
                    .find(|source| source.id() == source_id)
                    .cloned()
                else {
                    let _ = sink.show_error("The selected PipeWire source is no longer available.");
                    if config.smoke.is_some() {
                        let _ = sink.quit();
                        break;
                    }
                    continue;
                };
                let Some(model_path) = config.model_path.clone() else {
                    let _ = sink.show_error(
                        "No local Whisper model is configured. Pass --model PATH or set LCRT_MODEL_PATH.",
                    );
                    if config.smoke.is_some() {
                        let _ = sink.quit();
                        break;
                    }
                    continue;
                };
                let _ = sink.clear_error();
                let _ = sink.set_running(true);
                let _ = sink.set_status("Loading model…");
                match start_pipeline(source, model_path, config.language.clone(), sink.clone()) {
                    Ok(started) => session = Some(started),
                    Err(message) => {
                        let _ = sink.set_running(false);
                        let _ = sink.set_status("Error");
                        let _ = sink.show_error(message);
                        if config.smoke.is_some() {
                            let _ = sink.quit();
                            break;
                        }
                    }
                }
            }
            Ok(CaptionUiAction::Stop) => {
                if let Some(session) = session.as_ref() {
                    session.cancelled.store(true, Ordering::Release);
                    let _ = sink.set_status("Stopping…");
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                if let Some(session) = session.take() {
                    session.cancelled.store(true, Ordering::Release);
                    let _ = session.worker.join();
                }
                break;
            }
        }
    }
}

fn start_pipeline(
    source: AudioSourceDescriptor,
    model_path: PathBuf,
    language: Option<String>,
    sink: GtkCaptionSink,
) -> Result<PipelineSession, String> {
    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_cancelled = Arc::clone(&cancelled);
    let (result_sender, result) = sync_channel(1);
    let worker = thread::Builder::new()
        .name("lcrt-caption-pipeline".to_owned())
        .spawn(move || {
            let result = run_pipeline(source, model_path, language, sink, &worker_cancelled)
                .map_err(|error| error.to_string());
            let _ = result_sender.send(result);
        })
        .map_err(|error| format!("could not start the caption pipeline worker: {error}"))?;
    Ok(PipelineSession {
        cancelled,
        result,
        worker,
    })
}

fn run_pipeline(
    source: AudioSourceDescriptor,
    model_path: PathBuf,
    language: Option<String>,
    sink: GtkCaptionSink,
    cancelled: &AtomicBool,
) -> Result<RunSummary, Box<dyn std::error::Error + Send + Sync>> {
    let mut whisper_config = WhisperConfig::new(model_path);
    whisper_config.language = language;
    let transcriber = WhisperTranscriber::new(whisper_config)?;
    let audio = PipeWireCapture::start(source, PipeWireCaptureConfig::default())?;
    sink.set_status("Listening…")?;
    let pipeline = CaptionPipeline::new(audio, transcriber, sink, RuntimeConfig::default())?;
    Ok(pipeline.run(cancelled)?)
}

fn take_completed_session(
    session: &mut Option<PipelineSession>,
) -> Option<Result<RunSummary, String>> {
    let result = match session.as_ref()?.result.try_recv() {
        Ok(result) => result,
        Err(TryRecvError::Empty) => return None,
        Err(TryRecvError::Disconnected) => {
            Err("caption pipeline result channel disconnected".to_owned())
        }
    };
    let completed = session.take()?;
    if completed.worker.join().is_err() {
        return Some(Err("caption pipeline worker panicked".to_owned()));
    }
    Some(result)
}

fn publish_completion(sink: &GtkCaptionSink, result: Result<RunSummary, String>) {
    let _ = sink.set_running(false);
    match result {
        Ok(summary) => {
            info!(
                audio_chunks = summary.audio_chunks,
                caption_updates = summary.caption_updates,
                "caption session completed"
            );
            let _ = sink.set_status(format!(
                "Stopped · {} chunks · {} captions",
                summary.audio_chunks, summary.caption_updates
            ));
        }
        Err(message) => {
            error!(%message, "caption session failed");
            let _ = sink.set_status("Error");
            let _ = sink.show_error(message);
        }
    }
}

fn spawn_smoke_actions(actions: SyncSender<CaptionUiAction>, smoke: SmokeConfig) {
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(250));
        if actions
            .send(CaptionUiAction::Start {
                source_id: smoke.source_id,
            })
            .is_err()
        {
            return;
        }
        thread::sleep(smoke.duration);
        let _ = actions.send(CaptionUiAction::Stop);
    });
}

fn parse_arguments(arguments: impl IntoIterator<Item = OsString>) -> Result<ParsedCommand, String> {
    let mut model_path = env::var_os("LCRT_MODEL_PATH").map(PathBuf::from);
    let mut language = None;
    let mut list_sources = false;
    let mut smoke_source = None;
    let mut smoke_duration = Duration::from_secs(10);
    let mut smoke_duration_was_set = false;
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        let argument = argument
            .into_string()
            .map_err(|_| "arguments must be valid UTF-8".to_owned())?;
        match argument.as_str() {
            "--help" | "-h" => return Ok(ParsedCommand::Help),
            "--model" => {
                model_path = Some(PathBuf::from(next_value(&mut arguments, "--model")?));
            }
            "--language" => {
                language = Some(os_to_string(next_value(&mut arguments, "--language")?)?);
            }
            "--list-sources" => list_sources = true,
            "--smoke-source" => {
                smoke_source = Some(os_to_string(next_value(&mut arguments, "--smoke-source")?)?);
            }
            "--smoke-seconds" => {
                smoke_duration_was_set = true;
                let value = os_to_string(next_value(&mut arguments, "--smoke-seconds")?)?;
                let seconds = value
                    .parse::<u64>()
                    .map_err(|_| "--smoke-seconds must be an integer from 1 to 120".to_owned())?;
                if !(1..=120).contains(&seconds) {
                    return Err("--smoke-seconds must be an integer from 1 to 120".to_owned());
                }
                smoke_duration = Duration::from_secs(seconds);
            }
            _ => return Err(format!("unknown argument: {argument}")),
        }
    }
    if smoke_duration_was_set && smoke_source.is_none() {
        return Err("--smoke-seconds requires --smoke-source".to_owned());
    }
    Ok(ParsedCommand::Run(AppConfig {
        model_path,
        language,
        list_sources,
        smoke: smoke_source.map(|source_id| SmokeConfig {
            source_id,
            duration: smoke_duration,
        }),
    }))
}

fn next_value(
    arguments: &mut impl Iterator<Item = OsString>,
    option: &str,
) -> Result<OsString, String> {
    arguments
        .next()
        .ok_or_else(|| format!("{option} requires a value"))
}

fn os_to_string(value: OsString) -> Result<String, String> {
    value
        .into_string()
        .map_err(|_| "arguments must be valid UTF-8".to_owned())
}

fn usage() -> &'static str {
    concat!(
        "LCRT live captions\n\n",
        "Usage:\n",
        "  lcrt [--model PATH] [--language CODE]\n",
        "  lcrt --list-sources\n",
        "  lcrt --model PATH --smoke-source ID [--smoke-seconds 1..120]\n\n",
        "Configuration:\n",
        "  LCRT_MODEL_PATH may be used instead of --model.\n",
        "  RUST_LOG controls structured diagnostic logging."
    )
}

#[cfg(test)]
mod tests {
    use std::{ffi::OsString, time::Duration};

    use super::{AppConfig, ParsedCommand, SmokeConfig, parse_arguments};

    #[test]
    fn parses_model_language_and_bounded_smoke_configuration() {
        let command = parse_arguments([
            OsString::from("--model"),
            OsString::from("model.bin"),
            OsString::from("--language"),
            OsString::from("en"),
            OsString::from("--smoke-source"),
            OsString::from("source-id"),
            OsString::from("--smoke-seconds"),
            OsString::from("12"),
        ])
        .unwrap();

        assert!(matches!(
            command,
            ParsedCommand::Run(AppConfig {
                model_path: Some(path),
                language: Some(language),
                smoke: Some(SmokeConfig { source_id, duration }),
                list_sources: false,
            }) if path.as_os_str() == "model.bin"
                && language == "en"
                && source_id == "source-id"
                && duration == Duration::from_secs(12)
        ));
    }

    #[test]
    fn rejects_unbounded_or_unknown_arguments() {
        assert!(parse_arguments([OsString::from("--smoke-seconds"), OsString::from("0")]).is_err());
        assert!(parse_arguments([OsString::from("--smoke-seconds"), OsString::from("5")]).is_err());
        assert!(parse_arguments([OsString::from("--unknown")]).is_err());
    }
}
