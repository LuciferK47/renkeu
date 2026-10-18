<script>
  // Bottom bar: what the app is doing, whether the open file is saved, and
  // where the cursor is. Reads state; owns none.
  let { status = '', doc = null, compile = null, cursor = null } = $props()

  const save = $derived.by(() => {
    if (!doc) return ''
    if (doc.conflict) return 'Conflict — choose how to resolve'
    if (doc.error) return `Not saved: ${doc.error}`
    if (doc.saving) return 'Saving…'
    return doc.dirty ? 'Unsaved changes' : 'Saved'
  })
  const saveTone = $derived(doc?.conflict || doc?.error ? 'bad' : doc?.dirty || doc?.saving ? 'warn' : 'ok')
</script>

<div class="status-bar" role="status" aria-live="polite">
  <span class="status">{status}</span>
  {#if compile?.compiling}
    <span class="chip warn">Compiling {compile.progress ? `${compile.progress}%` : '…'}</span>
  {/if}
  <span class="spacer"></span>
  {#if cursor}<span class="chip">Ln {cursor.line}, Col {cursor.column}</span>{/if}
  {#if doc}<span class="chip {saveTone}">{save}</span>{/if}
</div>

<style>
  .status-bar {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding: 0 0.75rem;
    height: 24px;
    font-size: 12px;
    background: hsl(var(--surface-1));
    border-top: 1px solid hsl(var(--border));
    color: hsl(var(--text-dim));
  }
  .spacer { flex: 1; }
  .chip { padding: 0 0.4rem; border-radius: var(--radius-pill); }
  .chip.ok { color: hsl(var(--ok)); }
  .chip.warn { color: hsl(var(--warn)); }
  .chip.bad { color: hsl(var(--err)); font-weight: 600; }
</style>
