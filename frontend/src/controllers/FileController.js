import * as fileService from '../services/fileService.js';
import { fileStore } from '../stores/fileStore.js';
import { uiStore, pushToast } from '../stores/uiStore.js';

export async function loadFile(filePath) {
  if (!filePath) return;
  uiStore.update(ui => ({ ...ui, status: 'Loading...' }));
  try {
    const data = await fileService.loadFile(filePath);
    fileStore.update(f => ({ ...f, filePath, content: data.content, dirty: false }));
    uiStore.update(ui => ({ ...ui, status: 'Loaded' }));
  } catch (e) {
    uiStore.update(ui => ({ ...ui, status: `Error: ${e.message}` }));
  }
}

export async function applyCommand(filePath, command, dryRun, currentContent) {
  if (!filePath) {
    pushToast('Please select a file first', 'error');
    return null;
  }
  uiStore.update(ui => ({ ...ui, status: 'Applying...' }));
  try {
    const data = await fileService.applyCommand(filePath, command, dryRun);
    if (!data.success) throw new Error(data.message);
    
    uiStore.update(ui => ({ ...ui, status: data.message, activeRightTab: 'diff' }));
    pushToast(data.message || 'Command applied', 'info');
    
    if (!dryRun && data.changed) {
      await loadFile(filePath);
    }
    return data;
  } catch (e) {
    uiStore.update(ui => ({ ...ui, status: `Error: ${e.message}` }));
    pushToast(`Command error: ${e.message}`, 'error');
    return null;
  }
}
