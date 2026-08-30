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

## Engineering operating principles

Agents must prefer the smallest correct solution that preserves product quality,
security, maintainability, and future portability.

### Subtract before adding

Before introducing new code, abstractions, dependencies, state, configuration,
or files:

1. inspect whether existing code can be simplified, reused, corrected, or removed
2. remove dead, obsolete, duplicated, unreachable, or superseded code when doing
   so is safe and directly related to the task
3. prefer modifying an existing clear abstraction over creating a parallel one
4. introduce new abstractions only when they remove real duplication, isolate a
   real boundary, or encode a real domain concept

Do not create speculative infrastructure for hypothetical future requirements.

Do not add:
- unused extension points
- unnecessary traits or wrapper types
- placeholder modules
- duplicate helpers
- configuration that has no current consumer
- generalized frameworks for a single concrete use case
- future-proofing code with no present requirement

Prefer the smallest sufficient diff, not merely the smallest textual diff.

A slightly larger change is justified when it removes the root cause, reduces
overall complexity, or establishes a necessary architectural boundary.

Before finalizing a PR, explicitly inspect the diff for code that can be deleted
or simplified.

### Model the domain before coding

Do not begin substantial implementation while the relevant data model, state
transitions, ownership, and domain rules are still ambiguous.

Before implementing non-trivial behavior, determine:

- what the core entities and value types are
- which state is authoritative
- which state is derived
- valid and invalid state transitions
- ownership and lifecycle boundaries
- concurrency assumptions
- failure and cancellation semantics
- boundedness requirements
- which invariants should be represented by types

Prefer making invalid states difficult or impossible to represent.

Do not encode unclear domain rules as scattered conditionals.

For stateful or concurrent features, reason about the state machine explicitly
before modifying implementation code.

### Experience first

Optimize for the actual end-user experience, not theoretical elegance in
isolation.

For user-facing behavior, consider the complete flow:

user action
→ system response
→ visible feedback
→ error/recovery behavior
→ completion/cancellation

Prefer:
- predictable behavior
- low perceived latency
- useful feedback
- clear errors
- safe cancellation
- stable UI state

Do not preserve an elegant internal abstraction if it produces confusing,
fragile, slow, or incorrect user behavior.

User experience does not override correctness, privacy, security, or data
integrity.

### Boundary and type discipline

Keep domain logic, UI, infrastructure, and platform-specific integration
separated.

Use the type system to encode meaningful invariants where practical.

Do not:
- move business/domain rules into GTK callbacks
- expose PipeWire details through portable core APIs
- couple Whisper implementation details to UI state
- pass loosely structured strings when a meaningful typed state or error is
  appropriate
- duplicate the same state in multiple layers without a clear authority

Prefer explicit typed boundaries over hidden shared state.

Platform-specific behavior belongs behind platform-specific adapters.

### Prove it works

"Implemented", "compiled", "CI passed", and "runtime verified" are different
claims.

Never report a task as fully complete without evidence appropriate to the claim.

For every substantial PR, explicitly distinguish what is implemented,
unit-tested, CI-tested, runtime-tested, and hardware-tested. State the exact
evidence for each category and any category that remains unverified.

When practical, verification should include:

- reproduce the bug or failure before fixing it
- add a deterministic regression test
- run relevant unit/integration tests
- run the real affected execution path
- verify behavior on the actual supported runtime environment when available
- document exact manual verification steps
- verify failure and cancellation paths, not only the happy path

For performance work:

- establish a before measurement
- make the change
- measure again under comparable conditions
- report the actual numbers and methodology

Do not describe compile-only validation as runtime validation.

Do not describe cross-compilation as hardware validation.

Do not claim a hardware/device-specific scenario was tested when that hardware
or scenario was unavailable.

If real runtime verification cannot be performed, state exactly what was tested
and what remains unverified.

### Work in independently verifiable units

Sequence implementation so each meaningful step can be reviewed and verified
independently.

Prefer:

one domain change
→ focused tests
→ verification
→ next dependent change

over large batches of unrelated modifications.

Each PR should have one primary engineering purpose.

Do not combine unrelated cleanup, refactoring, features, dependency upgrades,
and behavior changes merely because they are convenient to perform together.

If a larger architectural change is necessary, divide it into coherent stages
with explicit invariants between stages.

### Fix root causes

Do not stop at suppressing symptoms.

For defects:

1. reproduce or precisely reason about the failure
2. identify the violated invariant or root cause
3. fix that cause at the correct architectural layer
4. add regression coverage
5. check adjacent paths that rely on the same invariant

Avoid:
- arbitrary retries that hide deterministic failures
- sleeps used to mask race conditions
- huge queue increases used to hide backpressure problems
- generic catch-all errors that discard actionable causes
- disabling checks to make CI green
- special cases that bypass a broken state model

A workaround is acceptable only when the underlying limitation is external or
cannot reasonably be fixed in scope, and the limitation must then be documented.

### Minimize reader load

Code should be easy for another engineer to understand without reconstructing
hidden assumptions.

Prefer:
- straightforward control flow
- descriptive domain names
- local reasoning
- explicit ownership
- small focused functions
- comments explaining why, not restating what
- limited state mutation
- few abstraction layers

Avoid:
- clever code
- premature genericization
- deep wrapper chains
- hidden side effects
- unnecessary indirection
- boolean combinations that conceal a state machine
- helper functions used only to disguise complexity

An abstraction must reduce total cognitive load. If it makes the code harder to
follow than the concrete implementation, do not introduce it.

### Final simplification pass

Before declaring implementation complete, perform a final simplification review.

Ask:

- Can any newly added code be removed?
- Did this PR create duplicate concepts?
- Is every new abstraction currently justified?
- Can state or configuration be reduced?
- Are there unnecessary dependencies?
- Is there dead code after the fix?
- Is the root cause actually fixed?
- Is the user-facing flow correct?
- Are important invariants encoded and tested?
- What exact evidence proves the change works?

Do not refactor unrelated code merely to make the diff aesthetically smaller or
cleaner.

The objective is minimum necessary complexity, not minimum line count.

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

1. synchronize cleanly from `develop`
2. create one focused feature or fix branch
3. implement the assigned change
4. run the relevant local quality gates
5. commit with a meaningful conventional commit message
6. push the feature branch
7. create a pull request targeting `develop`
8. wait for CI to complete
9. wait for automated Codex review to complete
10. inspect every review finding
11. fix every valid blocking P1/P2 finding
12. add regression tests where appropriate
13. push corrective commits
14. repeat CI and review as necessary
15. reply to and explicitly resolve or disposition review threads
16. verify all required CI checks are green
17. verify that no valid blocking review finding remains
18. only then enable squash auto-merge
19. verify the merge into `develop`
20. synchronize `develop` and prune stale remote references
21. stop, or proceed only when the human-assigned task explicitly allows another
    milestone

CI being green alone is not sufficient for merge. Automated review must finish
before auto-merge is enabled, and agents must not merge with unresolved valid
P1/P2 findings.

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
