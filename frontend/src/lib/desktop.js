// Hooks that exist only inside the Tauri desktop shell. In a browser the
// server's folder is fixed by where it was started, so none of this applies.

/** Whether the page is running inside the desktop app. */
export function isDesktop() {
  return !!globalThis.__TAURI_INTERNALS__;
}

/**
 * Ask the shell to let the user pick a project folder. On success the app
 * restarts into it, so this only resolves (to false) when the user cancels.
 */
export async function pickProject() {
  const invoke = globalThis.__TAURI_INTERNALS__?.invoke;
  if (!invoke) throw new Error('Choosing a folder is only available in the desktop app.');
  return invoke('pick_project');
}
