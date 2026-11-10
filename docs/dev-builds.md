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

- `gpu` — reserved; do not add wgpu or Vello to the default graph
- `js` — QuickJS-ng, slow to compile
- `a11y-os` — `accesskit_winit`, which pulls zbus / AT-SPI on Linux

Android compile uses `--no-default-features --features cpu` so the desktop/winit shell is not pulled in.

Nothing unexpected showed up in the default graph that we should gate or drop in Month 1.

## CI caches and times

Linux `fmt` + `clippy -D warnings` + `cargo test --workspace` runs on every PR and every push. Windows and Android compile jobs run only on `push` to `nightly`, or on `workflow_dispatch` with `full=true`. Swatinem/rust-cache and concurrency cancel are on.

Times from GitHub-hosted Linux (UTC, job "Linux check", step "Test (includes headless snapshot)"):

| Run | When (UTC) | rust-cache restore | Clippy | `cargo test --workspace` | Linux job wall |
| --- | --- | --- | --- | --- | --- |
| First green after skills (#66 era), [run 42855300189](https://github.com/0xbasinas/tektogui/actions/runs/42855300189) | 2026-11-10 16:31 | ~0 s (cache mostly a miss or already present) | 36 s | 41 s | 94 s |
| Warm (AccessKit, little Rust churn), [run 42856488301](https://github.com/0xbasinas/tektogui/actions/runs/42856488301) | 2026-11-10 16:38 | 6 s | 3 s | 3 s | 43 s (24 s of that was saving the cache) |
| Warm-ish after typography (many crates rebuilt), [run 42860855558](https://github.com/0xbasinas/tektogui/actions/runs/42860855558) | 2026-11-10 17:03 | 8 s | 7 s | 35 s | 57 s |

The first nightly run of the MVP ([42849414759](https://github.com/0xbasinas/tektogui/actions/runs/42849414759)) failed at `rustfmt` before tests, so it is not a usable cold-test number.

Reading: a warm `cargo test --workspace` on CI is a few seconds; after a change that recompiles the core crates it is around half a minute; a colder clippy+test after deps are cached is about 80 s. Windows and Android compile-only jobs stay off the PR path so those minutes are not spent on every change.
