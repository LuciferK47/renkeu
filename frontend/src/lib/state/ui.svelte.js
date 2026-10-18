// Layout, theme and transient UI state. Preferences persist in localStorage
// (guarded: the app must work without it).

import { readPref, writePref } from '../storage.js';

const PREFS = 'latex-ai:ui';

const DEFAULTS = {
  sidebar: true,
  chat: true,
  problems: true,
  theme: 'auto',
  /** Pane sizes in percent, as the splitter reports them. */
  sizes: { sidebar: 16, editor: 38, pdf: 34, chat: 12 }
};

export class UiState {
  sidebar = $state(DEFAULTS.sidebar);
  chat = $state(DEFAULTS.chat);
  problems = $state(DEFAULTS.problems);
  theme = $state(DEFAULTS.theme);
  sizes = $state({ ...DEFAULTS.sizes });

  status = $state('Ready');
  toasts = $state([]);
  dialog = $state(null); // null | 'settings' | 'history' | 'palette' | 'opener' | 'search'
  /** Panel shown beside the chat: 'chat' | 'proposal' */
  chatTab = $state('chat');

  #nextToast = 1;

  constructor() {
    const saved = readPref(PREFS, {});
    for (const key of ['sidebar', 'chat', 'problems']) {
      if (typeof saved[key] === 'boolean') this[key] = saved[key];
    }
    if (['light', 'dark', 'auto'].includes(saved.theme)) this.theme = saved.theme;
    if (saved.sizes && Object.values(saved.sizes).every((n) => Number.isFinite(n))) {
      this.sizes = { ...this.sizes, ...saved.sizes };
    }
  }

  toggle(pane) {
    this[pane] = !this[pane];
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
      theme: this.theme,
      sizes: this.sizes
    });
  }
}
