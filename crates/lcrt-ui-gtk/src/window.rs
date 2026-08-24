use std::{
    cell::Cell,
    rc::Rc,
    sync::mpsc::{Receiver, TryRecvError},
    time::Duration,
};

use gtk::{gdk, glib, pango, prelude::*};
use libadwaita as adw;

use crate::{CaptionUiAction, UiEvent};

const APPLICATION_ID: &str = "io.github.hoangnguyen7474.Lcrt";
const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(16);

/// Initial native-window presentation settings.
#[derive(Clone, Debug, PartialEq)]
pub struct CaptionUiOptions {
    /// Initial window width in logical pixels.
    pub width: i32,
    /// Initial window height in logical pixels.
    pub height: i32,
    /// Initial caption font size in points.
    pub font_size_points: f64,
}

impl Default for CaptionUiOptions {
    fn default() -> Self {
        Self {
            width: 760,
            height: 320,
            font_size_points: 32.0,
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
    install_css();
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
    let action_error = error_label.clone();
    let action_revealer = error_revealer.clone();
    let action_running = Rc::clone(&running);
    start_stop.connect_clicked(move |_| {
        let action = if action_running.get() {
            CaptionUiAction::Stop
        } else {
            CaptionUiAction::Start
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

    let controls = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    controls.set_margin_top(12);
    controls.append(&start_stop);
    controls.append(&caption_status);
    controls.append(&gtk::Label::new(Some("Font")));
    controls.append(&font_size);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.set_margin_start(24);
    content.set_margin_end(24);
    content.set_margin_bottom(20);
    content.append(&error_revealer);
    content.append(&caption);
    content.append(&controls);

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
    window.set_resizable(true);
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
                    caption.set_text(snapshot.caption().text());
                    caption_status.set_text(match snapshot.caption().status() {
                        lcrt_core::CaptionStatus::Partial => "Listening…",
                        lcrt_core::CaptionStatus::Final => "Final",
                    });
                }
                UiEvent::Running(is_running) => {
                    running.set(is_running);
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

fn install_css() {
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let provider = gtk::CssProvider::new();
    provider.load_from_data(
        ".caption-text { padding: 20px; }\n.error { color: @error_color; padding: 10px; }",
    );
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
