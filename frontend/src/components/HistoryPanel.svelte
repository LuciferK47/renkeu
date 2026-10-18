<script>
  // Every saved change, newest first: what kind it was, which file, and the
  // diff. Any version can be put back; the restore is a normal save, so it is
  // itself recorded and can be undone the same way.
  import { api } from '../lib/api/client.js'
  import { labelFor } from '../lib/history.js'

  let { filePath = '', onrestore = async (_entry, _which) => false, onclose = () => {} } = $props()

  let scope = $state(filePath ? 'file' : 'all')
  let aiOnly = $state(false)
  let entries = $state([])
  let loading = $state(false)
  let error = $state('')
  let open = $state(null)
  let restoring = $state(null)
  /** Entries whose "before" is the file not existing; there is nothing to undo to. */
  const undoable = (e) => e.has_before && e.edit_type !== 'delete' && e.edit_type !== 'create'

  async function load() {
    loading = true
    error = ''
    try {
      entries = await api.getHistory({
        path: scope === 'file' && filePath ? filePath : undefined,
        agent_only: aiOnly ? 'true' : undefined,
        limit: 200
      })
    } catch (e) {
      error = e.message
      entries = []
    } finally {
      loading = false
    }
  }

  $effect(() => {
    void scope
    void aiOnly
    load()
  })

  async function restore(entry, which) {
    restoring = `${entry.id}:${which}`
    try {
      if (await onrestore(entry, which)) await load()
    } finally {
      restoring = null
    }
  }

  const when = (iso) => {
    if (!iso) return ''
    const d = new Date(iso.includes('T') ? iso : `${iso.replace(' ', 'T')}Z`)
    return Number.isNaN(d.getTime()) ? iso : d.toLocaleString()
  }

  const lines = (diff) =>
    diff
      .split('\n')
      .filter((l) => !l.startsWith('---') && !l.startsWith('+++'))
      .map((text) => ({ text, kind: text.startsWith('+') ? 'add' : text.startsWith('-') ? 'del' : text.startsWith('@@') ? 'meta' : 'ctx' }))
</script>

<div class="overlay" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="history-title" tabindex="-1" onkeydown={(e) => e.key === 'Escape' && onclose()}>
    <header>
      <h2 id="history-title">History</h2>
      <button class="x" onclick={onclose} aria-label="Close history">✕</button>
    </header>

    <div class="filters">
      <label>
        <input type="radio" bind:group={scope} value="file" disabled={!filePath} />
        {filePath ? `This file (${filePath})` : 'This file'}
      </label>
      <label><input type="radio" bind:group={scope} value="all" /> All files</label>
      <label class="ai"><input type="checkbox" bind:checked={aiOnly} /> AI changes only</label>
    </div>

    {#if error}<p class="bad" role="alert">{error}</p>{/if}

    <ul class="entries">
      {#if loading && entries.length === 0}
        <li class="note">Loading…</li>
      {:else if entries.length === 0}
        <li class="note">Nothing here yet.</li>
      {/if}
      {#each entries as entry (entry.id)}
        <li>
          <div class="row">
            <button class="summary" aria-expanded={open === entry.id} onclick={() => (open = open === entry.id ? null : entry.id)}>
              <span class="badge {entry.edit_type}">{labelFor(entry.edit_type)}</span>
              <span class="path">{entry.file_path}</span>
              <span class="when">{when(entry.created_at)}</span>
            </button>
            {#if undoable(entry)}
              <button class="restore" onclick={() => restore(entry, 'before')} disabled={restoring !== null}
                title="Put the file back as it was before this change">
                {restoring === `${entry.id}:before` ? 'Undoing…' : 'Undo this change'}
              </button>
            {/if}
            {#if entry.edit_type === 'delete'}
              <button class="restore" onclick={() => restore(entry, 'before')} disabled={restoring !== null}>
                {restoring === `${entry.id}:before` ? 'Restoring…' : 'Bring back'}
              </button>
            {:else}
              <button class="restore" onclick={() => restore(entry, 'after')} disabled={restoring !== null}
                title="Put the file back as it was right after this entry">
                {restoring === `${entry.id}:after` ? 'Restoring…' : 'Restore this version'}
              </button>
            {/if}
          </div>
          {#if open === entry.id}
            <pre class="diff">{#each lines(entry.diff) as l, i (i)}<span class={l.kind}>{l.text}
</span>{/each}</pre>
          {/if}
        </li>
      {/each}
    </ul>
  </div>
</div>

<style>
  .overlay { position: fixed; inset: 0; display: flex; justify-content: center; padding-top: 8vh; background: rgb(0 0 0 / 0.4); z-index: 700; }
  .dialog { align-self: flex-start; width: min(46rem, 94vw); max-height: 80vh; display: flex; flex-direction: column; background: hsl(var(--popover)); color: hsl(var(--popover-foreground)); border: 1px solid hsl(var(--border)); border-radius: var(--radius-s); box-shadow: 0 12px 40px rgb(0 0 0 / 0.35); }
  header { display: flex; justify-content: space-between; align-items: center; padding: 0.7rem 1rem 0.3rem; }
  h2 { margin: 0; font-size: 1.05rem; }
  .x { background: none; border: none; color: inherit; cursor: pointer; font-size: 15px; }
  .filters { display: flex; gap: 1rem; flex-wrap: wrap; padding: 0.3rem 1rem 0.6rem; font-size: 13px; }
  .filters label { display: flex; align-items: center; gap: 0.3rem; }
  .ai { margin-left: auto; }
  .entries { list-style: none; margin: 0; padding: 0; overflow: auto; border-top: 1px solid hsl(var(--border)); }
  li { border-bottom: 1px solid hsl(var(--border)); }
  .note { padding: 1rem; color: hsl(var(--text-dim)); }
  .row { display: flex; align-items: center; gap: 0.5rem; padding-right: 0.75rem; }
  .summary { flex: 1; min-width: 0; display: flex; align-items: center; gap: 0.6rem; padding: 0.5rem 1rem; background: none; border: none; color: inherit; font: inherit; font-size: 13px; text-align: left; cursor: pointer; }
  .summary:hover { background: hsl(var(--accent)); }
  .path { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .when { color: hsl(var(--text-dim)); font-size: 12px; white-space: nowrap; }
  .badge { font-size: 11px; padding: 0 0.45rem; border: 1px solid hsl(var(--border)); border-radius: var(--radius-pill); white-space: nowrap; }
  .badge.agent-apply, .badge.apply-selected { border-color: hsl(var(--primary)); color: hsl(var(--primary)); }
  .badge.delete { border-color: hsl(var(--err)); color: hsl(var(--err)); }
  .restore { padding: 0.15rem 0.6rem; font: inherit; font-size: 12px; background: hsl(var(--secondary)); color: hsl(var(--secondary-foreground)); border: 1px solid hsl(var(--border)); border-radius: var(--radius-xs); cursor: pointer; }
  .restore:disabled { opacity: 0.5; cursor: default; }
  .diff { margin: 0; padding: 0.4rem 1rem; overflow-x: auto; background: hsl(var(--surface-0)); font: 12px/1.4 var(--font-mono, ui-monospace, monospace); }
  .diff .add { background: hsl(142 50% 40% / 0.18); }
  .diff .del { background: hsl(0 60% 50% / 0.18); }
  .diff .meta { color: hsl(var(--text-dim)); }
  .bad { margin: 0 1rem 0.5rem; color: hsl(var(--err)); }
</style>
