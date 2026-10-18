// The project as the server lists it, plus the user's choice of root document.

import { api as defaultApi } from '../api/client.js';
import { readPref, writePref } from '../storage.js';

const MAIN_KEY = 'latex-ai:main-file';

export class ProjectState {
  /** Nested nodes from GET /api/tree. */
  tree = $state([]);
  loading = $state(false);
  error = $state(null);
  /** Directories the user has opened in the tree. */
  expanded = $state(new Set());
  /** The root document the user chose; null means "let the server decide". */
  mainOverride = $state(null);

  #api;

  constructor({ api = defaultApi } = {}) {
    this.#api = api;
    this.mainOverride = readPref(MAIN_KEY, null);
  }

  async refresh() {
    this.loading = true;
    try {
      this.tree = (await this.#api.getTree()).tree;
      this.error = null;
      // A remembered root that no longer exists must not be sent to the server.
      if (this.mainOverride && !this.files().some((f) => f.path === this.mainOverride)) {
        this.setMain(null);
      }
    } catch (e) {
      this.error = e.message;
    } finally {
      this.loading = false;
    }
  }

  /** Every file node, flattened. */
  files() {
    const out = [];
    const walk = (nodes) => {
      for (const n of nodes) {
        if (n.kind === 'dir') walk(n.children ?? []);
        else out.push(n);
      }
    };
    walk(this.tree);
    return out;
  }

  /** Files the editor can open as text. */
  editableFiles() {
    return this.files().filter((f) => f.editable);
  }

  toggle(path) {
    const next = new Set(this.expanded);
    if (!next.delete(path)) next.add(path);
    this.expanded = next;
  }

  /** Open every directory above `path`, so the file is visible. */
  reveal(path) {
    const next = new Set(this.expanded);
    const parts = path.split('/');
    for (let i = 1; i < parts.length; i++) next.add(parts.slice(0, i).join('/'));
    this.expanded = next;
  }

  setMain(path) {
    this.mainOverride = path;
    writePref(MAIN_KEY, path);
  }
}
