# Store signing (not done)

Tekto does not sign builds. There is no keystore, no Apple certificate, and no Authenticode certificate in this repo. Do not commit private keys or provisioning profiles. Do not buy a certificate or a developer account from CI. This note is only what a later experiment would need.

Linux PR CI does not package or sign. See [packaging.md](packaging.md) for the unsigned Linux tarball and the unsigned Windows folder.

## Android (Play)

We do not upload to Play and we do not sign the Notes APK.

A later upload would need:

- An upload keystore (a private `.jks` or `.keystore` plus its passwords). Keep it outside git. CI must not generate one.
- An Android App Bundle (`.aab`), not only a debug APK. Play App Signing holds the app signing key. The upload key only proves the upload.
- `apksigner` (or the bundle tool's signing step) with v2/v3 signatures. A debug keystore from `cargo apk2` is not a store key.

Play Console account fees are out of scope until someone decides to pay. Nightly CI only `cargo check`s the Android target. It does not build an APK and it does not start an emulator.

## iOS (App Store)

There is no iOS target and no Mac runner. We cannot codesign or notarize here.

A later build would need:

- An Apple Developer Program membership (paid). Do not enroll from automation.
- A distribution certificate and a provisioning profile for the app id, created in the developer portal. Those files stay out of git.
- A Mac with Xcode to archive, sign, and upload to App Store Connect. GitHub-hosted macOS is not in this workflow.

Until that exists, do not add a macOS CI job.

## Desktop

### Windows

The zip in [packaging.md](packaging.md) is unsigned. SmartScreen can warn. That is expected.

Authenticode later would need a code-signing certificate (public CAs sell these; do not buy one yet), `signtool` on Windows, and a timestamp server. EV vs OV only changes how quickly SmartScreen trusts a new file. Neither is set up.

### macOS

No Mac package. A later disk image would need a Developer ID Application certificate, `codesign`, Apple notarization (`notarytool`), and `stapler`. Same constraint as iOS: a Mac, and a paid developer account. Do not add that job until a Mac runner exists.

### Linux

The tarball has no store. Users run the binary directly. Optional later steps, still not done: a detached signature over the tarball (for example `minisign` or GPG) and checksums next to the file. Flathub or a distro package would bring their own review. Do not add those tools to PR CI.

## What stays out of GitHub Actions

- Keystores, `.p12` / `.pfx` files, API keys for notarization, and Play service-account JSON.
- `signtool`, `codesign`, `apksigner` against a real identity, and any secret in the workflow.
- Extra PR jobs. Signing experiments, if they ever happen, stay manual or on `workflow_dispatch`, same as Windows and Android compile jobs.
