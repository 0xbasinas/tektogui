---
name: android-build-and-run
description: Compile the Tekto CPU core for Android (and later run it). Use when changing platform, native, or CI Android jobs. There is no Activity/APK yet, so this is compile-only until those land. Do not use the busy beddcode30 emulator.
---

# Android build (and run, later)

Windows and Android are compile-checked from day one. A real Android window is Month 2 (`Resumed` / `Suspended` must drop the surface safely). IME is later.

**Do not** launch or install onto AVD `beddcode30` / `emulator-5554` unless you have confirmed it is free and the owner agreed. That emulator is used by other work.

## Compile only (this is what works today)

```bash
# Optional, if present on this machine:
#   source ~/.android-env.sh
rustup target add aarch64-linux-android

# Same shape as CI:
cargo check -p tekto --target aarch64-linux-android --no-default-features --features cpu

# If cargo-ndk and an NDK are installed:
#   cargo ndk -t arm64-v8a check -p tekto --no-default-features --features cpu
```

Use `--no-default-features --features cpu` so the desktop/winit shell is not pulled in. `run_windowed` on Android currently returns: windowed mode is not available on this target yet.

## When an Activity exists (not yet)

Then, and only on a free device or AVD, document the exact `cargo ndk` / `adb` commands in this skill. Handle `Resumed` / `Suspended` so the surface is not used after it is dropped. Still do not add an emulator start to PR CI.

## CI

The Android job in `.github/workflows/ci.yml` runs only on `push` to `nightly` or `workflow_dispatch`, not on every PR. Keep it that way. Do not start an emulator in CI.

## Verify

The `cargo check` command above exits 0. `cargo test` is Linux-host; do not expect Android unit tests in CI yet.
