<script>
  import { onMount, tick } from 'svelte'
  import { Splitpanes, Pane } from 'svelte-splitpanes'
  import './globals.css'

  import { documents, compile, project, ui, commands } from './lib/app.svelte.js'
  import { chat } from './lib/chat.js'
  import { FileOps } from './lib/files.js'
  import { isDesktop, pickProject } from './lib/desktop.js'
  import { api } from './lib/api/client.js'
  import { readPref, writePref } from './lib/storage.js'
  import { setProposal } from './stores/aiStore.js'

  import Toolbar from './components/Toolbar.svelte'
  import ProjectTree from './components/ProjectTree.svelte'
  import EditorPane from './components/EditorPane.svelte'
  import PdfPane from './components/PdfPane.svelte'
  import ProblemsPanel from './components/ProblemsPanel.svelte'
  import AiPane from './components/AiPane.svelte'
  import StatusBar from './components/StatusBar.svelte'
  import Toasts from './components/Toasts.svelte'
  import ConflictDialog from './components/ConflictDialog.svelte'
  import CommandMenu from './components/CommandMenu.svelte'
  import CommandPalette from './components/CommandPalette.svelte'
  import FileOpener from './components/FileOpener.svelte'

  const OPEN_KEY = 'latex-ai:open-files'
  const RECENT_KEY = 'latex-ai:recent-files'

  // Read before the effect below overwrites it with the (still empty) tab list.
  const lastSession = readPref(OPEN_KEY, { paths: [], active: null })

  const ops = new FileOps({ documents, project, ui })

  let SettingsPanel = $state(null)
  let HistorySidebar = $state(null)
  let editorPane = $state()
  let aiEnabled = $state(true)
  let cursor = $state(null)
  let recent = $state(readPref(RECENT_KEY, []))
  /** Conflicts the user chose to decide later. */
  let deferred = $state(new Set())

  const conflicted = $derived(documents.docs.find((d) => d.conflict && !deferred.has(d.path)) ?? null)

  // ---- actions -------------------------------------------------------

  async function openFile(path) {
    try {
      await documents.open(path)
      project.reveal(path)
      recent = [path, ...recent.filter((p) => p !== path)].slice(0, 20)
      writePref(RECENT_KEY, recent)
    } catch (e) {
      ui.toast(`Could not open ${path}: ${e.message}`, 'error')
    }
  }

  /**
   * Everything that reads the project from disk (compile, the AI) sees only
   * what has been saved. Flushing first means unsaved text is neither ignored
   * nor discarded; if a save cannot complete, the action is abandoned.
   */
  async function ensureSaved() {
    const clean = await documents.flushAll()
    if (!clean) ui.toast('Your changes could not be saved, so nothing was started.', 'error')
    return clean
  }

  async function compileNow() {
    const active = documents.active?.path
    if (!active || !(await ensureSaved())) return
    ui.setStatus('Compiling…')
    await compile.start(active, { main: project.mainOverride })
    ui.setStatus(compile.status === 'success' ? 'Compiled' : compile.status === 'error' ? 'Compile failed' : 'Ready')
    if (compile.status === 'error') ui.show('problems')
  }

  async function revealProblem({ file, line }) {
    if (!file) return
    await openFile(file)
    await tick()
    editorPane?.reveal(line)
  }

  async function closeTab(path) {
    const doc = documents.get(path)
    if (!doc) return
    if (doc.dirty && !(await documents.save(path))) {
      if (!window.confirm(`Discard unsaved changes to ${path}?`)) return
      documents.close(path, { force: true })
    } else {
      documents.close(path)
    }
  }

  async function resolveConflict(choice) {
    const doc = conflicted
    if (!doc) return
    await documents.resolveConflict(doc.path, choice)
  }

  async function openProject() {
    // The app restarts into the new folder: keep what has been typed.
    if (!(await ensureSaved())) return
    try {
      await pickProject()
    } catch (e) {
      ui.toast(`Could not open the folder: ${e.message ?? e}`, 'error')
    }
  }

  async function sendToAi(content) {
    if (await ensureSaved()) await chat.handleChatMessage(content)
  }

  async function applyHunks(payload) {
    if (await ensureSaved()) await chat.handleApplyHunks(payload)
  }

  function onVisualProposal(result) {
    setProposal({
      summary: 'Visual fix',
      edit_count: result.edits?.length ?? 0,
      edits: result.edits ?? [],
      etag: result.etag,
      file: result.file
    })
    ui.show('chat')
  }

  async function loadConfig() {
    try {
      aiEnabled = (await api.getConfig()).ai_enabled ?? true
    } catch {
      /* the chat stays usable; the first request will say what is wrong */
    }
  }

  async function openSettings() {
    SettingsPanel ??= (await import('./components/SettingsPanel.svelte')).default
    ui.openDialog('settings')
  }

  async function closeSettings() {
    ui.closeDialog()
    await loadConfig()
  }

  async function openHistory() {
    HistorySidebar ??= (await import('./components/HistorySidebar.svelte')).default
    ui.openDialog('history')
  }

  /** Splitter sizes come back in pane order; give them names. */
  function onResized(event) {
    const names = [ui.sidebar && 'sidebar', 'editor', 'pdf', ui.chat && 'chat'].filter(Boolean)
    const sizes = {}
    event.detail.forEach((pane, i) => {
      if (names[i]) sizes[names[i]] = Math.round(pane.size * 10) / 10
    })
    ui.setSizes(sizes)
  }

  // ---- commands ------------------------------------------------------

  function registerCommands() {
    const hasDoc = () => !!documents.active
    const defs = [
      { id: 'save', title: 'Save file', keys: 'Mod+S', enabled: hasDoc, run: () => documents.save(documents.active.path) },
      { id: 'compile', title: 'Compile', keys: 'Mod+Enter', enabled: () => hasDoc() && !compile.compiling, run: compileNow },
      { id: 'stop-compile', title: 'Stop compiling', enabled: () => compile.compiling, run: () => compile.cancel() },
      { id: 'quick-open', title: 'Open file…', keys: 'Mod+P', run: () => ui.openDialog('opener') },
      { id: 'command-menu', title: 'Show all commands', keys: 'F1', run: () => ui.openDialog('commands') },
      { id: 'palette', title: 'Ask the AI…', keys: 'Mod+K', run: () => ui.openDialog('palette') },
      { id: 'toggle-sidebar', title: 'Toggle file tree', keys: 'Mod+B', run: () => ui.toggle('sidebar') },
      { id: 'toggle-chat', title: 'Toggle assistant', keys: 'Mod+Shift+L', run: () => ui.toggle('chat') },
      { id: 'toggle-problems', title: 'Toggle problems panel', keys: 'Mod+J', run: () => ui.toggle('problems') },
      {
        id: 'next-problem',
        title: 'Go to first problem',
        keys: 'F8',
        enabled: () => compile.errors.some((e) => e.file),
        run: () => revealProblem(compile.errors.find((e) => e.file))
      },
      { id: 'close-tab', title: 'Close tab', keys: 'Alt+W', enabled: hasDoc, run: () => closeTab(documents.active.path) },
      { id: 'history', title: 'Show history', keys: 'Alt+H', run: openHistory },
      { id: 'settings', title: 'Settings', keys: 'Mod+,', run: openSettings },
      { id: 'open-project', title: 'Open project folder…', enabled: isDesktop, run: openProject },
      { id: 'refresh-files', title: 'Refresh file list', run: () => project.refresh() },
      { id: 'focus-editor', title: 'Focus editor', enabled: hasDoc, run: () => editorPane?.focus() }
    ]
    return defs.map((d) => commands.register(d))
  }

  onMount(() => {
    ui.applyTheme()
    const unregister = registerCommands()
    const detachKeys = commands.attach(window)
    loadConfig()

    // Reopen what was open last time, then show the tree.
    ;(async () => {
      await project.refresh()
      for (const path of lastSession.paths ?? []) {
        if (project.files().some((f) => f.path === path)) await openFile(path)
      }
      if (lastSession.active) documents.setActive(lastSession.active)
    })()

    // Pick up edits made outside the editor whenever the window is used again.
    const onFocus = () => documents.checkExternalChanges()
    const onBeforeUnload = (e) => {
      if (documents.anyDirty) {
        e.preventDefault()
        e.returnValue = ''
      }
    }
    window.addEventListener('focus', onFocus)
    window.addEventListener('beforeunload', onBeforeUnload)

    return () => {
      unregister.forEach((off) => off())
      detachKeys()
      window.removeEventListener('focus', onFocus)
      window.removeEventListener('beforeunload', onBeforeUnload)
    }
  })

  // Remember the open tabs.
  $effect(() => {
    writePref(OPEN_KEY, { paths: documents.docs.map((d) => d.path), active: documents.activePath })
  })

  // A resolved conflict no longer needs its "decide later" mark.
  $effect(() => {
    const stillConflicted = new Set(documents.docs.filter((d) => d.conflict).map((d) => d.path))
    if ([...deferred].some((p) => !stillConflicted.has(p))) {
      deferred = new Set([...deferred].filter((p) => stillConflicted.has(p)))
    }
  })
</script>

<div class="app" role="application" aria-label="LaTeX-AI">
  <Toolbar
    {ui}
    {documents}
    {compile}
    {project}
    {commands}
    oncompile={compileNow}
    oncancel={() => compile.cancel()}
    onsettings={openSettings}
    onhistory={openHistory}
    onpalette={() => ui.openDialog('palette')}
    onopenproject={isDesktop() ? openProject : null}
  />

  <div class="workspace">
    <Splitpanes horizontal class="vertical-stack">
      <Pane minSize={30}>
        <Splitpanes on:resized={onResized}>
          {#if ui.sidebar}
            <Pane size={ui.sizes.sidebar} minSize={8} maxSize={40}>
              <ProjectTree {project} {documents} {ops} onopen={openFile} onrefresh={() => project.refresh()} />
            </Pane>
          {/if}
          <Pane size={ui.sizes.editor} minSize={20}>
            <EditorPane
              bind:this={editorPane}
              {documents}
              {ui}
              {commands}
              onclosetab={closeTab}
              oncursor={(c) => (cursor = c)}
            />
          </Pane>
          <Pane size={ui.sizes.pdf} minSize={15}>
            <PdfPane {compile} sourcePath={documents.active?.path ?? ''} oncompile={compileNow} onvisualproposal={onVisualProposal} />
          </Pane>
          {#if ui.chat}
            <Pane size={ui.sizes.chat} minSize={12}>
              <AiPane {aiEnabled} filePath={documents.active?.path ?? ''} onsend={sendToAi} onapply={applyHunks} />
            </Pane>
          {/if}
        </Splitpanes>
      </Pane>
      {#if ui.problems}
        <Pane size={22} minSize={6} maxSize={60}>
          <ProblemsPanel {compile} onreveal={revealProblem} onclose={() => ui.toggle('problems')} />
        </Pane>
      {/if}
    </Splitpanes>
  </div>

  <StatusBar status={ui.status} doc={documents.active} {compile} {cursor} />
  <Toasts toasts={ui.toasts} />

  {#if conflicted}
    <ConflictDialog
      doc={conflicted}
      onresolve={resolveConflict}
      ondismiss={() => (deferred = new Set(deferred).add(conflicted.path))}
    />
  {/if}

  {#if ui.dialog === 'commands'}
    <CommandMenu {commands} onclose={() => ui.closeDialog()} />
  {/if}

  {#if ui.dialog === 'palette'}
    <div class="modal" role="presentation" onclick={(e) => e.target === e.currentTarget && ui.closeDialog()}>
      <div class="modal-body" role="dialog" aria-modal="true" aria-label="Ask the AI">
        <CommandPalette
          on:command={(e) => (ui.closeDialog(), chat.requestAIPlan(e.detail))}
          on:ai-plan={(e) => (ui.closeDialog(), chat.requestAIPlan(e.detail.request ?? e.detail))}
        />
      </div>
    </div>
  {/if}

  {#if ui.dialog === 'opener'}
    <FileOpener
      visible={true}
      {recent}
      pinned={[]}
      on:close={() => ui.closeDialog()}
      on:open={(e) => (ui.closeDialog(), openFile(e.detail.path))}
    />
  {/if}

  {#if ui.dialog === 'history' && HistorySidebar}
    <div class="modal" role="presentation" onclick={(e) => e.target === e.currentTarget && ui.closeDialog()}>
      <div class="modal-body tall" role="dialog" aria-modal="true" aria-label="History">
        <HistorySidebar filePath={documents.active?.path ?? ''} agentOnly={false} />
      </div>
    </div>
  {/if}

  {#if ui.dialog === 'settings' && SettingsPanel}
    <SettingsPanel isOpen={true} onClose={closeSettings} onSave={() => {}} />
  {/if}
</div>

<style>
  :global(html, body, #app) { height: 100%; margin: 0; }
  :global(body) { background: hsl(var(--surface-0)); color: hsl(var(--foreground)); font-family: 'Outfit', system-ui, sans-serif; }
  .app { display: flex; flex-direction: column; height: 100vh; }
  .workspace { flex: 1; min-height: 0; }
  .workspace :global(.splitpanes) { height: 100%; }
  .workspace :global(.splitpanes__pane) { background: transparent; overflow: hidden; }
  .workspace :global(.splitpanes__splitter) { background: hsl(var(--border)) !important; border: none !important; position: relative; flex-shrink: 0; }
  .workspace :global(.splitpanes--vertical > .splitpanes__splitter) { width: 5px; cursor: col-resize; }
  .workspace :global(.splitpanes--horizontal > .splitpanes__splitter) { height: 5px; cursor: row-resize; }
  .workspace :global(.splitpanes__splitter:hover),
  .workspace :global(.splitpanes__splitter:focus-visible) { background: hsl(var(--primary) / 0.5) !important; }
  .modal { position: fixed; inset: 0; background: rgb(0 0 0 / 0.35); display: flex; justify-content: center; padding-top: 12vh; z-index: 700; }
  .modal-body { align-self: flex-start; width: min(64rem, 94vw); background: hsl(var(--popover)); border: 1px solid hsl(var(--border)); border-radius: var(--radius-s); box-shadow: 0 12px 40px rgb(0 0 0 / 0.35); overflow: auto; max-height: 76vh; }
  .modal-body.tall { width: min(40rem, 94vw); }
</style>
