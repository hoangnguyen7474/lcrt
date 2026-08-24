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
