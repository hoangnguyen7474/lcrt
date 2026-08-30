use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};

use lcrt_core::{CaptionSink, CaptionSinkError, CaptionSnapshot};

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

/// State change consumed on the GTK main thread.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UiEvent {
    /// New immutable caption state.
    Caption(CaptionSnapshot),
    /// Whether caption processing is active.
    Running(bool),
    /// Short lifecycle status shown beside the controls.
    Status(String),
    /// User-visible pipeline or configuration failure.
    Error(String),
    /// Remove the currently displayed error.
    ClearError,
    /// Close the application, used by bounded diagnostics and orderly shutdown.
    Quit,
}

/// Cloneable, bounded sender used by pipeline and controller threads.
#[derive(Clone, Debug)]
pub struct GtkCaptionSink {
    sender: SyncSender<UiEvent>,
    capacity: usize,
}

impl GtkCaptionSink {
    /// Creates a bounded channel whose receiver must be owned by the GTK thread.
    pub fn channel(capacity: usize) -> Result<(Self, Receiver<UiEvent>), CaptionSinkError> {
        if capacity == 0 {
            return Err(CaptionSinkError::new(
                "GTK caption event capacity must be greater than zero",
            ));
        }
        let (sender, receiver) = sync_channel(capacity);
        Ok((Self { sender, capacity }, receiver))
    }

    /// Updates whether captioning is running.
    pub fn set_running(&self, running: bool) -> Result<(), CaptionSinkError> {
        self.send(UiEvent::Running(running))
    }

    /// Shows a short pipeline lifecycle status.
    pub fn set_status(&self, status: impl Into<String>) -> Result<(), CaptionSinkError> {
        let status = status.into();
        if status.trim().is_empty() {
            return Err(CaptionSinkError::new(
                "GTK caption status must not be empty",
            ));
        }
        self.send(UiEvent::Status(status))
    }

    /// Shows an actionable error in the caption window.
    pub fn show_error(&self, message: impl Into<String>) -> Result<(), CaptionSinkError> {
        let message = message.into();
        if message.trim().is_empty() {
            return Err(CaptionSinkError::new(
                "GTK caption error message must not be empty",
            ));
        }
        self.send(UiEvent::Error(message))
    }

    /// Clears a previously displayed error.
    pub fn clear_error(&self) -> Result<(), CaptionSinkError> {
        self.send(UiEvent::ClearError)
    }

    /// Requests an orderly application exit.
    pub fn quit(&self) -> Result<(), CaptionSinkError> {
        self.send(UiEvent::Quit)
    }

    fn send(&self, event: UiEvent) -> Result<(), CaptionSinkError> {
        match self.sender.try_send(event) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(CaptionSinkError::new(format!(
                "GTK caption event queue reached its {}-event bound",
                self.capacity
            ))),
            Err(TrySendError::Disconnected(_)) => Err(CaptionSinkError::new(
                "GTK caption window is no longer available",
            )),
        }
    }
}

impl CaptionSink for GtkCaptionSink {
    fn publish(&mut self, snapshot: CaptionSnapshot) -> Result<(), CaptionSinkError> {
        self.send(UiEvent::Caption(snapshot))
    }
}

#[cfg(test)]
mod tests {
    use lcrt_core::{CaptionSink, CaptionState, TranscriptUpdate};

    use super::{GtkCaptionSink, UiEvent};

    #[test]
    fn bridge_preserves_caption_snapshot() {
        let (mut sink, receiver) = GtkCaptionSink::channel(2).unwrap();
        let snapshot = CaptionState::new()
            .apply(TranscriptUpdate::partial("incremental caption").unwrap())
            .unwrap();

        sink.publish(snapshot.clone()).unwrap();

        assert_eq!(receiver.recv().unwrap(), UiEvent::Caption(snapshot));
    }

    #[test]
    fn bridge_reports_its_queue_bound() {
        let (sink, _receiver) = GtkCaptionSink::channel(1).unwrap();
        sink.set_running(true).unwrap();

        let error = sink.set_running(false).unwrap_err();

        assert!(error.to_string().contains("1-event bound"));
    }

    #[test]
    fn bridge_rejects_zero_capacity_and_empty_errors() {
        assert!(GtkCaptionSink::channel(0).is_err());
        let (sink, _receiver) = GtkCaptionSink::channel(1).unwrap();
        assert!(sink.show_error("  ").is_err());
        assert!(sink.set_status("").is_err());
    }
}
