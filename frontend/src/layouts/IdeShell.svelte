<!--
  IdeShell - Top-level IDE container managing global state and layout
  Responsible for: sidebar visibility, theme, keyboard shortcuts, dialogs
-->
<script>
  import KeyboardShortcuts from '../components/KeyboardShortcuts.svelte'
  import ThemeToggle from '../components/ThemeToggle.svelte'
  import SettingsPanel from '../components/SettingsPanel.svelte'
  import Toasts from '../components/Toasts.svelte'
  import CommandPalette from '../components/CommandPalette.svelte'
  import WorkspaceLayout from './WorkspaceLayout.svelte'

  export let filePath = ''
  export let shortcuts = {}
  export let themeSettings = {}
  export let showSettings = false
  export let toasts = []
  export let dryRun = false
  export let bulkMode = false
  export let selectedFiles = []
  export let commandPaletteRef = null
  export let layoutMode = 'overleaf'

  // Events
  export let onFileChange = () => {}
  export let onShortcut = () => {}
  export let onThemeChange = () => {}
  export let onFileTreeToggle = () => {}
  export let onHistoryToggle = () => {}
  export let onCompile = () => {}
  export let onBulkCommand = () => {}
  export let onDryRunToggle = () => {}
  export let onAIPlan = () => {}
  export let onToastRemove = () => {}
  export let onConflictResolve = () => {}
  export let onSettingsChange = () => {}

  // Props from parent
  export let showHistory = false
  export let showFileTree = true
  export let isDirty = false
  export let compiling = false
  export let conflictDialogState = { show: false, data: null }

  let showSidebar = true
</script>

<div class="ide-shell" role="application" aria-label="LaTeX AI Editor">
  <!-- Keyboard shortcuts handler (invisible) -->
  <KeyboardShortcuts {shortcuts} on:shortcut={onShortcut} />

  <!-- Sidebar Navigation -->
  <div class="sidebar" role="navigation" aria-label="Document navigation" class:hidden={!showSidebar}>
    <div class="sidebar-header">
      <button
        on:click={() => (showSidebar = false)}
        class="sidebar-close"
        aria-label="Close sidebar"
      >
        ✕
      </button>
    </div>
    <slot name="sidebar" />
  </div>

  <!-- Main Content Area -->
  <div class="main">
    <!-- Top Bar with Controls -->
    <div class="top-bar" role="toolbar" aria-label="Document controls">
      {#if !showSidebar}
        <button
          on:click={() => (showSidebar = true)}
          class="sidebar-toggle"
          aria-label="Open sidebar"
        >
          ☰
        </button>
      {/if}

      <slot name="topbar" />

      <div class="ml-auto">
        <ThemeToggle onThemeChange={onThemeChange} />
      </div>
    </div>

    <!-- Main Workspace -->
    <WorkspaceLayout
      {filePath}
      {isDirty}
      {compiling}
      {layoutMode}
      {selectedFiles}
      {bulkMode}
      on:file-change={onFileChange}
      on:file-tree-toggle={onFileTreeToggle}
      on:history-toggle={onHistoryToggle}
      on:compile={onCompile}
    >
      <slot name="workspace" />
    </WorkspaceLayout>

    <!-- Command Palette -->
    <CommandPalette
      bind:this={commandPaletteRef}
      {dryRun}
      bulkMode={selectedFiles.length > 0}
      selectedCount={selectedFiles.length}
      on:command={onBulkCommand}
      on:ai-plan={onAIPlan}
      on:toggle-dry-run={onDryRunToggle}
    />

    <!-- Modals & Overlays -->
    <slot name="modals" />

    <!-- Toast Notifications -->
    <Toasts {toasts} on:remove={onToastRemove} />
  </div>

  <!-- Settings Panel (overlay) -->
  {#if showSettings}
    <SettingsPanel
      {themeSettings}
      on:setting-changed={onSettingsChange}
      on:close={() => (showSettings = false)}
    />
  {/if}
</div>

<style>
  .ide-shell {
    display: flex;
    height: 100vh;
    width: 100%;
    background: hsl(var(--background));
    color: hsl(var(--foreground));
  }

  .sidebar {
    width: 250px;
    border-right: 1px solid hsl(var(--border));
    display: flex;
    flex-direction: column;
    overflow: hidden;
    transition: transform 0.3s ease;
  }

  .sidebar.hidden {
    display: none;
  }

  .sidebar-header {
    padding: 0.5rem;
    border-bottom: 1px solid hsl(var(--border));
    display: flex;
    align-items: center;
    justify-content: flex-end;
  }

  .sidebar-close {
    background: none;
    border: none;
    cursor: pointer;
    font-size: 1.2rem;
    padding: 0.25rem 0.5rem;
  }

  .sidebar-toggle {
    background: none;
    border: none;
    cursor: pointer;
    padding: 0.5rem 1rem;
    font-size: 1.2rem;
  }

  .main {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .top-bar {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.5rem 1rem;
    border-bottom: 1px solid hsl(var(--border));
    background: hsl(var(--secondary));
    flex-wrap: wrap;
  }

  .ml-auto {
    margin-left: auto;
  }
</style>
