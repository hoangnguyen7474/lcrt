# Overnight Progress

Updated: 2026-08-25 (Asia/Ho_Chi_Minh)

## Completed milestones

- Repository foundation (pre-existing, PR #3).
- Milestone 1 — Application architecture (PR #4).
- Milestone 2 — Real Linux PipeWire audio (PR #5).
- Milestone 3 — Local real-time speech-to-text (PR #6).

## Merged PRs

- #1, #2, #3 (pre-existing repository setup).
- #4 — portable application architecture.
- #5 — bounded real PipeWire microphone/system-output capture.
- #6 — bounded local whisper.cpp speech-to-text.

## Current milestone

- Milestone 4 — Native Ubuntu caption UI (`feat/native-caption-ui`): in progress.

## Tests actually run

- Milestone 1: `cargo fmt --all -- --check` passed.
- Milestone 1: `cargo clippy --workspace --all-targets --all-features -- -D warnings` passed.
- Milestone 1: `cargo test --workspace --all-features` passed (8 unit tests, 0 failures).
- Milestone 1: rustdoc with warnings denied passed; `git diff --check` passed.
- Milestone 2: formatting, Clippy with warnings denied, workspace tests (14 unit tests, 0 failures), rustdoc with warnings denied, and `git diff --check` passed.
- Milestone 2 PR #5 CI attempt 1 failed during Clippy because the binding requested PipeWire 1.2 headers on Ubuntu 24.04's PipeWire 1.0 development environment. The feature floor was lowered to 1.0; CI attempt 2 passed formatting, Clippy, and tests.
- Milestone 3: formatting, Clippy with warnings denied, workspace tests (19 unit tests, 0 failures), rustdoc with warnings denied, downloader shell syntax, and `git diff --check` passed locally. The STT crate compiles whisper.cpp on Ubuntu AMD64 with Rust 1.85-compatible `whisper-rs` 0.15.1. PR #6 CI passed formatting, Clippy, and tests.
- Milestone 4 (in progress): GTK/libadwaita workspace check, formatting, Clippy with warnings denied, workspace tests (22 unit tests, 0 failures), rustdoc with warnings denied, and `git diff --check` passed locally.

## Runtime verification actually performed

- Milestone 1: no hardware/runtime behavior tested (architecture-only).
- Milestone 2 reconnaissance: `wpctl status` and `pw-cli info 0` connected to PipeWire 1.6.2; one built-in analog microphone source and one analog stereo sink were present.
- Milestone 2 adapter: source enumeration returned the microphone and system-output target. A bounded 3-second microphone capture received 141 chunks/144,384 frames at 48 kHz stereo (peak 0 because the host source was muted). A bounded 3-second system-output capture received 142 chunks/145,408 frames at 48 kHz stereo (peak 0.990387). Both stopped cleanly.
- After lowering the PipeWire API feature floor, a bounded 1-second system-output capture received 47 chunks/48,128 frames at 48 kHz stereo (peak 0.354764) and stopped cleanly.
- Post-smoke inspection found no remaining LCRT PipeWire node or capture process.
- Milestone 3: the CPU-only tiny English model transcribed the official 11-second, 16 kHz mono JFK sample to incremental partials and the correct final sentence. A warm direct diagnostic run took 8.75 seconds elapsed with 223,400 KiB maximum RSS. This is a local diagnostic measurement, not live end-to-end latency.
- Milestone 4: the GTK4/libadwaita caption window launched on the active Ubuntu GNOME Wayland session, processed deterministic incremental/final caption events, and exited on its own; the final run completed in 4.3 seconds. The initial smoke run exposed GTK command-line parsing of `--smoke-test`; the app now consumes diagnostic arguments before GTK, and subsequent runs passed. No UI process remained afterward. Visual appearance was not screenshot-verified.

## Known blockers

- GitHub CLI authentication is invalid (not blocking: the authenticated GitHub connector handles PR/review/CI operations, and Git over SSH works).

## Next planned milestone

- Deliver Milestone 4 through PR/CI/merge, then integrate PipeWire, local Whisper, caption state, and the GTK window into the runnable V1 application.
