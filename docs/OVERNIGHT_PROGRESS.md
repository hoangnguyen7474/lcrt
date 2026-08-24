# Overnight Progress

Updated: 2026-08-25 (Asia/Ho_Chi_Minh)

## Completed milestones

- Repository foundation (pre-existing, PR #3).
- Milestone 1 — Application architecture (PR #4).
- Milestone 2 — Real Linux PipeWire audio (PR #5).
- Milestone 3 — Local real-time speech-to-text (PR #6).
- Milestone 4 — Native Ubuntu caption UI (PR #7).
- Milestone 5 — End-to-end Ubuntu V1 (PR #8).
- Milestone 6 — Quality and latency pass (PR #9).
- Milestone 7 — Portability preparation (PR #10).
- Final overnight report (PR #11).

## Merged PRs

- #1, #2, #3 (pre-existing repository setup).
- #4 — portable application architecture.
- #5 — bounded real PipeWire microphone/system-output capture.
- #6 — bounded local whisper.cpp speech-to-text.
- #7 — native GTK4/libadwaita caption window and bounded UI bridge.
- #8 — runnable PipeWire → Whisper → GTK live-caption application.
- #9 — sustained-load STT catch-up, PCM ownership transfer, and stage timing.
- #10 — compile-only portable-core checks for Ubuntu ARM64 and Windows x64.
- #11 — final factual overnight report.

## Current milestone

- Terminal state reconciliation (`docs/finalize-overnight-state`): awaiting PR/CI.

## Tests actually run

- Milestone 1: `cargo fmt --all -- --check` passed.
- Milestone 1: `cargo clippy --workspace --all-targets --all-features -- -D warnings` passed.
- Milestone 1: `cargo test --workspace --all-features` passed (8 unit tests, 0 failures).
- Milestone 1: rustdoc with warnings denied passed; `git diff --check` passed.
- Milestone 2: formatting, Clippy with warnings denied, workspace tests (14 unit tests, 0 failures), rustdoc with warnings denied, and `git diff --check` passed.
- Milestone 2 PR #5 CI attempt 1 failed during Clippy because the binding requested PipeWire 1.2 headers on Ubuntu 24.04's PipeWire 1.0 development environment. The feature floor was lowered to 1.0; CI attempt 2 passed formatting, Clippy, and tests.
- Milestone 3: formatting, Clippy with warnings denied, workspace tests (19 unit tests, 0 failures), rustdoc with warnings denied, downloader shell syntax, and `git diff --check` passed locally. The STT crate compiles whisper.cpp on Ubuntu AMD64 with Rust 1.85-compatible `whisper-rs` 0.15.1. PR #6 CI passed formatting, Clippy, and tests.
- Milestone 4: GTK/libadwaita workspace check, formatting, Clippy with warnings denied, workspace tests (22 unit tests, 0 failures), rustdoc with warnings denied, and `git diff --check` passed locally. PR #7 CI passed formatting, Clippy, and tests.
- Milestone 5: workspace check, formatting, Clippy with warnings denied, workspace tests (24 unit tests, 0 failures), rustdoc with warnings denied, and `git diff --check` passed locally. PR #8 CI passed formatting, Clippy, and tests.
- Milestone 6: workspace check/build, formatting, Clippy with warnings denied, workspace tests (24 unit tests, 0 failures), rustdoc with warnings denied, and `git diff --check` passed locally.
- Milestone 6 PR #9 CI passed formatting, Clippy, and tests.
- Milestone 7: workflow YAML parsing, host `lcrt-core` locked check, formatting, Clippy with warnings denied, workspace tests (24 unit tests, 0 failures), rustdoc with warnings denied, and `git diff --check` passed locally.
- Milestone 7 PR #10 CI passed the full Ubuntu AMD64 formatting/Clippy/test gate plus compile-only `lcrt-core` checks for `aarch64-unknown-linux-gnu` and `x86_64-pc-windows-msvc`.
- Final report: formatting, Clippy with warnings denied, workspace tests (24 unit tests, 0 failures), rustdoc with warnings denied, and `git diff --check` passed locally.
- Final report PR #11 CI passed the full Ubuntu AMD64 gate plus both compile-only portable-core target checks.

## Runtime verification actually performed

- Milestone 1: no hardware/runtime behavior tested (architecture-only).
- Milestone 2 reconnaissance: `wpctl status` and `pw-cli info 0` connected to PipeWire 1.6.2; one built-in analog microphone source and one analog stereo sink were present.
- Milestone 2 adapter: source enumeration returned the microphone and system-output target. A bounded 3-second microphone capture received 141 chunks/144,384 frames at 48 kHz stereo (peak 0 because the host source was muted). A bounded 3-second system-output capture received 142 chunks/145,408 frames at 48 kHz stereo (peak 0.990387). Both stopped cleanly.
- After lowering the PipeWire API feature floor, a bounded 1-second system-output capture received 47 chunks/48,128 frames at 48 kHz stereo (peak 0.354764) and stopped cleanly.
- Post-smoke inspection found no remaining LCRT PipeWire node or capture process.
- Milestone 3: the CPU-only tiny English model transcribed the official 11-second, 16 kHz mono JFK sample to incremental partials and the correct final sentence. A warm direct diagnostic run took 8.75 seconds elapsed with 223,400 KiB maximum RSS. This is a local diagnostic measurement, not live end-to-end latency.
- Milestone 4: the GTK4/libadwaita caption window launched on the active Ubuntu GNOME Wayland session, processed deterministic incremental/final caption events, and exited on its own; the final run completed in 4.3 seconds. The initial smoke run exposed GTK command-line parsing of `--smoke-test`; the app now consumes diagnostic arguments before GTK, and subsequent runs passed. No UI process remained afterward. Visual appearance was not screenshot-verified.
- Milestone 5: integrated source discovery returned the real built-in microphone and system-output sink. A five-second microphone → local Whisper → GTK run processed 230 chunks and zero captions because the host microphone remained muted, then stopped cleanly. An 18-second system-output run played the official 11-second JFK WAV through PipeWire, processed 841 chunks, published 12 caption updates, and exited cleanly.
- The first integrated system-output run reported 37 audio chunks dropped during Whisper flush because capture was stopped afterward. Core shutdown ordering was changed to stop capture before STT flush; the identical final run processed 841 chunks/12 updates with no drop warning. A bounded invalid-model run surfaced the actionable missing-model error and exited cleanly.
- Post-run `wpctl` and process inspection found no remaining LCRT PipeWire node, integrated app, or standalone UI process.
- Milestone 6: an instrumented 20-second system-output run first reproduced a sustained-load failure at 825 chunks when redundant Whisper passes filled the 256-chunk queue. The worker now drains queued audio into the newest rolling window before one inference pass and transfers PCM ownership without a full-buffer clone. The identical fixed run completed with 935 chunks, 10 inference passes, 9 UI updates, and no overflow.
- Fixed-run stage measurements: capture-to-pipeline 68 us median/141 us p95 (935 samples); Whisper inference 1.940 s median/2.586 s p95 (10 samples); caption-state update 1 us median/2 us p95; UI enqueue 4 us median/8 us p95; GTK queue 8.139 ms median/10.998 ms p95 (9 samples). These are instrumented local measurements, not speech-onset-to-caption latency.
- A 35.36-second system-output soak played the 11-second JFK sample twice, processed 1,498 chunks/14 caption updates, exited successfully with no queue warning, and peaked at 350,876 KiB RSS. Post-run inspection found no LCRT process or PipeWire node.
- Milestone 7: no ARM64 or Windows runtime verification was performed; the added checks are compile-only and cover `lcrt-core`, not platform adapters or complete applications.

## Known blockers

- GitHub CLI authentication is invalid (not blocking: the authenticated GitHub connector handles PR/review/CI operations, and Git over SSH works).

## Next planned milestone

- Merge the terminal factual state update, synchronize `develop`, and end the overnight run.
