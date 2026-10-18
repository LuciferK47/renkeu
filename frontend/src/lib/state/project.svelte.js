// The project as the server lists it, plus the user's choice of root document
// (kept by the server, so the browser and the desktop app agree).

import { api as defaultApi } from '../api/client.js';
export class ProjectState {
  /** Nested nodes from GET /api/tree. */
  tree = $state([]);
  loading = $state(false);
  error = $state(null);
  /** Directories the user has opened in the tree. */
  expanded = $state(new Set());
  /** The root document the user chose; null means "let the server decide". */
  mainOverride = $state(null);
  /** Folder name and full location, for the title bar. */
  name = $state('');
  root = $state('');

  #api;

  constructor({ api = defaultApi } = {}) {
    this.#api = api;
  }

  async refresh() {
    this.loading = true;
    try {
      const [tree, settings] = await Promise.all([
        this.#api.getTree(),
        // Older servers and tests without the route simply have no choice saved.
        Promise.resolve(this.#api.getProject?.()).catch(() => null)
      ]);
      this.tree = tree.tree;
      this.name = settings?.name ?? this.name;
      this.root = settings?.root ?? this.root;
      this.error = null;
      // The server drops a chosen root that no longer exists; mirror that.
      const main = settings?.main ?? null;
      this.mainOverride = main && this.files().some((f) => f.path === main) ? main : null;
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

  /**
   * Choose the root document (null = detect it). Shown at once, saved on the
   * server; if saving fails the previous choice comes back.
   */
  async setMain(path) {
    const previous = this.mainOverride;
    this.mainOverride = path;
    try {
      await this.#api.setMain(path);
      return true;
    } catch (e) {
      this.mainOverride = previous;
      this.error = e.message;
      return false;
    }
  }
}
