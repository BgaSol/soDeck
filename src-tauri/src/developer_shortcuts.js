// Register before Tauri installs its document listener at DOMContentLoaded.
// Target handlers (including terminal/editor input) still run first.
document.addEventListener("keydown", (event) => {
  const inspector =
    event.code === "KeyI" &&
    ((event.metaKey && event.altKey && !event.ctrlKey) ||
      (event.ctrlKey && event.shiftKey && !event.metaKey));
  if (!inspector) return;
  const handled = event.defaultPrevented;
  event.preventDefault();
  event.stopImmediatePropagation();
  // Tauri's unguarded internal command is denied by the desktop capability.
  if (__DEVELOPER_MODE__ && !handled) {
    void window.__TAURI_INTERNALS__
      .invoke("open_window_devtools")
      .catch(console.error);
  }
});
