<script>
  import { onMount } from 'svelte'

  export let currentTheme = 'light' // 'light', 'dark', 'auto'
  export let onThemeChange = async (theme) => {}

  let mounted = false
  let systemPrefersDark = false

  /**
   * Apply theme to the document
   */
  function applyTheme(theme) {
    const html = document.documentElement
    const isDark = theme === 'dark' || (theme === 'auto' && systemPrefersDark)

    if (isDark) {
      html.setAttribute('data-theme', 'dark')
      html.classList.add('dark')
      document.body.classList.add('dark-mode')
      document.body.classList.remove('light-mode')
    } else {
      html.setAttribute('data-theme', 'light')
      html.classList.remove('dark')
      document.body.classList.add('light-mode')
      document.body.classList.remove('dark-mode')
    }

    currentTheme = theme
  }

  /**
   * Handle theme change
   */
  async function handleThemeChange(newTheme) {
    applyTheme(newTheme)
    await onThemeChange(newTheme)
  }

  /**
   * Check system preference for dark mode
   */
  function checkSystemPreference() {
    systemPrefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches
    if (currentTheme === 'auto') {
      applyTheme('auto')
    }
  }

  /**
   * Load theme from server configuration
   */
  async function loadTheme() {
    try {
      const res = await fetch('/api/config')
      if (!res.ok) throw new Error('Failed to load config')
      const config = await res.json()
      if (config.theme?.mode) {
        applyTheme(config.theme.mode)
      }
    } catch (e) {
      console.error('Error loading theme:', e)
    }
  }

  /**
   * Save theme to server configuration
   */
  async function saveTheme(theme) {
    try {
      const res = await fetch('/api/config', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ 'theme.mode': theme })
      })
      if (!res.ok) throw new Error('Failed to save config')
      return true
    } catch (e) {
      console.error('Error saving theme:', e)
      return false
    }
  }

  onMount(async () => {
    checkSystemPreference()

    // Listen to system preference changes
    const mediaQuery = window.matchMedia('(prefers-color-scheme: dark)')
    mediaQuery.addEventListener('change', (e) => {
      systemPrefersDark = e.matches
      if (currentTheme === 'auto') {
        applyTheme('auto')
      }
    })

    // Load persisted theme
    await loadTheme()
    mounted = true
  })
</script>

<div class="theme-toggle-container">
  <button
    class="theme-btn"
    on:click={() => handleThemeChange('light')}
    class:active={currentTheme === 'light'}
    title="Light mode"
  >
    [SUN]
  </button>

  <button
    class="theme-btn"
    on:click={() => handleThemeChange('dark')}
    class:active={currentTheme === 'dark'}
    title="Dark mode"
  >
    [MOON]
  </button>

  <button
    class="theme-btn"
    on:click={() => handleThemeChange('auto')}
    class:active={currentTheme === 'auto'}
    title="Auto (system preference)"
  >
    ⚙️
  </button>
</div>

<style>
  .theme-toggle-container {
    display: flex;
    gap: 0.25rem;
    align-items: center;
    background: var(--bg-secondary, #f5f5f5);
    padding: 0.25rem;
    border-radius: 0.375rem;
    border: 1px solid var(--border-color, #e0e0e0);
  }

  .theme-btn {
    padding: 0.375rem 0.5rem;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 0.25rem;
    cursor: pointer;
    font-size: 0.9rem;
    transition: all 0.2s ease;
    color: hsl(var(--muted-foreground));
  }

  .theme-btn:hover {
    background: hsl(var(--secondary));
    color: hsl(var(--foreground));
  }

  .theme-btn.active {
    background: hsl(var(--primary));
    color: hsl(var(--primary-foreground));
  }

  :global(.dark-mode) .theme-toggle-container {
    background: hsl(var(--secondary));
    border-color: hsl(var(--border));
  }
</style>
