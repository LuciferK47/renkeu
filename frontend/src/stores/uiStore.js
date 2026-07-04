import { writable } from 'svelte/store';

// Manages global UI state to avoid prop drilling through layout components
export const uiStore = writable({
  showFileTree: true,
  showHistory: false,
  showSettings: false,
  showSearch: false,
  showFileOpener: false,
  showConflictDialog: false,
  showChatPanel: true,
  
  layoutMode: 'overleaf', // 'overleaf' | 'fullscreen-editor' | 'fullscreen-pdf'
  activeRightTab: 'diff', // 'diff' | 'pdf' | 'ai' | 'errors' | 'suggestions'
  
  editorHeightPercent: 60,
  chatHeightPercent: 300,
  
  status: 'Ready',
  toasts: [],
  toastId: 0
});

export function toggleFileTree() {
  uiStore.update(ui => ({ ...ui, showFileTree: !ui.showFileTree, showHistory: false }));
}

export function toggleHistory() {
  uiStore.update(ui => ({ ...ui, showHistory: !ui.showHistory, showFileTree: false }));
}

export function toggleSettings() {
  uiStore.update(ui => ({ ...ui, showSettings: !ui.showSettings }));
}

export function setStatus(status) {
  uiStore.update(ui => ({ ...ui, status }));
}

export function setActiveRightTab(tab) {
  uiStore.update(ui => ({ ...ui, activeRightTab: tab }));
}

export function pushToast(message, type = 'info') {
  uiStore.update(ui => {
    const id = ++ui.toastId;
    const toasts = [...ui.toasts, { id, message, type }];
    setTimeout(() => removeToast(id), 4000);
    return { ...ui, toasts, toastId: id };
  });
}

function removeToast(id) {
  uiStore.update(ui => ({ ...ui, toasts: ui.toasts.filter(t => t.id !== id) }));
}

export function setLayoutMode(mode) {
  uiStore.update(ui => ({ ...ui, layoutMode: mode }));
}

export function setEditorHeight(percent) {
  uiStore.update(ui => ({ ...ui, editorHeightPercent: percent }));
  localStorage.setItem('layout:editorHeight', percent.toString());
}

export function setChatHeight(pixels) {
  uiStore.update(ui => ({ ...ui, chatHeightPercent: pixels }));
  localStorage.setItem('layout:chatHeight', pixels.toString());
}

// Load saved preferences
export function initLayoutPreferences() {
  const savedEditorHeight = localStorage.getItem('layout:editorHeight');
  const savedChatHeight = localStorage.getItem('layout:chatHeight');
  
  uiStore.update(ui => ({
    ...ui,
    editorHeightPercent: savedEditorHeight ? parseFloat(savedEditorHeight) : 60,
    chatHeightPercent: savedChatHeight ? parseFloat(savedChatHeight) : 300
  }));
}
