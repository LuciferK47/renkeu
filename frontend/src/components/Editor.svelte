<script>
  import { onMount, onDestroy, createEventDispatcher } from 'svelte'
  import { api } from '../lib/api/client.js'
  // Lazy-load Monaco to reduce initial bundle size and parsing cost
  let monacoApi
  
  export let content = ''
  export let filePath = ''
  // ETag of `content` as loaded/saved. Owned by fileStore; the editor only
  // follows it. Without this the first save of every file was unconditional
  // and, after a file switch or an applied AI edit, every save carried the
  // previous file's ETag and was rejected with 409 forever.
  export let etag = null
  export let autoSave = true
  export let autoSaveDelay = 2000
  export let editorSettings = {}
  export let theme = 'light' // 'light' | 'dark' | 'auto'
  
  const dispatch = createEventDispatcher()
  
  let editorContainer
  let editor
  let isUpdatingFromProp = false
  let isDirty = false
  let originalContent = content
  let autoSaveTimer = null
  let hasUnsavedChanges = false
  let lastSaveTime = 0
  let isSaving = false
  let savePromise = Promise.resolve()  // Serialize saves to avoid races
  let saveQueuedContent = null  // Latest content queued for save
  let saveStatus = ''  // Current save status for screen readers
  let saveStatusElement  // Reference for aria-live announcements
  let currentETag = null  // ETag for conflict detection
  // The path the buffer's text belongs to. `filePath` changes before the new
  // file has loaded, so saving to `filePath` could write file A's text into
  // file B's path. Saves always target this instead.
  let bufferPath = ''
  let lastPropKey = null
  let lastKeystroke = 0  // Timestamp of last keystroke for typing detection
  let saveRetries = 0  // Track save retry attempts
  const MAX_SAVE_RETRIES = 3
  const TYPING_PAUSE_MS = 1500  // Wait after typing stops before autosave

  // LSP (TexLab) integration state
  let lspSocket
  let lspReady = false
  let lspVersion = 1
  let lspPending = {}
  let currentFilePath = ''
  let completionProviderDisposable
  let hoverProviderDisposable
  
  onMount(async () => {
    
    // Wait for DOM to be ready
    await new Promise(resolve => setTimeout(resolve, 100))
    
    // Dynamically import Monaco editor API only when editor mounts
    monacoApi = await import('monaco-editor/esm/vs/editor/editor.api')
    
    // Create Monaco editor instance
    const isDark = theme === 'auto'
      ? (window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches)
      : theme === 'dark'

    monacoApi.editor.setTheme(isDark ? 'vs-dark' : 'vs-light')

    // Ensure container has proper dimensions
    if (editorContainer) {
      editorContainer.style.width = '100%'
      editorContainer.style.height = '100%'
      editorContainer.style.position = 'relative'
    }

    try {
      editor = monacoApi.editor.create(editorContainer, {
        value: content,
        language: 'latex',
        theme: isDark ? 'vs-dark' : 'vs-light',
        minimap: { enabled: !!editorSettings.minimap_enabled },
        fontSize: editorSettings.font_size ?? 14,
        tabSize: editorSettings.tab_size ?? 4,
        lineNumbers: 'on',
        wordWrap: (editorSettings.word_wrap ?? true) ? 'on' : 'off',
        automaticLayout: true,
        ariaLabel: `LaTeX editor${filePath ? ` for ${filePath}` : ''}`,
        accessibilitySupport: 'on',
      })
    } catch (err) {
      console.error('editor failed to create', err)
      throw err
    }

    // Manual layout updates for better responsiveness
    const resizeObserver = new ResizeObserver(() => {
      if (editor) {
        editor.layout()
      }
    })
    if (editorContainer) {
      resizeObserver.observe(editorContainer)
    }

    currentFilePath = filePath
    await connectLsp()
    registerLspProviders()
    
    // Listen for content changes from editor
    editor.onDidChangeModelContent(() => {
      if (!isUpdatingFromProp) {
        const newContent = editor.getValue()
        content = newContent
        isDirty = newContent !== originalContent
        hasUnsavedChanges = isDirty
        
        // Immediate visual feedback
        updateDirtyIndicator(isDirty)
        dispatch('content-changed', { content, isDirty })
        dispatch('dirty-change', { isDirty })
        
        // Track keystroke for typing detection
        lastKeystroke = Date.now()
        
        // Smart debounced auto-save with typing pause
        if (autoSaveTimer) clearTimeout(autoSaveTimer)
        
        // Only trigger auto-save if:
        // 1. Auto-save is enabled in config
        // 2. File has unsaved changes
        // 3. File path is set
        // 4. Not currently saving
        if (autoSave && hasUnsavedChanges && bufferPath) {
          // Use longer delay to avoid saving while user is actively typing
          const delay = Math.max(autoSaveDelay, TYPING_PAUSE_MS)
          autoSaveTimer = setTimeout(() => {
            // Double-check user stopped typing
            const timeSinceLastKey = Date.now() - lastKeystroke
            if (timeSinceLastKey >= TYPING_PAUSE_MS && hasUnsavedChanges) {
              saveFile()
            }
          }, delay)
        }

        // Push LSP didChange for real-time diagnostics/completions
        if (lspReady && filePath) {
          lspVersion += 1
          sendLspNotification('textDocument/didChange', {
            textDocument: {
              uri: `file://${filePath}`,
              version: lspVersion
            },
            contentChanges: [
              { text: newContent }
            ]
          })
        }
      }
    })
    
    // Keyboard shortcut: Ctrl+S to save
    editor.addCommand(monacoApi.KeyMod.CtrlCmd | monacoApi.KeyCode.KeyS, () => {
      saveFile()
    })
  })
  
  onDestroy(() => {
    if (autoSaveTimer) clearTimeout(autoSaveTimer)
    if (editor) {
      editor.dispose()
    }
    if (completionProviderDisposable) completionProviderDisposable.dispose()
    if (hoverProviderDisposable) hoverProviderDisposable.dispose()
    if (lspReady && filePath) {
      sendLspNotification('textDocument/didClose', {
        textDocument: { uri: `file://${filePath}` }
      })
    }
    if (lspSocket) {
      lspSocket.close()
    }
  })
  
  function updateDirtyIndicator(dirty) {
    // Visual feedback for unsaved changes
    if (editor && editorContainer) {
      editorContainer.classList.toggle('dirty', dirty)
    }
  }
  
  async function doSave(contentToSave) {
    // Snapshot the target now: the user can switch files while the request is
    // in flight, and the response must be applied to the file it was sent for.
    const targetPath = bufferPath
    const targetETag = currentETag
    if (!targetPath) return
    isSaving = true
    const saveStartTime = performance.now()
    try {
      const saved = await api.saveFile(targetPath, contentToSave, targetETag)
      const duration = performance.now() - saveStartTime
      if (targetPath !== bufferPath) return
      if (saved?.etag) currentETag = saved.etag
      if (content === contentToSave) {
        originalContent = content
        isDirty = false
        hasUnsavedChanges = false
        lastSaveTime = Date.now()
        saveRetries = 0
        updateDirtyIndicator(false)
        saveStatus = `File saved successfully. ${duration.toFixed(0)}ms`
      }
      dispatch('saved', { filePath: targetPath, duration, etag: saved?.etag ?? null })
    } catch (err) {
      if (err.status === 409) {
        saveStatus = 'Conflict: File was modified externally'
        dispatch('save-conflict', {
          filePath: targetPath,
          currentContent: contentToSave,
          serverETag: err.payload?.current_etag ?? null
        })
      } else if (err.status === 0 && saveRetries < MAX_SAVE_RETRIES && hasUnsavedChanges) {
        // Network failure only: a 4xx/5xx will not fix itself by retrying.
        saveRetries++
        const backoffDelay = Math.min(5000 * Math.pow(2, saveRetries - 1), 30000)
        saveStatus = `Save failed. Retrying in ${(backoffDelay / 1000).toFixed(0)}s... (${saveRetries}/${MAX_SAVE_RETRIES})`
        dispatch('save-error', { error: err.message })
        await new Promise(resolve => setTimeout(resolve, backoffDelay))
        // Queue another save after backoff to pick up latest content
        saveQueuedContent = content
      } else {
        saveStatus = `Save failed: ${err.message}`
        dispatch('save-error', { error: err.message, fatal: err.status === 0 })
      }
    } finally {
      isSaving = false
    }
  }

  async function saveFile() {
    if (!bufferPath || (!hasUnsavedChanges && !isSaving)) return
    // Always capture the latest content to be saved
    saveQueuedContent = content
    // Chain saves to ensure serialization and coalescing to latest
    savePromise = savePromise.then(async () => {
      // Process queued saves; coalesce to the latest content snapshot
      while (saveQueuedContent !== null) {
        const toSave = saveQueuedContent
        saveQueuedContent = null
        await doSave(toSave)
      }
    })
    return savePromise
  }
  
  // A new ETag from the store means a fresh load (or a save acknowledged by
  // the parent): adopt it, and bind the buffer to the path it belongs to.
  // filePath, content and etag are written to the store in one update by
  // FileController.loadFile, so they always arrive together. The key includes
  // the path because two different files can hash to the same ETag.
  $: if (etag) {
    const key = `${filePath}\0${etag}`
    if (key !== lastPropKey) {
      lastPropKey = key
      currentETag = etag
      bufferPath = filePath
    }
  }

  // Update editor when content prop changes externally
  $: if (editor && content !== editor.getValue()) {
    // A switch is reaching here with unsaved text only if the caller skipped
    // `flush()`; the autosave timer must not outlive the buffer it was for.
    if (autoSaveTimer) clearTimeout(autoSaveTimer)
    isUpdatingFromProp = true
    const position = editor.getPosition()
    editor.setValue(content)
    originalContent = content
    isDirty = false
    hasUnsavedChanges = false
    if (position) {
      editor.setPosition(position)
    }
    isUpdatingFromProp = false
  }

  // Export save function for parent components
  export function save() {
    return saveFile()
  }

  /**
   * Save any pending edits and report whether the buffer is clean afterwards.
   * Callers invoke this before anything that reads the file from disk
   * (switching files, compile, AI runs): those paths see only the last
   * autosave, so unsaved text would be silently ignored or discarded.
   */
  export async function flush() {
    if (autoSaveTimer) clearTimeout(autoSaveTimer)
    if (hasUnsavedChanges || isSaving) await saveFile()
    return !hasUnsavedChanges
  }

  /** Overwrite after a conflict: keep the local text, take the server's ETag. */
  export function adoptETag(newETag) {
    if (newETag) currentETag = newETag
  }

  // Jump to a specific line in the current file
  export function goToLine(lineNumber) {
    if (!editor || !lineNumber) return
    const line = Math.max(1, Number(lineNumber))
    editor.revealLineInCenter(line)
    editor.setPosition({ lineNumber: line, column: 1 })
    editor.focus()
  }
  
  // React to settings and theme changes
  $: if (editor) {
    const opts = {
      fontSize: editorSettings.font_size ?? 14,
      tabSize: editorSettings.tab_size ?? 4,
      wordWrap: (editorSettings.word_wrap ?? true) ? 'on' : 'off',
      minimap: { enabled: !!editorSettings.minimap_enabled }
    }
    editor.updateOptions(opts)

    const isDark = theme === 'auto'
      ? (window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches)
      : theme === 'dark'
    monacoApi.editor.setTheme(isDark ? 'vs-dark' : 'vs-light')
  }

  // Re-open file in LSP when filePath changes
  $: if (lspReady && editor && filePath && filePath !== currentFilePath) {
    currentFilePath = filePath
    lspVersion = 1
    sendLspNotification('textDocument/didOpen', {
      textDocument: {
        uri: `file://${filePath}`,
        languageId: 'latex',
        version: lspVersion,
        text: editor.getValue()
      }
    })
  }

  async function connectLsp() {
    // The in-binary LaTeX LSP is M5 scope (masterplan §4.2); until the
    // `crates/lsp` server exists there is no /ws/lsp endpoint to dial.
    if (!import.meta.env.VITE_ENABLE_LSP) return
    try {
      const protocol = window.location.protocol === 'https:' ? 'wss' : 'ws'
      const wsUrl = `${protocol}://${window.location.host}/ws/lsp`
      lspSocket = new WebSocket(wsUrl)

      lspSocket.onopen = async () => {
        lspReady = true
        if (filePath) {
          await sendLspNotification('textDocument/didOpen', {
            textDocument: {
              uri: `file://${filePath}`,
              languageId: 'latex',
              version: lspVersion,
              text: editor.getValue()
            }
          })
        }
      }

      lspSocket.onmessage = (event) => {
        try {
          const data = JSON.parse(event.data)
          if (data.type === 'response' && data.method && lspPending[data.method]?.length) {
            const resolver = lspPending[data.method].shift()
            resolver?.resolve(data.result)
          } else if (data.type === 'diagnostics') {
            applyDiagnostics(data)
          }
        } catch (err) {
          console.error('LSP message error', err)
        }
      }

      lspSocket.onerror = () => {
        lspReady = false
      }

      lspSocket.onclose = () => {
        lspReady = false
      }
    } catch (err) {
      console.warn('LSP unavailable; continuing without hints', err)
    }
  }

  function sendLspNotification(method, params) {
    if (!lspReady || !lspSocket) return
    lspSocket.send(JSON.stringify({ method, params }))
  }

  function sendLspRequest(method, params) {
    if (!lspReady || !lspSocket) return Promise.reject(new Error('LSP not available'))

    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => {
        reject(new Error(`LSP request timeout: ${method}`))
      }, 3000)

      const entry = { resolve: (result) => { clearTimeout(timeout); resolve(result) }, reject }
      if (!lspPending[method]) lspPending[method] = []
      lspPending[method].push(entry)

      lspSocket.send(JSON.stringify({ method, params }))
    })
  }

  function applyDiagnostics(payload) {
    if (!editor) return
    const model = editor.getModel()
    if (!model) return

    const markers = (payload.diagnostics || []).map((d) => ({
      severity: d.severity === 1 ? monacoApi.MarkerSeverity.Error : d.severity === 2 ? monacoApi.MarkerSeverity.Warning : monacoApi.MarkerSeverity.Info,
      message: d.message,
      startLineNumber: (d.range.start.line || 0) + 1,
      startColumn: (d.range.start.character || 0) + 1,
      endLineNumber: (d.range.end.line || 0) + 1,
      endColumn: (d.range.end.character || 0) + 1,
      source: d.source || 'texlab'
    }))

    monacoApi.editor.setModelMarkers(model, 'lsp', markers)
  }

  function registerLspProviders() {
    // Completion provider
    completionProviderDisposable = monacoApi.languages.registerCompletionItemProvider('latex', {
      triggerCharacters: ['\\', '{', '['],
      provideCompletionItems: async (model, position) => {
        if (!lspReady || !filePath) return { suggestions: [] }
        const params = {
          textDocument: { uri: `file://${filePath}` },
          position: { line: position.lineNumber - 1, character: position.column - 1 }
        }
        try {
          const items = await sendLspRequest('textDocument/completion', params)
          const suggestions = (items || []).map((item) => ({
            label: item.label,
            kind: monacoApi.languages.CompletionItemKind.Text,
            insertText: item.insertText || item.label,
            detail: item.detail,
            documentation: item.documentation?.value || item.documentation || ''
          }))
          return { suggestions }
        } catch (err) {
          console.debug('Completion error', err)
          return { suggestions: [] }
        }
      }
    })

    // Hover provider
    hoverProviderDisposable = monacoApi.languages.registerHoverProvider('latex', {
      provideHover: async (model, position) => {
        if (!lspReady || !filePath) return null
        const params = {
          textDocument: { uri: `file://${filePath}` },
          position: { line: position.lineNumber - 1, character: position.column - 1 }
        }
        try {
          const hover = await sendLspRequest('textDocument/hover', params)
          if (!hover) return null
          const contents = hover.contents?.value || hover.contents || ''
          return { contents: [{ value: contents }] }
        } catch (err) {
          console.debug('Hover error', err)
          return null
        }
      }
    })
  }
</script>

<div class="editor-wrapper">
  <div 
    bind:this={editorContainer} 
    class="editor-container"
    role="textbox"
    aria-label={`LaTeX editor${filePath ? ` for ${filePath}` : ''}`}
    aria-multiline="true"
    aria-readonly="false"
  ></div>
  
  <!-- Screen reader announcements -->
  <div
    bind:this={saveStatusElement}
    role="status"
    aria-live="polite"
    aria-atomic="true"
    class="sr-only"
  >
    {#if isSaving}
      Saving file...
    {:else if saveStatus}
      {saveStatus}
    {:else if isDirty}
      File has unsaved changes
    {:else}
      All changes saved
    {/if}
  </div>
</div>

<style>
  .editor-wrapper {
    position: relative;
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
  }

  .editor-container {
    width: 100%;
    height: 100%;
    flex: 1;
    background: hsl(var(--surface-1));
    border: none;
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
    border-width: 0;
  }
</style>
