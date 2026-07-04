<!--
  PanelManager - Manages sidebar content (FileTree + HistorySidebar)
  Responsible for: showing/hiding file tree, history, managing selection
-->
<script>
  import FileTree from '../components/FileTree.svelte'
  export let filePath = ''
  export let showHistory = false
  export let showFileTree = true
  export let onFileTreeSelection = () => {}
  export let onFileClick = () => {}
  export let onFileTreeToggle = () => {}
  export let onHistoryToggle = () => {}
  export let onRestoreHistory = () => {}
  
  let HistorySidebar;
  let historyLoaded = false;

  $: if (showHistory && !historyLoaded) {
    import('../components/HistorySidebar.svelte').then(module => {
      HistorySidebar = module.default;
      historyLoaded = true;
    });
  }
</script>

<div class="panel-manager">
  {#if showHistory && !showFileTree}
    <div class="panel">
      {#if HistorySidebar}
        <svelte:component 
          this={HistorySidebar}
          {filePath}
          agentOnly={true}
          on:restore={(e) => onRestoreHistory(e.detail)}
        />
      {:else}
        <div class="loading">Loading history...</div>
      {/if}
    </div>
  {/if}

  {#if showFileTree}
    <div class="panel">
      <FileTree
        on:selection-changed={onFileTreeSelection}
        on:file-selected={onFileClick}
      />
    </div>
  {/if}

  {#if !showFileTree && !showHistory}
    <div class="panel-placeholder">
      <p>No panel selected</p>
      <button on:click={onFileTreeToggle} class="panel-btn">Show Files</button>
      <button on:click={onHistoryToggle} class="panel-btn">Show History</button>
    </div>
  {/if}
</div>

<style>
  .panel-manager {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    min-height: 0;
  }

  .panel {
    flex: 1;
    overflow: auto;
    min-height: 0;
  }

  .panel-placeholder {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 1rem;
    color: var(--text-secondary);
  }

  .panel-btn {
    padding: 0.5rem 1rem;
    background: hsl(var(--primary));
    color: hsl(var(--primary-foreground));
    border: none;
    border-radius: 4px;
    cursor: pointer;
  }

  .panel-btn:hover {
    opacity: 0.9;
  }
</style>
