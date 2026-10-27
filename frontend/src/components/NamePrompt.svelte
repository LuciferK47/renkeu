<script>
  // Asks for a file or folder name. Validation that needs the server (does it
  // clash? is it allowed?) comes back as a toast; this only blocks the obvious.
  import { untrack } from 'svelte'

  let { title, value = '', confirmLabel = 'OK', onsubmit = () => {}, oncancel = () => {} } = $props()

  // The name to start from; later changes to the prop do not overwrite typing.
  let text = $state(untrack(() => value))
  let input = $state()

  const problem = $derived.by(() => {
    const t = text.trim()
    if (!t) return 'Enter a name.'
    if (t.startsWith('/') || t.split('/').includes('..')) return 'Use a path inside the project.'
    return ''
  })

  $effect(() => {
    input?.focus()
    // Select the name but not the extension, as file managers do.
    const dot = text.lastIndexOf('.')
    const slash = text.lastIndexOf('/')
    input?.setSelectionRange(slash + 1, dot > slash ? dot : text.length)
  })

  function submit(e) {
    e.preventDefault()
    if (!problem) onsubmit(text.trim())
  }
</script>

<div class="overlay" role="presentation" onclick={(e) => e.target === e.currentTarget && oncancel()}>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="name-title" tabindex="-1" onkeydown={(e) => e.key === 'Escape' && oncancel()}>
    <form onsubmit={submit}>
      <h2 id="name-title">{title}</h2>
      <input bind:this={input} bind:value={text} aria-label="Name" spellcheck="false" autocomplete="off" />
      <p class="hint" class:bad={problem && text.length > 0}>{problem || 'Folders in the path are created as needed.'}</p>
      <div class="actions">
        <button type="submit" class="primary" disabled={!!problem}>{confirmLabel}</button>
        <button type="button" onclick={oncancel}>Cancel</button>
      </div>
    </form>
  </div>
</div>

<style>
  .overlay { position: fixed; inset: 0; display: grid; place-items: center; background: rgb(0 0 0 / 0.45); z-index: 900; }
  .dialog { width: min(26rem, 92vw); padding: 1.1rem 1.25rem; background: hsl(var(--popover)); color: hsl(var(--popover-foreground)); border: 1px solid hsl(var(--border)); border-radius: var(--radius-s); box-shadow: 0 12px 40px rgb(0 0 0 / 0.35); }
  h2 { margin: 0 0 0.6rem; font-size: 1rem; }
  input { width: 100%; box-sizing: border-box; padding: 0.4rem 0.5rem; font: inherit; background: hsl(var(--surface-1)); color: inherit; border: 1px solid hsl(var(--border)); border-radius: var(--radius-xs); }
  .hint { margin: 0.4rem 0 0.8rem; font-size: 12px; color: hsl(var(--text-dim)); }
  .hint.bad { color: hsl(var(--err)); }
  .actions { display: flex; gap: 0.5rem; }
  button { padding: 0.35rem 0.9rem; font: inherit; border-radius: var(--radius-xs); border: 1px solid hsl(var(--border)); background: hsl(var(--secondary)); color: hsl(var(--secondary-foreground)); cursor: pointer; }
  button.primary { background: hsl(var(--primary)); color: hsl(var(--primary-foreground)); border-color: transparent; }
  button:disabled { opacity: 0.5; cursor: default; }
</style>
