# Notes (Android APK)

The Tekto CPU core, packaged as a `NativeActivity` APK (`com.tektogui.notes`) that runs the same Notes window as desktop. Entry point is `android_main` in `src/lib.rs`.

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

cargo apk2 build -p notes-android
# or: cargo run -p tekto-cli -- build --target android

# AVD (manual; do not add to CI). beddcode30 often needs software emulation:
emulator -avd beddcode30 -no-window -memory 2048 -accel off &
adb wait-for-device
# wait until: adb shell getprop sys.boot_completed  ->  1

cargo run -p tekto-cli -- dev --android
# installs, launches NativeActivity, adb forward tcp:8474, writes the host endpoint

cargo run -p tekto-cli -- tree
adb exec-out screencap -p > /workspace/tektogui-shots/notes-android.png
adb emu kill   # stop only this emulator
```

The dev server on the device listens on `127.0.0.1:8474`. After `adb forward`, host `tekto tree` / `tekto screenshot` / `tekto patch` work the same as on desktop.

CI runs `cargo check -p notes-android --target aarch64-linux-android` on `nightly` (and on a manual `workflow_dispatch` with `full=true`), not on every PR. It does not start an emulator or build an APK.

See [the android-build-and-run skill](../../.agents/skills/android-build-and-run/SKILL.md) for the full command list and lifecycle notes.
