use std::{env, error::Error, path::Path, process::ExitCode, time::Duration};

use hound::{SampleFormat, WavReader};
use lcrt_core::{AudioChunk, Transcriber, TranscriptUpdate};
use lcrt_stt_whisper::{WhisperConfig, WhisperTranscriber};

const CHUNK_FRAMES: usize = 4_096;
const OFFLINE_QUEUE_CAPACITY: usize = 8;
const INPUT_BACKPRESSURE_TIMEOUT: Duration = Duration::from_secs(30);

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let mut arguments = env::args_os().skip(1);
    let model_path = arguments
        .next()
        .ok_or("usage: lcrt-whisper-transcribe <model.bin> <audio.wav> [language]")?;
    let wav_path = arguments
        .next()
        .ok_or("usage: lcrt-whisper-transcribe <model.bin> <audio.wav> [language]")?;
    let language = arguments
        .next()
        .map(|value| value.into_string().map_err(|_| "language must be UTF-8"))
        .transpose()?;
    if arguments.next().is_some() {
        return Err("usage: lcrt-whisper-transcribe <model.bin> <audio.wav> [language]".into());
    }

    let (samples, sample_rate, channels) = read_wav(Path::new(&wav_path))?;
    let mut config = WhisperConfig::new(model_path);
    config.language = language;
    // Keep finite-file input close enough to completed inference that shutdown
    // never inherits a live-capture-sized backlog.
    config.input_queue_capacity = OFFLINE_QUEUE_CAPACITY;
    let mut transcriber = WhisperTranscriber::new(config)?;
    let samples_per_chunk = CHUNK_FRAMES * usize::from(channels);
    let mut update_count = 0_usize;
    let mut chunk_count = 0_usize;
    for samples in samples.chunks(samples_per_chunk) {
        let chunk = AudioChunk::new(samples.to_vec(), sample_rate, channels)?;
        update_count +=
            print_updates(transcriber.push_audio_with_timeout(chunk, INPUT_BACKPRESSURE_TIMEOUT)?);
        chunk_count += 1;
    }
    update_count += print_updates(transcriber.finish()?);
    let inference_count = transcriber.inference_count();
    eprintln!(
        "processed {chunk_count} bounded audio chunks with {inference_count} inference passes and emitted {update_count} transcript updates"
    );
    Ok(())
}

fn read_wav(path: &Path) -> Result<(Vec<f32>, u32, u16), Box<dyn Error>> {
    let mut reader = WavReader::open(path)?;
    let spec = reader.spec();
    let samples = match (spec.sample_format, spec.bits_per_sample) {
        (SampleFormat::Int, 16) => reader
            .samples::<i16>()
            .map(|sample| sample.map(|value| f32::from(value) / f32::from(i16::MAX)))
            .collect::<Result<Vec<_>, _>>()?,
        (SampleFormat::Float, 32) => reader.samples::<f32>().collect::<Result<Vec<_>, _>>()?,
        _ => {
            return Err(format!(
                "unsupported WAV encoding: {:?}/{}-bit; use signed 16-bit PCM or 32-bit float",
                spec.sample_format, spec.bits_per_sample
            )
            .into());
        }
    };
    if samples.is_empty() {
        return Err("WAV contains no audio samples".into());
    }
    Ok((samples, spec.sample_rate, spec.channels))
}

fn print_updates(updates: Vec<TranscriptUpdate>) -> usize {
    let count = updates.len();
    for update in updates {
        println!("{:?}: {}", update.status(), update.text());
    }
    count
}
