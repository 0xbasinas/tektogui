# Packaging experiments

First pass at shipping the Notes example. Builds are unsigned. What Play, the App Store, Authenticode, and macOS notarization would require is in [signing.md](signing.md). We do not do those steps. These packaging steps are local only. They are not part of GitHub Actions.

The default package is a **release** build of `examples/notes`. It does not turn on `quickjs` or `gpu`. The release profile uses thin LTO, one codegen unit, and `strip = "symbols"` (see the workspace `Cargo.toml`).

Do not commit binaries or tarballs. `dist/` is gitignored.

## Linux tarball

From the repo root, with `CARGO_TARGET_DIR` pointed at this worktree's `target/` if you share a machine with other checkouts:

```bash
export CARGO_TARGET_DIR=/path/to/this/worktree/target
./scripts/package-notes-linux.sh
```

The script runs `cargo build --release -p notes` and writes:

```text
dist/tektogui-notes-linux-x86_64.tar.gz
  tektogui-notes-linux-x86_64/
    notes          # stripped release binary
    README.txt
```

Unpack and run `./notes`, or `./notes --headless --out notes.png` when there is no display.

### Size recorded on 2026-11-10

Host: Linux x86_64, `cargo build --release -p notes` (default features).

| Artifact | Size |
|---|---|
| `target/release/notes` | 6.0 MiB (6,199,744 bytes) |
| `dist/tektogui-notes-linux-x86_64.tar.gz` | 2.7 MiB (2,742,813 bytes) |

Numbers move when dependencies or the release profile change. Re-run the script and update this table if you cut a real build.

An AppImage is not in this pass. A plain tarball stays smaller and needs no extra packaging tool.

## Windows folder or zip

Not produced in the Linux CI environment. `x86_64-pc-windows-gnu` can be used for `cargo check` on a machine that has a GNU linker. This checkout has the Rust target but no GNU linker and no Windows runtime, so no `.exe` was built here.

On a Windows machine with the MSVC Build Tools (same note as [getting-started.md](getting-started.md)):

```bat
cargo build --release -p notes
```

Assemble a folder by hand. No WiX, no paid MSI tool, no Authenticode:

```text
tektogui-notes-windows-x86_64/
  notes.exe      # copy of target\release\notes.exe
  README.txt     # how to run it; say it is unsigned
```

Zip that folder, for example `tektogui-notes-windows-x86_64.zip`. Ship the zip or the folder. Users may see a SmartScreen warning because the binary is unsigned. That is expected. Signing requirements are listed in [signing.md](signing.md). We have not bought a certificate.

Expected layout matches Linux: one binary plus a short readme. Do not add `quickjs` or `gpu` to this package.
