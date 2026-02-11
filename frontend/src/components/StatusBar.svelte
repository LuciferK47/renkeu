<script>
  import { compileStore } from '../stores/compileStore.js'
  import { fileStore } from '../stores/fileStore.js'
  
  export let status = ''

  $: statusColor = status.startsWith('Error') ? 'text-red-600' : 'text-green-600'
  
  // Auto-save indicator
  $: autoSaveStatus = $fileStore.saving ? 'Saving...' : 
                      $fileStore.lastSaved ? `Saved ${formatTime($fileStore.lastSaved)}` : ''
  
  // Pending compile indicator
  $: pendingCompile = $compileStore.pendingAutoCompile ? '⏱ Compile pending' : ''
  
  function formatTime(date) {
    if (!date) return ''
    const now = new Date()
    const diff = Math.floor((now - date) / 1000) // seconds
    
    if (diff < 5) return 'just now'
    if (diff < 60) return `${diff}s ago`
    if (diff < 3600) return `${Math.floor(diff / 60)}m ago`
    return date.toLocaleTimeString()
  }
</script>

<div class="status-bar" role="status" aria-live="polite" aria-atomic="true">
  <span class={statusColor}>{status}</span>
  
  {#if autoSaveStatus}
    <span class="auto-save-indicator" class:saving={$fileStore.saving}>
      {#if $fileStore.saving}
        <span class="spinner"></span>
      {:else}
        <span class="checkmark">[OK]</span>
      {/if}
      {autoSaveStatus}
    </span>
  {/if}
  
  {#if pendingCompile}
    <span class="pending-compile">
      {pendingCompile}
    </span>
  {/if}
</div>

<style>
  .status-bar {
    padding: 0.5rem 1rem;
    background: #f0f0f0;
    border-top: 1px solid #ddd;
    font-size: 13px;
    min-height: 24px;
    display: flex;
    align-items: center;
    gap: 1rem;
  }
  
  .auto-save-indicator {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    color: #666;
    font-size: 12px;
  }
  
  .auto-save-indicator.saving {
    color: #007bff;
  }
  
  .checkmark {
    color: #28a745;
    font-weight: bold;
  }
  
  .spinner {
    display: inline-block;
    width: 12px;
    height: 12px;
    border: 2px solid #007bff;
    border-top-color: transparent;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  
  @keyframes spin {
    to { transform: rotate(360deg); }
  }
  
  .pending-compile {
    color: #ff9800;
    font-size: 12px;
  }
</style>
