---
name: android-build-and-run
description: Package Notes as a NativeActivity APK with cargo-apk2, install it on AVD beddcode30, and talk to the dev server over adb forward. Use when changing the Android window path, the APK metadata, or tekto build --target android / tekto dev --android. Do not add an emulator to CI.
---

# Android build and run

Notes ships as a real APK: `winit` + `android-activity` (`NativeActivity`) + the CPU renderer, packaged by **cargo-apk2** (MIT OR Apache-2.0).

Why cargo-apk2, not Gradle or the older `cargo-apk`:

- Permissive license (MIT OR Apache-2.0), same family as the rest of Tekto.
- Maintained fork of the rust-mobile `cargo-apk` tool, which has stagnated.
- No Gradle / Java / Kotlin project. `android-activity` already provides the NativeActivity glue, which is the leanest path that matches `winit`'s `android-native-activity` feature.
- `[package.metadata.android]` lives next to the crate; `include_cplusplus_shared = true` pulls in `libc++_shared.so` (needed by winit).

A Gradle wrapper would only be worth it later if we need GameActivity, a custom Java Activity, or Play App Signing flows that cargo-apk2 cannot express.

Do not wrap Flutter, Tauri, or Electron.

## Tooling

```bash
source ~/.android-env.sh   # if present; sets ANDROID_HOME, JDK, PATH
export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-$ANDROID_HOME/ndk/27.1.12297006}"
export ANDROID_NDK_ROOT="$ANDROID_NDK_HOME"
rustup target add aarch64-linux-android x86_64-linux-android
cargo install cargo-apk2   # 1.5.x; provides `cargo apk2`
```

- SDK: `~/Android/Sdk` or `$ANDROID_HOME`
- NDK: `27.1.12297006`
- Rust targets: `aarch64-linux-android` (devices) and `x86_64-linux-android` (AVD `beddcode30` is **x86_64**, not ARM)
- rustc 1.95+ is the cargo-apk2 *tool* MSRV, not the app's
- SDK platforms: NDK 27.1 supports API 21..=35. cargo-apk2 needs an installed platform in that range (this box often only has 36+). If it errors, run `sdkmanager "platforms;android-30"` (and optionally `platforms;android-35`).

The host `notes-android` crate stays CPU-only so `android-activity` does not enter the default Linux graph. On `target_os = "android"` it enables `tekto` features `cpu` + `desktop`.

## Package

```bash
# From the repo root. Builds aarch64 + x86_64 by default (see
# examples/notes-android/Cargo.toml [package.metadata.android]).
cargo apk2 build -p notes-android
# or only the AVD ABI:
cargo apk2 build -p notes-android --target x86_64-linux-android

# Same thing through the CLI (finds the notes.apk under target/):
cargo run -p tekto-cli -- build --target android
```

The APK is typically at
`target/<triple>/debug/apk/notes.apk` (or `.../release/apk/` with `--release`).
Package id: `com.tektogui.notes`. Activity: `android.app.NativeActivity`.
The activity *must* declare `android.app.lib_name` = `notes_android` (cargo-apk2 does not insert this unless the key is already in the manifest; without it NativeActivity looks for `libmain.so` and crashes).

`cargo check -p notes-android --target aarch64-linux-android` still works (CI uses this on nightly). `cargo ndk` is optional for a check-only path:

```bash
cargo check -p tekto --target aarch64-linux-android --no-default-features --features cpu
cargo check -p notes-android --target aarch64-linux-android
```

## AVD `beddcode30` (manual only)

Do not put this in PR CI. Do not start the emulator if another heavy build is running, and do not run heavy `cargo test` / compiles in parallel with it. This machine often has `/dev/kvm` but no permission, so you **must** pass `-accel off` (TCG, software). Be patient: a cold boot can take several minutes. `-gpu swiftshader_indirect` is optional.

Build the APK first (see Package), with the emulator stopped. Reuse `target/x86_64-linux-android/debug/apk/notes.apk` when it is already current. `tekto dev --android` rebuilds the APK; do not run it while the emulator is up.

```bash
source ~/.android-env.sh
emulator -avd beddcode30 -no-window -memory 2048 -accel off &
adb wait-for-device
# Wait until the property is 1 (this can take a few minutes):
adb shell getprop sys.boot_completed
```

Then install and launch the APK you already built:

```bash
adb install -r target/x86_64-linux-android/debug/apk/notes.apk
adb shell am start -n com.tektogui.notes/android.app.NativeActivity
adb forward tcp:8474 tcp:8474
```

The APK requests `android.permission.INTERNET` so the dev server can bind `127.0.0.1:8474` (`ANDROID_DEV_PORT`, overridable with `TEKTO_DEV_HOST` / `TEKTO_DEV_PORT`). After `adb forward`:

```bash
cargo run -p tekto-cli -- tree
cargo run -p tekto-cli -- screenshot /workspace/tektogui-shots/notes-android-via-dev.png
```

A device framebuffer shot (the official path for a visual proof):

```bash
mkdir -p /workspace/tektogui-shots
adb exec-out screencap -p > /workspace/tektogui-shots/notes-android.png
```

Give the Activity time to resume and paint before the screenshot (software emulation is slow; wait, then retry the screencap if it is still a boot or splash).

Do not use displays `:1` or `:2`. Leave the emulator running only as long as you need it, then stop **that** emulator only:

```bash
adb emu kill
# or: adb -s emulator-5554 emu kill
```

Do not use a broad `pkill`.

## JS hot reload

The default APK does **not** enable `quickjs` (keeps nightly Android CI lean). `tekto dev --js --android` builds with `-F quickjs`, installs, forwards port 8474, bundles the TSX entry with esbuild, and `POST /reload`. The host CLI does **not** need `--features quickjs`.

```bash
# Emulator stopped. Prefer one ABI. beddcode30 is x86_64.
cargo apk2 build -p notes-android -F quickjs --target x86_64-linux-android
# Start beddcode30 only after that APK exists, then adb install, am start, and
# adb forward tcp:8474. Host esbuild + POST /reload do not need --features quickjs.
# `tekto dev --js --android` rebuilds the APK; do not run it while the emulator is up.
```

The first painted frame may be the Rust `notes_tree`; the posted bundle then replaces it. Saving the `.tsx` rebuilds and posts again. `POST /reload` still works by hand after `adb forward tcp:8474 tcp:8474`.

Judge QuickJS cost on the `.so` / release size delta, not the raw debug APK (debug already includes a very large unstripped `.so`).

`rquickjs-sys` 0.14 has no pregenerated Android bindings, so the Android `quickjs` build enables rquickjs `bindgen`. That needs a host `libclang` plus the NDK sysroot (otherwise `stdbool.h` is missing). `tekto dev --js --android` sets `LIBCLANG_PATH` to the NDK copy at `toolchains/llvm/prebuilt/<host>/musl/lib` and `BINDGEN_EXTRA_CLANG_ARGS_<triple>` to `--sysroot` + clang builtin includes, when those variables are unset.

## Lifecycle

`android_main` in `examples/notes-android` calls `tekto::run_android` (or `run_android_with_session` when the `quickjs` feature is on, so `POST /reload` has a hook). That builds the winit event loop with `with_android_app`.

winit turns `InitWindow` into `ApplicationHandler::resumed` and `TerminateWindow` into `suspended`. `resumed` creates the winit window and the softbuffer CPU surface and records one [`SurfaceEpoch`](../../../crates/tekto-platform/src/lifecycle.rs). `suspended` retires that epoch, drops the softbuffer surface, then drops the window. A redraw after suspend returns without `buffer_mut` / `present`. A second `resumed` drops any held surface first so a replaced `ANativeWindow` cannot leak or be locked. IME is later.

Host-only proof (no emulator):

```bash
cargo test -p tekto-platform --lib lifecycle
cargo test -p notes-android
```

### Prove suspend / resume on AVD `beddcode30` when it is free

Do not start the emulator while another heavy build is running. Do not use displays `:1` or `:2`. Stop only with `adb emu kill`.

```bash
source ~/.android-env.sh
export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-$ANDROID_HOME/ndk/27.1.12297006}"

# Build (or reuse) the APK first. Do not compile alongside the emulator.
cargo apk2 build -p notes-android --target x86_64-linux-android

emulator -avd beddcode30 -no-window -memory 2048 -accel off &
adb wait-for-device
until [ "$(adb shell getprop sys.boot_completed | tr -d '\r')" = "1" ]; do sleep 2; done

adb install -r target/x86_64-linux-android/debug/apk/notes.apk
adb shell am start -n com.tektogui.notes/android.app.NativeActivity
sleep 3
mkdir -p /workspace/tektogui-shots
adb exec-out screencap -p > /workspace/tektogui-shots/notes-android-resumed.png

adb shell input keyevent KEYCODE_HOME
sleep 2
adb shell am start -n com.tektogui.notes/android.app.NativeActivity
sleep 3
adb exec-out screencap -p > /workspace/tektogui-shots/notes-android-resumed-again.png
adb logcat -d -s AndroidRuntime:E | head
adb emu kill
```

Both screenshots should show the Notes tree, and logcat should not contain a fatal `ANativeWindow` / softbuffer error.

## CI

The Android job in `.github/workflows/ci.yml` runs only on `push` to `nightly` or `workflow_dispatch` with `full=true`, not on every PR. It **checks** `tekto` (cpu) and `notes-android` for `aarch64-linux-android`. Keep it that way. Do not start an emulator in CI, and do not package an APK in PR CI.

## Verify

- Host: `cargo test -p notes-android` (lifecycle flag cycles + headless Notes PNG) and `cargo test -p tekto-platform --lib lifecycle` (one live surface epoch; suspend retires it; resume replaces it).
- APK: `cargo apk2 build -p notes-android` exits 0 and writes `notes.apk`.
- Device: package is installed, `am start` does not print `Error type`, `adb exec-out screencap -p` shows the Notes UI, and `tekto tree` over `adb forward tcp:8474` prints a window tree.
