<script>
  import { onMount, tick } from 'svelte'
  import { Splitpanes, Pane } from 'svelte-splitpanes'
  import './globals.css'

  import { documents, compile, project, ui, commands, latexIndex, spell, ai, paper } from './lib/app.svelte.js'
  import { restoreVersion } from './lib/history.js'
  import { FileOps } from './lib/files.js'
  import { isDesktop, pickProject } from './lib/desktop.js'
  import { api } from './lib/api/client.js'
  import { readPref, writePref } from './lib/storage.js'

  import Toolbar from './components/Toolbar.svelte'
  import ProjectTree from './components/ProjectTree.svelte'
  import OutlinePanel from './components/OutlinePanel.svelte'
  import EditorPane from './components/EditorPane.svelte'
  import PdfPane from './components/PdfPane.svelte'
  import ProblemsPanel from './components/ProblemsPanel.svelte'
  import AssistantPane from './components/AssistantPane.svelte'
  import SettingsDialog from './components/SettingsDialog.svelte'
  import HistoryPanel from './components/HistoryPanel.svelte'
  import AskDialog from './components/AskDialog.svelte'
  import StatusBar from './components/StatusBar.svelte'
  import Toasts from './components/Toasts.svelte'
  import ConflictDialog from './components/ConflictDialog.svelte'
  import CommandMenu from './components/CommandMenu.svelte'
  import QuickOpen from './components/QuickOpen.svelte'

  const OPEN_KEY = 'latex-ai:open-files'
  const RECENT_KEY = 'latex-ai:recent-files'
  const BUILD_KEY = 'latex-ai:last-build'

  // Read before the effect below overwrites it with the (still empty) tab list.
  const lastSession = readPref(OPEN_KEY, { paths: [], active: null })

  const ops = new FileOps({ documents, project, ui })

  let editorPane = $state()
  let pdfPane = $state()
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

  // ---- SyncTeX -------------------------------------------------------

  const staleNote = 'The PDF is older than your edits, so the position may be off. Compile to refresh.'

  /** Source line under the cursor -> its place in the PDF. */
  async function showInPdf() {
    const doc = documents.active
    if (!doc) return
    if (!compile.buildId) {
      ui.toast('Compile first: there is no PDF to point into yet.', 'info')
      return
    }
    try {
      const found = await api.synctexForward({ file: doc.path, line: cursor?.line ?? 1, build: compile.buildId })
      const where = found.region ?? found.hits[0]
      if (!where) {
        ui.toast('That line produced nothing in the PDF.', 'info')
        return
      }
      pdfPane?.showHit(where)
      if (found.stale) ui.toast(staleNote, 'info')
    } catch (e) {
      ui.toast(e.status === 404 ? 'This file is not part of the compiled PDF.' : `Could not locate the line: ${e.message}`, 'error')
    }
  }

  /** A spot on the PDF was double-clicked: open the source behind it. */
  async function jumpToSource({ file, line, stale }) {
    await openFile(file)
    await tick()
    editorPane?.reveal(line)
    if (stale) ui.toast(staleNote, 'info')
  }

  /** A reference was followed in the editor: open where it is defined. */
  async function followReference({ ref, def }) {
    if (!def) {
      ui.toast(`No definition found for ${ref.name}`, 'info')
      return
    }
    await openFile(def.path)
    await tick()
    editorPane?.reveal(def.line)
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

  /** Anything that reads the project (the AI, a compile) sees saved text only. */
  async function ask(run, instruction) {
    ui.closeDialog()
    if (await ensureSaved()) await run(instruction)
  }

  function onVisualProposal(result) {
    ai.showProposal({
      summary: 'Visual fix',
      edit_count: result.edits?.length ?? 0,
      edits: result.edits ?? [],
      etag: result.etag,
      file: result.file
    })
  }

  async function loadConfig() {
    try {
      aiEnabled = (await api.getConfig()).ai_enabled ?? true
    } catch {
      /* the chat stays usable; the first request will say what is wrong */
    }
  }

  const openSettings = () => ui.openDialog('settings')
  const openHistory = () => ui.openDialog('history')

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
      { id: 'forward-search', title: 'Show this line in the PDF', keys: 'Mod+Alt+J', enabled: hasDoc, run: showInPdf },
      { id: 'go-to-definition', title: 'Go to definition', keys: 'F12', enabled: hasDoc, run: () => editorPane?.followReference() },
      { id: 'toggle-spellcheck', title: 'Toggle spell check', run: () => ui.toggle('spellcheck') },
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
      // Show the PDF of the previous session instead of an empty pane.
      if (project.root) await compile.restore(readPref(`${BUILD_KEY}:${project.root}`, null))
      for (const path of lastSession.paths ?? []) {
        if (project.files().some((f) => f.path === path)) await openFile(path)
      }
      if (lastSession.active) documents.setActive(lastSession.active)
    })()

    // Pick up edits made outside the editor whenever the window is used again.
    const onFocus = () => {
      latexIndex.invalidate()
      documents.checkExternalChanges()
    }
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

  // Remember the latest build so a reload can show it again.
  $effect(() => {
    if (project.root && compile.buildId && (compile.status === 'success' || compile.status === 'error')) {
      writePref(`${BUILD_KEY}:${project.root}`, compile.buildId)
    }
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
              <div class="sidebar">
                <ProjectTree {project} {documents} {ops} onopen={openFile} onrefresh={() => project.refresh()} />
                <OutlinePanel {documents} onreveal={(line) => editorPane?.reveal(line)} />
              </div>
            </Pane>
          {/if}
          <Pane size={ui.sizes.editor} minSize={20}>
            <EditorPane
              bind:this={editorPane}
              {documents}
              {ui}
              {commands}
              {compile}
              {spell}
              index={latexIndex}
              onfollow={followReference}
              onclosetab={closeTab}
              oncursor={(c) => (cursor = c)}
            />
          </Pane>
          <Pane size={ui.sizes.pdf} minSize={15}>
            <PdfPane
              bind:this={pdfPane}
              {compile}
              {paper}
              sourcePath={documents.active?.path ?? ''}
              oncompile={compileNow}
              onvisualproposal={onVisualProposal}
              onjump={jumpToSource}
              onlocate={showInPdf}
              onnotice={(m) => ui.toast(m, 'info')}
            />
          </Pane>
          {#if ui.chat}
            <Pane size={ui.sizes.chat} minSize={12}>
              <AssistantPane
                {ai}
                {aiEnabled}
                hasFile={!!documents.active}
                onsend={(text) => ask((t) => ai.send(t), text)}
                onsettings={openSettings}
                onopenfile={openFile}
              />
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
    <AskDialog
      onclose={() => ui.closeDialog()}
      onplan={(text) => ask((t) => ai.plan(t), text)}
      onrun={(text) => ask((t) => ai.send(t), text)}
    />
  {/if}

  {#if ui.dialog === 'opener'}
    <QuickOpen
      {recent}
      onclose={() => ui.closeDialog()}
      onopen={(path) => (ui.closeDialog(), openFile(path))}
    />
  {/if}

  {#if ui.dialog === 'history'}
    <HistoryPanel
      filePath={documents.active?.path ?? ''}
      onclose={() => ui.closeDialog()}
      onrestore={(entry, which) => restoreVersion(entry, { which, documents, project, ui })}
    />
  {/if}

  {#if ui.dialog === 'settings'}
    <SettingsDialog {ui} {documents} {commands} {paper} onclose={() => ui.closeDialog()} onchange={loadConfig} />
  {/if}
</div>

<style>
  :global(html, body, #app) { height: 100%; margin: 0; }
  :global(body) { background: hsl(var(--surface-0)); color: hsl(var(--foreground)); font-family: 'Outfit', system-ui, sans-serif; }
  .app { display: flex; flex-direction: column; height: 100vh; }
  .workspace { flex: 1; min-height: 0; }
  .sidebar { display: flex; flex-direction: column; height: 100%; }
  .sidebar :global(.tree) { flex: 1; min-height: 0; height: auto; }
  .workspace :global(.splitpanes) { height: 100%; }
  .workspace :global(.splitpanes__pane) { background: transparent; overflow: hidden; }
  .workspace :global(.splitpanes__splitter) { background: hsl(var(--border)) !important; border: none !important; position: relative; flex-shrink: 0; }
  .workspace :global(.splitpanes--vertical > .splitpanes__splitter) { width: 5px; cursor: col-resize; }
  .workspace :global(.splitpanes--horizontal > .splitpanes__splitter) { height: 5px; cursor: row-resize; }
  .workspace :global(.splitpanes__splitter:hover),
  .workspace :global(.splitpanes__splitter:focus-visible) { background: hsl(var(--primary) / 0.5) !important; }
</style>
