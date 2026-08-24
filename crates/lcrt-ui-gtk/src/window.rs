use std::{
    cell::Cell,
    rc::Rc,
    sync::mpsc::{Receiver, TryRecvError},
    time::Duration,
};

use gtk::{gdk, glib, pango, prelude::*};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use lcrt_core::{AudioSourceDescriptor, AudioSourceKind};
use libadwaita as adw;
use tracing::{debug, info};

use crate::{CaptionUiAction, UiEvent};

const APPLICATION_ID: &str = "io.github.hoangnguyen7474.Lcrt";
const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(16);
const MINIMUM_BACKGROUND_OPACITY: f64 = 0.3;
const OVERLAY_BOTTOM_MARGIN: i32 = 36;
const MINIMUM_OVERLAY_WIDTH: f64 = 320.0;
const MINIMUM_OVERLAY_HEIGHT: f64 = 160.0;
const MAXIMUM_OVERLAY_WIDTH: f64 = 2_560.0;
const MAXIMUM_OVERLAY_HEIGHT: f64 = 1_440.0;
const ON_DEMAND_KEYBOARD_PROTOCOL_VERSION: u32 = 4;

/// Initial native-window presentation settings.
#[derive(Clone, Debug, PartialEq)]
pub struct CaptionUiOptions {
    /// Initial window width in logical pixels.
    pub width: i32,
    /// Initial window height in logical pixels.
    pub height: i32,
    /// Initial caption font size in points.
    pub font_size_points: f64,
    /// Initial opacity of the window surfaces behind text and controls.
    pub background_opacity: f64,
    /// Prefer compositor-managed always-on-top presentation when supported.
    pub prefer_overlay: bool,
    /// PipeWire sources available for the current application session.
    pub sources: Vec<AudioSourceDescriptor>,
}

impl Default for CaptionUiOptions {
    fn default() -> Self {
        Self {
            width: 760,
            height: 320,
            font_size_points: 32.0,
            background_opacity: 0.86,
            prefer_overlay: true,
            sources: Vec::new(),
        }
    }
}

/// Runs the GTK main loop until the user closes the window or sends [`UiEvent::Quit`].
pub fn run_caption_ui(
    events: Receiver<UiEvent>,
    actions: std::sync::mpsc::SyncSender<CaptionUiAction>,
    options: CaptionUiOptions,
) -> glib::ExitCode {
    let application = adw::Application::builder()
        .application_id(APPLICATION_ID)
        .build();
    let events = Rc::new(std::cell::RefCell::new(Some(events)));
    application.connect_activate(move |application| {
        if let Some(window) = application.active_window() {
            window.present();
            return;
        }
        let Some(events) = events.borrow_mut().take() else {
            return;
        };
        build_window(application, events, actions.clone(), &options);
    });
    application.run_with_args(&[APPLICATION_ID])
}

fn build_window(
    application: &adw::Application,
    events: Receiver<UiEvent>,
    actions: std::sync::mpsc::SyncSender<CaptionUiAction>,
    options: &CaptionUiOptions,
) {
    let style_provider = install_css(options.background_opacity);
    let running = Rc::new(Cell::new(false));
    let caption = gtk::Label::builder()
        .label("Press Start to begin live captions")
        .wrap(true)
        .wrap_mode(pango::WrapMode::WordChar)
        .xalign(0.5)
        .yalign(0.5)
        .selectable(true)
        .hexpand(true)
        .vexpand(true)
        .build();
    caption.add_css_class("caption-text");
    set_caption_font(&caption, options.font_size_points);

    let caption_status = gtk::Label::new(Some("Ready"));
    caption_status.add_css_class("dim-label");
    let error_label = gtk::Label::builder().wrap(true).xalign(0.0).build();
    error_label.add_css_class("error");
    let error_revealer = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideDown)
        .child(&error_label)
        .build();

    let start_stop = gtk::Button::builder()
        .label("Start")
        .tooltip_text("Start or stop caption processing")
        .build();
    start_stop.add_css_class("suggested-action");
    let sources = Rc::new(options.sources.clone());
    let source_labels = sources
        .iter()
        .map(|source| {
            let kind = match source.kind() {
                AudioSourceKind::Microphone => "Microphone",
                AudioSourceKind::SystemOutput => "System audio",
            };
            format!("{kind} — {}", source.name())
        })
        .collect::<Vec<_>>();
    let source_label_refs = source_labels.iter().map(String::as_str).collect::<Vec<_>>();
    let source_picker = gtk::DropDown::from_strings(&source_label_refs);
    source_picker.set_tooltip_text(Some("Audio source"));
    source_picker.set_hexpand(true);
    if sources.is_empty() {
        source_picker.set_sensitive(false);
        start_stop.set_sensitive(false);
        error_label.set_text("No PipeWire microphone or system-audio source is available.");
        error_revealer.set_reveal_child(true);
    }
    let action_error = error_label.clone();
    let action_revealer = error_revealer.clone();
    let action_running = Rc::clone(&running);
    let action_sources = Rc::clone(&sources);
    let action_source_picker = source_picker.clone();
    start_stop.connect_clicked(move |_| {
        let action = if action_running.get() {
            CaptionUiAction::Stop
        } else {
            let Ok(index) = usize::try_from(action_source_picker.selected()) else {
                action_error.set_text("Select an audio source before starting captions.");
                action_revealer.set_reveal_child(true);
                return;
            };
            let Some(source) = action_sources.get(index) else {
                action_error.set_text("Select an audio source before starting captions.");
                action_revealer.set_reveal_child(true);
                return;
            };
            CaptionUiAction::Start {
                source_id: source.id().to_owned(),
            }
        };
        if actions.try_send(action).is_err() {
            action_error.set_text("Caption controller is busy or unavailable.");
            action_revealer.set_reveal_child(true);
        }
    });

    let font_adjustment = gtk::Adjustment::new(options.font_size_points, 16.0, 64.0, 1.0, 4.0, 0.0);
    let font_size = gtk::SpinButton::builder()
        .adjustment(&font_adjustment)
        .tooltip_text("Caption font size")
        .width_chars(3)
        .build();
    let font_caption = caption.clone();
    font_size.connect_value_changed(move |control| {
        set_caption_font(&font_caption, control.value());
    });

    let opacity_adjustment = gtk::Adjustment::new(
        clamp_background_opacity(options.background_opacity) * 100.0,
        MINIMUM_BACKGROUND_OPACITY * 100.0,
        100.0,
        5.0,
        10.0,
        0.0,
    );
    let opacity = gtk::SpinButton::builder()
        .adjustment(&opacity_adjustment)
        .tooltip_text("Overlay background opacity")
        .width_chars(3)
        .build();
    if let Some(provider) = style_provider {
        opacity.connect_value_changed(move |control| {
            load_css(&provider, control.value() / 100.0);
        });
    }

    let session_controls = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    session_controls.set_margin_top(12);
    session_controls.append(&source_picker);
    session_controls.append(&start_stop);
    session_controls.append(&caption_status);

    let width = dimension_control(
        options.width,
        MINIMUM_OVERLAY_WIDTH,
        MAXIMUM_OVERLAY_WIDTH,
        "Caption window width",
    );
    let height = dimension_control(
        options.height,
        MINIMUM_OVERLAY_HEIGHT,
        MAXIMUM_OVERLAY_HEIGHT,
        "Caption window height",
    );
    let presentation_controls = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    presentation_controls.set_margin_top(8);
    presentation_controls.append(&gtk::Label::new(Some("Font")));
    presentation_controls.append(&font_size);
    presentation_controls.append(&gtk::Label::new(Some("Opacity")));
    presentation_controls.append(&opacity);
    presentation_controls.append(&gtk::Label::new(Some("Width")));
    presentation_controls.append(&width);
    presentation_controls.append(&gtk::Label::new(Some("Height")));
    presentation_controls.append(&height);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.set_margin_start(24);
    content.set_margin_end(24);
    content.set_margin_bottom(20);
    content.add_css_class("caption-panel");
    content.append(&error_revealer);
    content.append(&caption);
    content.append(&session_controls);
    content.append(&presentation_controls);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&content));
    let window = adw::ApplicationWindow::builder()
        .application(application)
        .title("LCRT Live Captions")
        .default_width(options.width)
        .default_height(options.height)
        .content(&toolbar)
        .build();
    window.add_css_class("caption-overlay");
    let layer_shell_active = configure_overlay(&window, options.prefer_overlay);
    let overlay_status = gtk::Label::new(Some(if layer_shell_active {
        "Pinned overlay"
    } else {
        "Standard window"
    }));
    overlay_status.add_css_class("dim-label");
    overlay_status.set_tooltip_text(Some(if layer_shell_active {
        "The compositor is keeping this caption window above normal windows."
    } else {
        "Always-on-top is unavailable because this environment does not support interactive Wayland layer-shell v4 presentation."
    }));
    presentation_controls.append(&overlay_status);
    window.set_resizable(true);

    let resizable_window = window.clone();
    width.connect_value_changed(move |control| {
        resizable_window.set_default_width(control.value_as_int());
    });
    let resizable_window = window.clone();
    height.connect_value_changed(move |control| {
        resizable_window.set_default_height(control.value_as_int());
    });
    window.present();

    let weak_application = application.downgrade();
    glib::timeout_add_local(EVENT_POLL_INTERVAL, move || {
        loop {
            let event = match events.try_recv() {
                Ok(event) => event,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    error_label.set_text("Caption controller stopped unexpectedly.");
                    error_revealer.set_reveal_child(true);
                    running.set(false);
                    start_stop.set_label("Start");
                    return glib::ControlFlow::Break;
                }
            };
            match event {
                UiEvent::Caption(snapshot) => {
                    debug!(
                        revision = snapshot.revision(),
                        ui_queue_us = snapshot.age().as_micros(),
                        "caption update reached GTK"
                    );
                    caption.set_text(snapshot.caption().text());
                    caption_status.set_text(match snapshot.caption().status() {
                        lcrt_core::CaptionStatus::Partial => "Listening…",
                        lcrt_core::CaptionStatus::Final => "Final",
                    });
                }
                UiEvent::Running(is_running) => {
                    running.set(is_running);
                    source_picker.set_sensitive(!is_running && !sources.is_empty());
                    start_stop.set_label(if is_running { "Stop" } else { "Start" });
                    if is_running {
                        start_stop.remove_css_class("suggested-action");
                        start_stop.add_css_class("destructive-action");
                        caption_status.set_text("Listening…");
                    } else {
                        start_stop.remove_css_class("destructive-action");
                        start_stop.add_css_class("suggested-action");
                        caption_status.set_text("Stopped");
                    }
                }
                UiEvent::Status(status) => caption_status.set_text(&status),
                UiEvent::Error(message) => {
                    error_label.set_text(&message);
                    error_revealer.set_reveal_child(true);
                }
                UiEvent::ClearError => error_revealer.set_reveal_child(false),
                UiEvent::Quit => {
                    if let Some(application) = weak_application.upgrade() {
                        application.quit();
                    }
                    return glib::ControlFlow::Break;
                }
            }
        }
        glib::ControlFlow::Continue
    });
}

fn set_caption_font(label: &gtk::Label, points: f64) {
    let attributes = pango::AttrList::new();
    attributes.insert(pango::AttrSize::new(
        (points * f64::from(pango::SCALE)) as i32,
    ));
    label.set_attributes(Some(&attributes));
}

fn dimension_control(value: i32, minimum: f64, maximum: f64, tooltip: &str) -> gtk::SpinButton {
    let control = gtk::SpinButton::with_range(minimum, maximum, 20.0);
    control.set_value(f64::from(value).clamp(minimum, maximum));
    control.set_tooltip_text(Some(tooltip));
    control.set_width_chars(4);
    control
}

fn configure_overlay(window: &adw::ApplicationWindow, prefer_overlay: bool) -> bool {
    let wayland_display = gdk::Display::default()
        .is_some_and(|display| display.type_().name() == "GdkWaylandDisplay");
    let protocol_version = if prefer_overlay && wayland_display {
        gtk4_layer_shell::protocol_version()
    } else {
        0
    };
    let layer_shell_active =
        overlay_protocol_is_usable(prefer_overlay, wayland_display, protocol_version);
    if !layer_shell_active {
        info!(
            prefer_overlay,
            wayland_display,
            protocol_version,
            "interactive layer-shell unavailable; using compositor-managed standard window"
        );
        return false;
    }

    window.init_layer_shell();
    window.set_namespace(Some("lcrt-caption-overlay"));
    window.set_layer(Layer::Overlay);
    window.set_anchor(Edge::Bottom, true);
    window.set_margin(Edge::Bottom, OVERLAY_BOTTOM_MARGIN);
    window.set_keyboard_mode(KeyboardMode::OnDemand);
    window.set_exclusive_zone(0);
    info!(protocol_version, "layer-shell overlay presentation enabled");
    true
}

fn overlay_protocol_is_usable(
    prefer_overlay: bool,
    wayland_display: bool,
    protocol_version: u32,
) -> bool {
    prefer_overlay && wayland_display && protocol_version >= ON_DEMAND_KEYBOARD_PROTOCOL_VERSION
}

fn clamp_background_opacity(opacity: f64) -> f64 {
    if opacity.is_finite() {
        opacity.clamp(MINIMUM_BACKGROUND_OPACITY, 1.0)
    } else {
        CaptionUiOptions::default().background_opacity
    }
}

fn install_css(opacity: f64) -> Option<gtk::CssProvider> {
    let display = gdk::Display::default()?;
    let provider = gtk::CssProvider::new();
    load_css(&provider, opacity);
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    Some(provider)
}

fn load_css(provider: &gtk::CssProvider, opacity: f64) {
    let opacity = clamp_background_opacity(opacity);
    provider.load_from_data(&format!(
        "window.caption-overlay, window.caption-overlay > contents, \
         window.caption-overlay toolbarview {{ background-color: transparent; }}\n\
         window.caption-overlay headerbar {{ \
             background-color: alpha(@headerbar_bg_color, {opacity:.3}); \
             box-shadow: none; \
         }}\n\
         .caption-panel {{ \
             background-color: alpha(@window_bg_color, {opacity:.3}); \
             border-radius: 16px; \
         }}\n\
         .caption-text {{ padding: 20px; }}\n\
         .error {{ color: @error_color; padding: 10px; }}"
    ));
}

#[cfg(test)]
mod tests {
    use super::{
        CaptionUiOptions, MINIMUM_BACKGROUND_OPACITY, clamp_background_opacity,
        overlay_protocol_is_usable,
    };

    #[test]
    fn overlay_options_default_to_a_readable_translucent_surface() {
        let options = CaptionUiOptions::default();

        assert!(options.prefer_overlay);
        assert!(options.background_opacity < 1.0);
        assert!(options.background_opacity >= MINIMUM_BACKGROUND_OPACITY);
    }

    #[test]
    fn background_opacity_is_bounded_and_rejects_non_finite_values() {
        assert_eq!(clamp_background_opacity(0.1), MINIMUM_BACKGROUND_OPACITY);
        assert_eq!(clamp_background_opacity(1.5), 1.0);
        assert_eq!(
            clamp_background_opacity(f64::NAN),
            CaptionUiOptions::default().background_opacity
        );
    }

    #[test]
    fn overlay_requires_wayland_and_on_demand_keyboard_protocol() {
        assert!(overlay_protocol_is_usable(true, true, 4));
        assert!(!overlay_protocol_is_usable(true, true, 3));
        assert!(!overlay_protocol_is_usable(true, false, 4));
        assert!(!overlay_protocol_is_usable(false, true, 4));
    }
}
