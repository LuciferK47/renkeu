<script>
  import { onMount, onDestroy } from 'svelte'
  import { createEventDispatcher } from 'svelte'

  export let shortcuts = {
    save: 'ctrl+s',
    undo: 'ctrl+z',
    compile: 'ctrl+enter',
    apply_command: 'alt+a',
    toggle_history: 'alt+h',
    toggle_file_tree: 'alt+f',
    quick_open: 'ctrl+p',
    search: 'ctrl+shift+f'
  }

  export let enabled = true

  const dispatch = createEventDispatcher()

  let mappedShortcuts = {}

  /**
   * Parse a shortcut string like "ctrl+s" into event checker
   */
  function parseShortcut(shortcutString) {
    const parts = shortcutString.toLowerCase().split('+')
    return {
      ctrl: parts.includes('ctrl'),
      shift: parts.includes('shift'),
      alt: parts.includes('alt'),
      meta: parts.includes('meta'),
      key: parts.find(p => !['ctrl', 'shift', 'alt', 'meta'].includes(p))
    }
  }

  /**
   * Check if keyboard event matches a parsed shortcut
   */
  function matchesShortcut(event, parsed) {
    const eventKey = event.key.toLowerCase()
    return (
      event.ctrlKey === parsed.ctrl &&
      event.shiftKey === parsed.shift &&
      event.altKey === parsed.alt &&
      event.metaKey === parsed.meta &&
      eventKey === parsed.key
    )
  }

  /**
   * Handle global keyboard events
   */
  function handleKeyDown(event) {
    if (!enabled) return

    // Check each shortcut
    for (const [action, shortcutString] of Object.entries(shortcuts)) {
      const parsed = mappedShortcuts[action]
      if (parsed && matchesShortcut(event, parsed)) {
        event.preventDefault()
        event.stopPropagation()
        dispatch('shortcut', { action, event })
        break
      }
    }
  }

  onMount(() => {
    // Parse all shortcuts on mount
    for (const [action, shortcutString] of Object.entries(shortcuts)) {
      mappedShortcuts[action] = parseShortcut(shortcutString)
    }

    // Attach global keyboard listener
    window.addEventListener('keydown', handleKeyDown)
  })

  onDestroy(() => {
    window.removeEventListener('keydown', handleKeyDown)
  })

  /**
   * Get human-readable shortcut string
   */
  export function getShortcutLabel(shortcutString) {
    // Detect OS using browser APIs
    const isMac = navigator.platform.toUpperCase().includes('MAC') ||
                  (navigator.userAgentData && navigator.userAgentData.platform === 'macOS')
    
    return shortcutString
      .split('+')
      .map(part => {
        const mapping = {
          ctrl: isMac ? '[CMD]' : 'Ctrl',
          shift: 'Shift',
          alt: isMac ? '[OPT]' : 'Alt',
          meta: '[CMD]'
        }
        return mapping[part] || part.toUpperCase()
      })
      .join(' + ')
  }

  /**
   * Load shortcuts from server configuration
   */
  export async function loadShortcuts() {
    try {
      const res = await fetch('/api/config')
      if (!res.ok) throw new Error('Failed to load config')
      const config = await res.json()
      if (config.keyboard_shortcuts) {
        shortcuts = config.keyboard_shortcuts
        // Re-parse shortcuts
        for (const [action, shortcutString] of Object.entries(shortcuts)) {
          mappedShortcuts[action] = parseShortcut(shortcutString)
        }
      }
    } catch (e) {
      console.error('Error loading shortcuts:', e)
    }
  }

  /**
   * Save shortcuts to server configuration
   */
  export async function saveShortcuts(newShortcuts) {
    try {
      const res = await fetch('/api/config', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(newShortcuts)
      })
      if (!res.ok) throw new Error('Failed to save config')
      shortcuts = { ...shortcuts, ...newShortcuts }
      // Re-parse shortcuts
      for (const [action, shortcutString] of Object.entries(shortcuts)) {
        mappedShortcuts[action] = parseShortcut(shortcutString)
      }
      return true
    } catch (e) {
      console.error('Error saving shortcuts:', e)
      return false
    }
  }
</script>

<!-- This component is invisible - it only handles keyboard events -->
