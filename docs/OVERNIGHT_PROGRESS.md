# Overnight Progress

Updated: 2026-08-25 (Asia/Ho_Chi_Minh)

## Completed milestones

- Repository foundation (pre-existing, PR #3).
- Milestone 1 — Application architecture (PR #4).

## Merged PRs

- #1, #2, #3 (pre-existing repository setup).
- #4 — portable application architecture.

## Current milestone

- Milestone 2 — Real Linux PipeWire audio (`feat/linux-pipewire-audio`): in progress.

## Tests actually run

- Milestone 1: `cargo fmt --all -- --check` passed.
- Milestone 1: `cargo clippy --workspace --all-targets --all-features -- -D warnings` passed.
- Milestone 1: `cargo test --workspace --all-features` passed (8 unit tests, 0 failures).
- Milestone 1: rustdoc with warnings denied passed; `git diff --check` passed.
- Milestone 2 (in progress): formatting, Clippy with warnings denied, workspace tests (14 unit tests, 0 failures), rustdoc with warnings denied, and `git diff --check` passed.

## Runtime verification actually performed

- Milestone 1: no hardware/runtime behavior tested (architecture-only).
- Milestone 2 reconnaissance: `wpctl status` and `pw-cli info 0` connected to PipeWire 1.6.2; one built-in analog microphone source and one analog stereo sink were present.
- Milestone 2 adapter: source enumeration returned the microphone and system-output target. A bounded 3-second microphone capture received 141 chunks/144,384 frames at 48 kHz stereo (peak 0 because the host source was muted). A bounded 3-second system-output capture received 142 chunks/145,408 frames at 48 kHz stereo (peak 0.990387). Both stopped cleanly.
- Post-smoke inspection found no remaining LCRT PipeWire node or capture process.

## Known blockers

- GitHub CLI authentication is invalid (not blocking: the authenticated GitHub connector handles PR/review/CI operations, and Git over SSH works).

## Next planned milestone

- Implement bounded PipeWire microphone/system-output capture and source enumeration, then run local hardware smoke tests and deliver Milestone 2 through PR/CI/merge.
