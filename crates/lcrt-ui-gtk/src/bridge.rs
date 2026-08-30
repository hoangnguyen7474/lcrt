use std::sync::{Arc, Mutex};

use lcrt_core::{CaptionSink, CaptionSinkError, CaptionSnapshot, CaptionStatus};

/// User intent emitted by the native window.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CaptionUiAction {
    /// Start caption processing.
    Start {
        /// Stable platform adapter identifier selected in the window.
        source_id: String,
    },
    /// Stop caption processing and release capture resources.
    Stop,
    /// Terminate the controller after cancelling any active caption session.
    Shutdown,
}

/// The changes the GTK thread should apply during one bounded poll.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct UiUpdate {
    pub(crate) caption: Option<CaptionSnapshot>,
    pub(crate) running: Option<bool>,
    pub(crate) status: Option<String>,
    pub(crate) error: Option<Option<String>>,
    pub(crate) quit: bool,
}

#[derive(Debug, Default)]
struct BridgeState {
    closed: bool,
    caption: Option<CaptionSnapshot>,
    caption_dirty: bool,
    running: Option<bool>,
    running_dirty: bool,
    status: Option<String>,
    status_dirty: bool,
    error: Option<String>,
    error_dirty: bool,
    quit: bool,
}

/// Cloneable producer endpoint for the GTK-local caption state bridge.
///
/// Caption snapshots share one replaceable slot, so a slow GTK consumer can
/// retain only the newest snapshot. Lifecycle state is kept separately as
/// authoritative state rather than competing with captions in a queue.
#[derive(Clone, Debug)]
pub struct GtkCaptionSink {
    state: Arc<Mutex<BridgeState>>,
}

/// GTK-thread endpoint for the caption state bridge.
#[derive(Debug)]
pub struct GtkCaptionReceiver {
    state: Arc<Mutex<BridgeState>>,
}

impl GtkCaptionSink {
    /// Creates a state bridge whose receiver must be owned by the GTK thread.
    pub fn bridge() -> (Self, GtkCaptionReceiver) {
        let state = Arc::new(Mutex::new(BridgeState::default()));
        (
            Self {
                state: Arc::clone(&state),
            },
            GtkCaptionReceiver { state },
        )
    }

    /// Updates whether captioning is running.
    pub fn set_running(&self, running: bool) -> Result<(), CaptionSinkError> {
        self.update(|state| {
            if running {
                // Each pipeline creates its own caption revision sequence.
                state.caption = None;
                state.caption_dirty = false;
            }
            state.running = Some(running);
            state.running_dirty = true;
        })
    }

    /// Shows a short pipeline lifecycle status.
    pub fn set_status(&self, status: impl Into<String>) -> Result<(), CaptionSinkError> {
        let status = status.into();
        if status.trim().is_empty() {
            return Err(CaptionSinkError::new(
                "GTK caption status must not be empty",
            ));
        }
        self.update(move |state| {
            state.status = Some(status);
            state.status_dirty = true;
        })
    }

    /// Shows an actionable error in the caption window.
    pub fn show_error(&self, message: impl Into<String>) -> Result<(), CaptionSinkError> {
        let message = message.into();
        if message.trim().is_empty() {
            return Err(CaptionSinkError::new(
                "GTK caption error message must not be empty",
            ));
        }
        self.update(move |state| {
            state.error = Some(message);
            state.error_dirty = true;
        })
    }

    /// Clears a previously displayed error.
    pub fn clear_error(&self) -> Result<(), CaptionSinkError> {
        self.update(|state| {
            state.error = None;
            state.error_dirty = true;
        })
    }

    /// Requests an orderly application exit.
    pub fn quit(&self) -> Result<(), CaptionSinkError> {
        self.update(|state| state.quit = true)
    }

    fn update(&self, update: impl FnOnce(&mut BridgeState)) -> Result<(), CaptionSinkError> {
        let mut state = self.lock_state()?;
        if state.closed {
            return Err(CaptionSinkError::new(
                "GTK caption window is no longer available",
            ));
        }
        update(&mut state);
        Ok(())
    }

    fn lock_state(&self) -> Result<std::sync::MutexGuard<'_, BridgeState>, CaptionSinkError> {
        self.state
            .lock()
            .map_err(|_| CaptionSinkError::new("GTK caption bridge state is unavailable"))
    }
}

impl CaptionSink for GtkCaptionSink {
    /// Replaces obsolete caption state without treating ordinary UI lag as failure.
    fn publish(&mut self, snapshot: CaptionSnapshot) -> Result<(), CaptionSinkError> {
        self.update(|state| {
            let replace = match state.caption.as_ref() {
                None => true,
                Some(current) if snapshot.revision() > current.revision() => true,
                Some(current) if snapshot.revision() < current.revision() => false,
                Some(current) => {
                    current.caption().status() == CaptionStatus::Partial
                        && snapshot.caption().status() == CaptionStatus::Final
                }
            };
            if replace {
                state.caption = Some(snapshot);
                state.caption_dirty = true;
            }
        })
    }
}

impl GtkCaptionReceiver {
    /// Takes the current coalesced state for one GTK main-loop poll.
    pub(crate) fn take_update(&self) -> Result<UiUpdate, CaptionSinkError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| CaptionSinkError::new("GTK caption bridge state is unavailable"))?;
        let update = UiUpdate {
            caption: state.caption_dirty.then(|| state.caption.clone()).flatten(),
            running: state.running_dirty.then(|| state.running).flatten(),
            status: state.status_dirty.then(|| state.status.clone()).flatten(),
            error: state.error_dirty.then(|| state.error.clone()),
            quit: state.quit,
        };
        state.caption_dirty = false;
        state.running_dirty = false;
        state.status_dirty = false;
        state.error_dirty = false;
        Ok(update)
    }
}

impl Drop for GtkCaptionReceiver {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.lock() {
            state.closed = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use lcrt_core::{CaptionSink, CaptionSnapshot, CaptionState, CaptionStatus, TranscriptUpdate};

    use super::GtkCaptionSink;

    fn snapshot(state: &mut CaptionState, text: String, final_caption: bool) -> CaptionSnapshot {
        let update = if final_caption {
            TranscriptUpdate::finalized(text)
        } else {
            TranscriptUpdate::partial(text)
        };
        state.apply(update.unwrap()).unwrap()
    }

    #[test]
    fn caption_flood_is_coalesced_to_the_newest_snapshot() {
        let (mut sink, receiver) = GtkCaptionSink::bridge();
        let mut captions = CaptionState::new();
        for revision in 1..=1_024 {
            sink.publish(snapshot(
                &mut captions,
                format!("caption {revision}"),
                false,
            ))
            .unwrap();
        }

        let update = receiver.take_update().unwrap();
        assert_eq!(update.caption.unwrap().revision(), 1_024);
        assert!(receiver.take_update().unwrap().caption.is_none());
    }

    #[test]
    fn newer_final_caption_is_not_replaced_by_stale_partial() {
        let (mut sink, receiver) = GtkCaptionSink::bridge();
        let mut captions = CaptionState::new();
        let partial = snapshot(&mut captions, "draft".to_owned(), false);
        let final_caption = snapshot(&mut captions, "final".to_owned(), true);

        sink.publish(final_caption).unwrap();
        sink.publish(partial).unwrap();

        let caption = receiver.take_update().unwrap().caption.unwrap();
        assert_eq!(caption.caption().status(), CaptionStatus::Final);
        assert_eq!(caption.caption().text(), "final");
    }

    #[test]
    fn control_state_remains_observable_during_caption_saturation() {
        let (mut sink, receiver) = GtkCaptionSink::bridge();
        let mut captions = CaptionState::new();
        for revision in 0..1_024 {
            sink.publish(snapshot(
                &mut captions,
                format!("caption {revision}"),
                false,
            ))
            .unwrap();
        }
        sink.set_status("Error").unwrap();
        sink.show_error("capture failed").unwrap();
        sink.set_running(false).unwrap();
        sink.quit().unwrap();

        let update = receiver.take_update().unwrap();
        assert_eq!(update.caption.unwrap().revision(), 1_024);
        assert_eq!(update.status.as_deref(), Some("Error"));
        assert_eq!(update.error, Some(Some("capture failed".to_owned())));
        assert_eq!(update.running, Some(false));
        assert!(update.quit);
    }

    #[test]
    fn delayed_consumer_receives_current_state_without_replaying_a_backlog() {
        let (mut sink, receiver) = GtkCaptionSink::bridge();
        let mut captions = CaptionState::new();
        for revision in 0..10_000 {
            sink.publish(snapshot(
                &mut captions,
                format!("caption {revision}"),
                false,
            ))
            .unwrap();
        }

        let update = receiver.take_update().unwrap();
        assert_eq!(update.caption.unwrap().revision(), 10_000);
        assert!(receiver.take_update().unwrap().caption.is_none());
    }

    #[test]
    fn closed_receiver_is_a_fatal_transport_error() {
        let (mut sink, receiver) = GtkCaptionSink::bridge();
        drop(receiver);
        let snapshot = snapshot(&mut CaptionState::new(), "caption".to_owned(), false);

        assert!(sink.publish(snapshot).is_err());
        assert!(sink.quit().is_err());
    }

    #[test]
    fn bridge_rejects_empty_errors_and_statuses() {
        let (sink, _receiver) = GtkCaptionSink::bridge();
        assert!(sink.show_error("  ").is_err());
        assert!(sink.set_status("").is_err());
    }
}
