use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

use lcrt_core::{AudioSourceDescriptor, AudioSourceKind};
use pipewire as pw;
use pw::types::ObjectType;

use crate::PipeWireError;

/// Enumerates microphone sources and output sinks available for monitor capture.
///
/// Output sinks are returned as [`AudioSourceKind::SystemOutput`]; capture uses
/// PipeWire's sink-monitor routing rather than presenting fake loopback audio.
pub fn enumerate_audio_sources(
    timeout: Duration,
) -> Result<Vec<AudioSourceDescriptor>, PipeWireError> {
    if timeout.is_zero() {
        return Err(PipeWireError::InvalidConfiguration(
            "enumeration timeout must be greater than zero",
        ));
    }

    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None).map_err(pipewire_error)?;
    let context = pw::context::ContextRc::new(&mainloop, None).map_err(pipewire_error)?;
    let core = context.connect_rc(None).map_err(pipewire_error)?;
    let registry = core.get_registry_rc().map_err(pipewire_error)?;
    let sources = Rc::new(RefCell::new(Vec::new()));
    let sources_for_listener = Rc::clone(&sources);

    let _registry_listener = registry
        .add_listener_local()
        .global(move |object| {
            if object.type_ != ObjectType::Node {
                return;
            }
            let Some(properties) = object.props else {
                return;
            };
            let Some(media_class) = properties.get(*pw::keys::MEDIA_CLASS) else {
                return;
            };
            let Some(kind) = source_kind(media_class) else {
                return;
            };
            let Some(id) = properties.get(*pw::keys::NODE_NAME) else {
                return;
            };
            let name = properties
                .get(*pw::keys::NODE_DESCRIPTION)
                .or_else(|| properties.get(*pw::keys::NODE_NICK))
                .unwrap_or(id);
            sources_for_listener
                .borrow_mut()
                .push(AudioSourceDescriptor::new(id, name, kind));
        })
        .register();

    let completed = Rc::new(Cell::new(false));
    let completed_for_listener = Rc::clone(&completed);
    let loop_for_listener = mainloop.clone();
    let pending = core.sync(0).map_err(pipewire_error)?;
    let _core_listener = core
        .add_listener_local()
        .done(move |id, sequence| {
            if id == pw::core::PW_ID_CORE && sequence == pending {
                completed_for_listener.set(true);
                loop_for_listener.quit();
            }
        })
        .register();

    let loop_for_timer = mainloop.clone();
    let timer = mainloop.loop_().add_timer(move |_| loop_for_timer.quit());
    timer
        .update_timer(Some(timeout), None)
        .into_result()
        .map_err(|error| {
            PipeWireError::PipeWire(format!("failed to arm enumeration timer: {error}"))
        })?;

    mainloop.run();
    if !completed.get() {
        return Err(PipeWireError::EnumerationTimeout(timeout));
    }

    let mut result = sources.take();
    result.sort_by(|left, right| {
        left.name()
            .cmp(right.name())
            .then(left.id().cmp(right.id()))
    });
    result.dedup_by(|left, right| left.id() == right.id());
    Ok(result)
}

fn pipewire_error(error: pw::Error) -> PipeWireError {
    PipeWireError::PipeWire(error.to_string())
}

fn source_kind(media_class: &str) -> Option<AudioSourceKind> {
    match media_class {
        "Audio/Source" => Some(AudioSourceKind::Microphone),
        "Audio/Sink" => Some(AudioSourceKind::SystemOutput),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{enumerate_audio_sources, source_kind};
    use crate::PipeWireError;
    use lcrt_core::AudioSourceKind;

    #[test]
    fn enumeration_rejects_zero_timeout_without_contacting_server() {
        assert_eq!(
            enumerate_audio_sources(Duration::ZERO),
            Err(PipeWireError::InvalidConfiguration(
                "enumeration timeout must be greater than zero"
            ))
        );
    }

    #[test]
    fn classifies_microphones_and_output_sinks_only() {
        assert_eq!(
            source_kind("Audio/Source"),
            Some(AudioSourceKind::Microphone)
        );
        assert_eq!(
            source_kind("Audio/Sink"),
            Some(AudioSourceKind::SystemOutput)
        );
        assert_eq!(source_kind("Video/Source"), None);
    }
}
