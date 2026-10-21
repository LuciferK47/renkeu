<script>
  // Edit the source line behind a spot on the PDF, without leaving the page.
  // Enter saves and recompiles, Escape closes, and "Open in editor" goes to the
  // line for anything bigger than a one-line change.
  import { untrack } from 'svelte'
  import { singleLine } from '../lib/quickedit.js'

  let { file, line, text, exact = true, x, y, onsubmit = () => {}, onopen = () => {}, onclose = () => {} } = $props()

  let value = $state(untrack(() => text))
  let input = $state()
  $effect(() => {
    input?.focus()
    input?.select()
  })

  // Keep the box on screen: it opens where the click was.
  const left = $derived(Math.max(8, Math.min(x, (typeof window === 'undefined' ? 1200 : window.innerWidth) - 440)))
  const top = $derived(Math.max(8, y + 14))

  function submit(e) {
    e.preventDefault()
    onsubmit(singleLine(value))
  }
</script>

<svelte:window onkeydown={(e) => e.key === 'Escape' && onclose()} />

<form class="quick" style:left={`${left}px`} style:top={`${top}px`} onsubmit={submit} aria-label="Quick edit">
  <label>
    <span class="where">{file}:{line}{#if !exact} · nearest line to your click{/if}</span>
    <input bind:this={input} bind:value spellcheck="false" autocomplete="off" aria-label={`Source line ${line} of ${file}`} />
  </label>
  <span class="actions">
    <button type="submit" class="primary" disabled={value === text}>Save and recompile</button>
    <button type="button" onclick={onopen}>Open in editor</button>
    <button type="button" onclick={onclose}>Cancel</button>
  </span>
</form>

<style>
  .quick { position: fixed; z-index: 750; width: min(26rem, 92vw); display: flex; flex-direction: column; gap: 0.5rem; padding: 0.6rem 0.7rem; background: hsl(var(--popover)); border: 1px solid hsl(var(--primary)); border-radius: var(--radius-s); box-shadow: 0 10px 32px rgb(0 0 0 / 0.4); }
  label { display: flex; flex-direction: column; gap: 0.25rem; }
  .where { font-size: 11px; color: hsl(var(--text-dim)); font-family: 'JetBrains Mono', monospace; }
  input { padding: 0.4rem 0.55rem; font: inherit; font-family: 'JetBrains Mono', monospace; font-size: 13px; color: hsl(var(--foreground)); background: hsl(var(--surface-0)); border: 1px solid hsl(var(--border)); border-radius: var(--radius-xs); }
  .actions { display: flex; gap: 0.4rem; flex-wrap: wrap; }
  button { padding: 0.25rem 0.65rem; font: inherit; font-size: 13px; color: hsl(var(--secondary-foreground)); background: hsl(var(--secondary)); border: 1px solid hsl(var(--border)); border-radius: var(--radius-xs); cursor: pointer; }
  button.primary { color: hsl(var(--primary-foreground)); background: hsl(var(--primary)); border-color: transparent; }
  button:disabled { opacity: 0.5; cursor: default; }
</style>
