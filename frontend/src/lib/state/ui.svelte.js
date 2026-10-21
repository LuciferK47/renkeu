// Layout, theme and transient UI state. Preferences persist in localStorage
// (guarded: the app must work without it).

import { readPref, writePref } from '../storage.js';

const PREFS = 'latex-ai:ui';

const DEFAULTS = {
  sidebar: true,
  chat: true,
  problems: true,
  spellcheck: true,
  compileOnSave: false,
  treeView: 'folders',
  theme: 'auto',
  editor: { fontSize: 14, wordWrap: true, tabSize: 4 },
  /** Pane sizes in percent, as the splitter reports them. */
  sizes: { sidebar: 15, editor: 32, pdf: 31, chat: 22 }
};

const clamp = (v, lo, hi, fallback) => (Number.isFinite(Number(v)) ? Math.min(hi, Math.max(lo, Math.round(Number(v)))) : fallback);

function cleanEditor(e) {
  return {
    fontSize: clamp(e.fontSize, 10, 28, DEFAULTS.editor.fontSize),
    tabSize: [2, 4, 8].includes(Number(e.tabSize)) ? Number(e.tabSize) : DEFAULTS.editor.tabSize,
    wordWrap: e.wordWrap !== false
  };
}

export class UiState {
  sidebar = $state(DEFAULTS.sidebar);
  chat = $state(DEFAULTS.chat);
  problems = $state(DEFAULTS.problems);
  spellcheck = $state(DEFAULTS.spellcheck);
  /** Compile after a save (the debounce lives in App). */
  compileOnSave = $state(DEFAULTS.compileOnSave);
  /** File tree layout: 'folders' or 'type'. */
  treeView = $state(DEFAULTS.treeView);
  theme = $state(DEFAULTS.theme);
  /** Editor look and feel. */
  editor = $state({ ...DEFAULTS.editor });
  sizes = $state({ ...DEFAULTS.sizes });

  status = $state('Ready');
  toasts = $state([]);
  dialog = $state(null); // null | 'settings' | 'history' | 'palette' | 'opener' | 'search'
  /** Panel shown beside the chat: 'chat' | 'proposal' */
  chatTab = $state('chat');

  #nextToast = 1;

  constructor() {
    const saved = readPref(PREFS, {});
    for (const key of ['sidebar', 'chat', 'problems', 'spellcheck', 'compileOnSave']) {
      if (typeof saved[key] === 'boolean') this[key] = saved[key];
    }
    if (['light', 'dark', 'auto'].includes(saved.theme)) this.theme = saved.theme;
    if (['folders', 'type'].includes(saved.treeView)) this.treeView = saved.treeView;
    if (saved.editor) this.editor = cleanEditor({ ...DEFAULTS.editor, ...saved.editor });
    if (saved.sizes && Object.values(saved.sizes).every((n) => Number.isFinite(n))) {
      this.sizes = { ...this.sizes, ...saved.sizes };
    }
  }

  /** Make sure a pane is visible. */
  show(pane) {
    if (!this[pane]) this.toggle(pane);
  }

  toggle(pane) {
    this[pane] = !this[pane];
    this.#persist();
  }

  setTreeView(view) {
    this.treeView = view;
    this.#persist();
  }

  setTheme(theme) {
    this.theme = theme;
    this.#persist();
    this.applyTheme();
  }

  /** Reflect the theme on <html data-theme>, resolving "auto" from the OS. */
  applyTheme() {
    const dark =
      this.theme === 'dark' ||
      (this.theme === 'auto' && globalThis.matchMedia?.('(prefers-color-scheme: dark)').matches);
    globalThis.document?.documentElement?.setAttribute('data-theme', dark ? 'dark' : 'light');
  }

  /** Change editor settings; out-of-range values are clamped, not trusted. */
  setEditor(patch) {
    this.editor = cleanEditor({ ...this.editor, ...patch });
    this.#persist();
  }

  setSizes(sizes) {
    this.sizes = { ...this.sizes, ...sizes };
    this.#persist();
  }

  openDialog(name) {
    this.dialog = name;
  }

  closeDialog() {
    this.dialog = null;
  }

  setStatus(text) {
    this.status = text;
  }

  toast(message, type = 'info') {
    const id = this.#nextToast++;
    this.toasts = [...this.toasts, { id, message, type }];
    setTimeout(() => {
      this.toasts = this.toasts.filter((t) => t.id !== id);
    }, 4000);
  }

  #persist() {
    writePref(PREFS, {
      sidebar: this.sidebar,
      chat: this.chat,
      problems: this.problems,
      spellcheck: this.spellcheck,
      compileOnSave: this.compileOnSave,
      treeView: this.treeView,
      theme: this.theme,
      editor: this.editor,
      sizes: this.sizes
    });
  }
}
