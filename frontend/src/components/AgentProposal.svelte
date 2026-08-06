<script>
  import Icon from './Icon.svelte'
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
        <Icon name={proposal.source === 'error-fix' ? 'wrench' : 'sparkle'} />
        {proposal.source === 'error-fix' ? 'Fix' : 'Proposal'}
      </span>
    </div>
    
    {#each proposal.edits as edit, editIdx}
      <div class="edit-section">
        <div class="edit-file">
          <span class="icon"><Icon name="document" /></span>
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
          <div class="success"><Icon name="check-circle" /> Compiled cleanly ({proposal.compileResult.page_count} pages)</div>
        {:else}
          <div class="error-message">
            <span><Icon name="alert" /> Compile failed ({proposal.compileResult.error_count} errors)</span>
            {#if proposal.source !== 'error-fix'}
              <button on:click={fixErrors} disabled={applying}>
                <Icon name="wrench" /> Fix it
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
    border: 1px solid hsl(var(--border));
    border-radius: var(--radius-s);
    padding: 16px;
    background: hsl(var(--surface-1));
    color: hsl(var(--foreground));
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
    border-radius: var(--radius-xs);
    background: hsl(var(--surface-2));
    color: hsl(var(--text-dim));
  }
  
  .source-badge.error-fix {
    background: hsl(var(--warn) / 0.16);
    color: hsl(var(--warn));
  }
  
  .edit-section {
    border-top: 1px solid hsl(var(--border));
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
    font-family: 'JetBrains Mono', ui-monospace, SFMono-Regular, monospace;
    font-size: 12px;
    color: hsl(var(--foreground));
  }
  
  .diff-viewer {
    background: hsl(var(--surface-1));
    border: 1px solid hsl(var(--border));
    border-radius: var(--radius-s);
    overflow: auto;
  }
  
  .hunk {
    border-bottom: 1px solid hsl(var(--border));
  }
  
  .hunk:last-child {
    border-bottom: none;
  }
  
  .hunk.selected {
    background: hsl(var(--surface-2));
  }
  
  .hunk-select {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    cursor: pointer;
    background: hsl(var(--surface-1));
    font-size: 12px;
    color: hsl(var(--text-faint));
  }
  
  .hunk-select input {
    cursor: pointer;
  }
  
  .hunk-select:hover {
    background: hsl(var(--surface-2));
  }
  
  .hunk-content {
    padding: 8px 0;
    font-family: 'JetBrains Mono', ui-monospace, SFMono-Regular, monospace;
    font-size: 12px;
  }
  
  .line {
    padding: 2px 12px;
    display: flex;
    align-items: center;
  }
  
  .line.added {
    background: hsl(var(--ok) / 0.1);
    color: hsl(var(--ok));
  }
  
  .line.removed {
    background: hsl(var(--err) / 0.1);
    color: hsl(var(--err));
  }
  
  .line .prefix {
    width: 20px;
    text-align: center;
    color: hsl(var(--text-dim));
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
    color: hsl(var(--text-faint));
    margin: 0;
  }
  
  .compile-result {
    border-top: 1px solid hsl(var(--border));
    padding-top: 12px;
  }
  
  .compile-result.error {
    background: hsl(var(--err) / 0.1);
    border: 1px solid hsl(var(--err) / 0.4);
  }
  
  .success {
    color: hsl(var(--ok));
    font-weight: 500;
  }
  
  .error-message {
    display: flex;
    justify-content: space-between;
    align-items: center;
    color: hsl(var(--err));
    font-weight: 500;
  }
  
  .error-message button {
    padding: 4px 8px;
    font-size: 12px;
    background: hsl(var(--warn) / 0.16);
    border: 1px solid hsl(var(--warn) / 0.4);
    border-radius: var(--radius-xs);
    color: hsl(var(--warn));
    cursor: pointer;
  }
  
  .error-message button:hover {
    background: hsl(var(--warn) / 0.28);
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
    background: hsl(var(--surface-2));
    border-radius: var(--radius-xs);
    font-family: 'JetBrains Mono', ui-monospace, SFMono-Regular, monospace;
    font-size: 11px;
    color: hsl(var(--err));
  }
  
  .error-banner {
    background: hsl(var(--err) / 0.14);
    border: 1px solid hsl(var(--err) / 0.4);
    color: hsl(var(--err));
    padding: 8px 12px;
    border-radius: var(--radius-xs);
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
    border-radius: var(--radius-s);
    font-weight: 500;
    cursor: pointer;
    font-size: 13px;
    background: hsl(var(--primary));
    color: hsl(var(--primary-foreground));
    transition: background 0.2s;
  }
  
  /* Was `button: hover:not(:disabled)`. The space after the colon made it an
     invalid declaration rather than a selector, so this hover state never
     applied to anything. */
  button:hover:not(:disabled) {
    background: hsl(var(--primary));
  }
  
  button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  
  button.secondary {
    background: hsl(var(--accent));
    color: hsl(var(--foreground));
  }
  
  button.secondary:hover:not(:disabled) {
    background: hsl(var(--accent));
  }
  
  .empty {
    text-align: center;
    color: hsl(var(--text-dim));
    padding: 20px;
  }
</style>
