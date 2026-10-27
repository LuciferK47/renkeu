<script>
  // Ask the assistant to bring the document down to a page limit. The server
  // measures the PDF after every attempt and only offers a result that meets
  // the limit, so this asks for little: the number, and anything to protect.
  import { untrack } from 'svelte'
  import { readPref, writePref } from '../lib/storage.js'

  let { pages = '', onsubmit = () => {}, oncancel = () => {} } = $props()

  const BUDGET_KEY = 'latex-ai:page-budget'
  let limit = $state(String(untrack(() => pages) || readPref(BUDGET_KEY, '') || ''))
  let note = $state('')
  let input = $state()

  const value = $derived.by(() => {
    const n = Number.parseInt(limit, 10)
    return Number.isInteger(n) && n >= 1 && n <= 10000 ? n : null
  })

  $effect(() => input?.focus())

  function submit(e) {
    e.preventDefault()
    if (value === null) return
    writePref(BUDGET_KEY, String(value))
    onsubmit({ pages: value, note: note.trim() })
  }
</script>

<div class="overlay" role="presentation" onclick={(e) => e.target === e.currentTarget && oncancel()}>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="fit-title" tabindex="-1" onkeydown={(e) => e.key === 'Escape' && oncancel()}>
    <form onsubmit={submit}>
      <h2 id="fit-title">Fit to a page limit</h2>
      <label>
        Pages at most
        <input bind:this={input} bind:value={limit} type="number" min="1" max="10000" inputmode="numeric" aria-label="Page limit" />
      </label>
      <label>
        Anything to keep or change? (optional)
        <textarea bind:value={note} rows="2" placeholder="Keep the abstract as it is"></textarea>
      </label>
      <p class="hint">
        The assistant tightens wording, narrows figures and moves floats, trying each version on a copy. It never removes sections, figures, equations or
        citations, and never shrinks margins, fonts or spacing. You review the result before anything is changed.
      </p>
      <div class="actions">
        <button type="submit" class="primary" disabled={value === null}>Fit to {value ?? 'N'} page{value === 1 ? '' : 's'}</button>
        <button type="button" onclick={oncancel}>Cancel</button>
      </div>
    </form>
  </div>
</div>

<style>
  .overlay { position: fixed; inset: 0; display: grid; place-items: center; background: rgb(0 0 0 / 0.45); z-index: 900; }
  .dialog { width: min(28rem, 92vw); padding: 1.1rem 1.25rem; background: hsl(var(--popover)); color: hsl(var(--popover-foreground)); border: 1px solid hsl(var(--border)); border-radius: var(--radius-s); box-shadow: 0 12px 40px rgb(0 0 0 / 0.35); }
  h2 { margin: 0 0 0.7rem; font-size: 1rem; }
  form { display: flex; flex-direction: column; gap: 0.7rem; }
  label { display: flex; flex-direction: column; gap: 0.25rem; font-size: 13px; color: hsl(var(--text-dim)); }
  input, textarea { padding: 0.4rem 0.5rem; font: inherit; color: hsl(var(--foreground)); background: hsl(var(--surface-1)); border: 1px solid hsl(var(--border)); border-radius: var(--radius-xs); }
  input { width: 6rem; }
  textarea { resize: vertical; }
  .hint { margin: 0; font-size: 12px; color: hsl(var(--text-dim)); }
  .actions { display: flex; gap: 0.5rem; }
  button { padding: 0.35rem 0.9rem; font: inherit; border-radius: var(--radius-xs); border: 1px solid hsl(var(--border)); background: hsl(var(--secondary)); color: hsl(var(--secondary-foreground)); cursor: pointer; }
  button.primary { background: hsl(var(--primary)); color: hsl(var(--primary-foreground)); border-color: transparent; }
  button:disabled { opacity: 0.5; cursor: default; }
</style>
