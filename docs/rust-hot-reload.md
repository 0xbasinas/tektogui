# Rust iteration and hot-patching

Issue #29 investigates changing Rust UI code without waiting for a full app relink. This is separate from TS hot reload. No prototype or dependency is added yet: the promising patching approach needs a measured, isolated integration before Tekto can rely on it.

## Options

| Option | License and cost | Expected effect and fit |
| --- | --- | --- |
| Dynamic library plugin | Rust supports `cdylib`; a loader such as [libloading](https://github.com/nagisa/rust_libloading) is MIT/Apache-2.0. No loader is needed for the current build. | Relink the plugin instead of the executable, but still compile and link changed plugin code. Keep winit's event loop, CPU renderer, and state in the host. Rust's native ABI is not stable, so use a versioned C ABI and explicitly owned data; never retain plugin callbacks, objects, or threads after unloading. A changed interface or host code requires a restart. Copy libraries to unique paths for replacement, especially on Windows. This would impose a boundary on ordinary Rust apps and needs careful lifetime and panic handling. |
| [Cranelift backend](https://github.com/rust-lang/rustc_codegen_cranelift) | MIT/Apache-2.0 backend, separately installed nightly toolchain component; no app dependency. Check the licenses of any additional tools before adoption. | May reduce debug code generation time, but still links and restarts the app; it is not hot reload. Benefit depends on the fraction spent in codegen. The upstream backend lists limitations including panic unwinding and some SIMD support. Test Tekto's desktop and CPU paths before recommending it; don't change the default toolchain or CI. |
| [Subsecond / ThinLink](https://docs.rs/subsecond/latest/subsecond/) | MIT, experimental. A real integration must audit the exact version and its full dependency and tool license graph. | Patches running Rust code and can avoid a full executable relink; compilation and patch linking still take time. It may preserve the winit window and host state, but changes to types, state layout, active stack frames, or runtime internals need explicit validation and may require restart. Ensure a patch schedules a redraw of the CPU-rendered tree on the event-loop thread. Its toolchain and platform requirements, safety, and latency in Tekto are unmeasured. Keep any experiment opt-in and out of default features. |
| `cargo check` on edits | Cargo/rustc are MIT/Apache-2.0; no new dependency. A watcher is optional. [cargo-watch](https://github.com/watchexec/cargo-watch) is CC0, outside this project's allowed license list, so do not add it as a dependency. | Avoids code generation and linking, giving quick diagnostics, but does not update a running window. `cargo check -p notes` is already useful without a watcher. A separate `cargo run -p notes` rebuild/restart is still needed to see changes. |

These are expected effects, not measured hot-patch timings. The existing [dev build measurements](dev-builds.md#incremental-cargo-check--p-notes-issue-31) show warm checks after Notes and widget edits at 0.17 s and 0.23 s respectively on the recorded machine; they do not measure linking, startup, or visible update latency. A backend swap cannot improve time spent waiting on the linker, and a patching system cannot avoid recompiling changed code.

## Follow-up experiment

Subsecond is the best candidate to investigate first because it targets the running process without forcing every Rust app through a plugin ABI. Do not ship it yet: its compatibility and total build cost for Tekto are unknown. A follow-up should:

1. Audit pinned tool and transitive dependency licenses against MIT, Apache-2.0, BSD, or zlib; keep the experiment outside default features and CI.
2. On the same machine and warm target directory, measure at least five runs each of a small Notes UI edit with ordinary incremental build/restart and with patching. Record toolchain, commands, medians and range for edit-to-build and edit-to-visible-frame, plus cold setup/build time and binary size. Restore edits afterward.
3. Verify a live winit window redraws with the CPU renderer, preserves app state and agent tree, and keeps dev patches and TS hot reload working. Exercise a compile error, a state/type change, and a change requiring restart; document failure and recovery behavior.
4. Check Linux and Windows support and the Android/web implications before proposing an API. If the patcher cannot safely handle a change, report it and restart rather than silently running stale code.

Only promote a prototype if the measured visible update is meaningfully faster and the safety, license, and platform costs are acceptable. Otherwise retain incremental check and normal restart.
