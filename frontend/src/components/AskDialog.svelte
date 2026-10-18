<script>
  // "Ask the AI…": one line, then either a quick plan (shown for review at
  // once) or a verified run (tries the edits, checks they compile, then shows
  // them). Both end in the assistant's review; nothing is applied from here.
  let { onplan = () => {}, onrun = () => {}, onclose = () => {} } = $props()

  let text = $state('')
  let input = $state()
  $effect(() => input?.focus())

  const go = (fn) => {
    const t = text.trim()
    if (t) fn(t)
  }
</script>

<div class="overlay" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <form
    class="box"
    role="dialog"
    aria-modal="true"
    aria-label="Ask the AI"
    onsubmit={(e) => (e.preventDefault(), go(onplan))}
    onkeydown={(e) => e.key === 'Escape' && onclose()}
  >
    <input bind:this={input} bind:value={text} placeholder="What should change in this file?" aria-label="What should change" autocomplete="off" />
    <div class="actions">
      <button type="submit" class="primary" disabled={!text.trim()}>Quick plan</button>
      <button type="button" onclick={() => go(onrun)} disabled={!text.trim()} title="Tries the edits on a copy and checks they compile before showing them">Plan and verify</button>
      <span class="hint">Enter = quick plan</span>
    </div>
  </form>
</div>

<style>
  .overlay { position: fixed; inset: 0; display: flex; justify-content: center; padding-top: 14vh; background: rgb(0 0 0 / 0.35); z-index: 700; }
  .box { align-self: flex-start; width: min(40rem, 92vw); padding: 0.9rem; background: hsl(var(--popover)); border: 1px solid hsl(var(--border)); border-radius: var(--radius-s); box-shadow: 0 12px 40px rgb(0 0 0 / 0.35); }
  input { width: 100%; box-sizing: border-box; padding: 0.55rem 0.7rem; font: inherit; font-size: 15px; background: hsl(var(--surface-0)); color: inherit; border: 1px solid hsl(var(--border)); border-radius: var(--radius-xs); }
  .actions { display: flex; align-items: center; gap: 0.5rem; margin-top: 0.6rem; }
  .hint { margin-left: auto; font-size: 12px; color: hsl(var(--text-dim)); }
  button { padding: 0.3rem 0.8rem; font: inherit; border: 1px solid hsl(var(--border)); border-radius: var(--radius-xs); background: hsl(var(--secondary)); color: hsl(var(--secondary-foreground)); cursor: pointer; }
  button.primary { background: hsl(var(--primary)); color: hsl(var(--primary-foreground)); border-color: transparent; }
  button:disabled { opacity: 0.5; cursor: default; }
</style>
