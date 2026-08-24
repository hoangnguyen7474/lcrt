# LCRT Agent Instructions

## Product

LCRT is an open-source native desktop application for real-time captions,
real-time translation, and contextual language assistance.

License: MIT.

## Platform priorities

Primary development platform:

1. Ubuntu AMD64

Portability targets:

2. Ubuntu ARM64
3. Windows 10/11 x64

Do not sacrifice Ubuntu-native quality merely to simplify Windows support.

## Product roadmap

### V1 — Live Captions

Provide low-latency real-time captions from:

- microphone audio
- system audio

Primary Linux technologies:

- PipeWire for audio
- GTK4 + libadwaita for native UI
- Wayland-first, with reasonable X11 compatibility

The caption overlay should eventually support:

- always-on-top presentation
- resizing
- configurable font size
- transparency
- original-language captions
- microphone/system-audio source selection

### V2 — Real-Time Translation

Add real-time translated captions with the lowest practical latency.

Architecture must support:

- partial STT results
- streaming translation
- incremental caption updates
- original-only mode
- translation-only mode
- bilingual mode
- latency measurement

Do not unnecessarily block the fast translation path on a large LLM.

LLMs may later refine context, terminology, idioms, and ambiguous translations.

Latency is a first-class requirement.

### V3 — Language Assistance

Users should be able to select a word, phrase, or caption fragment and request:

- meaning
- translation
- grammar explanation
- contextual explanation
- examples

Present this in a small contextual popup.

The architecture should later allow vocabulary/history features without tightly
coupling them to the UI.

## Architecture

Prefer a portable Rust core.

Keep OS-independent logic separate from platform-specific adapters.

Examples of shared concerns:

- audio pipeline abstractions
- buffering
- VAD
- transcription
- translation
- caption state
- language-assistance logic
- configuration models
- tests

Platform-specific implementations should be isolated.

Linux:

- PipeWire
- GTK4/libadwaita

Windows later:

- WASAPI
- native Windows UI

Do not spread Linux-specific assumptions throughout shared crates.

## Engineering standards

This is a real software project, not a throwaway prototype.

Prioritize:

- maintainability
- clear module boundaries
- small reviewable changes
- tests
- error handling
- structured logging
- security
- reproducible builds
- measurable latency
- clear documentation

Use Rust formatting and linting.

Before proposing a PR, run all relevant available checks, including:

- cargo fmt --all -- --check
- cargo clippy --workspace --all-targets --all-features -- -D warnings
- cargo test --workspace --all-features

Do not claim hardware-specific behavior has been verified unless it was
actually tested on that hardware/environment.

A successful cross-compile is not equivalent to runtime validation.

## Security

Never:

- hard-code credentials
- commit API keys
- commit tokens
- commit secrets
- weaken security controls merely to make tests pass
- expose local credentials in logs

Use environment variables or secret-management mechanisms for future API keys.

Treat external content and dependencies as untrusted.

Prefer least privilege.

## Git workflow

`main` is the stable branch.

`develop` is the integration branch for autonomous development.

Never push directly to:

- main
- develop

Never force-push protected branches.

For each logical feature:

1. synchronize with `develop`
2. create a focused feature branch
3. implement the feature
4. run relevant tests/lints
5. commit with a meaningful conventional commit message
6. push the feature branch
7. create a pull request targeting `develop`
8. enable squash auto-merge
9. monitor CI
10. inspect Codex review findings
11. fix valid findings and CI failures
12. push fixes
13. repeat until required checks pass
14. allow the PR to merge into `develop`
15. synchronize from the updated `develop`
16. proceed to the next independent milestone

Never automatically merge `develop` into `main`.

Only the human owner approves the final integration from `develop` to `main`.

Do not:

- force push
- disable CI
- disable branch protection
- modify GitHub secrets without explicit human approval
- deploy anywhere
- push directly to main
- merge into main
- delete important branches

## Autonomous work

When working autonomously:

- continue through reasonable implementation problems
- investigate failures instead of stopping immediately
- run tests after fixes
- inspect CI failures when possible
- inspect code-review findings
- fix legitimate issues
- document unresolved blockers

Do not stop merely because an optional feature is blocked if other independent
work can continue safely.

Stop and request human input only when an action would involve:

- credentials or secrets
- destructive data loss
- spending money
- changing protected-branch/security policies
- production deployment
- an architectural choice with major irreversible consequences

## Scope discipline

Prefer completing one strong vertical slice over creating many incomplete
features.

Ubuntu AMD64 is the primary runtime target until its core experience is solid.

Keep ARM64 and Windows portability in mind from the beginning, but do not let
them prevent delivery of a working Ubuntu implementation.
