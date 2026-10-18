import { describe, it, expect, vi } from 'vitest'
import { CommandRegistry, parseChord, matches, formatChord } from '../src/lib/commands.js'

const key = (key, mods = {}) => ({ key, ctrlKey: false, metaKey: false, shiftKey: false, altKey: false, ...mods })

describe('chords', () => {
  it('Mod is Ctrl off a Mac and Cmd on one, never both', () => {
    expect(matches(key('s', { ctrlKey: true }), 'Mod+S', false)).toBe(true)
    expect(matches(key('s', { metaKey: true }), 'Mod+S', false)).toBe(false)
    expect(matches(key('s', { metaKey: true }), 'Mod+S', true)).toBe(true)
    expect(matches(key('s', { ctrlKey: true }), 'Mod+S', true)).toBe(false)
  })

  it('requires exactly the modifiers named', () => {
    expect(matches(key('p', { ctrlKey: true, shiftKey: true }), 'Mod+P', false)).toBe(false)
    expect(matches(key('p', { ctrlKey: true, shiftKey: true }), 'Mod+Shift+P', false)).toBe(true)
    expect(matches(key('Enter', { ctrlKey: true }), 'Mod+Enter', false)).toBe(true)
    expect(matches(key('j', { ctrlKey: true, altKey: true }), 'Ctrl+Alt+J', false)).toBe(true)
  })

  it('ignores case of the key and a chord with no key', () => {
    expect(matches(key('S', { ctrlKey: true }), 'Mod+s', false)).toBe(true)
    expect(parseChord('Mod+Shift')).toBeNull()
    expect(matches(key('s'), 'Mod+Shift', false)).toBe(false)
  })

  it('formats for the platform', () => {
    expect(formatChord('Mod+Shift+P', false)).toBe('Ctrl+Shift+P')
    expect(formatChord('Mod+Enter', true)).toBe('⌘Enter')
  })
})

describe('registry', () => {
  it('runs a command by id, and not when it is disabled or unknown', async () => {
    const r = new CommandRegistry()
    const run = vi.fn()
    let on = false
    r.register({ id: 'compile', title: 'Compile', keys: 'Mod+Enter', run, enabled: () => on })

    expect(await r.run('compile')).toBe(false)
    expect(await r.run('nope')).toBe(false)
    on = true
    expect(await r.run('compile')).toBe(true)
    expect(run).toHaveBeenCalledTimes(1)
  })

  it('refuses a duplicate id and can unregister', () => {
    const r = new CommandRegistry()
    const off = r.register({ id: 'a', title: 'A', run() {} })
    expect(() => r.register({ id: 'a', title: 'A2', run() {} })).toThrow(/already registered/)
    off()
    expect(r.get('a')).toBeUndefined()
    r.register({ id: 'a', title: 'A again', run() {} })
  })

  it('lists titles, shortcuts and whether each is enabled', () => {
    const r = new CommandRegistry()
    r.register({ id: 'save', title: 'Save', keys: 'Mod+S', run() {} })
    r.register({ id: 'cancel', title: 'Cancel compile', run() {}, enabled: () => false })
    expect(r.list()).toEqual([
      { id: 'save', title: 'Save', keys: 'Mod+S', enabled: true },
      { id: 'cancel', title: 'Cancel compile', keys: undefined, enabled: false }
    ])
  })

  it('a shortcut on a target runs its command and stops the key reaching anyone else', () => {
    const r = new CommandRegistry()
    const run = vi.fn()
    r.register({ id: 'save', title: 'Save', keys: 'Ctrl+S', run })
    const target = document.createElement('div')
    const inner = document.createElement('textarea')
    target.append(inner)
    document.body.append(target)
    const reachedEditor = vi.fn()
    inner.addEventListener('keydown', reachedEditor)
    const detach = r.attach(target, { capture: true })

    const event = new KeyboardEvent('keydown', { key: 's', ctrlKey: true, bubbles: true, cancelable: true })
    inner.dispatchEvent(event)

    expect(run).toHaveBeenCalledTimes(1)
    expect(event.defaultPrevented).toBe(true)
    expect(reachedEditor).not.toHaveBeenCalled() // the editor never saw the chord

    detach()
    inner.dispatchEvent(new KeyboardEvent('keydown', { key: 's', ctrlKey: true, bubbles: true }))
    expect(run).toHaveBeenCalledTimes(1)
    target.remove()
  })

  it('keys that are no command are left alone', () => {
    const r = new CommandRegistry()
    r.register({ id: 'save', title: 'Save', keys: 'Ctrl+S', run() {} })
    const target = document.createElement('div')
    const detach = r.attach(target)
    const event = new KeyboardEvent('keydown', { key: 'a', bubbles: true, cancelable: true })
    target.dispatchEvent(event)
    expect(event.defaultPrevented).toBe(false)
    detach()
  })
})
