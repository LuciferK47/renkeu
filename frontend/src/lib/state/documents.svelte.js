// Open documents: one text model, one ETag and one save queue per file.
//
// The old editor held a single buffer shared by every file, with one ETag it
// had to be told about. That is how a save could write file A's text to file
// B's path, how undo history vanished on every tab switch, and how a 5-second
// poll could overwrite what the user was typing. Here each document owns its
// model, so none of that can be expressed.
//
// A document is *dirty* when its model's alternative version id differs from
// the one recorded at the last load or save. Undo back to the saved text is
// therefore clean again, which comparing strings (or a boolean flag) gets
// wrong or does expensively.

import { api } from '../api/client.js';

/** Wait this long after the last keystroke before autosaving. */
const AUTOSAVE_PAUSE_MS = 1500;

class OpenDocument {
  path;
  /** ETag of the text last loaded from, or saved to, the server. */
  etag = $state(null);
  dirty = $state(false);
  saving = $state(false);
  /** { serverEtag } while a save was refused because the file moved. */
  conflict = $state(null);
  /** Last save failure that is not a conflict, for the status bar. */
  error = $state(null);

  /** The text model (Monaco's, or a stand-in under test). */
  model;
  savedVersion = 0;
  /** Saves for one document run strictly one after another. */
  queue = Promise.resolve(true);
  listener = null;
  timer = null;
  /** Set while we change the model ourselves, so it is not read as typing. */
  applying = false;

  constructor(path, etag, model) {
    this.path = path;
    this.etag = etag;
    this.model = model;
    this.savedVersion = model.getAlternativeVersionId();
  }

  get text() {
    return this.model.getValue();
  }
}

export class DocumentStore {
  /** Open tabs, in order. */
  docs = $state([]);
  activePath = $state(null);
  autosave = $state(true);
  /** Called with `(path, { auto })` after each save the server accepted. */
  onsaved = null;

  #models;
  #pauseMs;
  /** Opens in progress, so two requests for one file make one tab. */
  #opening = new Map();

  /**
   * @param {{ create(path: string, text: string): Promise<object>|object,
   *           dispose(model: object): void }} models builds text models
   */
  constructor(models, { autosavePauseMs = AUTOSAVE_PAUSE_MS } = {}) {
    this.#models = models;
    this.#pauseMs = autosavePauseMs;
  }

  get active() {
    return this.docs.find((d) => d.path === this.activePath) ?? null;
  }

  get(path) {
    return this.docs.find((d) => d.path === path) ?? null;
  }

  /** Whether any open document has text the server does not have. */
  get anyDirty() {
    return this.docs.some((d) => d.dirty);
  }

  setActive(path) {
    if (this.get(path)) this.activePath = path;
  }

  /**
   * Open `path` in a tab (loading it on first use) and make it active.
   * Throws if the file cannot be read; nothing is opened in that case.
   */
  async open(path) {
    const existing = this.get(path);
    if (existing) {
      this.activePath = path;
      return existing;
    }
    let pending = this.#opening.get(path);
    if (!pending) {
      pending = this.#load(path).finally(() => this.#opening.delete(path));
      this.#opening.set(path, pending);
    }
    const doc = await pending;
    this.activePath = path;
    return doc;
  }

  async #load(path) {
    const { content, etag } = await api.getFile(path);
    const model = await this.#models.create(path, content);
    const doc = new OpenDocument(path, etag, model);
    this.#watch(doc);
    this.docs.push(doc);
    return this.get(path);
  }

  /**
   * Close a tab. Refuses (returns false) if it has unsaved text unless
   * `force`, so the caller can ask the user first.
   */
  close(path, { force = false } = {}) {
    const doc = this.get(path);
    if (!doc) return true;
    if (doc.dirty && !force) return false;

    clearTimeout(doc.timer);
    doc.listener?.dispose?.();
    this.#models.dispose(doc.model);

    const index = this.docs.findIndex((d) => d.path === path);
    this.docs.splice(index, 1);
    if (this.activePath === path) {
      const next = this.docs[Math.min(index, this.docs.length - 1)];
      this.activePath = next?.path ?? null;
    }
    return true;
  }

  /**
   * Replace `[start, end)` (character offsets) in an open document as the
   * user would type it: one undo step, and the document turns dirty and
   * autosaves like any edit. Returns false when the file is not open.
   */
  replaceRange(path, start, end, text) {
    const doc = this.get(path);
    if (!doc) return false;
    const a = doc.model.getPositionAt(start);
    const b = doc.model.getPositionAt(end);
    doc.model.pushEditOperations(
      [],
      [
        {
          range: { startLineNumber: a.lineNumber, startColumn: a.column, endLineNumber: b.lineNumber, endColumn: b.column },
          text
        }
      ],
      () => null
    );
    return true;
  }

  /**
   * Save one document. Resolves true when it is clean afterwards. Calls for
   * the same document run one at a time, and each saves the text as it is when
   * its turn comes, so rapid saves coalesce rather than reorder.
   */
  save(path, { auto = false } = {}) {
    const doc = this.get(path);
    if (!doc) return Promise.resolve(true);
    doc.queue = doc.queue.then(() => this.#saveNow(doc, auto));
    return doc.queue;
  }

  /** Save everything. True only if every document ended up clean. */
  async flushAll() {
    for (const doc of this.docs) clearTimeout(doc.timer);
    const results = await Promise.all(this.docs.filter((d) => d.dirty).map((d) => this.save(d.path)));
    return results.every(Boolean) && !this.docs.some((d) => d.dirty);
  }

  /**
   * Compare each open document with the server. A clean one that moved on is
   * reloaded quietly; a dirty one is flagged as a conflict (never overwritten
   * and never silently kept stale).
   */
  async checkExternalChanges() {
    for (const doc of [...this.docs]) {
      if (doc.saving || doc.conflict) continue;
      let remote;
      try {
        remote = await api.getFile(doc.path);
      } catch {
        continue; // offline or removed: nothing to compare against
      }
      if (!this.get(doc.path) || remote.etag === doc.etag) continue;
      if (doc.dirty) {
        doc.conflict = { serverEtag: remote.etag };
      } else {
        this.#replaceText(doc, remote.content, remote.etag);
      }
    }
  }

  /**
   * Settle a conflict. `overwrite` keeps the local text and adopts the
   * server's ETag so the next save is accepted: the user's explicit choice.
   * `reload` takes the server's text and drops local edits.
   */
  async resolveConflict(path, choice) {
    const doc = this.get(path);
    if (!doc?.conflict) return true;
    if (choice === 'overwrite') {
      doc.etag = doc.conflict.serverEtag;
      doc.conflict = null;
      return this.save(path);
    }
    const remote = await api.getFile(path);
    doc.conflict = null;
    this.#replaceText(doc, remote.content, remote.etag);
    return true;
  }

  /** Re-read a document from the server (after an AI edit, say). */
  async reload(path) {
    const doc = this.get(path);
    if (!doc) return;
    const remote = await api.getFile(path);
    this.#replaceText(doc, remote.content, remote.etag);
  }

  // ---- internals ------------------------------------------------------

  #watch(doc) {
    doc.listener = doc.model.onDidChangeContent(() => {
      if (doc.applying) return;
      doc.dirty = doc.model.getAlternativeVersionId() !== doc.savedVersion;
      this.#scheduleAutosave(doc);
    });
  }

  #scheduleAutosave(doc) {
    clearTimeout(doc.timer);
    if (!this.autosave || !doc.dirty || doc.conflict) return;
    doc.timer = setTimeout(() => void this.save(doc.path, { auto: true }), this.#pauseMs);
  }

  #replaceText(doc, text, etag) {
    doc.applying = true;
    try {
      doc.model.setValue(text);
    } finally {
      doc.applying = false;
    }
    doc.etag = etag;
    doc.savedVersion = doc.model.getAlternativeVersionId();
    doc.dirty = false;
    doc.error = null;
  }

  async #saveNow(doc, auto) {
    if (doc.conflict) return false;
    if (!doc.dirty) return true;

    const text = doc.model.getValue();
    const version = doc.model.getAlternativeVersionId();
    doc.saving = true;
    try {
      const saved = await api.saveFile(doc.path, text, doc.etag, { autosave: auto });
      doc.etag = saved.etag;
      doc.savedVersion = version;
      this.onsaved?.(doc.path, { auto });
      doc.error = null;
      // Typing during the request leaves the document dirty, and the change
      // listener has already scheduled its autosave.
      doc.dirty = doc.model.getAlternativeVersionId() !== version;
      return !doc.dirty;
    } catch (err) {
      if (err.status === 409) {
        doc.conflict = { serverEtag: err.payload?.current_etag ?? null };
      } else {
        doc.error = err.message;
      }
      return false;
    } finally {
      doc.saving = false;
    }
  }
}
