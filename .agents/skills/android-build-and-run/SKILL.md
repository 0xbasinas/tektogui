---
name: android-build-and-run
description: Compile the Tekto CPU core and the Notes Android skeleton with cargo-ndk. Use when changing platform, native, or CI Android jobs. Starting AVD beddcode30 is allowed for a manual run attempt; do not add an emulator to CI.
---

# Android build (and run)

Windows and Android are compile-checked from day one. A real Android window is still Month 2: `on_resumed` / `on_suspended` (and the `tekto_on_resumed` / `tekto_on_suspended` C symbols in `examples/notes-android`) must create and drop the surface safely. IME is later.

## Compile

```bash
# Optional, if present on this machine:
source ~/.android-env.sh
rustup target add aarch64-linux-android

# Same shape as CI:
cargo check -p tekto --target aarch64-linux-android --no-default-features --features cpu
cargo check -p notes-android --target aarch64-linux-android

# If cargo-ndk and an NDK are installed (this repo uses NDK 27.1.12297006):
cargo ndk -t arm64-v8a check -p notes-android
```

Use `--no-default-features --features cpu` on `tekto` so the desktop/winit shell is not pulled in. `run_windowed` on Android still returns: windowed mode is not available on this target yet.

SDK root is `~/Android/Sdk` or `$ANDROID_HOME`.

## AVD `beddcode30` (manual only)

You may start the AVD yourself. Do not put this in PR CI. Do not run heavy `cargo test` / compiles in parallel with the emulator (it is tight on RAM, and there is often no KVM).

```bash
source ~/.android-env.sh
emulator -avd beddcode30 -no-window -memory 2048 &
adb wait-for-device
```

There is no Activity / APK yet, so installing and opening Notes will fail until that wrapper exists. Report that honestly rather than inventing a run path. Leave the emulator running only as long as you need it, then stop that emulator process only (do not use a broad `pkill`).

Do not use displays `:1` or `:2` for anything else; they are not Android.

## CI

The Android job in `.github/workflows/ci.yml` runs only on `push` to `nightly` or `workflow_dispatch` with `full=true`, not on every PR. It checks `tekto` (cpu) and `notes-android` for `aarch64-linux-android`. Keep it that way. Do not start an emulator in CI.

## Verify

The `cargo check` commands above exit 0. `cargo test -p notes-android` (host) covers the lifecycle stubs and a headless Notes PNG. Host `cargo test` is Linux; do not expect Android unit tests in CI yet.
