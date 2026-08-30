use std::{env, process::ExitCode, sync::mpsc::sync_channel, thread, time::Duration};

use lcrt_core::{
    AudioSourceDescriptor, AudioSourceKind, CaptionSink, CaptionState, TranscriptUpdate,
};
use lcrt_ui_gtk::{
    CaptionUiAction, CaptionUiMode, CaptionUiOptions, GtkCaptionSink, run_caption_ui,
};

fn main() -> ExitCode {
    let smoke_test = env::args()
        .skip(1)
        .any(|argument| argument == "--smoke-test");
    let (sink, events) = GtkCaptionSink::bridge();
    let (actions, action_receiver) = sync_channel(4);
    let controller = if smoke_test {
        spawn_smoke_controller(sink)
    } else {
        spawn_demo_controller(sink, action_receiver)
    };

    let options = CaptionUiOptions {
        mode: if smoke_test {
            CaptionUiMode::Diagnostic
        } else {
            CaptionUiMode::Normal
        },
        sources: vec![AudioSourceDescriptor::new(
            "demo",
            "Deterministic demo",
            AudioSourceKind::Microphone,
        )],
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

fn spawn_smoke_controller(sink: GtkCaptionSink) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(250));
        publish_demo(&sink, Duration::from_millis(250), None);
        thread::sleep(Duration::from_millis(400));
        if let Err(error) = sink.quit() {
            eprintln!("error: {error}");
        }
    })
}

fn spawn_demo_controller(
    sink: GtkCaptionSink,
    actions: std::sync::mpsc::Receiver<CaptionUiAction>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        while let Ok(action) = actions.recv() {
            match action {
                CaptionUiAction::Start { .. } => {
                    publish_demo(&sink, Duration::from_millis(450), Some(&actions));
                }
                CaptionUiAction::Stop => {
                    if sink.set_running(false).is_err() {
                        break;
                    }
                }
                CaptionUiAction::Shutdown => break,
            }
        }
    })
}

fn publish_demo(
    sink: &GtkCaptionSink,
    interval: Duration,
    actions: Option<&std::sync::mpsc::Receiver<CaptionUiAction>>,
) {
    if sink.clear_error().is_err() || sink.set_running(true).is_err() {
        return;
    }
    let mut state = CaptionState::new();
    let updates = [
        ("Native captions", false),
        ("Native captions update incrementally", false),
        ("Native captions update incrementally on Ubuntu.", true),
    ];
    for (text, is_final) in updates {
        let update = if is_final {
            TranscriptUpdate::finalized(text)
        } else {
            TranscriptUpdate::partial(text)
        };
        let Ok(update) = update else {
            if sink.show_error("Demo caption was invalid.").is_err() {
                return;
            }
            return;
        };
        let Ok(snapshot) = state.apply(update) else {
            if sink
                .show_error("Caption state could not be updated.")
                .is_err()
            {
                return;
            }
            return;
        };
        let mut caption_sink = sink.clone();
        if caption_sink.publish(snapshot).is_err() {
            break;
        }
        let stopped = actions.is_some_and(|actions| match actions.recv_timeout(interval) {
            Ok(CaptionUiAction::Stop)
            | Ok(CaptionUiAction::Shutdown)
            | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => true,
            Ok(CaptionUiAction::Start { .. }) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                false
            }
        });
        if stopped {
            break;
        }
        if actions.is_none() {
            thread::sleep(interval);
        }
    }
    if let Err(error) = sink.set_running(false) {
        eprintln!("error: {error}");
    }
}
