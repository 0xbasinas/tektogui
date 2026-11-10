#!/usr/bin/env bash
# Build a small release tarball of the Notes example. Local only; not used in CI.
# Does not enable quickjs or gpu. Does not commit the tarball (see /dist in .gitignore).
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$root/target}"

cargo build --release -p notes

bin="$CARGO_TARGET_DIR/release/notes"
if [[ ! -f "$bin" ]]; then
  echo "missing release binary: $bin" >&2
  exit 1
fi

name="tektogui-notes-linux-x86_64"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/$name"
cp "$bin" "$stage/$name/notes"
chmod +x "$stage/$name/notes"

cat >"$stage/$name/README.txt" <<'EOF'
TektoGUI Notes (Linux x86_64 experiment)

This folder is one release binary. It is not signed. quickjs and gpu are off.

Run a window:
  ./notes

Write a headless PNG (no display):
  ./notes --headless --out notes.png

See docs/packaging.md in the source repo for how this tarball was made
and for the Windows folder/zip experiment.
EOF

mkdir -p "$root/dist"
out="$root/dist/${name}.tar.gz"
tar -C "$stage" -czf "$out" "$name"

echo "binary: $(wc -c <"$bin") bytes ($(du -h "$bin" | awk '{print $1}'))"
echo "tarball: $(wc -c <"$out") bytes ($(du -h "$out" | awk '{print $1}')) $out"
