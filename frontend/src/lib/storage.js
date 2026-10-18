// localStorage for per-viewer conveniences only (pane sizes, theme, last
// choices). It can be missing or throw (private windows, blocked site data), so
// every access is guarded and the app must work without it.

export function readPref(key, fallback) {
  try {
    const raw = globalThis.localStorage?.getItem(key);
    return raw == null ? fallback : JSON.parse(raw);
  } catch {
    return fallback;
  }
}

export function writePref(key, value) {
  try {
    globalThis.localStorage?.setItem(key, JSON.stringify(value));
  } catch {
    /* a preference that cannot be saved is not worth an error */
  }
}
