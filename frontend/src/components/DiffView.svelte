<script>
  import { createEventDispatcher } from 'svelte'

  export let diff = ''
  export let edits = null
  export let conflicts = []  // Array of conflict objects: {line, status, versions}
  
  let showConflictModal = false
  let selectedConflict = null

  const dispatch = createEventDispatcher()

  let worker
  let workerAvailable = false

  function initWorker() {
    try {
      worker = new Worker(new URL('../workers/diffWorker.js', import.meta.url), { type: 'module' })
      workerAvailable = true
    } catch (_) {
      workerAvailable = false
    }
  }

  function parseDiffSync(diffText) {
    const lines = diffText.split('\n')
    const hunks = []
    let current = null
    for (const line of lines) {
      if (line.startsWith('@@')) {
        if (current) hunks.push(current)
        current = { header: line, lines: [] }
        continue
      }
      if (!current) {
        current = { header: 'preamble', lines: [] }
      }
      let type = 'normal'
      if (line.startsWith('+')) type = 'add'
      else if (line.startsWith('-')) type = 'remove'
      else if (line.startsWith('@@')) type = 'meta'
      current.lines.push({ raw: line, type, text: line.replace(/^[\-+]/, '') })
    }
    if (current) hunks.push(current)
    return hunks
  }

  function parseDiff(diffText, agentEdits) {
    if (agentEdits) {
      return agentEdits.map((edit, idx) => {
        let lines = []
        if (edit.op === 'replace_range' && edit.text && edit.insert_text) {
          edit.text.split('\n').forEach(l => lines.push({ raw: '-' + l, type: 'remove', text: l }))
          edit.insert_text.split('\n').forEach(l => lines.push({ raw: '+' + l, type: 'add', text: l }))
        } else if (edit.op === 'insert' && edit.insert_text) {
          edit.insert_text.split('\n').forEach(l => lines.push({ raw: '+' + l, type: 'add', text: l }))
        } else if (edit.op === 'rewrite_file' && edit.payload) {
          lines.push({ raw: '+ [File Rewritten]', type: 'add', text: '[File Rewritten]' })
        }
        return {
          header: `@@ ${edit.op} lines ${edit.start_line || '?'}-${edit.end_line || '?'} @@`,
          lines,
          id: edit.hunk_id
        }
      })
    }
    // Use worker for large diffs
    if (workerAvailable && diffText && diffText.length > 20000) {
      return new Promise((resolve) => {
        const handle = (evt) => {
          const { hunks, error } = evt.data || {}
          worker.removeEventListener('message', handle)
          resolve(error ? parseDiffSync(diffText) : hunks)
        }
        worker.addEventListener('message', handle)
        worker.postMessage({ diffText })
      })
    }
    return parseDiffSync(diffText)
  }

  // Initialize worker once
  $: initWorker()
  $: hunksPromise = parseDiff(diff, edits)
  let hunks = []
  $: if (hunksPromise && typeof hunksPromise.then === 'function') {
    hunksPromise.then((h) => { hunks = h })
  } else {
    hunks = hunksPromise || []
  }
  let selected = []
  $: selected = hunks.map((_, i) => selected[i] ?? true)

  function toggle(idx) {
    selected = selected.map((v, i) => (i === idx ? !v : v))
  }

  function applySelected() {
    dispatch('apply-selected', { hunks, selected })
  }

  function rejectAll() {
    dispatch('reject-all')
  }
  
  function openConflictResolution(conflict) {
    selectedConflict = conflict
    showConflictModal = true
  }
  
  function resolveConflict(chosenVersion) {
    dispatch('resolve-conflict', {
      conflictId: selectedConflict.id,
      chosenVersion: chosenVersion
    })
    showConflictModal = false
    selectedConflict = null
  }
</script>

<div class="diff-view">
  <div class="diff-actions">
    <button class="apply-btn" on:click={applySelected} aria-label="Apply selected hunks">Apply selected hunks</button>
    <button class="reject-btn" on:click={rejectAll} aria-label="Discard all changes">Reject all</button>
  </div>
  
  {#if conflicts && conflicts.length > 0}
    <div class="conflicts-section">
      <div class="conflicts-header">
        <h3>[WARNING] Conflicts Detected</h3>
        <p>{conflicts.length} conflict{conflicts.length !== 1 ? 's' : ''} need resolution</p>
      </div>
      {#each conflicts as conflict}
        <div class="conflict-item">
          <div class="conflict-line">Line {conflict.line}</div>
          <div class="conflict-versions">
            {#each conflict.versions as version, idx}
              <button
                class="version-btn"
                on:click={() => openConflictResolution(conflict)}
                title={`Version ${idx + 1}`}
              >
                Version {idx + 1}
              </button>
            {/each}
          </div>
        </div>
      {/each}
    </div>
  {/if}
  
  {#each hunks as hunk, idx}
    <div class="hunk">
      <div class="hunk-header">
        <label>
          <input type="checkbox" checked={selected[idx]} on:change={() => toggle(idx)} />
          <span>{hunk.header}</span>
        </label>
      </div>
      {#each hunk.lines as line, lineIdx}
        <div class="diff-line {line.type}" data-hunk={idx}>
          <span class="prefix">{line.type === 'remove' ? '-' : line.type === 'add' ? '+' : ' '}</span>
          <span class="content">{line.text}</span>
        </div>
      {/each}
    </div>
  {/each}
</div>

{#if showConflictModal && selectedConflict}
  <div class="modal-overlay" on:click={() => (showConflictModal = false)}>
    <div class="modal-content" on:click={(e) => e.stopPropagation()}>
      <div class="modal-header">
        <h2>Resolve Conflict at Line {selectedConflict.line}</h2>
        <button class="close-btn" on:click={() => (showConflictModal = false)}>×</button>
      </div>
      
      <div class="versions-comparison">
        {#each selectedConflict.versions as version, idx}
          <div class="version-panel">
            <h3>Version {idx + 1}</h3>
            <div class="version-content">
              <pre>{version}</pre>
            </div>
            <button
              class="choose-btn"
              on:click={() => resolveConflict(idx)}
            >
              Choose This Version
            </button>
          </div>
        {/each}
      </div>
    </div>
  </div>
{/if}

<style>
  .diff-view {
    font-family: 'Courier New', monospace;
    font-size: 13px;
    line-height: 1.5;
    background: #fafafa;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .diff-actions {
    display: flex;
    gap: 0.5rem;
    padding: 0 0.5rem;
  }

  .diff-line {
    padding: 0 1rem;
    white-space: pre-wrap;
    word-break: break-all;
    border-bottom: 1px solid #e0e0e0;
  }

  .hunk {
    border: 1px solid #e0e0e0;
    border-radius: 4px;
    overflow: hidden;
    background: #fff;
  }

  .hunk-header {
    background: #f1f5f9;
    padding: 0.5rem 0.75rem;
    font-weight: 600;
    border-bottom: 1px solid #e0e0e0;
  }

  .hunk-header label {
    display: flex;
    gap: 0.5rem;
    align-items: center;
    cursor: pointer;
  }

  .apply-btn,
  .reject-btn {
    padding: 0.35rem 0.75rem;
    border: none;
    border-radius: 4px;
    cursor: pointer;
    font-size: 12px;
    transition: all 0.2s ease;
  }

  .apply-btn {
    background: #10b981;
    color: #fff;
  }

  .apply-btn:hover {
    background: #059669;
    transform: translateY(-1px);
  }

  .reject-btn {
    background: #f97316;
    color: #fff;
  }

  .reject-btn:hover {
    background: #ea580c;
    transform: translateY(-1px);
  }

  .diff-line.remove {
    background: #ffebee;
    color: #c62828;
  }

  .diff-line.add {
    background: #e8f5e9;
    color: #2e7d32;
  }

  .diff-line.normal {
    background: #fafafa;
  }

  .diff-line.meta {
    background: #e3f2fd;
    color: #1565c0;
    font-weight: bold;
  }

  .prefix {
    display: inline-block;
    width: 2rem;
    text-align: center;
    font-weight: bold;
  }

  .meta-prefix {
    color: #1976d2;
  }

  .content {
    white-space: pre;
  }

  /* Conflicts section */
  .conflicts-section {
    background: #fff3cd;
    border: 2px solid #ff9800;
    border-radius: 4px;
    padding: 0.75rem;
    margin: 0.5rem;
  }

  .conflicts-header {
    margin-bottom: 0.5rem;
  }

  .conflicts-header h3 {
    margin: 0 0 0.25rem 0;
    color: #f57f17;
  }

  .conflicts-header p {
    margin: 0;
    font-size: 12px;
    color: #e65100;
  }

  .conflict-item {
    background: #fff;
    border: 1px solid #ffb74d;
    border-radius: 3px;
    padding: 0.5rem;
    margin-bottom: 0.5rem;
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .conflict-line {
    font-weight: 600;
    color: #ff6f00;
    flex: 1;
  }

  .conflict-versions {
    display: flex;
    gap: 0.25rem;
  }

  .version-btn {
    padding: 0.25rem 0.5rem;
    background: #ff9800;
    color: #fff;
    border: none;
    border-radius: 3px;
    font-size: 11px;
    cursor: pointer;
    transition: all 0.2s ease;
  }

  .version-btn:hover {
    background: #f57c00;
  }

  /* Modal styles */
  .modal-overlay {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }

  .modal-content {
    background: #fff;
    border-radius: 8px;
    box-shadow: 0 10px 40px rgba(0, 0, 0, 0.2);
    max-width: 800px;
    width: 90%;
    max-height: 80vh;
    overflow-y: auto;
  }

  .modal-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 1.5rem;
    border-bottom: 1px solid #e0e0e0;
    background: #f5f5f5;
  }

  .modal-header h2 {
    margin: 0;
    color: #333;
    font-size: 1.3rem;
  }

  .close-btn {
    background: none;
    border: none;
    font-size: 28px;
    cursor: pointer;
    color: #666;
  }

  .close-btn:hover {
    color: #000;
  }

  .versions-comparison {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 1rem;
    padding: 1.5rem;
  }

  .version-panel {
    border: 1px solid #e0e0e0;
    border-radius: 4px;
    overflow: hidden;
  }

  .version-panel h3 {
    margin: 0;
    padding: 0.75rem;
    background: #f5f5f5;
    color: #333;
    font-size: 0.95rem;
    border-bottom: 1px solid #e0e0e0;
  }

  .version-content {
    padding: 1rem;
    background: #fafafa;
    max-height: 300px;
    overflow-y: auto;
  }

  .version-content pre {
    margin: 0;
    font-size: 12px;
    line-height: 1.4;
    color: #333;
  }

  .choose-btn {
    display: block;
    width: 100%;
    padding: 0.75rem;
    background: #4caf50;
    color: #fff;
    border: none;
    cursor: pointer;
    font-weight: 600;
    transition: all 0.2s ease;
  }

  .choose-btn:hover {
    background: #45a049;
  }

  .choose-btn:active {
    transform: scale(0.98);
  }
</style>
