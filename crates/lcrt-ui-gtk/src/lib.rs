//! Native GTK4/libadwaita presentation and its bounded core-to-UI bridge.

mod bridge;
mod window;

pub use bridge::{CaptionUiAction, GtkCaptionReceiver, GtkCaptionSink};
pub use window::{CaptionUiMode, CaptionUiOptions, run_caption_ui};
