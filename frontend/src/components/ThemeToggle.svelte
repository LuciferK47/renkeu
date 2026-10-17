<script>
  import { api } from '../lib/api/client.js'
  import { onMount, onDestroy } from 'svelte'
  import Icon from './Icon.svelte'

  export let currentTheme = 'light' // 'light', 'dark', 'auto'
  export let onThemeChange = async (theme) => {}

  let mounted = false
  let systemPrefersDark = false
  let mediaQuery = null

  const MODES = [
    { id: 'light', label: 'Light', icon: 'sun' },
    { id: 'dark', label: 'Dark', icon: 'moon' },
    { id: 'auto', label: 'Match system', icon: 'monitor' },
  ]

  /**
   * Apply theme to the document.
   *
   * One hook, `data-theme` on <html>. This used to set four (`data-theme`,
   * `.dark`, `.dark-mode`, `.light-mode`), two of which nothing read. They
   * existed because components hardcoded their colors; now that colors resolve
   * through tokens, the attribute alone is enough — and Tailwind's darkMode
   * selector points at it too.
   */
  function applyTheme(theme) {
    const isDark = theme === 'dark' || (theme === 'auto' && systemPrefersDark)
    document.documentElement.setAttribute('data-theme', isDark ? 'dark' : 'light')
    currentTheme = theme
  }

  async function handleThemeChange(newTheme) {
    applyTheme(newTheme)
    await onThemeChange(newTheme)
  }

  function onSystemChange(e) {
    systemPrefersDark = e.matches
    if (currentTheme === 'auto') applyTheme('auto')
  }

  async function loadTheme() {
    try {
      const config = await api.getConfig()
      if (config.theme?.mode) applyTheme(config.theme.mode)
    } catch (e) {
      console.error('Error loading theme:', e)
    }
  }

  onMount(async () => {
    mediaQuery = window.matchMedia('(prefers-color-scheme: dark)')
    systemPrefersDark = mediaQuery.matches
    if (currentTheme === 'auto') applyTheme('auto')
    mediaQuery.addEventListener('change', onSystemChange)

    await loadTheme()
    mounted = true
  })

  onDestroy(() => {
    mediaQuery?.removeEventListener('change', onSystemChange)
  })
</script>

<div class="theme-toggle" role="radiogroup" aria-label="Colour theme">
  {#each MODES as mode (mode.id)}
    <button
      type="button"
      class="mode"
      role="radio"
      aria-checked={currentTheme === mode.id}
      class:active={currentTheme === mode.id}
      on:click={() => handleThemeChange(mode.id)}
      title={mode.label}
    >
      <span class="sr-only">{mode.label}</span>
      <Icon name={mode.icon} size={15} />
    </button>
  {/each}
</div>

<style>
  .theme-toggle {
    display: flex;
    gap: 2px;
    align-items: center;
    background: hsl(var(--surface-2));
    padding: 3px;
    border-radius: var(--radius-pill);
    border: 1px solid hsl(var(--border));
  }

  .mode {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    background: transparent;
    border: 0;
    border-radius: var(--radius-pill);
    cursor: pointer;
    color: hsl(var(--text-dim));
    transition:
      background-color 0.16s ease,
      color 0.16s ease;
  }

  .mode:hover {
    background: hsl(var(--accent));
    color: hsl(var(--foreground));
  }

  .mode.active {
    background: hsl(var(--primary));
    color: hsl(var(--primary-foreground));
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
    border-width: 0;
  }
</style>
