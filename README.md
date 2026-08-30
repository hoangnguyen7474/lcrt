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

The checked-in `rust-toolchain.toml` pins the primary development and CI
toolchain to Rust 1.98.0 with the `rustfmt` and `clippy` components. This is
separate from the workspace's declared Rust 1.85 MSRV in `Cargo.toml`.

Run the same local quality gates as CI with:

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --locked --workspace --all-features --no-deps
```

## Run live captions on Ubuntu

Use the repository-pinned Rust toolchain on a PipeWire-based desktop and
install the native development prerequisites on Ubuntu 24.04 or newer:

```sh
sudo apt install build-essential clang cmake libadwaita-1-dev libgtk-4-dev \
  libpipewire-0.3-dev libspa-0.2-dev libwayland-dev meson ninja-build \
  pkg-config wayland-protocols
scripts/install-gtk4-layer-shell.sh
```

The installer skips its checksum-verified source build when the system already
provides GTK4 layer shell 1.0.4 or newer. Ubuntu 24.04 does not package the GTK4
library; later Ubuntu releases may provide `libgtk4-layer-shell-dev` directly.

Download the checksum-verified tiny English model, build, and launch the native
application:

```sh
./scripts/download-whisper-model.sh
cargo build -p lcrt-app --bin lcrt
cargo run -p lcrt-app --bin lcrt -- --model models/ggml-tiny.en.bin --language en
```

`LCRT_MODEL_PATH` may be set instead of passing `--model`. The application does
not download a model implicitly and never sends captured audio to a remote
service.

In the window, select a source labeled **Microphone** for spoken input or
**System audio** for sound playing through the selected output sink, then press
Start. Partial captions replace themselves as recognition improves; Stop first
ends PipeWire capture and then flushes the final local transcript. Model,
device, capture, and transcription failures are shown in the window.

For source IDs and bounded diagnostics:

```sh
cargo run -p lcrt-app --bin lcrt -- --list-sources
cargo run -p lcrt-app --bin lcrt -- \
  --model models/ggml-tiny.en.bin --smoke-source SOURCE_ID --smoke-seconds 10
```

The caption surface starts translucent and exposes opacity, width, and height
controls. On Wayland compositors that advertise layer-shell protocol v4 or
newer, it is anchored near the bottom in the overlay layer; the window reports
`Pinned overlay`. The explicit size controls preserve resizing because layer
surfaces do not have compositor-provided resize handles. GNOME Wayland, X11,
and older protocol versions use `Standard window`; transparency remains, but
LCRT cannot enforce always-on-top there.

Current limitations: the tiny model is CPU-only and English-focused; source
discovery occurs at launch; and responsiveness depends on model, CPU, language,
and audio conditions. The bounded smoke option is intended for diagnostics,
not normal use.

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

### Native caption UI development

The Ubuntu window uses GTK4 and libadwaita. Install `libgtk-4-dev` and
`libadwaita-1-dev`, then launch its incremental-caption demonstration with:

```sh
cargo run -p lcrt-ui-gtk --bin lcrt-caption-ui
```

The window has bounded Start/Stop actions, partial/final status, inline errors,
resizing, selectable caption text, and a 16–64 point font control. Its
`--smoke-test` mode injects deterministic partial/final updates and closes
itself; it does not exercise audio capture. The end-to-end milestone wires this
presentation layer to the real audio and transcription adapters.

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
