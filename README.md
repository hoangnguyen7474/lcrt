# LCRT

Native real-time captions, translation, and language-assistance desktop application.

Primary platform:
- Ubuntu AMD64

Planned platforms:
- Ubuntu ARM64
- Windows 10/11

Planned milestones:
- V1: Real-time live captions
- V2: Low-latency real-time translation
- V3: Select text for meaning, grammar, and contextual explanation

## Development

The Rust workspace uses the stable toolchain with the `rustfmt` and `clippy`
components. Run the local quality gates with:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

### Linux audio development

PipeWire capture development requires `libpipewire-0.3-dev`,
`libspa-0.2-dev`, and `pkg-config`. The bounded diagnostic utility enumerates
both microphone sources and system-output monitor targets:

```sh
cargo run -p lcrt-audio-pipewire --bin lcrt-pw-capture -- list
cargo run -p lcrt-audio-pipewire --bin lcrt-pw-capture -- capture <source-id> 3
```

The capture duration is clamped to 1–30 seconds. The utility reports negotiated
format and aggregate sample statistics; it neither records audio to disk nor
silently substitutes synthetic audio when PipeWire fails.

### Local Whisper development

The speech-to-text adapter uses whisper.cpp through `whisper-rs`, runs model
inference on a dedicated worker, downsamples input to 16 kHz mono, and keeps
both its input queue and rolling audio window bounded. Models are deliberately
excluded from Git. Download the English tiny model locally with:

```sh
./scripts/download-whisper-model.sh
```

The downloader verifies the model's pinned SHA-256 digest before installation.

Transcribe a signed 16-bit PCM or 32-bit float WAV file with the bounded
diagnostic utility:

```sh
cargo run -p lcrt-stt-whisper --bin lcrt-whisper-transcribe -- \
  models/ggml-tiny.en.bin path/to/audio.wav en
```

Set an explicit model path in application configuration. A missing or invalid
model produces an actionable startup error; the application does not silently
download models or send audio to a remote service.
