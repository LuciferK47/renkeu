// File management: create, rename, delete and upload, with the open tabs kept
// consistent. A rename or delete that left a tab pointing at a path that no
// longer exists would make its next autosave fail (or, worse, recreate it), so
// the tabs are saved first and closed or reopened around the change.

import { api as defaultApi } from './api/client.js';

const under = (path, dir) => path === dir || path.startsWith(`${dir}/`);

export class FileOps {
  #api;
  #documents;
  #project;
  #ui;

  constructor({ documents, project, ui, api = defaultApi }) {
    this.#documents = documents;
    this.#project = project;
    this.#ui = ui;
    this.#api = api;
  }

  /** Create an empty text file and open it. Returns the path, or null. */
  async create(path) {
    try {
      const made = await this.#api.createFile(path);
      await this.#project.refresh();
      this.#project.reveal(made.path);
      await this.#documents.open(made.path);
      return made.path;
    } catch (e) {
      this.#ui.toast(`Could not create ${path}: ${e.message}`, 'error');
      return null;
    }
  }

  /** Rename a file or folder; tabs inside it follow. Returns true on success. */
  async rename(from, to) {
    if (from === to) return true;
    const affected = this.#documents.docs.filter((d) => under(d.path, from)).map((d) => d.path);
    for (const path of affected) {
      if (!(await this.#documents.save(path))) {
        this.#ui.toast(`Save ${path} before renaming it.`, 'error');
        return false;
      }
    }
    const wasActive = this.#documents.activePath;
    try {
      await this.#api.renameFile(from, to);
    } catch (e) {
      this.#ui.toast(`Could not rename: ${e.message}`, 'error');
      return false;
    }
    const moved = (p) => to + p.slice(from.length);
    for (const path of affected) this.#documents.close(path, { force: true });
    if (this.#project.mainOverride && under(this.#project.mainOverride, from)) {
      await this.#project.setMain(moved(this.#project.mainOverride));
    }
    await this.#project.refresh();
    for (const path of affected) {
      try {
        await this.#documents.open(moved(path));
      } catch {
        /* it will simply not be reopened; the tree still shows it */
      }
    }
    if (wasActive && affected.includes(wasActive)) this.#documents.setActive(moved(wasActive));
    return true;
  }

  /** Every file at or below `node`, so a folder deletes as its files. */
  #filesIn(node) {
    if (node.kind === 'file') return [node.path];
    return (node.children ?? []).flatMap((c) => this.#filesIn(c));
  }

  /**
   * Delete a file, or every file in a folder. Each deletion carries the ETag
   * just read, so a file changed elsewhere is kept rather than lost. Returns
   * how many files were removed.
   */
  async remove(node) {
    const paths = this.#filesIn(node);
    let removed = 0;
    const failed = [];
    for (const path of paths) {
      try {
        const open = this.#documents.get(path);
        // The tab's text is what the user sees; closing it discards unsaved
        // edits, which the caller has already confirmed.
        if (open) this.#documents.close(path, { force: true });
        const etag = await this.#api.fileEtag(path);
        await this.#api.deleteFile(path, etag);
        removed += 1;
        if (this.#project.mainOverride === path) await this.#project.setMain(null);
      } catch (e) {
        failed.push(`${path} (${e.message})`);
      }
    }
    await this.#project.refresh();
    if (failed.length) this.#ui.toast(`Could not delete: ${failed.join(', ')}`, 'error');
    return removed;
  }

  /** Upload dropped or chosen files into `dir` ('' = project root). */
  async upload(files, dir = '') {
    let added = 0;
    for (const file of files) {
      const path = dir ? `${dir}/${file.name}` : file.name;
      try {
        await this.#api.uploadFile(path, file);
        added += 1;
      } catch (e) {
        this.#ui.toast(`Could not add ${file.name}: ${e.message}`, 'error');
      }
    }
    if (added) {
      await this.#project.refresh();
      if (dir) this.#project.reveal(`${dir}/x`);
      this.#ui.toast(added === 1 ? 'Added 1 file' : `Added ${added} files`, 'info');
    }
    return added;
  }
}

/** Whether the preview can show this file as an image. */
export function isImage(path) {
  return /\.(png|jpe?g|gif|svg)$/i.test(path);
}

/** The group a file belongs to in the "By type" view of the project tree. */
export function fileGroup(path) {
  if (/\.(tex|sty|cls|tikz)$/i.test(path)) return 'Documents';
  if (/\.bib$/i.test(path)) return 'Bibliography';
  if (isImage(path) || /\.pdf$/i.test(path)) return 'Figures';
  return 'Other';
}

export const FILE_GROUPS = ['Documents', 'Bibliography', 'Figures', 'Other'];
