# SoDeck native lifetime fix

Source: crates.io `tauri-runtime-wry` 2.12.3 (upstream revision in
`.cargo_vcs_info.json`). Original MIT and Apache-2.0 licenses are retained.
Only `src/lib.rs` is patched; registry metadata, sources and build script
otherwise match the published package. The upstream crate's development
lockfile and local registry cache marker are not included.

`Context` is `Send + Sync`, but its derived clone previously cloned
`DispatcherMainThreadContext` by value, including Tao's Windows `Rc`-backed
event-loop target. Concurrent IPC and window operations can race the Rc count.
In the captured failure an IPC response worker destroyed the event handler,
then the UI thread freed it again on exit (`0xc0000374`).

The private `main_thread` field now uses `Arc`, so worker clone/drop touches
only an atomic count. The event handler retains this shared owner for the
event-loop lifetime. Thread checks and main-thread access rules are unchanged.
There are no SoDeck business types, plugin API changes or diagnostic hooks.

Evidence: `.qa/native/1792926907119/app/debugger.log` (first release on Tokio
IPC response thread; second release on main-thread `reset_runner`).
Regression: `tests/desktop/native.mjs`, `tests/desktop/exit-stress.mjs` and the
T-004 native/IME/fault/package checks documented in T-004 results.

Remove this patch when a released upstream version fixes the shared context's
ownership. Compare the upstream change and rerun the same native regressions
before removing the Cargo override. Do not replace it with `.local` diagnostic
sources or edit the global Cargo registry cache.
