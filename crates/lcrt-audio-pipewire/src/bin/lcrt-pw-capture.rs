#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::time::{Duration, Instant};

    use lcrt_audio_pipewire::{PipeWireCapture, PipeWireCaptureConfig, enumerate_audio_sources};
    use lcrt_core::{AudioCapture, AudioInputEvent};

    let mut arguments = std::env::args().skip(1);
    let command = arguments.next().unwrap_or_else(|| "list".to_owned());
    let sources = enumerate_audio_sources(Duration::from_secs(3))?;

    match command.as_str() {
        "list" => {
            for source in sources {
                println!("{:?}\t{}\t{}", source.kind(), source.id(), source.name());
            }
        }
        "capture" => {
            let source_id = arguments
                .next()
                .ok_or("usage: lcrt-pw-capture capture <source-id> [seconds]")?;
            let seconds = arguments
                .next()
                .map(|value| value.parse::<u64>())
                .transpose()?
                .unwrap_or(3)
                .clamp(1, 30);
            let source = sources
                .into_iter()
                .find(|source| source.id() == source_id)
                .ok_or_else(|| format!("PipeWire source `{source_id}` was not found"))?;
            let mut capture = PipeWireCapture::start(source, PipeWireCaptureConfig::default())?;
            let deadline = Instant::now() + Duration::from_secs(seconds);
            let mut chunks = 0_u64;
            let mut frames = 0_u64;
            let mut peak = 0.0_f32;
            let mut format = None;

            while Instant::now() < deadline {
                match capture.next_event(Duration::from_millis(250))? {
                    AudioInputEvent::Chunk(chunk) => {
                        chunks += 1;
                        frames += u64::try_from(chunk.frame_count())?;
                        peak = chunk
                            .samples()
                            .iter()
                            .fold(peak, |current, sample| current.max(sample.abs()));
                        format = Some((chunk.sample_rate_hz(), chunk.channels()));
                    }
                    AudioInputEvent::Idle => {}
                    AudioInputEvent::EndOfStream => break,
                }
            }
            capture.stop()?;
            match format {
                Some((sample_rate, channels)) => println!(
                    "captured {chunks} chunks, {frames} frames, {sample_rate} Hz, {channels} channel(s), peak {peak:.6}"
                ),
                None => println!("capture ran for {seconds}s but received no audio chunks"),
            }
        }
        _ => {
            return Err(
                format!("unknown command `{command}`; expected `list` or `capture`").into(),
            );
        }
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("lcrt-pw-capture is available only on Linux");
    std::process::exit(1);
}
