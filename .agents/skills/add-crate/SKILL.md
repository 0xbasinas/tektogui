---
name: add-crate
description: Add a Rust crate to the tektogui workspace while keeping the crate graph lean, default features light, and incremental builds fast. Use before creating a new crates/* package or adding a heavy dependency.
---

# Add a crate

One crate does one job. Put new code in an existing crate if that crate already owns the job (`tekto-tree`, `tekto-layout`, `tekto-text`, `tekto-render`, `tekto-platform`, `tekto-a11y`, `tekto-mcp`, `tekto-js`, `tekto-native`, `tekto-widgets`, `tekto`, `tekto-cli`, `tekto-wasm`). `tekto-wasm` is only the wasm32 cdylib. Do not depend on it from `tekto`.

## Files to touch

- `Cargo.toml` (workspace `members` and `[workspace.dependencies]`)
- `crates/<name>/Cargo.toml` and `crates/<name>/src/lib.rs`
- Downstream `Cargo.toml` files that should depend on it (usually `crates/tekto`)
- This skill, if the workspace rules change

## Rules

- License: MIT, Apache-2.0, BSD, or zlib only. Write the new dep's license in the PR.
- Default features stay lean. Do not add wgpu, Vello, QuickJS, fontconfig, or `accesskit_winit` to the default graph. Optional heavy work goes behind a feature that is **off by default**. The `gpu` feature (wgpu 29 + Vello 0.10) is that gate for the GPU renderer; leave it off on PR CI.
- Parley must stay `{ version = "0.12", default-features = false, features = ["std"] }` with bundled Roboto (`system_fonts: false`).
- Pin `skrifa` to `0.44` so it matches Parley (do not jump to a newer skrifa).
- Use `workspace = true` for internal crates and shared deps.
- `publish = false` until we actually publish.
- Profiles already exist: `dev` (`debug = 1`, `incremental = true`), `dev-fast` (`opt-level = 1`), `release` (`lto = "thin"`, `codegen-units = 1`, `strip = "symbols"`). Do not raise `opt-level` on deps in `dev`; that slows compile.

## Steps

1. Create `crates/<name>/Cargo.toml` with `version.workspace`, `edition.workspace`, `license.workspace`, and a short description.
2. Add the path to workspace `members` and to `[workspace.dependencies]` as `{ path = "crates/<name>", version = "0.1.0" }`.
3. Depend on it only where needed. If it is optional (JS engine, GPU), add a feature on `tekto` similar to `gpu` / `js` and do not put it in `default`.
4. Keep the public API small. Re-export from `tekto` only if app authors should see it.

## Verify

```bash
cargo tree -p tekto --edges normal,features
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
# Optional: time a one-line-edit rebuild
cargo check -p notes
```

The default `cargo tree -p tekto` must not suddenly include wgpu, Vello, or QuickJS.
