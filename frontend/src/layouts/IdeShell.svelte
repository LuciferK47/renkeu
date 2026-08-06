<script>
  import { createEventDispatcher } from 'svelte'
  import Icon from '../components/Icon.svelte'
  import KeyboardShortcuts from '../components/KeyboardShortcuts.svelte'
  import ThemeToggle from '../components/ThemeToggle.svelte'
  import CommandPalette from '../components/CommandPalette.svelte'
  import Toasts from '../components/Toasts.svelte'
  import FileOpener from '../components/FileOpener.svelte'
  import SearchResults from '../components/SearchResults.svelte'
  import StatusBar from '../components/StatusBar.svelte'
  
  // `on:compile` and friends on a plain <button> forward a DOM event of that
  // name -- and a button only ever fires `click`, so those handlers could never
  // run. Compile and Settings did nothing at all when clicked. Dispatch the
  // named events explicitly instead.
  const dispatch = createEventDispatcher()

  import { uiStore, toggleFileTree, toggleHistory, toggleSettings } from '../stores/uiStore.js'
  import { fileStore } from '../stores/fileStore.js'
  import { compileStore } from '../stores/compileStore.js'
  
  export let shortcuts = {}
  export let selectedFiles = []
  export let dryRun = false
  export let bulkMode = false
  export let conflictData = null
  export let recentFiles = []
  export let pinnedFiles = []
  export let searchQuery = ''
  export let searchResults = []
  export let searchLoading = false
  export let commandPaletteRef = null
  export let SettingsPanel = null
  export let settingsLoaded = false
</script>

<div class="app" role="application" aria-label="LaTeX AI Editor">
  <KeyboardShortcuts {shortcuts} on:shortcut />

  <div class="sidebar" role="navigation" aria-label="Document navigation">
    <slot name="sidebar" />
  </div>

  <div class="main" role="main">
    <div class="top-bar" role="toolbar" aria-label="Document controls">
      <label for="file-path-input" class="sr-only">File path</label>
      <input id="file-path-input" type="text" placeholder="File path..." value={$fileStore.filePath} on:change class="flex-1 px-3 py-2 border rounded" aria-label="Current file path" />
      {#if $fileStore.dirty}<span aria-live="polite" class="text-destructive" title="Unsaved changes">•</span>{/if}
      <button on:click={toggleFileTree} class="ml-2 px-4 py-2 bg-primary text-primary-foreground rounded hover:opacity-90 transition-opacity" aria-label={$uiStore.showFileTree ? 'Hide file tree' : 'Show file tree'} aria-pressed={$uiStore.showFileTree}>{$uiStore.showFileTree ? 'Hide Tree' : 'Files'}</button>
      <button on:click={toggleHistory} class="ml-2 px-4 py-2 bg-primary text-primary-foreground rounded hover:opacity-90 transition-opacity" aria-label={$uiStore.showHistory ? 'Hide history' : 'Show file history'} aria-pressed={$uiStore.showHistory}>{$uiStore.showHistory ? 'Hide' : 'History'}</button>
      <button on:click|preventDefault={() => dispatch('compile')} disabled={!$fileStore.filePath || $compileStore.status === 'compiling'} class="ml-2 px-4 py-2 bg-primary text-primary-foreground rounded hover:opacity-90 transition-opacity disabled:opacity-40 disabled:cursor-not-allowed" title="Compile to PDF (Ctrl+Enter)" aria-busy={$compileStore.status === 'compiling'}>{$compileStore.status === 'compiling' ? 'Compiling…' : 'Compile'}</button>
      <button on:click|preventDefault={() => dispatch('settings-toggle')} class="ml-2 px-4 py-2 bg-secondary text-secondary-foreground border border-border rounded hover:bg-accent transition-colors" aria-label={$uiStore.showSettings ? 'Close settings' : 'Open settings'} aria-pressed={$uiStore.showSettings}>Settings</button>
      {#if selectedFiles.length > 0}<span class="ml-2 px-3 py-2 bg-secondary text-foreground border border-border rounded" role="status" aria-live="polite" aria-atomic="true">{selectedFiles.length} {selectedFiles.length === 1 ? 'file' : 'files'} selected</span>{/if}
      <div class="ml-auto"><ThemeToggle on:theme-change /></div>
    </div>

    <slot name="workspace" />

    <CommandPalette bind:this={commandPaletteRef} {dryRun} bulkMode={selectedFiles.length > 0} selectedCount={selectedFiles.length} on:command on:ai-plan on:toggle-dry-run />
    <StatusBar status={$uiStore.status} />
    <Toasts toasts={$uiStore.toasts} />
    
    {#if $uiStore.showConflictDialog && conflictData}
      <div class="conflict-dialog-overlay" role="dialog" aria-modal="true" aria-labelledby="conflict-title">
        <div class="conflict-dialog">
          <h2 id="conflict-title">File Conflict Detected</h2>
          <p>The file has been modified by another process. How would you like to proceed?</p>
          <div class="conflict-actions">
            <button class="conflict-btn conflict-btn-primary" on:click|preventDefault={() => dispatch('conflict-resolve', { action: 'overwrite' })}>Overwrite Server Version</button>
            <button class="conflict-btn conflict-btn-secondary" on:click|preventDefault={() => dispatch('conflict-resolve', { action: 'reload' })}>Reload from Server</button>
            <button class="conflict-btn conflict-btn-cancel" on:click={() => uiStore.update(ui => ({ ...ui, showConflictDialog: false }))}>Cancel</button>
          </div>
        </div>
      </div>
    {/if}
  </div>

  {#if $uiStore.showSettings && settingsLoaded && SettingsPanel}
    <svelte:component this={SettingsPanel} isOpen={$uiStore.showSettings} onClose={toggleSettings} onSave={(u) => {}} on:settings-save />
  {/if}

  <FileOpener visible={$uiStore.showFileOpener} recent={recentFiles} pinned={pinnedFiles} on:close={() => uiStore.update(ui => ({ ...ui, showFileOpener: false }))} on:open on:pin on:unpin />
  <SearchResults visible={$uiStore.showSearch} {searchQuery} results={searchResults} loading={searchLoading} on:open on:close={() => uiStore.update(ui => ({ ...ui, showSearch: false }))} />
</div>

<style>
  :global(body) { margin: 0; padding: 0; }
  .app { display: flex; height: 100vh; background: hsl(var(--surface-0)); }
  .sidebar { width: 280px; background: hsl(var(--surface-1)); border-right: 1px solid hsl(var(--border)); overflow-y: auto; }
  .main { flex: 1; display: flex; flex-direction: column; }
  .top-bar { padding: 1rem; background: hsl(var(--surface-1)); border-bottom: 1px solid hsl(var(--border)); display: flex; gap: 0.5rem; align-items: center; }
</style>
