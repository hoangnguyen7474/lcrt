# Overnight Development Report

Date: 2026-08-25 (Asia/Ho_Chi_Minh)

## Outcome

LCRT now has an implemented and locally runtime-tested Ubuntu AMD64 V1 vertical
slice: native PipeWire microphone/system-output capture feeds bounded local
Whisper transcription, portable caption state, and a translucent
GTK4/libadwaita caption window. The window uses a pinned layer-shell overlay
when the compositor supports it and an explicit standard-window fallback
otherwise. No change was merged into `main`.

## Completed milestones and merged PRs

- Repository setup: PRs #1–#3.
- Milestone 1, portable application architecture: PR #4.
- Milestone 2, real Linux PipeWire audio: PR #5.
- Milestone 3, bounded local Whisper speech-to-text: PR #6.
- Milestone 4, native Ubuntu caption UI: PR #7.
- Milestone 5, end-to-end Ubuntu V1 application: PR #8.
- Milestone 6, quality and latency pass: PR #9.
- Milestone 7, compile-only portable-core checks: PR #10.
- Final factual report delivery: PR #11.
- Terminal state reconciliation: PR #12.
- Milestone 8, native overlay presentation: implemented locally; PR/CI pending.

## Exact features implemented

- Portable Rust ports and domain types for audio capture, transcription,
  incremental/final captions, UI publication, configuration, cancellation,
  typed errors, and structured logging.
- Real PipeWire source enumeration and bounded F32LE capture for microphones and
  output-sink monitors, with timeouts, explicit stop, and a diagnostic CLI.
- Local `whisper.cpp` transcription through `whisper-rs` 0.15.1, including
  stereo downmix, 16 kHz resampling, energy gating, an eight-second rolling
  window, partial/final updates, bounded worker queues, and checksum-verified
  tiny-English model download tooling.
- Native GTK4/libadwaita UI with source selection, Start/Stop, partial/final and
  lifecycle status, inline errors, selectable wrapped captions, resizing, and a
  16–64 point font control.
- Runnable `lcrt` application wiring PipeWire -> Whisper -> caption state -> GTK
  while keeping model loading, capture, and inference off the GTK thread.
- Explicit model configuration by `--model` or `LCRT_MODEL_PATH`, optional
  language selection, source listing, bounded smoke mode, actionable failures,
  and practical Ubuntu build/run documentation.
- Stop-before-final-flush shutdown, PCM ownership transfer without a full-buffer
  STT clone, backlog coalescing to avoid redundant stale inference, and
  privacy-safe stage timing that does not log caption text.
- CI compile-only checks of `lcrt-core` for Ubuntu ARM64 and Windows x64 targets.
- Adjustable 30–100% caption-surface opacity, capability-based Wayland
  layer-shell placement in the overlay layer, bottom anchoring, on-demand
  keyboard focus, and an honest in-window pinned/standard presentation status.

## Local verification actually performed

### Ubuntu AMD64 — runtime-tested

- PipeWire 1.6.2 source discovery found one built-in analog microphone and one
  analog stereo output sink.
- Microphone capture, three seconds: 141 chunks/144,384 frames at 48 kHz stereo;
  peak was zero because the host microphone was muted. A later five-second full
  application run processed 230 chunks and no captions for the same reason.
- System-output capture, three seconds: 142 chunks/145,408 frames at 48 kHz
  stereo with peak 0.990387. A one-second follow-up after the PipeWire API-floor
  fix received 47 chunks/48,128 frames with peak 0.354764.
- The tiny-English model transcribed the official 11-second JFK WAV to seven
  incremental updates and the correct final sentence. A warm direct diagnostic
  run took 8.75 seconds and peaked at 223,400 KiB RSS; this measured offline
  diagnostic throughput, not live end-to-end latency.
- The GTK window launched on the active GNOME Wayland session, consumed
  deterministic partial/final events, and self-terminated in 4.3 seconds.
  Appearance was not screenshot-verified.
- End-to-end system-output run: the app played the JFK WAV through PipeWire,
  processed 841 chunks, published 12 caption updates, and exited cleanly. A
  shutdown-order defect that dropped 37 queued chunks was reproduced, fixed,
  and absent in the identical final run.
- The missing-model path produced an actionable in-window failure and a clean
  bounded exit.
- Milestone 6 first reproduced a sustained-input overflow at 825 chunks. The
  fixed identical run processed 935 chunks, 10 inference passes, and 9 UI
  updates without overflow.
- A 35.36-second soak played the JFK WAV twice, processed 1,498 chunks and 14
  caption updates, emitted no queue warning, exited successfully, and peaked at
  350,876 KiB RSS.
- Post-run process and PipeWire inspection found no remaining LCRT process or
  PipeWire node.
- Milestone 8 GTK smoke testing on GNOME Wayland detected the unsupported
  layer-shell capability and cleanly exercised the standard-window fallback.
  An integrated 15-second replay processed 435 system-output chunks, published
  6 caption updates, and stopped cleanly. CSS loaded without parser errors.
  Transparency was not screenshot-verified, and the pinned-overlay path was not
  runtime-tested because this host compositor does not advertise layer shell.

### Local automated checks

- Formatting, Clippy for all workspace targets/features with warnings denied,
  workspace tests, rustdoc with warnings denied, and `git diff --check` passed
  at each applicable milestone.
- The workspace test count grew from 8 at Milestone 1 to 26 at Milestone 8;
  all recorded runs had zero failures.
- The final Milestone 8 local gate used the repository-pinned stable Rust 1.98.0
  toolchain from `~/.cargo`; formatting, warnings-denied Clippy, all 26 tests,
  rustdoc with warnings denied, and `git diff --check` passed.
- Workflow YAML parsing and a locked host check of `lcrt-core` passed for
  Milestone 7.

## CI verification actually performed

- PRs #4–#12 passed the required Ubuntu 24.04 CI gate before merge: formatting,
  Clippy across all targets/features with warnings denied, and workspace tests.
- PR #5 CI initially failed because the bindings requested PipeWire 1.2 headers
  while Ubuntu 24.04 supplied PipeWire 1.0 development headers. The feature
  floor was corrected to 1.0 and the next CI attempt passed.
- PR #10 additionally passed `cargo check --locked -p lcrt-core` for
  `aarch64-unknown-linux-gnu` and `x86_64-pc-windows-msvc`.
- CI did not exercise PipeWire hardware, Whisper model inference, GTK display,
  or any platform runtime.
- Milestone 8 CI is pending; no CI result is claimed for its overlay changes.

## Measured latency and performance

All values below are from one instrumented Ubuntu AMD64 system-output run after
the sustained-load fix:

| Stage | Samples | Median | p95 |
| --- | ---: | ---: | ---: |
| Capture chunk construction to pipeline | 935 | 68 us | 141 us |
| Whisper inference | 10 | 1.940 s | 2.586 s |
| Caption-state update | 9 | 1 us | 2 us |
| GTK event enqueue | 9 | 4 us | 8 us |
| GTK queue delay | 9 | 8.139 ms | 10.998 ms |

Speech-onset-to-visible-caption latency was not measured. The figures must not
be interpreted as an end-to-end latency claim.

## Platform status

### Ubuntu AMD64

- Implemented: V1 microphone/system-output capture, local STT, native UI, and
  integrated application. Adjustable transparency and the compositor-capable
  overlay path are implemented.
- Runtime-tested: yes, on the local GNOME Wayland/PipeWire desktop as detailed
  above. The standard-window fallback was runtime-tested; pinned layer-shell
  behavior was not.
- CI-tested: yes, on Ubuntu 24.04 for formatting, Clippy, and tests.
- Not tested: audible live microphone transcription, screenshot-level visual
  quality, multi-hour operation, packaging/installers, and X11 runtime.

### Ubuntu ARM64

- Compile-tested only: `lcrt-core` passed CI for
  `aarch64-unknown-linux-gnu`.
- Not tested: full application compilation/linking, PipeWire/GTK/Whisper
  adapters, packaging, and runtime on ARM64 hardware.

### Windows x64

- Compile-tested only: `lcrt-core` passed CI for
  `x86_64-pc-windows-msvc`.
- Not implemented: WASAPI capture and a native Windows UI adapter.
- Not tested: full application compilation/linking, packaging, and Windows
  runtime.

## Known bugs, blockers, and limitations

- The host microphone was muted, so real audible microphone-to-caption behavior
  remains unverified despite successful microphone capture plumbing.
- GNOME Wayland does not advertise layer shell, so it cannot enforce
  always-on-top; LCRT reports and uses its translucent standard-window fallback.
- Pinned-overlay behavior is compile-tested only and still needs runtime testing
  on a compositor that supports `zwlr_layer_shell_v1`.
- Audio sources are discovered at launch; hot-plug refresh is not implemented.
- The current tiny model is CPU-only and English-focused. Inference is the
  dominant measured stage and responsiveness depends on model, CPU, language,
  and audio conditions.
- The UI was runtime-exercised but not screenshot-verified.
- Only a 35.36-second sustained soak was run; multi-hour stability is unknown.
- GitHub CLI authentication is invalid in this environment. This did not block
  work because the authenticated GitHub connector handled PR, CI, review, and
  merge operations while Git over SSH handled branches.

## Unfinished work

- Runtime-test pinned-overlay behavior on a layer-shell compositor and perform
  screenshot-level visual verification of both presentation modes.
- Repeat microphone runtime validation with an audible source and measure true
  speech-onset-to-visible-caption latency.
- Add source hot-plug refresh, longer soak coverage, release packaging, and X11
  runtime verification.
- Build and runtime-test the full application on Ubuntu ARM64.
- Implement and test WASAPI plus native Windows presentation.
- V2 streaming translation and V3 contextual language assistance are not
  implemented.

## Recommended next step

Land the Milestone 8 overlay PR, then validate its pinned mode on a
layer-shell-capable compositor and repeat an audible microphone end-to-end run
with speech-onset-to-visible-caption timing.
