<script>
  // Review of what the assistant proposes, possibly across several files.
  //
  // The diff for each hunk comes from `POST /api/agent/preview`, which runs the
  // same code that performs the write, so what is shown is what applying does.
  // Hunks are grouped by file; each can be taken or left, per hunk or per file.
  import { api } from '../lib/api/client.js'
  import {
    mergePreviews,
    applicable as applicableHunks,
    defaultSelection,
    toggleHunk,
    isAllSelected,
    toggleAll as toggleAllHunks,
    selectedHunkIds,
    groupByFile,
    isFileSelected,
    toggleFile
  } from '../utils/hunks.js'

  let { proposal, busy = false, onapply = () => {}, ondiscard = () => {}, onopenfile = null } = $props()

  let previews = $state([])
  let selected = $state(new Set())
  let etag = $state(null)
  let loading = $state(false)
  let error = $state('')

  const groups = $derived(groupByFile(previews))
  const usable = $derived(applicableHunks(previews))
  const chosen = $derived(selectedHunkIds(previews, selected))
  const stale = $derived(previews.length - usable.length)

  // Preview whenever a new proposal arrives.
  $effect(() => {
    const edits = proposal.edits
    const file = proposal.file
    let live = true
    loading = true
    error = ''
    api
      .previewHunks(file, edits)
      .then((res) => {
        if (!live) return
        etag = res.etag
        previews = mergePreviews(edits, res.hunks)
        selected = defaultSelection(previews)
      })
      .catch((e) => {
        if (!live) return
        error = e.message || 'Could not preview the proposed changes'
        previews = []
      })
      .finally(() => live && (loading = false))
    return () => (live = false)
  })

  const lines = (diff) =>
    diff
      .split('\n')
      .filter((l) => !l.startsWith('---') && !l.startsWith('+++'))
      .map((text) => ({
        text,
        kind: text.startsWith('+') ? 'add' : text.startsWith('-') ? 'del' : text.startsWith('@@') ? 'meta' : 'ctx'
      }))

  const apply = () => onapply({ edits: proposal.edits, selectedHunkIds: chosen, expectedEtag: etag })
</script>

<section class="review" aria-label="Proposed changes">
  <header>
    <label class="all">
      <input
        type="checkbox"
        checked={isAllSelected(previews, selected)}
        disabled={usable.length === 0 || busy}
        onchange={() => (selected = toggleAllHunks(previews, selected))}
      />
      {chosen.length} of {usable.length} selected
    </label>
    {#if stale > 0}<span class="note" role="status">{stale} no longer apply</span>{/if}
    <span class="spacer"></span>
    <button onclick={ondiscard} disabled={busy}>Discard</button>
    <button class="primary" onclick={apply} disabled={chosen.length === 0 || busy}>
      {busy ? 'Applying…' : `Apply ${chosen.length} change${chosen.length === 1 ? '' : 's'}`}
    </button>
  </header>

  {#if proposal.verified}<p class="verified">These edits compile.</p>{/if}
  {#if error}<p class="bad" role="alert">{error}</p>{/if}

  {#if loading}
    <p class="note">Previewing changes…</p>
  {:else if previews.length === 0 && !error}
    <p class="note">Nothing to review.</p>
  {/if}

  {#each groups as group (group.path)}
    <div class="file">
      <div class="file-head">
        <label>
          <input
            type="checkbox"
            checked={isFileSelected(group, selected)}
            disabled={applicableHunks(group.previews).length === 0 || busy}
            onchange={() => (selected = toggleFile(group, selected))}
            aria-label={`Select every change in ${group.path}`}
          />
          <strong>{group.path}</strong>
        </label>
        <span class="count">{group.previews.length} change{group.previews.length === 1 ? '' : 's'}</span>
        {#if onopenfile}<button class="link" onclick={() => onopenfile(group.path)}>Open</button>{/if}
      </div>
      <ul>
        {#each group.previews as p (p.hunk_id)}
          <li class:stale={!p.applies}>
            <label class="hunk">
              <input
                type="checkbox"
                checked={selected.has(p.hunk_id)}
                disabled={!p.applies || busy}
                onchange={() => (selected = toggleHunk(selected, p.hunk_id))}
                aria-label={`Change ${p.hunk_id} in ${group.path}`}
              />
              {#if p.edit}<span class="op">{p.edit.op}</span>{/if}
              {#if !p.applies}<span class="why">{p.error ?? 'no longer applies'}</span>{/if}
            </label>
            {#if p.applies}
              <pre class="diff">{#each lines(p.diff) as l, i (i)}<span class={l.kind}>{l.text}
</span>{/each}</pre>
            {/if}
          </li>
        {/each}
      </ul>
    </div>
  {/each}
</section>

<style>
  .review { display: flex; flex-direction: column; gap: 0.4rem; padding: 0 0.75rem 0.6rem; font-size: 13px; }
  header { position: sticky; top: 0; z-index: 1; display: flex; align-items: center; gap: 0.6rem; flex-wrap: wrap; padding: 0.4rem 0; background: hsl(var(--surface-1)); }
  .all { display: flex; align-items: center; gap: 0.35rem; white-space: nowrap; }
  .spacer { flex: 1; }
  .verified { margin: 0; color: hsl(var(--ok, 142 60% 40%)); font-size: 12px; }
  .note { margin: 0; color: hsl(var(--text-dim)); font-size: 12px; }
  .bad { margin: 0; color: hsl(var(--err)); }
  .file { border: 1px solid hsl(var(--border)); border-radius: var(--radius-xs); overflow: hidden; }
  .file-head { display: flex; align-items: center; gap: 0.5rem; padding: 0.3rem 0.5rem; background: hsl(var(--surface-2)); }
  .file-head label { display: flex; align-items: center; gap: 0.4rem; min-width: 0; flex: 1; }
  .file-head strong { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .count { color: hsl(var(--text-dim)); font-size: 12px; }
  ul { list-style: none; margin: 0; padding: 0; }
  li { border-top: 1px solid hsl(var(--border)); }
  li.stale { opacity: 0.6; }
  .hunk { display: flex; align-items: center; gap: 0.5rem; padding: 0.25rem 0.5rem; }
  .op { font-size: 11px; padding: 0 0.35rem; border: 1px solid hsl(var(--border)); border-radius: var(--radius-pill); color: hsl(var(--text-dim)); }
  .why { color: hsl(var(--err)); font-size: 12px; }
  .diff { margin: 0; padding: 0.3rem 0.5rem; overflow-x: auto; font: 12px/1.4 var(--font-mono, ui-monospace, monospace); }
  .diff .add { background: hsl(142 50% 40% / 0.18); }
  .diff .del { background: hsl(0 60% 50% / 0.18); }
  .diff .meta { color: hsl(var(--text-dim)); }
  button { padding: 0.2rem 0.7rem; background: hsl(var(--secondary)); color: hsl(var(--secondary-foreground)); border: 1px solid hsl(var(--border)); border-radius: var(--radius-xs); font: inherit; cursor: pointer; }
  button.primary { background: hsl(var(--primary)); color: hsl(var(--primary-foreground)); border-color: transparent; }
  button.link { background: none; border: none; color: hsl(var(--primary)); padding: 0; }
  button:disabled { opacity: 0.5; cursor: default; }
</style>
