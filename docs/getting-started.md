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

While it runs, in another terminal:

```bash
cargo run -p tekto-cli -- tree
cargo run -p tekto-cli -- screenshot shot.png
cargo run -p tekto-cli -- patch 4 --text "Hello from an agent"
```

(Node ids can differ slightly if the tree shape changes; use `tekto tree` to see them.)

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

Set `TEKTO_SHOT_DIR` to choose where the snapshot PNG is written.

## Targets

- Linux and Windows: windowed + headless.
- Android: compile the CPU core (`cargo check -p tekto --target aarch64-linux-android --no-default-features --features cpu`). Running on a device comes later.
- Web: not built yet; the platform and JS layers are shaped so a WASM + canvas / WebGPU backend can plug in.
