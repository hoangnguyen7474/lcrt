use std::{env, process::ExitCode, sync::mpsc::sync_channel, thread, time::Duration};

use lcrt_core::{CaptionSink, CaptionState, TranscriptUpdate};
use lcrt_ui_gtk::{CaptionUiAction, CaptionUiOptions, GtkCaptionSink, run_caption_ui};

fn main() -> ExitCode {
    let smoke_test = env::args()
        .skip(1)
        .any(|argument| argument == "--smoke-test");
    let (sink, events) = match GtkCaptionSink::channel(32) {
        Ok(channel) => channel,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    let (actions, action_receiver) = sync_channel(4);
    let controller = if smoke_test {
        spawn_smoke_controller(sink)
    } else {
        spawn_demo_controller(sink, action_receiver)
    };

    let status = run_caption_ui(events, actions, CaptionUiOptions::default());
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
        let _ = sink.quit();
    })
}

fn spawn_demo_controller(
    sink: GtkCaptionSink,
    actions: std::sync::mpsc::Receiver<CaptionUiAction>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        while let Ok(action) = actions.recv() {
            match action {
                CaptionUiAction::Start => {
                    publish_demo(&sink, Duration::from_millis(450), Some(&actions));
                }
                CaptionUiAction::Stop => {
                    let _ = sink.set_running(false);
                }
            }
        }
    })
}

fn publish_demo(
    sink: &GtkCaptionSink,
    interval: Duration,
    actions: Option<&std::sync::mpsc::Receiver<CaptionUiAction>>,
) {
    let _ = sink.clear_error();
    let _ = sink.set_running(true);
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
            let _ = sink.show_error("Demo caption was invalid.");
            break;
        };
        let Ok(snapshot) = state.apply(update) else {
            let _ = sink.show_error("Caption state could not be updated.");
            break;
        };
        let mut caption_sink = sink.clone();
        if caption_sink.publish(snapshot).is_err() {
            break;
        }
        let stopped = actions.is_some_and(|actions| match actions.recv_timeout(interval) {
            Ok(CaptionUiAction::Stop) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                true
            }
            Ok(CaptionUiAction::Start) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => false,
        });
        if stopped {
            break;
        }
        if actions.is_none() {
            thread::sleep(interval);
        }
    }
    let _ = sink.set_running(false);
}
