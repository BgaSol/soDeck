# Workspace UI test fixture

Only `scripts/test-workspace-ui.mjs` uses this manifest and UI. The protocol process
is `src-tauri/tests/fixtures/workspace_ui.rs`, built as `workspace-ui-fixture` with
the core's `test-fixtures` feature. The test assembles a temporary directory and
removes it on exit. This fixture has no release metadata or independent Cargo
project and is never listed, packaged, or distributed as a product plugin.

It keeps state for sidebar placement, multiple editor tabs, drafts, save failure,
theme propagation, and window remount regression checks. It does not connect to
servers; actual remote behavior is covered by terminal/files/network tests.
