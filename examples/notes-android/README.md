# Notes (Android APK)

The Tekto CPU core, packaged as a `NativeActivity` APK (`com.tektogui.notes`) that runs the same Notes window as desktop. Entry point is `android_main` in `src/lib.rs`.

GPU paint is optional and not the default. `--features gpu` compiles wgpu / Vello, but the activity still uses the CPU renderer unless you also set `TEKTO_ANDROID_GPU=1`. This repo could not prove Android drivers (the build host has no `/dev/dri`, and the emulator is not part of the check). If the opt-in cannot create a device, paint falls back to CPU. Suspend drops the window surface; a later redraw does not present into a lost `ANativeWindow`.

## Why cargo-apk2

We use [cargo-apk2](https://crates.io/crates/cargo-apk2) (MIT OR Apache-2.0) instead of a Gradle wrapper or the older rust-mobile `cargo-apk`:

- Permissive license, matching this repo.
- Maintained (`cargo-apk` has stagnated).
- No Gradle project: NativeActivity + `android-activity` (via winit's `android-native-activity` feature) is enough.
- `include_cplusplus_shared = true` bundles `libc++_shared.so` for winit.

The leanest path that still produces a real, installable APK.

## SDK / NDK

- Android SDK: `~/Android/Sdk` (or `$ANDROID_HOME`)
- NDK: `27.1.12297006`
- Rust targets: `aarch64-linux-android` (devices) and `x86_64-linux-android` (AVD `beddcode30` is x86_64)
- Tool: `cargo install cargo-apk2` (the `cargo apk2` subcommand)
- If this machine has it: `source ~/.android-env.sh`
- Set `ANDROID_NDK_HOME` to `$ANDROID_HOME/ndk/27.1.12297006` if it is not already set

## Package and run

```bash
source ~/.android-env.sh   # if present
export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-$ANDROID_HOME/ndk/27.1.12297006}"

# Build (or reuse) the APK before starting the emulator. Do not compile alongside it.
cargo apk2 build -p notes-android
# or: cargo run -p tekto-cli -- build --target android

# AVD (manual; do not add to CI), only after notes.apk exists.
# Do not start this if another heavy build is running.
# beddcode30 often needs software emulation:
emulator -avd beddcode30 -no-window -memory 2048 -accel off &
adb wait-for-device
# wait until: adb shell getprop sys.boot_completed  ->  1

# Install the APK you already built. `tekto dev --android` rebuilds, so do not use it while the emulator is up.
adb install -r target/x86_64-linux-android/debug/apk/notes.apk
adb shell am start -n com.tektogui.notes/android.app.NativeActivity
adb forward tcp:8474 tcp:8474

cargo run -p tekto-cli -- tree
adb exec-out screencap -p > /workspace/tektogui-shots/notes-android.png
adb emu kill   # stop only this emulator
```

The dev server on the device listens on `127.0.0.1:8474`. After `adb forward`, host `tekto tree` / `tekto screenshot` / `tekto patch` work the same as on desktop.

## JS hot reload (`tekto dev --js --android`)

The default APK stays lean (no QuickJS) so nightly Android CI does not compile rquickjs for two ABIs. To reload TSX over `adb forward`, the CLI builds with `-F quickjs`:

```bash
# Build the QuickJS APK first, emulator stopped. beddcode30 is x86_64.
cargo apk2 build -p notes-android -F quickjs --target x86_64-linux-android
# Then start beddcode30 (commands above), install that APK, and adb forward tcp:8474.
# Host esbuild + POST /reload do not need --features quickjs.
# `tekto dev --js --android` rebuilds the APK; do not run it while the emulator is up.
```

The first frame may still be the Rust Notes tree; `POST /reload` then swaps in the TSX bundle. Saving the `.tsx` rebuilds and posts again.

Or by hand: `cargo apk2 build -p notes-android -F quickjs --target x86_64-linux-android`.

CI runs `cargo check -p notes-android --target aarch64-linux-android` on `nightly` (and on a manual `workflow_dispatch` with `full=true`), not on every PR. It does not start an emulator or build an APK. It does not pass `--features gpu`. A local GPU compile check is `cargo check -p notes-android --target aarch64-linux-android --features gpu`.

## Suspend / resume

winit maps native-window create/destroy to `resumed` / `suspended`. The CPU softbuffer is acquired on `resumed` and dropped on `suspended` (surface first, then the window) so resume does not lock a destroyed `ANativeWindow`.

## Text fields

A focused `text_field` shows a native `EditText` (`java/com/tektogui/ime/FieldOverlay.java`, `java_sources` in `Cargo.toml`). cargo-apk2 compiles it into the APK. The Rust loop polls the string into the Tekto tree, so `tekto tree` / AccessKit / MCP see it. winit on Android does not deliver IME commits, so this overlay is the path we use. Limits: one single-line field, the IME UI is Android's, Tekto stores no preedit (the overlay string is the value), and `adjustResize` is the only keyboard inset. A push waits until the UI thread applies it before the old native string can count as typing. The tree caret is a scalar index; the overlay uses UTF-16. Keys are ignored only while the overlay is on a text field. A live type check needs a free emulator or device. Do not start one beside a heavy build. Host tests cover the sync plan (`cargo test -p tekto-platform --lib android_field`).

Host checks (no emulator):

```bash
cargo test -p tekto-platform --lib lifecycle
cargo test -p notes-android
```

On AVD `beddcode30`, only when it is free. Do not start the emulator if another heavy build is running. Do not use displays `:1` or `:2`. Stop only this emulator with `adb emu kill`.

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

# Home destroys the native window (suspended).
adb shell input keyevent KEYCODE_HOME
sleep 2

# Foreground again. resumed recreates the surface and paints Notes.
adb shell am start -n com.tektogui.notes/android.app.NativeActivity
sleep 3
adb exec-out screencap -p > /workspace/tektogui-shots/notes-android-resumed-again.png
adb logcat -d -s AndroidRuntime:E | head
adb shell pidof com.tektogui.notes

adb emu kill
```

Both screenshots should show the Notes tree. `pidof` should still print a pid, and logcat should not show a fatal softbuffer / `ANativeWindow` error.

See [the android-build-and-run skill](../../.agents/skills/android-build-and-run/SKILL.md) for the full command list and lifecycle notes.
