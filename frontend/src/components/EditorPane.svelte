<script>
  // Tabs plus one Monaco editor. The editor is created once; switching tabs
  // swaps the *model* (each document owns its own, so undo history and the
  // saved/unsaved state survive a switch) and restores the cursor and scroll.
  import { onMount, untrack } from 'svelte'
  import { loadMonaco } from '../lib/monaco.js'
  import { editorThemeName } from '../lib/editorThemes.js'
  import { markersFor } from '../lib/diagnostics.js'
  import { referenceAt, definitionFor } from '../lib/latex/navigate.js'
  import { applySpellMarkers, clearSpellMarkers, isProseFile } from '../lib/latex/spellMarkers.js'

  let { documents, ui, commands, compile = null, spell = null, index = null, onfollow = () => {}, onclosetab = () => {}, onnew = () => {}, oncursor = () => {} } = $props()

  let host = $state()
  let editor = $state.raw(null)
  let monaco = null
  let current = null
  const viewStates = new Map()

  const isDark = () =>
    ui.theme === 'dark' ||
    (ui.theme === 'auto' && !!globalThis.matchMedia?.('(prefers-color-scheme: dark)').matches)

  onMount(() => {
    let detachKeys = () => {}
    let disposed = false

    loadMonaco().then((m) => {
      if (disposed) return
      monaco = m
      editor = m.editor.create(host, {
        model: null,
        theme: editorThemeName(isDark()),
        automaticLayout: true,
        minimap: { enabled: false },
        fontSize: ui.editor.fontSize,
        tabSize: ui.editor.tabSize,
        detectIndentation: false,
        fontFamily: "'JetBrains Mono', ui-monospace, monospace",
        wordWrap: ui.editor.wordWrap ? 'on' : 'off',
        lineNumbers: 'on',
        scrollBeyondLastLine: false,
        accessibilitySupport: 'on',
        ariaLabel: 'LaTeX editor'
      })
      editor.onDidChangeCursorPosition((e) =>
        oncursor({ line: e.position.lineNumber, column: e.position.column })
      )
      // Ctrl/Cmd+click follows a reference, like F12.
      editor.onMouseDown((e) => {
        const mods = e.event.ctrlKey || e.event.metaKey
        if (mods && e.target.position && e.event.leftButton) {
          e.event.preventDefault()
          void followAt(e.target.position)
        }
      })
      // Capture phase: the editor would otherwise act on Mod+Enter and the
      // like before the command registry saw them.
      detachKeys = commands.attach(host, { capture: true })
    })

    return () => {
      disposed = true
      detachKeys()
      editor?.dispose()
      editor = null
    }
  })

  // Follow the active document.
  $effect(() => {
    const doc = documents.active
    const ed = editor
    if (!ed) return
    untrack(() => {
      if (current) viewStates.set(current, ed.saveViewState())
      current = doc?.path ?? null
      ed.setModel(doc?.model ?? null)
      if (doc) {
        const saved = viewStates.get(doc.path)
        if (saved) ed.restoreViewState(saved)
        ed.focus()
      }
    })
  })

  // Editor preferences from Settings, applied to the live editor and every model.
  $effect(() => {
    const { fontSize, tabSize, wordWrap } = ui.editor
    const docs = [...documents.docs]
    if (!editor) return
    editor.updateOptions({ fontSize, tabSize, wordWrap: wordWrap ? 'on' : 'off' })
    for (const doc of docs) doc.model.updateOptions?.({ tabSize })
  })

  $effect(() => {
    void ui.theme
    monaco?.editor.setTheme(editorThemeName(isDark()))
  })

  // Show what the last compile found as squiggles in the files it names.
  $effect(() => {
    // Read everything this depends on before any early return, or an effect
    // that first runs before the editor exists would never run again.
    void editor
    const found = { errors: compile?.errors ?? [], warnings: compile?.warnings ?? [] }
    const docs = [...documents.docs]
    if (!monaco?.editor.setModelMarkers) return
    for (const doc of docs) {
      monaco.editor.setModelMarkers(
        doc.model,
        'latex-compile',
        markersFor(doc.path, found, doc.model, monaco.MarkerSeverity)
      )
    }
  })

  // Spelling: check each open prose file shortly after it stops changing.
  $effect(() => {
    void editor
    const on = ui.spellcheck
    const docs = [...documents.docs]
    if (!monaco || !spell) return
    if (!on) {
      for (const doc of docs) clearSpellMarkers(monaco, doc.model)
      return
    }
    const timers = new Map()
    const subs = []
    let alive = true
    const check = (doc) => {
      if (alive && isProseFile(doc.path)) applySpellMarkers(monaco, doc.model, spell)
    }
    const schedule = (doc) => {
      clearTimeout(timers.get(doc.path))
      timers.set(doc.path, setTimeout(() => check(doc), 600))
    }
    spell.ensureReady().then((ready) => {
      if (!ready || !alive) return
      docs.forEach(check)
      docs.forEach((doc) => subs.push(doc.model.onDidChangeContent?.(() => schedule(doc))))
    })
    const off = spell.onChange(() => docs.forEach(check))
    return () => {
      alive = false
      off()
      timers.forEach(clearTimeout)
      subs.forEach((s) => s?.dispose?.())
    }
  })

  async function followAt(position) {
    const model = editor?.getModel()
    if (!model || !index) return false
    const ref = referenceAt(model.getLineContent(position.lineNumber), position.column)
    if (!ref) return false
    await index.ensureLoaded()
    onfollow({ ref, def: definitionFor(ref, index.snapshot()) })
    return true
  }

  /** Go to the definition of what is under the cursor (F12). */
  export function followReference() {
    const position = editor?.getPosition()
    return position ? followAt(position) : Promise.resolve(false)
  }

  /** Scroll to a line and put the cursor there. */
  export function reveal(line, column = 1) {
    if (!editor || !line) return
    const target = Math.max(1, Number(line))
    editor.revealLineInCenter(target)
    editor.setPosition({ lineNumber: target, column })
    editor.focus()
  }

  export function focus() {
    editor?.focus()
  }

  function closeOnMiddleClick(event, path) {
    if (event.button === 1) {
      event.preventDefault()
      onclosetab(path)
    }
  }
</script>

<section class="editor-pane" aria-label="Editor">
  <div class="tabs" role="tablist" aria-label="Open files">
    {#each documents.docs as doc (doc.path)}
      <div
        class="tab"
        class:active={doc.path === documents.activePath}
        onauxclick={(e) => closeOnMiddleClick(e, doc.path)}
        role="presentation"
      >
        <button
          class="tab-label"
          role="tab"
          aria-selected={doc.path === documents.activePath}
          title={doc.path}
          onclick={() => documents.setActive(doc.path)}
        >
          {doc.path.split('/').pop()}
          {#if doc.conflict}<span class="mark bad" title="Conflict" aria-label="conflict">!</span>
          {:else if doc.dirty}<span class="mark" title="Unsaved changes" aria-label="unsaved changes">●</span>{/if}
        </button>
        <button class="tab-close" aria-label={`Close ${doc.path}`} onclick={() => onclosetab(doc.path)}>×</button>
      </div>
    {/each}
  </div>

  <div class="surface">
    <div class="host" bind:this={host} hidden={!documents.active}></div>
    {#if !documents.active}
      <div class="empty">
        <p>No file open</p>
        <p class="hint">Pick one from the project tree, or press <kbd>Ctrl</kbd>+<kbd>P</kbd>.</p>
        <p><button type="button" class="start" onclick={onnew}>Start from a template…</button></p>
      </div>
    {/if}
  </div>
</section>

<style>
  .editor-pane { display: flex; flex-direction: column; height: 100%; min-width: 0; background: hsl(var(--surface-1)); }
  .tabs { display: flex; overflow-x: auto; border-bottom: 1px solid hsl(var(--border)); background: hsl(var(--surface-2)); flex-shrink: 0; }
  .tab { display: flex; align-items: center; border-right: 1px solid hsl(var(--border)); white-space: nowrap; }
  .tab.active { background: hsl(var(--surface-1)); box-shadow: inset 0 -2px 0 hsl(var(--primary)); }
  .tab-label { padding: 0.45rem 0.4rem 0.45rem 0.75rem; background: none; border: none; color: hsl(var(--text-dim)); font: inherit; font-size: 13px; cursor: pointer; }
  .tab.active .tab-label { color: hsl(var(--foreground)); }
  .tab-close { background: none; border: none; color: hsl(var(--text-faint)); cursor: pointer; padding: 0 0.5rem; font-size: 15px; line-height: 1; border-radius: var(--radius-xs); }
  .tab-close:hover { color: hsl(var(--foreground)); background: hsl(var(--accent)); }
  .mark { margin-left: 0.3rem; font-size: 10px; color: hsl(var(--warn)); }
  .mark.bad { color: hsl(var(--err)); font-weight: 700; font-size: 12px; }
  .surface { position: relative; flex: 1; min-height: 0; }
  .host { position: absolute; inset: 0; }
  .host[hidden] { display: none; }
  .empty { position: absolute; inset: 0; display: grid; place-content: center; text-align: center; color: hsl(var(--text-dim)); }
  .start { padding: 0.4rem 0.9rem; font: inherit; color: hsl(var(--primary-foreground)); background: hsl(var(--primary)); border: none; border-radius: var(--radius-xs); cursor: pointer; }
  .hint { font-size: 13px; color: hsl(var(--text-faint)); }
  kbd { padding: 0 0.3rem; border: 1px solid hsl(var(--border)); border-radius: var(--radius-xs); font-size: 11px; }
</style>
