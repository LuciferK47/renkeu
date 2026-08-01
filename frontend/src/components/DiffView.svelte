<script>
  /**
   * Per-hunk accept/reject review (masterplan §12 M4).
   *
   * One row per proposed edit. The diff shown for each comes from
   * `POST /api/agent/preview`, which computes it with the same code that
   * performs the write — a diff rendered client-side would eventually
   * disagree with what applying actually does.
   *
   * Svelte 5 runes; migrated as a touched file, not as a repo-wide sweep.
   */
  import { api } from '../lib/api/client.js';
  import {
    mergePreviews,
    applicable as applicableHunks,
    defaultSelection,
    toggleHunk,
    isAllSelected,
    toggleAll as toggleAllHunks,
    selectedHunkIds
  } from '../utils/hunks.js';

  let {
    /** Proposed edits, each already carrying a server-computed `hunk_id`. */
    edits = [],
    filePath = '',
    /** Called with ({ edits, selectedHunkIds, expectedEtag }) on apply. */
    onapply = null,
    onreject = null
  } = $props();

  let previews = $state([]);
  let selected = $state(new Set());
  let etag = $state(null);
  let loading = $state(false);
  let error = $state('');
  let applying = $state(false);

  // Only hunks that still apply can be selected; including a stale one would
  // abort the whole batch server-side.
  const applicable = $derived(applicableHunks(previews));
  const chosen = $derived(selectedHunkIds(previews, selected));
  const selectedCount = $derived(chosen.length);
  const allSelected = $derived(isAllSelected(previews, selected));
  const staleCount = $derived(previews.length - applicable.length);

  async function loadPreviews() {
    if (!filePath || edits.length === 0) {
      previews = [];
      selected = new Set();
      return;
    }
    loading = true;
    error = '';
    try {
      const res = await api.previewHunks(filePath, edits);
      etag = res.etag;
      previews = mergePreviews(edits, res.hunks);
      selected = defaultSelection(previews);
    } catch (e) {
      error = e.message || 'Could not preview the proposed changes';
      previews = [];
    } finally {
      loading = false;
    }
  }

  // Re-preview whenever a new proposal (or file) arrives.
  $effect(() => {
    void edits;
    void filePath;
    loadPreviews();
  });

  function toggle(hunkId) {
    selected = toggleHunk(selected, hunkId);
  }

  function toggleAll() {
    selected = toggleAllHunks(previews, selected);
  }

  async function applySelected() {
    if (selectedCount === 0 || applying) return;
    applying = true;
    error = '';
    try {
      await onapply?.({
        edits,
        selectedHunkIds: chosen,
        expectedEtag: etag
      });
    } catch (e) {
      error = e.message || 'Apply failed';
    } finally {
      applying = false;
    }
  }

  /** Split a unified diff into rows for display. */
  function diffLines(diff) {
    return diff
      .split('\n')
      .filter((line) => !line.startsWith('---') && !line.startsWith('+++'))
      .map((line) => ({
        text: line,
        kind: line.startsWith('+')
          ? 'add'
          : line.startsWith('-')
            ? 'del'
            : line.startsWith('@@')
              ? 'meta'
              : 'ctx'
      }));
  }
</script>

<div class="diff-view">
  <header class="toolbar">
    <label class="select-all">
      <input
        type="checkbox"
        checked={allSelected}
        onchange={toggleAll}
        disabled={applicable.length === 0}
      />
      {selectedCount} of {applicable.length} selected
    </label>

    {#if staleCount > 0}
      <span class="stale-note" role="status">
        {staleCount} hunk{staleCount === 1 ? '' : 's'} no longer applies
      </span>
    {/if}

    <div class="spacer"></div>
    <button class="reject" onclick={() => onreject?.()} disabled={applying}>Reject all</button>
    <button class="apply" onclick={applySelected} disabled={selectedCount === 0 || applying}>
      {applying ? 'Applying…' : `Apply ${selectedCount} hunk${selectedCount === 1 ? '' : 's'}`}
    </button>
  </header>

  {#if error}
    <p class="error" role="alert">{error}</p>
  {/if}

  {#if loading}
    <p class="status">Previewing changes…</p>
  {:else if previews.length === 0}
    <p class="status">No proposed changes.</p>
  {:else}
    <ul class="hunks">
      {#each previews as preview (preview.hunk_id)}
        <li class="hunk" class:stale={!preview.applies}>
          <label class="hunk-header">
            <input
              type="checkbox"
              checked={selected.has(preview.hunk_id)}
              onchange={() => toggle(preview.hunk_id)}
              disabled={!preview.applies}
            />
            <code class="hunk-id">{preview.hunk_id}</code>
            {#if preview.edit}
              <span class="op">{preview.edit.op}</span>
            {/if}
            {#if !preview.applies}
              <span class="stale-reason">{preview.error ?? 'no longer applies'}</span>
            {/if}
          </label>

          {#if preview.applies}
            <pre class="diff">{#each diffLines(preview.diff) as line}<span class={line.kind}>{line.text}
</span>{/each}</pre>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .diff-view {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    height: 100%;
    overflow: auto;
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    flex-wrap: wrap;
    position: sticky;
    top: 0;
    padding: 0.4rem 0;
    background: inherit;
  }
  .select-all {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    white-space: nowrap;
  }
  .spacer {
    flex: 1;
  }
  .stale-note {
    font-size: 0.85rem;
    opacity: 0.75;
  }
  .error {
    color: #c33;
    margin: 0;
  }
  .status {
    opacity: 0.75;
    margin: 0;
  }
  .hunks {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }
  .hunk {
    border: 1px solid rgba(128, 128, 128, 0.35);
    border-radius: 6px;
    overflow: hidden;
  }
  .hunk.stale {
    opacity: 0.6;
  }
  .hunk-header {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.4rem 0.6rem;
    background: rgba(128, 128, 128, 0.1);
    cursor: pointer;
  }
  .hunk-id {
    font-size: 0.8rem;
    opacity: 0.7;
  }
  .op {
    font-size: 0.8rem;
    padding: 0.05rem 0.4rem;
    border-radius: 999px;
    background: rgba(128, 128, 128, 0.2);
  }
  .stale-reason {
    font-size: 0.8rem;
    color: #c33;
  }
  .diff {
    margin: 0;
    padding: 0.4rem 0.6rem;
    overflow-x: auto;
    font-family: ui-monospace, monospace;
    font-size: 0.85rem;
    line-height: 1.4;
  }
  .diff .add {
    background: rgba(46, 160, 67, 0.15);
  }
  .diff .del {
    background: rgba(248, 81, 73, 0.15);
  }
  .diff .meta {
    opacity: 0.6;
  }
  button {
    padding: 0.3rem 0.7rem;
  }
</style>
