import { describe, it, expect } from 'vitest'

/**
 * The settings contract.
 *
 * `GET /api/config` answers with a far smaller object than SettingsPanel reads:
 * `{ ai_enabled, model }`, with no `editor` or `theme` key at all. The panel
 * assigned the response straight over its defaults, so `config.editor` became
 * undefined and the very next thing the markup does -- `config.editor.font_size`
 * -- threw during render. The panel never appeared, which looked exactly like
 * the Settings button being broken.
 *
 * This mirrors the merge so a thin response can never blank the defaults again.
 */
const DEFAULT_CONFIG = {
  keyboard_shortcuts: {},
  theme: { mode: 'auto', primary_color: '#0b0b0c', accent_color: '#2a2a2a', error_color: '#f0616b' },
  editor: { font_size: 14, font_family: 'JetBrains Mono', tab_size: 2, word_wrap: true, minimap_enabled: false },
  auto_save: true,
  history_limit: 100,
}

function mergeConfig(fromServer) {
  return {
    ...DEFAULT_CONFIG,
    ...fromServer,
    theme: { ...DEFAULT_CONFIG.theme, ...(fromServer.theme ?? {}) },
    editor: { ...DEFAULT_CONFIG.editor, ...(fromServer.editor ?? {}) },
    keyboard_shortcuts: {
      ...DEFAULT_CONFIG.keyboard_shortcuts,
      ...(fromServer.keyboard_shortcuts ?? {}),
    },
  }
}

describe('settings config merge', () => {
  // The exact response the server sends today.
  it('keeps nested defaults when the server omits them', () => {
    const merged = mergeConfig({ ai_enabled: false, model: 'llama-3.3-70b-versatile' })
    expect(merged.editor.font_size).toBe(14)
    expect(merged.theme.mode).toBe('auto')
    expect(merged.keyboard_shortcuts).toEqual({})
    expect(merged.model).toBe('llama-3.3-70b-versatile')
  })

  it('lets the server override individual nested fields', () => {
    const merged = mergeConfig({ editor: { font_size: 18 } })
    expect(merged.editor.font_size).toBe(18)
    // The fields it did not mention must survive.
    expect(merged.editor.tab_size).toBe(2)
    expect(merged.editor.font_family).toBe('JetBrains Mono')
  })

  it('survives an empty response', () => {
    const merged = mergeConfig({})
    expect(merged.editor.font_size).toBe(14)
    expect(merged.theme.mode).toBe('auto')
  })

  // Every field the panel binds to must be defined, or rendering throws and the
  // whole dialog disappears.
  it('defines every field the panel reads', () => {
    const merged = mergeConfig({ ai_enabled: true, model: 'x' })
    for (const key of ['font_size', 'font_family', 'tab_size', 'word_wrap', 'minimap_enabled']) {
      expect(merged.editor[key], `editor.${key}`).toBeDefined()
    }
    for (const key of ['mode', 'primary_color', 'accent_color', 'error_color']) {
      expect(merged.theme[key], `theme.${key}`).toBeDefined()
    }
  })
})
