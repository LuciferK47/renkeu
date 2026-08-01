import * as fileService from '../services/fileService.js';
import { fileStore } from '../stores/fileStore.js';
import { uiStore, pushToast } from '../stores/uiStore.js';

export async function loadFile(filePath) {
  if (!filePath) return;
  uiStore.update((ui) => ({ ...ui, status: 'Loading...' }));
  try {
    const data = await fileService.loadFile(filePath);
    fileStore.update((f) => ({
      ...f,
      filePath,
      content: data.content,
      savedContent: data.content,
      etag: data.eTag,
      dirty: false,
      lastSavedAt: new Date()
    }));
    uiStore.update((ui) => ({ ...ui, status: 'Loaded' }));
  } catch (e) {
    uiStore.update((ui) => ({ ...ui, status: `Error: ${e.message}` }));
    pushToast(`Could not open ${filePath}: ${e.message}`, 'error');
  }
}

/**
 * Save the current buffer. On a 409 the file changed underneath us — surface
 * it instead of clobbering (masterplan §6.1).
 */
export async function saveFile(filePath, content, etag) {
  if (!filePath) {
    pushToast('Please select a file first', 'error');
    return null;
  }
  uiStore.update((ui) => ({ ...ui, status: 'Saving...' }));
  try {
    const res = await fileService.saveFile(filePath, content, etag);
    fileStore.update((f) => ({
      ...f,
      content,
      savedContent: content,
      etag: res.etag,
      dirty: false,
      lastSavedAt: new Date()
    }));
    uiStore.update((ui) => ({ ...ui, status: 'Saved' }));
    return res;
  } catch (e) {
    if (e.status === 409) {
      uiStore.update((ui) => ({ ...ui, status: 'Conflict: file changed on disk' }));
      pushToast('File changed elsewhere — reload before saving', 'error');
    } else {
      uiStore.update((ui) => ({ ...ui, status: `Save failed: ${e.message}` }));
      pushToast(`Save failed: ${e.message}`, 'error');
    }
    return null;
  }
}
