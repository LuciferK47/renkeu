<script>
  import { onMount, onDestroy } from 'svelte'

  export let currentTheme = 'light' // 'light', 'dark', 'auto'
  export let onThemeChange = async (theme) => {}

  let mounted = false
  let systemPrefersDark = false
  let mediaQuery = null

  const MODES = [
    { id: 'light', label: 'Light' },
    { id: 'dark', label: 'Dark' },
    { id: 'auto', label: 'Match system' },
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
      const res = await fetch('/api/config')
      if (!res.ok) throw new Error('Failed to load config')
      const config = await res.json()
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
      <!-- Drawn icons on one 16px grid at a single stroke weight, rather than
           the [SUN] / [MOON] placeholders and a stray emoji this replaced. -->
      <svg viewBox="0 0 16 16" width="15" height="15" fill="none"
           stroke="currentColor" stroke-width="1.4"
           stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        {#if mode.id === 'light'}
          <circle cx="8" cy="8" r="3.1" />
          <path d="M8 1v1.6M8 13.4V15M15 8h-1.6M2.6 8H1M12.95 3.05l-1.13 1.13M4.18 11.82l-1.13 1.13M12.95 12.95l-1.13-1.13M4.18 4.18L3.05 3.05" />
        {:else if mode.id === 'dark'}
          <path d="M13.5 9.6A5.9 5.9 0 0 1 6.4 2.5a5.9 5.9 0 1 0 7.1 7.1z" />
        {:else}
          <rect x="1.8" y="2.8" width="12.4" height="8.4" rx="1.3" />
          <path d="M5.8 14h4.4M8 11.2V14" />
        {/if}
      </svg>
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
    border-radius: 999px;
    border: 1px solid hsl(var(--border));
  }

  .mode {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    background: transparent;
    border: 0;
    border-radius: 999px;
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
