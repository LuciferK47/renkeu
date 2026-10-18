// One place that knows every command and its shortcut.
//
// The old app had four sources of truth for "what does Ctrl+S do": a global
// listener, the editor's own binding, a settings table and the palette. Most
// of the shortcuts did nothing. Now a command is registered once, and the same
// matcher serves the window and the editor (which would otherwise swallow keys
// such as Ctrl+Enter).

const isMac = () => /mac/i.test(globalThis.navigator?.platform ?? '');

/**
 * Parse "Mod+Shift+P" into { mod, shift, alt, key }. `Mod` is Cmd on a Mac and
 * Ctrl elsewhere. Returns null for a chord with no key.
 */
export function parseChord(chord) {
  const parts = chord.split('+').map((p) => p.trim().toLowerCase());
  const key = parts.find((p) => !['mod', 'ctrl', 'cmd', 'meta', 'shift', 'alt'].includes(p));
  if (!key) return null;
  return {
    mod: parts.includes('mod'),
    ctrl: parts.includes('ctrl'),
    meta: parts.includes('cmd') || parts.includes('meta'),
    shift: parts.includes('shift'),
    alt: parts.includes('alt'),
    key
  };
}

/** Whether a keyboard event is exactly this chord. */
export function matches(event, chord, mac = isMac()) {
  const c = typeof chord === 'string' ? parseChord(chord) : chord;
  if (!c) return false;
  const wantCtrl = c.ctrl || (c.mod && !mac);
  const wantMeta = c.meta || (c.mod && mac);
  return (
    event.ctrlKey === wantCtrl &&
    event.metaKey === wantMeta &&
    event.shiftKey === c.shift &&
    event.altKey === c.alt &&
    String(event.key).toLowerCase() === c.key
  );
}

/** "Mod+Shift+P" as the user's platform writes it. */
export function formatChord(chord, mac = isMac()) {
  return chord
    .split('+')
    .map((part) => {
      switch (part.toLowerCase()) {
        case 'mod':
          return mac ? '⌘' : 'Ctrl';
        case 'shift':
          return mac ? '⇧' : 'Shift';
        case 'alt':
          return mac ? '⌥' : 'Alt';
        default:
          return part.length === 1 ? part.toUpperCase() : part;
      }
    })
    .join(mac ? '' : '+');
}

export class CommandRegistry {
  #commands = new Map();

  /**
   * @param {{ id: string, title: string, keys?: string, run: () => unknown,
   *           enabled?: () => boolean }} command
   * @returns {() => void} unregister
   */
  register(command) {
    if (this.#commands.has(command.id)) {
      throw new Error(`command already registered: ${command.id}`);
    }
    this.#commands.set(command.id, { ...command, chord: command.keys ? parseChord(command.keys) : null });
    return () => this.#commands.delete(command.id);
  }

  list() {
    return [...this.#commands.values()].map(({ id, title, keys, enabled }) => ({
      id,
      title,
      keys,
      enabled: enabled ? enabled() : true
    }));
  }

  get(id) {
    return this.#commands.get(id);
  }

  /** Run a command by id. Returns false if unknown or currently disabled. */
  async run(id) {
    const command = this.#commands.get(id);
    if (!command || (command.enabled && !command.enabled())) return false;
    await command.run();
    return true;
  }

  /** The command a key event triggers, if any. */
  forEvent(event, mac = isMac()) {
    for (const command of this.#commands.values()) {
      if (command.chord && matches(event, command.chord, mac)) return command;
    }
    return null;
  }

  /**
   * Listen for shortcuts on `target`. With `capture`, the listener runs before
   * the target's own handlers, which is how the editor is kept from swallowing
   * a chord such as Mod+Enter. Returns a function that removes it.
   */
  attach(target, { capture = false } = {}) {
    const onKeyDown = (event) => {
      const command = this.forEvent(event);
      if (!command) return;
      event.preventDefault();
      event.stopPropagation();
      void this.run(command.id);
    };
    target.addEventListener('keydown', onKeyDown, capture);
    return () => target.removeEventListener('keydown', onKeyDown, capture);
  }
}
