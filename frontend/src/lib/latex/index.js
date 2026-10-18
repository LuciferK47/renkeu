// What the project contains that completion needs: labels, bibliography keys,
// file names, macros. Open documents are read live from their editors; files
// that are not open are fetched once and kept until `invalidate()` (window
// focus, tree changes), so completion is quick but never a long way behind.

import { api as defaultApi } from '../api/client.js';
import { scan } from './scan.js';
import { scanBib } from './bibtex.js';

const MAX_FETCH = 60;
const kindOf = (path) => (/\.bib$/i.test(path) ? 'bib' : /\.(tex|sty|cls|tikz)$/i.test(path) ? 'tex' : null);

export class ProjectIndex {
  #project;
  #documents;
  #api;
  /** path -> text of files that are not open */
  #texts = new Map();
  #loading = null;
  #treeSeen = null;

  constructor({ project, documents, api = defaultApi }) {
    this.#project = project;
    this.#documents = documents;
    this.#api = api;
  }

  invalidate() {
    this.#texts.clear();
  }

  /** Fetch the files that are neither open nor cached. Never rejects. */
  ensureLoaded() {
    // Files were added, renamed or removed: what was fetched may be gone.
    if (this.#treeSeen !== this.#project.tree) {
      this.#treeSeen = this.#project.tree;
      this.invalidate();
    }
    this.#loading ??= this.#load().finally(() => {
      this.#loading = null;
    });
    return this.#loading;
  }

  async #load() {
    const wanted = this.#project
      .editableFiles()
      .map((f) => f.path)
      .filter((p) => kindOf(p) && !this.#documents.get(p) && !this.#texts.has(p))
      .slice(0, MAX_FETCH);
    await Promise.all(
      wanted.map(async (path) => {
        try {
          this.#texts.set(path, (await this.#api.getFile(path)).content);
        } catch {
          /* unreadable now; completion simply lacks this file */
        }
      })
    );
  }

  #sources() {
    const out = new Map(this.#texts);
    for (const doc of this.#documents.docs) {
      out.set(doc.path, doc.text);
      // The editor is now the truth; a copy fetched earlier would go stale.
      this.#texts.delete(doc.path);
    }
    return out;
  }

  /** Everything completion offers, as of now. */
  snapshot() {
    const labels = [];
    const bib = [];
    const macros = new Set();
    const environments = new Set();
    const live = this.#sources();
    for (const [path, text] of live) {
      const kind = kindOf(path);
      if (kind === 'bib') {
        for (const e of scanBib(text)) bib.push({ ...e, path });
      } else if (kind === 'tex') {
        const s = scan(text);
        for (const l of s.labels) labels.push({ ...l, path });
        s.macros.forEach((m) => macros.add(m));
        s.environments.forEach((e) => environments.add(e));
      }
    }
    return {
      labels,
      bib,
      files: this.#project.files().map((f) => f.path),
      macros: [...macros],
      environments: [...environments]
    };
  }
}
