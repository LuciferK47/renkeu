// Which screen is showing: the landing page or the editor. The hash is the
// route (no router library for two screens), so a link to `#/editor` works in
// the browser and the desktop shell, and the back button does what it should.

import { readPref, writePref } from '../storage.js';

const VISITED = 'latex-ai:opened-editor';

const EDITOR_HASHES = new Set(['#/editor', '#editor']);
const LANDING_HASHES = new Set(['#/landing', '#landing']);

/**
 * Decide the first screen. The desktop app is the editor. Anyone who has
 * opened the editor before goes straight back to it; a first visit, or an
 * explicit `#/landing`, shows the landing page.
 */
export function initialView({ desktop = false, hash = '', search = '', visited = false } = {}) {
  if (desktop) return 'editor';
  if (EDITOR_HASHES.has(hash) || new URLSearchParams(search).has('app')) return 'editor';
  if (LANDING_HASHES.has(hash)) return 'landing';
  return visited ? 'editor' : 'landing';
}

export class RouteState {
  view = $state('landing');
  #desktop;

  constructor({ desktop = false } = {}) {
    this.#desktop = desktop;
    const w = globalThis.window;
    this.view = initialView({
      desktop,
      hash: w?.location.hash ?? '',
      search: w?.location.search ?? '',
      visited: readPref(VISITED, false) === true
    });
  }

  openEditor() {
    this.view = 'editor';
    writePref(VISITED, true);
    if (globalThis.window && !EDITOR_HASHES.has(window.location.hash)) window.location.hash = '/editor';
  }

  openLanding() {
    if (this.#desktop) return;
    this.view = 'landing';
    if (globalThis.window && !LANDING_HASHES.has(window.location.hash)) window.location.hash = '/landing';
  }

  /** Follow the address bar (back/forward, or a pasted link). */
  syncFromHash(hash) {
    if (EDITOR_HASHES.has(hash)) this.view = 'editor';
    else if (LANDING_HASHES.has(hash) && !this.#desktop) this.view = 'landing';
  }
}
