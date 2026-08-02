<script>
  import Icon from './Icon.svelte'
  import { compileStore } from '../stores/compileStore.js'
  import { fileStore } from '../stores/fileStore.js'
  
  export let status = ''

  $: statusColor = status.startsWith('Error') ? 'text-destructive' : 'text-foreground'
  
  // Auto-save indicator
  $: autoSaveStatus = $fileStore.saving ? 'Saving...' : 
                      $fileStore.lastSaved ? `Saved ${formatTime($fileStore.lastSaved)}` : ''
  
  // Pending compile indicator
  $: pendingCompile = $compileStore.pendingAutoCompile ? 'Compile pending' : ''
  
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
        <span class="checkmark"><Icon name="check" /></span>
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
    background: hsl(var(--surface-2));
    border-top: 1px solid hsl(var(--border));
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
    color: hsl(var(--text-dim));
    font-size: 12px;
  }
  
  .auto-save-indicator.saving {
    color: hsl(var(--primary));
  }
  
  .checkmark {
    color: hsl(var(--ok));
    font-weight: bold;
  }
  
  .spinner {
    display: inline-block;
    width: 12px;
    height: 12px;
    border: 2px solid hsl(var(--primary));
    border-top-color: transparent;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  
  @keyframes spin {
    to { transform: rotate(360deg); }
  }
  
  .pending-compile {
    color: hsl(var(--warn));
    font-size: 12px;
  }
</style>
