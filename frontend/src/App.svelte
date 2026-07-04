<script>
  import { onMount } from 'svelte'
  import { uiStore, initLayoutPreferences, toggleFileTree, toggleHistory, toggleSettings } from './stores/uiStore.js'
  import { fileStore } from './stores/fileStore.js'
  import { compileStore } from './stores/compileStore.js'
  import { aiStore, resetAI } from './stores/aiStore.js'
  import * as configService from './services/configService.js'
  import { loadFile, applyCommand } from './controllers/FileController.js'
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
  let autoSaveInterval = 30
  let recentFiles = []
  let pinnedFiles = []
  
  let editorRef = null
  let commandPaletteRef = null
  
  async function loadConfig() {
    try {
      const cfg = await configService.loadConfig()
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
  conflictData={null}
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
  on:change={() => loadFile($fileStore.filePath)}
  on:compile={() => compileDocument($fileStore.filePath)}
  on:settings-toggle={async () => { if (!$uiStore.showSettings) await loadSettingsPanel(); toggleSettings(); }}
  on:command={(e) => applyCommand($fileStore.filePath, e.detail, false)}
  on:ai-plan={(e) => requestAIPlan(e.detail, 'gemini')}
>
  <svelte:fragment slot="sidebar">
    <PanelManager
      filePath={$fileStore.filePath}
      showHistory={$uiStore.showHistory}
      showFileTree={$uiStore.showFileTree}
      onFileClick={(path) => {
        fileStore.update(f => ({ ...f, filePath: path }));
        loadFile(path);
      }}
      onFileTreeToggle={toggleFileTree}
      onHistoryToggle={toggleHistory}
    />
  </svelte:fragment>

  <svelte:fragment slot="workspace">
    <WorkspaceLayout
      filePath={$fileStore.filePath}
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
      chatLoading={$aiStore.status === 'streaming' || $aiStore.status === 'thinking'}
      chatStreamingMessage={$aiStore.streamingMessage}
      on:apply-selected={(e) => handleApplyHunks(e.detail.hunks, e.detail.selected, $aiStore.currentProposal)}
      on:message-sent={(e) => handleChatMessage(e.detail.content, e.detail.timestamp)}
      on:compiled={(e) => compileStore.update(c => ({...c, status: 'success', pdfUrl: e.detail.pdf_url, pageCount: e.detail.page_count, errors: e.detail.diagnostics?.errors, warnings: e.detail.diagnostics?.warnings}))}
      on:compileFailed={(e) => compileStore.update(c => ({...c, status: 'error', errors: e.detail.diagnostics?.errors}))}
    />
  </svelte:fragment>
</IdeShell>
