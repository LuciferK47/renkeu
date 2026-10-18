// Restoring an earlier version of a file from the history.
//
// A restore is an ordinary save (or create, for a deleted file), so it goes
// through the same conditional-write routes as everything else and leaves its
// own history entry: nothing about it can be lost or silently overwrite a
// newer edit.

import { api as defaultApi } from './api/client.js';

/** Plain-language names for `edit_type`. */
export const EDIT_LABELS = {
  'manual-save': 'Saved',
  autosave: 'Autosaved',
  'agent-apply': 'AI change',
  'apply-selected': 'AI change',
  create: 'Created',
  delete: 'Deleted'
};

export const labelFor = (type) => EDIT_LABELS[type] ?? type;

/**
 * Put a version of the file back. Resolves true when the file now has that text.
 *
 * `which` picks the version: `'after'` is the file as it stood once the entry
 * was made, `'before'` is what the entry replaced (so `'before'` undoes it).
 * A delete can only be undone, so it defaults to `'before'`.
 *
 * @param {{ id: number, file_path: string, edit_type: string }} entry
 * @param {{ which?: 'after'|'before', documents, project, ui, api?, confirm?: (message: string) => boolean }} deps
 */
export async function restoreVersion(
  entry,
  { which = entry.edit_type === 'delete' ? 'before' : 'after', documents, project, ui, api = defaultApi, confirm = (m) => globalThis.confirm(m) }
) {
  const path = entry.file_path;
  // Going back before a change is an "undo"; bringing back a deleted file is not.
  const undoing = which === 'before' && entry.edit_type !== 'delete';
  try {
    const detail = await api.getHistoryEntry(entry.id);
    const text = which === 'before' ? detail.text_before : detail.text_after;
    if (text === null || text === undefined) {
      ui.toast(
        undoing
          ? 'That entry did not keep the text it replaced, so it cannot be undone.'
          : 'That entry did not keep the file text, so it cannot be restored.',
        'error'
      );
      return false;
    }

    const open = documents.get(path);
    if (open?.dirty && !confirm(`${path} has unsaved changes that restoring will replace. Continue?`)) {
      return false;
    }

    let current = null;
    try {
      current = await api.getFile(path);
    } catch (e) {
      if (e.status !== 404) throw e; // 404: the file is gone, so recreate it
    }

    if (current) {
      if (current.content === text) {
        ui.toast('The file already has that content.', 'info');
        return true;
      }
      await api.saveFile(path, text, current.etag);
    } else {
      await api.createFile(path, text);
    }

    if (open) await documents.reload(path);
    await project.refresh();
    ui.toast(undoing ? `Undid that change to ${path}` : `Restored ${path}`, 'success');
    return true;
  } catch (e) {
    ui.toast(
      e.status === 409
        ? `${path} changed while restoring; try again.`
        : `Could not restore ${path}: ${e.message}`,
      'error'
    );
    return false;
  }
}
