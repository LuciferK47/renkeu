import os
import re
import sys

FRONTEND_DIR = "/home/kushagrasingh/Downloads/Latex-AI-main/frontend/src"
APP_SVELTE = os.path.join(FRONTEND_DIR, "App.svelte")
WORKSPACE_LAYOUT = os.path.join(FRONTEND_DIR, "layouts/WorkspaceLayout.svelte")
IDE_SHELL = os.path.join(FRONTEND_DIR, "layouts/IdeShell.svelte")

def update_workspace_layout():
    # To properly implement WorkspaceLayout, we make it accept props for all state and forward all events
    # We will just write a new clean version
    content = """<script>
  import { Splitpanes, Pane } from 'svelte-splitpanes'
  import RightPanel from '../components/RightPanel.svelte'
  import Editor from '../components/Editor.svelte'
  import ChatPanel from '../components/ChatPanel.svelte'
  import StatusBar from '../components/StatusBar.svelte'
  import { createEventDispatcher } from 'svelte';
  const dispatch = createEventDispatcher();

  export let filePath = ''
  export let editorContent = ''
  export let editorRef = null
  export let autoSave = true
  export let autoSaveDelay = 30000
  export let editorSettings = {}
  export let themeSettings = {}
  export let layoutMode = 'overleaf' // 'overleaf' | 'fullscreen-editor' | 'fullscreen-pdf'
  export let isDirty = false
  export let compiling = false
  export let bulkMode = false
  export let bulkTotal = 0
  export let bulkProgress = 0
  export let editorHeightPercent = 60
  export let chatHeightPercent = 300
  export let isDraggingDivider = false
  export let contentWrapperRef = null
  export let showChatPanel = true
  export let pdfUrl = ''
  export let pdfPages = 0
  export let compileErrors = []
  export let compileWarnings = []
  export let diff = ''
  export let aiPlan = null
  export let compilationJobId = null
  export let activeRightTab = 'diff'
  export let chatMessages = []
  export let chatLoading = false
  export let chatStreamingMessage = ''

  function handleDividerMouseDown(e) { dispatch('divider-mousedown', e); }
  function handleChatDividerMouseDown(e) { dispatch('chat-divider-mousedown', e); }
</script>

<div class="content-wrapper" class:overleaf-layout={layoutMode === 'overleaf'} class:fullscreen-editor={layoutMode === 'fullscreen-editor'} class:fullscreen-pdf={layoutMode === 'fullscreen-pdf'} bind:this={contentWrapperRef}>
  <div class="editor-section" style={layoutMode === 'overleaf' ? `height: ${editorHeightPercent}%` : ''}>
    <div class="editor-header">
      <span class="section-label">[NOTE] Editor</span>
      <div class="editor-controls">
        <button on:click={() => layoutMode = layoutMode === 'fullscreen-editor' ? 'overleaf' : 'fullscreen-editor'} title="Toggle fullscreen" class="control-btn">
          {layoutMode === 'fullscreen-editor' ? '↙️' : '↗️'}
        </button>
      </div>
    </div>
    <div class="editor-pane">
      <Editor
        bind:this={editorRef}
        bind:content={editorContent}
        {filePath}
        {autoSave}
        {autoSaveDelay}
        {editorSettings}
        theme={themeSettings?.mode || 'light'}
        on:saved
        on:save-error
        on:dirty-change
        on:content-changed
        on:save-conflict
      />
    </div>
  </div>

  {#if layoutMode === 'overleaf'}
    <button type="button" class="divider" class:dragging={isDraggingDivider} aria-label="Resize editor and preview panels" on:mousedown={handleDividerMouseDown} on:keydown={(e) => (e.key === 'Enter' || e.key === ' ') && handleDividerMouseDown(e)}></button>
    
    <div class="pdf-section" style={`height: ${100 - editorHeightPercent}%`}>
      <div class="pdf-header">
        <span class="section-label">📄 Preview</span>
        <div class="pdf-controls">
          <button on:click={() => layoutMode = 'fullscreen-pdf'} title="Toggle fullscreen" class="control-btn">↗️</button>
        </div>
      </div>
      <div class="pdf-pane">
        {#if bulkMode}
          <div class="bulk-progress">
            <div class="progress-header">Applying command to {bulkTotal} files...</div>
            <div class="progress-bar"><div class="progress-fill" style="width: {(bulkProgress / bulkTotal) * 100}%"></div></div>
            <div class="progress-text">{bulkProgress} / {bulkTotal}</div>
          </div>
        {:else}
          <RightPanel
            {diff} {pdfUrl} {pdfPages} {compileErrors} {compileWarnings} {compiling} {aiPlan} jobId={compilationJobId} bind:activeTab={activeRightTab}
            on:apply-step on:apply-all on:compiled on:compileFailed on:apply-selected on:apply-fix on:reject-all on:jump-to-line on:explain-errors on:get-suggestions on:code-review on:insert-template
          />
        {/if}
      </div>
    </div>
  {/if}
</div>

{#if showChatPanel && layoutMode === 'overleaf'}
  <button type="button" class="chat-divider" class:dragging={isDraggingDivider} aria-label="Resize chat panel" on:mousedown={handleChatDividerMouseDown} on:keydown={(e) => (e.key === 'Enter' || e.key === ' ') && handleChatDividerMouseDown(e)}></button>
  <div class="chat-section" style="height: {chatHeightPercent}px">
    <div class="chat-header">
      <span class="section-label">💬 AI Assistant</span>
      <div class="chat-controls">
        <button on:click={() => showChatPanel = false} title="Toggle chat panel" class="control-btn">▼</button>
      </div>
    </div>
    <ChatPanel messages={chatMessages} loading={chatLoading} streamingMessage={chatStreamingMessage} on:message-sent />
  </div>
{/if}
"""
    with open(WORKSPACE_LAYOUT, "w") as f:
        f.write(content)


def update_ide_shell():
    content = """<script>
  import KeyboardShortcuts from '../components/KeyboardShortcuts.svelte'
  import ThemeToggle from '../components/ThemeToggle.svelte'
  import CommandPalette from '../components/CommandPalette.svelte'
  import Toasts from '../components/Toasts.svelte'
  import FileOpener from '../components/FileOpener.svelte'
  import SearchResults from '../components/SearchResults.svelte'
  import StatusBar from '../components/StatusBar.svelte'
  
  export let shortcuts = {}
  export let filePath = ''
  export let isDirty = false
  export let showFileTree = true
  export let showHistory = false
  export let compiling = false
  export let showSettings = false
  export let selectedFiles = []
  export let dryRun = false
  export let bulkMode = false
  export let toasts = []
  export let showConflictDialog = false
  export let conflictData = null
  export let showFileOpener = false
  export let recentFiles = []
  export let pinnedFiles = []
  export let showSearch = false
  export let searchQuery = ''
  export let searchResults = []
  export let searchLoading = false
  export let status = 'Ready'
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
      <input id="file-path-input" type="text" placeholder="File path..." bind:value={filePath} on:change class="flex-1 px-3 py-2 border rounded" aria-label="Current file path" />
      {#if isDirty}<span aria-live="polite" class="text-red-600" title="Unsaved changes">•</span>{/if}
      <button on:click={() => { showFileTree = !showFileTree; showHistory = false }} class="ml-2 px-4 py-2 bg-blue-500 text-white rounded hover:bg-blue-600" aria-label={showFileTree ? 'Hide file tree' : 'Show file tree'} aria-pressed={showFileTree}>{showFileTree ? 'Hide Tree' : 'Files'}</button>
      <button on:click={() => { showHistory = !showHistory; showFileTree = false }} class="ml-2 px-4 py-2 bg-blue-500 text-white rounded hover:bg-blue-600" aria-label={showHistory ? 'Hide history' : 'Show file history'} aria-pressed={showHistory}>{showHistory ? 'Hide' : 'History'}</button>
      <button on:click|preventDefault on:compile disabled={!filePath || compiling} class="ml-2 px-4 py-2 bg-green-500 text-white rounded hover:bg-green-600 disabled:bg-gray-400 disabled:cursor-not-allowed" title="Compile to PDF (Ctrl+Enter)" aria-busy={compiling} aria-label={compiling ? 'Compiling document' : 'Compile document to PDF'}>{compiling ? '⏳ Compiling...' : '📄 Compile'}</button>
      <button on:click|preventDefault on:settings-toggle class="ml-2 px-4 py-2 bg-gray-500 text-white rounded hover:bg-gray-600" aria-label={showSettings ? 'Close settings' : 'Open settings'} aria-pressed={showSettings}>⚙️ Settings</button>
      {#if selectedFiles.length > 0}<span class="ml-2 px-3 py-2 bg-green-100 text-green-800 rounded" role="status" aria-live="polite" aria-atomic="true">{selectedFiles.length} {selectedFiles.length === 1 ? 'file' : 'files'} selected</span>{/if}
      <div class="ml-auto"><ThemeToggle on:theme-change /></div>
    </div>

    <slot name="workspace" />

    <CommandPalette bind:this={commandPaletteRef} {dryRun} bulkMode={selectedFiles.length > 0} selectedCount={selectedFiles.length} on:command on:ai-plan on:toggle-dry-run />
    <StatusBar {status} />
    <Toasts {toasts} />
    
    {#if showConflictDialog && conflictData}
      <div class="conflict-dialog-overlay" role="dialog" aria-modal="true" aria-labelledby="conflict-title">
        <div class="conflict-dialog">
          <h2 id="conflict-title">File Conflict Detected</h2>
          <p>The file has been modified by another process. How would you like to proceed?</p>
          <div class="conflict-actions">
            <button class="conflict-btn conflict-btn-primary" on:click|preventDefault on:conflict-resolve={{action: 'overwrite'}}>Overwrite Server Version</button>
            <button class="conflict-btn conflict-btn-secondary" on:click|preventDefault on:conflict-resolve={{action: 'reload'}}>Reload from Server</button>
            <button class="conflict-btn conflict-btn-cancel" on:click={() => { showConflictDialog = false; conflictData = null; }}>Cancel</button>
          </div>
        </div>
      </div>
    {/if}
  </div>

  {#if showSettings && settingsLoaded && SettingsPanel}
    <svelte:component this={SettingsPanel} isOpen={showSettings} onClose={() => showSettings = false} onSave={(u) => {}} on:settings-save />
  {/if}

  <FileOpener visible={showFileOpener} recent={recentFiles} pinned={pinnedFiles} on:close={() => showFileOpener = false} on:open on:pin on:unpin />
  <SearchResults bind:visible={showSearch} bind:searchQuery={searchQuery} bind:results={searchResults} bind:loading={searchLoading} on:open on:close={() => showSearch = false} />
</div>

<style>
  :global(body) { margin: 0; padding: 0; }
  .app { display: flex; height: 100vh; background: #f5f5f5; }
  .sidebar { width: 280px; background: #fff; border-right: 1px solid #ddd; overflow-y: auto; }
  .main { flex: 1; display: flex; flex-direction: column; }
  .top-bar { padding: 1rem; background: #fff; border-bottom: 1px solid #ddd; display: flex; gap: 0.5rem; align-items: center; }
</style>
"""
    with open(IDE_SHELL, "w") as f:
        f.write(content)

def update_app_svelte():
    with open(APP_SVELTE, "r") as f:
        app_text = f.read()

    # The goal is to replace the entire template in App.svelte with a much simpler one using IdeShell and WorkspaceLayout.
    # We will find the start of the template `<div class="app" role="application" aria-label="LaTeX AI Editor">` 
    # and replace it down to `</div>` before the styles.
    
    start_tag = '<div class="app" role="application" aria-label="LaTeX AI Editor">'
    idx = app_text.find(start_tag)
    if idx == -1:
        return
    
    # Also find `<style>` tag
    style_idx = app_text.find('<style>', idx)
    if style_idx == -1:
        return

    script_and_logic = app_text[:idx]
    
    # Create the new template
    new_template = """
<IdeShell
  {shortcuts} bind:filePath bind:isDirty bind:showFileTree bind:showHistory bind:compiling bind:showSettings {selectedFiles} bind:dryRun bind:bulkMode {toasts} bind:showConflictDialog bind:conflictData bind:showFileOpener bind:recentFiles bind:pinnedFiles bind:showSearch bind:searchQuery bind:searchResults bind:searchLoading {status} bind:commandPaletteRef {SettingsPanel} {settingsLoaded}
  on:shortcut={handleShortcut}
  on:change={loadFile}
  on:compile={compileDocument}
  on:settings-toggle={async () => { if (!showSettings) await loadSettingsPanel(); showSettings = !showSettings; }}
  on:theme-change={(e) => handleThemeChange(e.detail)}
  on:command={(e) => bulkMode ? applyBulkCommand(e.detail) : applyCommand(e.detail)}
  on:ai-plan={(e) => requestAIPlan(e.detail)}
  on:toggle-dry-run={() => dryRun = !dryRun}
  on:conflict-resolve={(e) => resolveConflict(e.detail.action)}
  on:settings-save={(e) => handleSettingsSave(e.detail)}
  on:open={(e) => handleOpenFromPalette(e.detail.path)}
  on:pin={(e) => togglePin(e.detail.path)}
  on:unpin={(e) => togglePin(e.detail.path)}
>
  <svelte:fragment slot="sidebar">
    <PanelManager
      {filePath}
      {showHistory}
      {showFileTree}
      onFileTreeSelection={handleFileTreeSelection}
      onFileClick={handleFileClick}
      onFileTreeToggle={() => showFileTree = !showFileTree}
      onHistoryToggle={() => showHistory = !showHistory}
      onRestoreHistory={restoreHistoryEntry}
    />
  </svelte:fragment>

  <svelte:fragment slot="workspace">
    <WorkspaceLayout
      {filePath} bind:editorContent bind:editorRef {autoSave} autoSaveDelay={autoSaveInterval * 1000} {editorSettings} {themeSettings} bind:layoutMode {isDirty} {compiling} {bulkMode} {bulkTotal} {bulkProgress} bind:editorHeightPercent bind:chatHeightPercent bind:isDraggingDivider bind:contentWrapperRef bind:showChatPanel {pdfUrl} {pdfPages} {compileErrors} {compileWarnings} {diff} {aiPlan} compilationJobId={compilationJobId} bind:activeRightTab {chatMessages} {chatLoading} {chatStreamingMessage}
      on:saved={() => pushToast('Saved', 'success')}
      on:save-error={(e) => pushToast(`Save failed: ${e.detail?.error || 'Unknown'}`, 'error')}
      on:dirty-change={handleDirtyChange}
      on:content-changed={handleEditorContentChanged}
      on:save-conflict={handleSaveConflict}
      on:divider-mousedown={handleDividerMouseDown}
      on:chat-divider-mousedown={handleChatDividerMouseDown}
      on:apply-step={e => applyPlanStep(e.detail.step)}
      on:apply-all={e => applyPlanAll(e.detail.plan)}
      on:compiled={handleCompiled}
      on:compileFailed={handleCompileFailed}
      on:apply-selected={handleApplyHunks}
      on:apply-fix={e => applyFix(e.detail.fix)}
      on:reject-all={handleRejectAll}
      on:jump-to-line={handleJumpToLine}
      on:explain-errors={handleExplainErrors}
      on:get-suggestions={handleGetSuggestions}
      on:code-review={handleCodeReview}
      on:insert-template={handleInsertTemplate}
      on:message-sent={handleChatMessage}
    />
  </svelte:fragment>
</IdeShell>
"""
    
    # Now we need to add the imports for IdeShell and WorkspaceLayout to the top of App.svelte
    # We will just inject them at the end of the imports
    imports_end = script_and_logic.find('import \'./globals.css\'')
    if imports_end != -1:
        new_imports = "  import IdeShell from './layouts/IdeShell.svelte';\n  import WorkspaceLayout from './layouts/WorkspaceLayout.svelte';\n  import PanelManager from './layouts/PanelManager.svelte';\n"
        script_and_logic = script_and_logic[:imports_end] + new_imports + script_and_logic[imports_end:]

    with open(APP_SVELTE, "w") as f:
        f.write(script_and_logic + new_template)

if __name__ == "__main__":
    update_workspace_layout()
    update_ide_shell()
    update_app_svelte()
