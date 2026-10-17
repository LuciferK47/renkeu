<script>
  import { onMount } from 'svelte'
  if (import.meta.env.DEV) window.__probe = { }
  import { uiStore, initLayoutPreferences, toggleFileTree, toggleHistory, toggleSettings, setStatus, pushToast } from './stores/uiStore.js'
  import { fileStore } from './stores/fileStore.js'
  import { compileStore } from './stores/compileStore.js'
  import { aiStore, resetAI } from './stores/aiStore.js'
  import * as configService from './services/configService.js'
  import { loadFile } from './controllers/FileController.js'
  import { compileDocument } from './controllers/CompileController.js'
  import { requestAIPlan, handleChatMessage, handleApplyHunks } from './controllers/ChatController.js'

  import IdeShell from './layouts/IdeShell.svelte'
  import WorkspaceLayout from './layouts/WorkspaceLayout.svelte'
  import PanelManager from './layouts/PanelManager.svelte'
  import './globals.css'
  
  let HistorySidebar
  let SettingsPanel
  let settingsLoaded = false
  let historyLoaded = false
  
  let shortcuts = {}
  let editorSettings = {}
  let themeSettings = {}
  let autoSave = true
  // False when the server has no API key; the chat explains how to fix it.
  let aiEnabled = true
  let autoSaveInterval = 30
  let recentFiles = []
  let pinnedFiles = []
  
  let editorRef = null
  let commandPaletteRef = null
  let conflict = null

  // Everything below that reads the file from the server (switching files,
  // compiling, the AI) sees only the last autosave. Flushing first means the
  // user's unsaved text is neither ignored nor silently discarded. If the save
  // cannot complete (conflict, network) the action is abandoned.
  async function ensureSaved() {
    if (!editorRef) return true
    const clean = await editorRef.flush()
    if (!clean) pushToast('Save your changes first — they could not be saved automatically.', 'error')
    return clean
  }

  async function resolveConflict(action) {
    const current = conflict
    uiStore.update((ui) => ({ ...ui, showConflictDialog: false }))
    conflict = null
    if (!current) return
    if (action === 'overwrite') {
      // Keep the local text; adopt the server's ETag so the write is
      // accepted, which is exactly the user's explicit choice.
      editorRef?.adoptETag(current.serverETag)
      await editorRef?.save()
    } else if (action === 'reload') {
      await loadFile(current.filePath)
    }
  }
  
  async function loadConfig() {
    try {
      const cfg = await configService.loadConfig()
      aiEnabled = cfg.ai_enabled ?? true
      autoSave = cfg.auto_save ?? true
      autoSaveInterval = cfg.auto_save_interval ?? 30
      shortcuts = cfg.keyboard_shortcuts || {}
      editorSettings = cfg.editor || {}
      themeSettings = cfg.theme || {}
    } catch (e) {
      console.error('Failed to load config:', e)
    }
  }

  async function loadSettingsPanel() {
    if (!settingsLoaded) {
      const module = await import('./components/SettingsPanel.svelte')
      SettingsPanel = module.default
      settingsLoaded = true
    }
  }
  
  async function loadHistorySidebar() {
    if (!historyLoaded) {
      const module = await import('./components/HistorySidebar.svelte')
      HistorySidebar = module.default
      historyLoaded = true
    }
  }

  onMount(() => {
    initLayoutPreferences()
    loadConfig()
    recentFiles = JSON.parse(localStorage.getItem('recent_files') || '[]')
    pinnedFiles = JSON.parse(localStorage.getItem('pinned_files') || '[]')
    
    const beforeUnload = (event) => {
      if ($fileStore.dirty) {
        event.preventDefault()
        event.returnValue = ''
      }
    }
    window.addEventListener('beforeunload', beforeUnload)
    return () => window.removeEventListener('beforeunload', beforeUnload)
  })
</script>

<IdeShell
  {shortcuts}
  filePath={$fileStore.filePath}
  isDirty={$fileStore.dirty}
  showFileTree={$uiStore.showFileTree}
  showHistory={$uiStore.showHistory}
  compiling={$compileStore.status === 'compiling'}
  showSettings={$uiStore.showSettings}
  selectedFiles={[]}
  dryRun={false}
  bulkMode={false}
  toasts={$uiStore.toasts}
  showConflictDialog={$uiStore.showConflictDialog}
  conflictData={conflict}
  showFileOpener={$uiStore.showFileOpener}
  {recentFiles}
  {pinnedFiles}
  showSearch={$uiStore.showSearch}
  searchQuery=""
  searchResults={[]}
  searchLoading={false}
  status={$uiStore.status}
  bind:commandPaletteRef
  {SettingsPanel}
  {settingsLoaded}
  on:shortcut={(e) => {
    if (e.detail.action === 'save' && editorRef) editorRef.save();
    if (e.detail.action === 'apply_command' && commandPaletteRef) commandPaletteRef.focus();
    if (e.detail.action === 'toggle_history') toggleHistory();
    if (e.detail.action === 'toggle_file_tree') toggleFileTree();
  }}
  on:change={(e) => {
    // Read the path the user just typed. This used to pass
    // `$fileStore.filePath`, but the input is one-way (`value=`, not `bind:`),
    // so the store still held the *previous* path -- null on first use. Typing
    // a path and pressing Enter did nothing at all, and Compile stayed disabled
    // because nothing ever opened.
    const path = e.target.value;
    ensureSaved().then((clean) => { if (clean) loadFile(path); });
  }}
  on:compile={async () => { if (await ensureSaved()) compileDocument($fileStore.filePath) }}
  on:conflict-resolve={(e) => resolveConflict(e.detail.action)}
  on:settings-toggle={async () => { const wasOpen = $uiStore.showSettings; if (!wasOpen) await loadSettingsPanel(); toggleSettings(); if (wasOpen) loadConfig(); }}
  on:command={async (e) => { if (await ensureSaved()) requestAIPlan(e.detail.command ?? e.detail) }}
  on:ai-plan={async (e) => { if (await ensureSaved()) requestAIPlan(e.detail.request ?? e.detail) }}
>
  <svelte:fragment slot="sidebar">
    <PanelManager
      filePath={$fileStore.filePath}
      showHistory={$uiStore.showHistory}
      showFileTree={$uiStore.showFileTree}
      onFileClick={async (path) => {
        // Not setting filePath here: it must change together with content and
        // etag (FileController.loadFile does all three at once), or a save
        // landing in between would write the old file's text to the new path.
        if (path === $fileStore.filePath) return;
        if (await ensureSaved()) loadFile(path);
      }}
      onFileTreeToggle={toggleFileTree}
      onHistoryToggle={toggleHistory}
    />
  </svelte:fragment>

  <svelte:fragment slot="workspace">
    <!-- The editor dispatches dirty-change, content-changed, saved, save-error
         and save-conflict, and WorkspaceLayout forwards all five, but nothing
         listened: the unsaved-changes dot never appeared, the status bar never
         said "Saved", and a conflict was reported only inside the editor.
         Saving worked; you just could not tell it had. -->
    <WorkspaceLayout
      filePath={$fileStore.filePath}
      etag={$fileStore.etag}
      bind:editorContent={$fileStore.content}
      bind:editorRef
      {autoSave}
      autoSaveDelay={autoSaveInterval * 1000}
      {editorSettings}
      {themeSettings}
      layoutMode={$uiStore.layoutMode}
      isDirty={$fileStore.dirty}
      compiling={$compileStore.status === 'compiling'}
      bulkMode={false}
      editorHeightPercent={$uiStore.editorHeightPercent}
      chatHeightPercent={$uiStore.chatHeightPercent}
      isDraggingDivider={false}
      showChatPanel={$uiStore.showChatPanel}
      pdfUrl={$compileStore.pdfUrl}
      pdfPages={$compileStore.pageCount}
      compileErrors={$compileStore.errors}
      compileWarnings={$compileStore.warnings}
      aiPlan={$aiStore.currentProposal}
      compilationJobId={$compileStore.jobId}
      activeRightTab={$uiStore.activeRightTab}
      chatMessages={$aiStore.messages}
      {aiEnabled}
      chatLoading={$aiStore.status === 'streaming' || $aiStore.status === 'thinking'}
      chatStreamingMessage={$aiStore.streamingMessage}
      on:apply-selected={async (e) => { if (await ensureSaved()) handleApplyHunks(e.detail) }}
      on:message-sent={async (e) => { if (await ensureSaved()) handleChatMessage(e.detail.content) }}
      on:dirty-change={(e) => fileStore.update((f) => ({ ...f, dirty: e.detail.isDirty }))}
      on:content-changed={(e) =>
        fileStore.update((f) => ({
          ...f,
          content: e.detail.content,
          dirty: e.detail.isDirty,
        }))}
      on:saved={(e) => {
        fileStore.update((f) => ({
          ...f,
          dirty: false,
          savedContent: f.content,
          // Keep the store's ETag current: AI runs and apply read it.
          etag: e.detail.etag ?? f.etag,
          lastSavedAt: new Date()
        }));
        setStatus('Saved');
      }}
      on:save-error={(e) => {
        setStatus(`Save failed: ${e.detail.error}`);
        pushToast(`Save failed: ${e.detail.error}`, 'error');
      }}
      on:save-conflict={(e) => {
        // The file moved under the edit, so the buffer stays dirty: telling the
        // user it saved would be a lie, and a retry would clobber the other
        // change with a stale etag.
        setStatus('Conflict: file changed on disk');
        conflict = { filePath: e.detail.filePath, serverETag: e.detail.serverETag };
        uiStore.update((ui) => ({ ...ui, showConflictDialog: true }));
      }}
    />
  </svelte:fragment>
</IdeShell>
