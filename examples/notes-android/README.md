# Notes (Android skeleton)

The Tekto CPU core, compiled as `libnotes_android.so` for `aarch64-linux-android`.

## SDK / NDK

- Android SDK: `~/Android/Sdk` (or `$ANDROID_HOME`)
- NDK: `27.1.12297006` (the version used on this project)
- Rust target: `aarch64-linux-android` (`rustup target add aarch64-linux-android`)
- Optional helper: `cargo-ndk` 4.x
- If this machine has it: `source ~/.android-env.sh`

## Compile (arm64)

```bash
source ~/.android-env.sh   # if present
cargo check -p notes-android --target aarch64-linux-android
# or
cargo ndk -t arm64-v8a check -p notes-android
```

CI runs that `cargo check` on `nightly` (and on a manual `workflow_dispatch` with `full=true`), not on every PR.

## Lifecycle

`tekto_on_resumed` and `tekto_on_suspended` are exported C symbols. Today they only flip a "surface is live" flag. When an Activity exists, `on_suspended` must drop the window (and later GPU) surface before returning.

## Run on a device or AVD

There is no Activity or APK yet, so `adb install` / running Notes on `beddcode30` is not possible from this crate alone. That is the next Android step (Month 2).
