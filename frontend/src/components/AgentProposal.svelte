<script>
  import { createEventDispatcher } from 'svelte'
  import { aiStore, setApplying } from '../stores/aiStore'
  
  const dispatch = createEventDispatcher()
  
  // A Proposal is a unified concept:
  // {
  //   id: unique ID
  //   summary: user-facing description
  //   source: 'chat' | 'error-fix'
  //   edits: [{file_path, diff, op, payload}]
  //   compileResult?: {success, errors?, warnings?}
  //   hunks: [{file, added, removed, context}]
  //   applyButtonFocused: boolean
  // }
  
  export let proposal = null
  export let applying = false
  export let error = null
  
  let selectedHunks = new Set()
  
  function toggleHunk(hunkId) {
    if (selectedHunks.has(hunkId)) {
      selectedHunks.delete(hunkId)
    } else {
      selectedHunks.add(hunkId)
    }
    selectedHunks = selectedHunks  // trigger reactivity
  }
  
  async function applyProposal(mode = 'all') {
    // mode: 'all' (apply all), 'selected' (apply selected hunks), 'none' (just accept)
    setApplying()
    applying = true
    
    try {
      const editsToApply = mode === 'all' 
        ? proposal.edits 
        : mode === 'selected'
          ? proposal.edits.filter((e, i) => selectedHunks.has(i))
          : []
      
      if (editsToApply.length === 0) {
        dispatch('proposal-accepted', {proposal_id: proposal.id})
        return
      }
      
      // POST to /api/agent with edits
      const res = await fetch('/api/agent', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          edits: editsToApply,
          auto_compile: true,
          compile_timeout: 60
        })
      })
      
      if (!res.ok) {
        error = `Apply failed: ${res.statusText}`
        applying = false
        return
      }
      
      const result = await res.json()
      
      if (result.success) {
        dispatch('proposal-applied', {
          proposal_id: proposal.id,
          results: result.results,
          compile_result: result.compile_result
        })
      } else {
        error = 'Apply failed'
      }
    } catch (e) {
      error = e.message
    } finally {
      applying = false
    }
  }
  
  function fixErrors() {
    // Trigger agent-loop to propose fixes
    dispatch('fix-requested', {proposal_id: proposal.id, errors: proposal.compileResult?.errors})
  }
</script>

<div class="proposal" class:applying>
  {#if proposal}
    <div class="header">
      <h3>{proposal.summary}</h3>
      <span class="source-badge" class:error-fix={proposal.source === 'error-fix'}>
        {proposal.source === 'error-fix' ? '[TOOLS] Fix' : '[SPARKLE] Proposal'}
      </span>
    </div>
    
    {#each proposal.edits as edit, editIdx}
      <div class="edit-section">
        <div class="edit-file">
          <span class="icon">📄</span>
          <span class="path">{edit.file_path}</span>
        </div>
        
        <div class="diff-viewer">
          {#each edit.hunks || [] as hunk, hunkIdx}
            <div class="hunk" class:selected={selectedHunks.has(`${editIdx}-${hunkIdx}`)}>
              <label class="hunk-select">
                <input 
                  type="checkbox" 
                  checked={selectedHunks.has(`${editIdx}-${hunkIdx}`)}
                  on:change={() => toggleHunk(`${editIdx}-${hunkIdx}`)}
                  disabled={applying}
                />
                {hunk.context || `@@ hunk ${hunkIdx + 1} @@`}
              </label>
              
              <div class="hunk-content">
                {#each hunk.removed || [] as line}
                  <div class="line removed">
                    <span class="prefix">−</span>
                    <code>{line}</code>
                  </div>
                {/each}
                {#each hunk.added || [] as line}
                  <div class="line added">
                    <span class="prefix">+</span>
                    <code>{line}</code>
                  </div>
                {/each}
              </div>
            </div>
          {/each}
          
          {#if !edit.hunks && edit.diff}
            <pre class="diff">{edit.diff}</pre>
          {/if}
        </div>
      </div>
    {/each}
    
    {#if proposal.compileResult}
      <div class="compile-result" class:error={!proposal.compileResult.success}>
        {#if proposal.compileResult.success}
          <div class="success">[SUCCESS] Compilation successful ({proposal.compileResult.page_count} pages)</div>
        {:else}
          <div class="error-message">
            <span>[ERROR] Compilation failed ({proposal.compileResult.error_count} errors)</span>
            {#if proposal.source !== 'error-fix'}
              <button on:click={fixErrors} disabled={applying}>
                [TOOLS] Fix It
              </button>
            {/if}
          </div>
          {#if proposal.compileResult.errors && proposal.compileResult.errors.length > 0}
            <div class="errors">
              {#each proposal.compileResult.errors.slice(0, 3) as err}
                <div class="error-item">
                  <code>{err.line || '?'}: {err.message || 'unknown'}</code>
                </div>
              {/each}
              {#if proposal.compileResult.errors.length > 3}
                <div class="error-item">
                  ... and {proposal.compileResult.errors.length - 3} more
                </div>
              {/if}
            </div>
          {/if}
        {/if}
      </div>
    {/if}
    
    {#if error}
      <div class="error-banner">{error}</div>
    {/if}
    
    <div class="actions">
      {#if proposal.edits.length > 1}
        <button 
          on:click={() => applyProposal('selected')} 
          disabled={applying || selectedHunks.size === 0}
          class="secondary"
        >
          Apply Selected
        </button>
      {/if}
      <button 
        on:click={() => applyProposal('all')} 
        disabled={applying}
      >
        {applying ? 'Applying...' : 'Apply All'}
      </button>
      <button on:click={() => dispatch('proposal-dismissed')} disabled={applying} class="secondary">
        Cancel
      </button>
    </div>
  {:else}
    <div class="empty">No proposal yet</div>
  {/if}
</div>

<style>
  .proposal {
    border: 1px solid #2d3748;
    border-radius: 8px;
    padding: 16px;
    background: #1a202c;
    color: #e5e7eb;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  
  .proposal.applying {
    opacity: 0.7;
    pointer-events: none;
  }
  
  .header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 8px;
  }
  
  .header h3 {
    margin: 0;
    font-size: 16px;
    font-weight: 500;
  }
  
  .source-badge {
    font-size: 12px;
    padding: 4px 8px;
    border-radius: 4px;
    background: #2d3748;
    color: #a0aec0;
  }
  
  .source-badge.error-fix {
    background: #7c2d12;
    color: #fed7aa;
  }
  
  .edit-section {
    border-top: 1px solid #2d3748;
    padding-top: 12px;
  }
  
  .edit-file {
    display: flex;
    align-items: center;
    gap: 8px;
    font-weight: 500;
    margin-bottom: 8px;
  }
  
  .icon {
    font-size: 14px;
  }
  
  .path {
    font-family: monospace;
    font-size: 12px;
    color: #cbd5e0;
  }
  
  .diff-viewer {
    background: #0f172a;
    border: 1px solid #1f2937;
    border-radius: 6px;
    overflow: auto;
  }
  
  .hunk {
    border-bottom: 1px solid #1f2937;
  }
  
  .hunk:last-child {
    border-bottom: none;
  }
  
  .hunk.selected {
    background: #1f2937;
  }
  
  .hunk-select {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    cursor: pointer;
    background: #111827;
    font-size: 12px;
    color: #9ca3af;
  }
  
  .hunk-select input {
    cursor: pointer;
  }
  
  .hunk-select:hover {
    background: #1f2937;
  }
  
  .hunk-content {
    padding: 8px 0;
    font-family: monospace;
    font-size: 12px;
  }
  
  .line {
    padding: 2px 12px;
    display: flex;
    align-items: center;
  }
  
  .line.added {
    background: rgba(34, 197, 94, 0.1);
    color: #86efac;
  }
  
  .line.removed {
    background: rgba(239, 68, 68, 0.1);
    color: #fca5a5;
  }
  
  .line .prefix {
    width: 20px;
    text-align: center;
    color: #6b7280;
  }
  
  .line code {
    flex: 1;
    white-space: pre-wrap;
    word-break: break-all;
  }
  
  .diff {
    padding: 12px;
    white-space: pre-wrap;
    word-break: break-all;
    color: #9ca3af;
    margin: 0;
  }
  
  .compile-result {
    border-top: 1px solid #2d3748;
    padding-top: 12px;
  }
  
  .compile-result.error {
    background: rgba(127, 29, 29, 0.1);
    border-left: 3px solid #dc2626;
  }
  
  .success {
    color: #86efac;
    font-weight: 500;
  }
  
  .error-message {
    display: flex;
    justify-content: space-between;
    align-items: center;
    color: #fca5a5;
    font-weight: 500;
  }
  
  .error-message button {
    padding: 4px 8px;
    font-size: 12px;
    background: #7c2d12;
    border: none;
    border-radius: 4px;
    color: #fed7aa;
    cursor: pointer;
  }
  
  .error-message button:hover {
    background: #9a3412;
  }
  
  .error-message button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  
  .errors {
    margin-top: 8px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  
  .error-item {
    padding: 6px 12px;
    background: #1f2937;
    border-radius: 4px;
    font-family: monospace;
    font-size: 11px;
    color: #f87171;
  }
  
  .error-banner {
    background: #7c2d12;
    border: 1px solid #dc2626;
    color: #fed7aa;
    padding: 8px 12px;
    border-radius: 4px;
    font-size: 12px;
  }
  
  .actions {
    display: flex;
    gap: 8px;
    margin-top: 4px;
  }
  
  button {
    flex: 1;
    padding: 8px 12px;
    border: none;
    border-radius: 6px;
    font-weight: 500;
    cursor: pointer;
    font-size: 13px;
    background: #2563eb;
    color: white;
    transition: background 0.2s;
  }
  
  button:hover:not(:disabled) {
    background: #1d4ed8;
  }
  
  button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  
  button.secondary {
    background: #4b5563;
    color: #e5e7eb;
  }
  
  button.secondary:hover:not(:disabled) {
    background: #5a6370;
  }
  
  .empty {
    text-align: center;
    color: #6b7280;
    padding: 20px;
  }
</style>
