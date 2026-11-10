# Dev builds, compile times, and default features

Tekto is meant to stay fast to compile. This note records what is in a default `cargo test --workspace`, when to use each profile, and CI timings after the Month 1 cache settled.

## Profiles

Defined in the workspace `Cargo.toml`:

- `dev` (default): `debug = 1`, `incremental = true`. Everyday `cargo test` and `cargo run`. Do not raise `opt-level` on dependencies here; that slows compiles.
- `dev-fast`: inherits `dev` and sets `opt-level = 1`. Use for a local demo when you want the window a bit snappier without a full release build: `cargo run --profile dev-fast -p notes`.
- `release`: `lto = "thin"`, `codegen-units = 1`, `strip = "symbols"`. Ship or time a small binary: `cargo build --release -p notes`.

## Default features

`tekto` default features are `cpu` + `desktop`. The default `cargo tree -p tekto` (recorded 2026-11-10) does **not** include wgpu, Vello, QuickJS, `accesskit_winit`, or zbus.

It does include, on purpose:

- in-process AccessKit (`accesskit` only) for the agent UI tree
- Parley + ICU (bundled Roboto, `system_fonts` off, no fontconfig)
- tiny-skia + Skrifa
- winit + softbuffer and the Linux Wayland/X11 stack (from the `desktop` feature)

Off by default (opt in only when you need them):

- `gpu` — Vello 0.10 and wgpu 29.0.4 (both MIT OR Apache-2.0), plus `pollster` 0.4 and `png` 0.18 (same license). Desktop wgpu is Vulkan, GLES, and Metal only (no DX12). The same native backends compile for Android. The wasm `tekto-wasm` feature `gpu` switches wgpu to its WebGPU backend and adds `wasm-bindgen` 0.2.130, `wasm-bindgen-futures` 0.4.80, and `web-sys` 0.3.107 (MIT OR Apache-2.0). Off by default. `cargo run -p notes --features gpu` paints the window on the GPU. `cargo test -p tekto --features gpu --lib gpu_notes` paints the Notes tree when an adapter exists (llvmpipe counts). On Android the default APK stays CPU-only. `notes-android` feature `gpu` only tries the GPU when `TEKTO_ANDROID_GPU=1`, then falls back to CPU if the device cannot be created. Do not enable `gpu` on default tests or PR CI. The web host falls back to the CPU canvas when the flag is off or WebGPU is missing.
- `js` — the `HostOp` protocol only (no engine)
- `quickjs` — embeds QuickJS-ng via `rquickjs` (both MIT). Slow to compile. Do not enable on PR CI. Local check: `cargo test -p tekto-js --features quickjs`. The CLI flag is the same: `cargo run -p tekto-cli --features quickjs -- dev --js examples/notes/src/app.tsx`. Hot reload also needs the MIT `esbuild` binary on `PATH` (not a Rust dependency, not on PR CI).
- `a11y-os` — `accesskit_winit`, which pulls zbus / AT-SPI on Linux
- `tray` — system tray inside `tekto-native` (no extra crate). Linux uses StatusNotifierItem. Windows uses `Shell_NotifyIconW` and `TrackPopupMenu`. Off by default. `cargo test -p tekto-native --features tray`. Windows is compile-only here: `cargo check -p tekto-native --target x86_64-pc-windows-gnu --features tray`. Do not enable it on default tests or PR CI.
- `shortcuts` — global shortcuts inside `tekto-native`. Off by default. Linux is an X11 key grab (`x11rb` 0.13.2, MIT OR Apache-2.0, default features off). Windows is `RegisterHotKey` (no extra crate). `cargo test -p tekto-native --features shortcuts`. Windows compile-check: `cargo check -p tekto-native --target x86_64-pc-windows-gnu --features shortcuts` (no linker, no Windows runtime). Do not enable it on default tests or PR CI. Wayland-only sessions, a denied grab, and a reserved or taken Windows hotkey return `Unavailable`. See `docs/architecture.md`.

The `tekto` crate still Android-checks with `--no-default-features --features cpu` so the desktop/winit shell is not in that graph. The Notes APK (`examples/notes-android`) enables `cpu` + `desktop` only for `target_os = "android"` and is packaged with `cargo apk2` / `tekto build --target android`. The default APK does not enable `quickjs`; `tekto dev --js --android` (or `cargo apk2 build -p notes-android -F quickjs`) embeds QuickJS-ng so a TSX bundle can be posted over `adb forward`. Do not enable `quickjs` on the nightly Android CI check, and do not add APK packaging or an emulator to PR CI.

Nothing unexpected showed up in the default graph that we should gate or drop in Month 1.

## CI caches and times

Linux `fmt` + `clippy -D warnings` + `cargo test --workspace --all-targets` runs on every PR and every push. That one test command is what runs the record/replay integration test and the headless Notes PNG compare. Windows and Android compile jobs run only on `push` to `nightly`, or on `workflow_dispatch` with `full=true`. The WASM job (`cargo check -p tekto-wasm --target wasm32-unknown-unknown`, plus `tekto` with `--no-default-features --features cpu,js`) runs on `push` to `nightly` or any `workflow_dispatch`. It is not on PR CI. Swatinem/rust-cache and concurrency cancel are on.

Times from GitHub-hosted Linux (UTC, job "Linux check", step "Test (includes headless snapshot)"):

| Run | When (UTC) | rust-cache restore | Clippy | `cargo test --workspace` | Linux job wall |
| --- | --- | --- | --- | --- | --- |
| First green after skills (#66 era), [run 42855300189](https://github.com/0xbasinas/tektogui/actions/runs/42855300189) | 2026-11-10 16:31 | ~0 s (cache mostly a miss or already present) | 36 s | 41 s | 94 s |
| Warm (AccessKit, little Rust churn), [run 42856488301](https://github.com/0xbasinas/tektogui/actions/runs/42856488301) | 2026-11-10 16:38 | 6 s | 3 s | 3 s | 43 s (24 s of that was saving the cache) |
| Warm-ish after typography (many crates rebuilt), [run 42860855558](https://github.com/0xbasinas/tektogui/actions/runs/42860855558) | 2026-11-10 17:03 | 8 s | 7 s | 35 s | 57 s |

The first nightly run of the MVP ([42849414759](https://github.com/0xbasinas/tektogui/actions/runs/42849414759)) failed at `rustfmt` before tests, so it is not a usable cold-test number.

Reading: a warm `cargo test --workspace` on CI is a few seconds; after a change that recompiles the core crates it is around half a minute; a colder clippy+test after deps are cached is about 80 s. Windows and Android compile-only jobs stay off the PR path so those minutes are not spent on every change.

## Incremental `cargo check -p notes` (issue #31)

Question: after a small edit, does the crate graph make `cargo check` of a typical app (the Notes example) slow enough to justify a split or a new feature gate?

No code change. Default features stay `cpu` + `desktop`. `js`, `quickjs`, and `gpu` stay off. The `dev` profile stays `incremental = true` and `debug = 1`. A facade split (for example dropping `tekto-widgets` out of `tekto`) would not pay for itself: a widget edit already rechecks only `tekto-widgets`, the thin `tekto` re-export, and `notes`. It does not rebuild winit, Parley, or ICU.

### Machine

- Linux x86_64, KVM guest, 8 vCPU Intel Xeon (family 6, model 207), 15 GiB RAM, no swap
- `rustc 1.99.1 (58cf8f9c3 2026-10-13)`, `cargo 1.99.1`
- `CARGO_TARGET_DIR=/workspace/tektogui-i31/target`
- Profile printed by cargo: `dev` `[unoptimized + debuginfo]` (workspace sets `debug = 1`, so this is line tables, not full debuginfo)

### Commands and times

Deps were already built. Timings are wall clock from `bash` `TIMEFORMAT` (`real`), with cargo's own `Finished ... in` line in parentheses.

```bash
export CARGO_TARGET_DIR=/workspace/tektogui-i31/target
cargo check -p notes
```

| Step | What changed | `cargo check -p notes` |
| --- | --- | --- |
| Warm, no edit | nothing | 0.14 s real (Finished in 0.10 s) |
| App edit | `examples/notes/src/main.rs`, window title `"Notes"` → `"Notes "` | 0.17 s real (0.13 s). Rechecked `notes` only |
| Widget edit | `crates/tekto-widgets/src/lib.rs`, greeting `"Hello from Tekto"` → `"Hello from Tekto!"` | 0.23 s real (0.19 s). Rechecked `tekto-widgets`, `tekto`, `notes` |
| Doc-comment touch | one trailing space on the `notes_tree` doc comment | 0.22 s real (0.18 s). Same three crates |
| Heavier crates (not the acceptance edit) | `LINE_HEIGHT` in `tekto-text` and a comment in `tekto-platform`, plus widgets restored to the pre-edit source so they rechecked too | 0.38 s real (0.33 s). Also rechecked `tekto-render` and `tekto-layout` |

The first check in this worktree, with the registry already populated, finished in 10.4 s real and compiled 233 units. That is a from-scratch `check` of Notes, not an incremental edit.

Probe edits were reverted. They are not in the tree.

### Why nothing was split or gated

- JS is already behind `tekto`'s `js` / `quickjs` features and is not in `cargo check -p notes`.
- `gpu` now depends on wgpu and Vello, but only when that feature is on. A default Notes check still does not build them.
- `a11y-os` (and `accesskit_winit`) stays off. In-process `accesskit` stays, on purpose.
- A one-line Notes or widget edit is well under a second. Gating widgets, or making the example depend on leaf crates instead of `tekto`, would change the default API (`notes_tree`, `window`, `button`) for a gain measured in tens of milliseconds.
- Editing the text or platform crate still does not rebuild the heavy third-party graph; those crates are small wrappers. Their compile cost shows up on a cold check, which feature flags cannot remove from an app that windows and draws text.

Rust hot-patch options (dylib, Cranelift, Subsecond) are in [rust-hot-reload.md](rust-hot-reload.md) (issue #29). None is adopted yet.
