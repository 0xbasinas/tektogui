# Getting started

## Install

Install a recent Rust (1.88+). On this repo:

```bash
git clone https://github.com/0xbasinas/tektogui.git
cd tektogui
git checkout nightly
```

## Run the Notes example

Headless (always works, no window needed):

```bash
cargo run -p notes -- --headless --out notes.png
```

You should get a PNG and a printed tree with a stable `window#1` and the text `Hello from Tekto`.

Windowed, on Linux or Windows:

```bash
cargo run -p notes
```


The window layouts in logical pixels and paints at the current scale factor (HiDPI text stays sharp). Resize relayouts the tree. Closing the window shuts down the dev server (the `tekto-dev.json` endpoint file is removed).


While it runs, in another terminal:

```bash
cargo run -p tekto-cli -- tree
cargo run -p tekto-cli -- screenshot shot.png
cargo run -p tekto-cli -- patch 4 --text "Hello from an agent"
```

(Node ids can differ slightly if the tree shape changes; use `tekto tree` to see them.)

## Start a new app

From the tektogui repo (or any folder, if `TEKTO_ROOT` points at it):

```bash
cargo run -p tekto-cli -- new hello
cd hello
cargo run -- --headless --out hello.png
```

That writes a compiling Rust app, a `src/app.tsx` sketch, and a README. The app depends on this checkout by path, so you only need a Rust toolchain.

`npm create tektogui hello` (or `npx create-tektogui hello`) does the same thing by calling `tekto new`.

## Write a tiny Rust app

```rust
use tekto::{notes_tree, run, WindowOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tree = notes_tree(640.0, 400.0)?;
    run(tree, WindowOptions { title: "Notes".into(), width: 640, height: 400, dev_server: true })?;
    Ok(())
}
```

You can also build a tree yourself with `tekto::window`, `tekto::banner`, and `tekto::text`.

## TypeScript

The JSX API is sketched in `examples/notes/src/app.tsx`. The signals package already works:

```bash
npm test
```

Wiring JSX into the live Rust tree is a Month 2 job.

## Tests

```bash
cargo test --workspace
npm test
```

Set `TEKTO_SHOT_DIR` to choose where a copy of the PNG is written. The test also compares bytes to the blessed reference at `crates/tekto/tests/snapshots/notes-headless.png`. After an intentional visual change:

```bash
TEKTO_BLESS=1 cargo test -p tekto --test headless_snapshot
```

Commit the updated reference PNG. Do not bless a change you did not mean to make.

## Profiles

- `cargo test` / `cargo run` uses `dev` (incremental, `debug = 1`).
- `cargo run --profile dev-fast -p notes` turns on `opt-level = 1` for a slightly snappier local window.
- `cargo build --release -p notes` is for a small binary (`thin` LTO).

See [dev-builds.md](dev-builds.md) for default features, what is *not* in the default crate graph, and recorded CI compile times.

## Targets

- Linux: windowed + headless. This is what everyday CI runs.
- Windows: the CPU window path (`winit` + `softbuffer` + `tiny-skia`) is the same code as Linux. Check it with `cargo check --target x86_64-pc-windows-msvc` (the MSVC target, not MinGW). To *run* a window you need a Windows machine with the Visual Studio Build Tools (MSVC + Windows SDK) and a linker. MinGW is not used. A real window has not been run from this Linux box; see issue #3.
- Android: package Notes as a `NativeActivity` APK with cargo-apk2 (MIT OR Apache-2.0; no Gradle). The host `tekto` crate still compiles CPU-only; the APK enables the desktop/winit shell only for `target_os = "android"`:

  ```bash
  source ~/.android-env.sh   # if present; sets ANDROID_HOME and a JDK
  export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-$ANDROID_HOME/ndk/27.1.12297006}"
  rustup target add aarch64-linux-android x86_64-linux-android
  cargo check -p tekto --target aarch64-linux-android --no-default-features --features cpu
  cargo check -p notes-android --target aarch64-linux-android
  cargo apk2 build -p notes-android
  # or: cargo run -p tekto-cli -- build --target android
  # run on a device/AVD: cargo run -p tekto-cli -- dev --android
  ```

  SDK root is `~/Android/Sdk` (or `$ANDROID_HOME`). This repo uses NDK `27.1.12297006`. `android_main` starts the windowed Notes app; the surface is created on resume and dropped on suspend. Details: [examples/notes-android/README.md](../examples/notes-android/README.md) and [the android-build-and-run skill](../.agents/skills/android-build-and-run/SKILL.md).
- Web: not built yet; the platform and JS layers are shaped so a WASM + canvas / WebGPU backend can plug in.
