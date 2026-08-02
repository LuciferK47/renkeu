<script>
  import { onMount, onDestroy, createEventDispatcher } from 'svelte'
  // Lazy-load Monaco to reduce initial bundle size and parsing cost
  let monacoApi
  
  export let content = ''
  export let filePath = ''
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
  
  // Background sync state
  let backgroundSyncInterval = null
  const BACKGROUND_SYNC_DELAY = 5000  // 5 seconds
  let lastSyncedETag = null
  
  onMount(async () => {
    console.log('[TARGET] Editor.svelte onMount called')
    console.log('editorContainer:', editorContainer)
    console.log('editorContainer dimensions:', editorContainer?.offsetWidth, 'x', editorContainer?.offsetHeight)
    
    // Wait for DOM to be ready
    await new Promise(resolve => setTimeout(resolve, 100))
    
    // Dynamically import Monaco editor API only when editor mounts
    monacoApi = await import('monaco-editor/esm/vs/editor/editor.api')
    
    // Create Monaco editor instance
    const isDark = theme === 'auto'
      ? (window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches)
      : theme === 'dark'

    console.log('[MARKER] Theme:', isDark ? 'dark' : 'light')
    monacoApi.editor.setTheme(isDark ? 'vs-dark' : 'vs-light')

    // Ensure container has proper dimensions
    if (editorContainer) {
      editorContainer.style.width = '100%'
      editorContainer.style.height = '100%'
      editorContainer.style.position = 'relative'
      console.log('[MEASURE] Container styled, new dimensions:', editorContainer.offsetWidth, 'x', editorContainer.offsetHeight)
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
      console.log('[SUCCESS] Editor created successfully:', editor)
    } catch (err) {
      console.error('[ERROR] Error creating editor:', err)
      throw err
    }

    // Manual layout updates for better responsiveness
    const resizeObserver = new ResizeObserver(() => {
      if (editor) {
        console.log('[MEASURE] ResizeObserver triggered, calling layout()')
        editor.layout()
      }
    })
    if (editorContainer) {
      resizeObserver.observe(editorContainer)
      console.log('[WATCH] ResizeObserver attached')
    }

    currentFilePath = filePath
    await connectLsp()
    registerLspProviders()
    
    // Start background sync to detect external file changes
    startBackgroundSync()
    
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
        if (autoSave && hasUnsavedChanges && filePath) {
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
    if (backgroundSyncInterval) clearInterval(backgroundSyncInterval)
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
  
  /**
   * Start background sync to detect external file changes
   */
  function startBackgroundSync() {
    if (backgroundSyncInterval) return
    
    backgroundSyncInterval = setInterval(async () => {
      await checkForExternalChanges()
    }, BACKGROUND_SYNC_DELAY)
  }
  
  /**
   * Check if file was modified externally and merge changes
   */
  async function checkForExternalChanges() {
    if (!filePath || hasUnsavedChanges || isSaving) {
      // Don't sync if user has local unsaved changes or is currently saving
      return
    }
    
    try {
      const res = await fetch(`/api/file?path=${encodeURIComponent(filePath)}`)

      if (res.ok) {
        const data = await res.json()
        // The server has no conditional-GET support; compare the body etag.
        const newETag = data.etag
        
        // File was modified externally
        if (newETag && newETag !== lastSyncedETag && newETag !== currentETag) {
          // Update editor content
          isUpdatingFromProp = true
          editor.setValue(data.content || '')
          isUpdatingFromProp = false
          
          content = data.content || ''
          originalContent = content
          lastSyncedETag = newETag
          currentETag = newETag
          isDirty = false
          hasUnsavedChanges = false
          
          updateDirtyIndicator(false)
          dispatch('external-change-detected', { content, etag: newETag })
          console.log('External file change detected and merged:', filePath)
        }
      }
    } catch (error) {
      // Silently fail - don't interrupt user if network is down
      console.debug('Background sync check failed:', error)
    }
  }
  
  function updateDirtyIndicator(dirty) {
    // Visual feedback for unsaved changes
    if (editor && editorContainer) {
      editorContainer.classList.toggle('dirty', dirty)
    }
  }
  
  async function doSave(contentToSave) {
    if (!filePath) return
    isSaving = true
    const saveStartTime = performance.now()
    try {
      const headers = { 'Content-Type': 'application/json' }
      if (currentETag) headers['If-Match'] = currentETag
      const response = await fetch('/api/file', {
        method: 'POST',
        headers,
        body: JSON.stringify({ path: filePath, content: contentToSave })
      })
      if (response.ok) {
        const duration = performance.now() - saveStartTime
        const saved = await response.json().catch(() => null)
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
        dispatch('saved', { filePath, duration })
      } else if (response.status === 409) {
        const conflict = await response.json()
        saveStatus = 'Conflict: File was modified externally'
        dispatch('save-conflict', {
          filePath,
          currentContent: contentToSave,
          serverContent: conflict.current_content,
          serverETag: conflict.current_etag
        })
      } else {
        const error = await response.json()
        saveStatus = `Save failed: ${error.detail || 'Unknown error'}`
        dispatch('save-error', { error: error.detail || 'Save failed' })
      }
    } catch (err) {
      saveStatus = `Save error: ${err.message}`
      dispatch('save-error', { error: err.message })
      if (saveRetries < MAX_SAVE_RETRIES && hasUnsavedChanges) {
        saveRetries++
        const backoffDelay = Math.min(5000 * Math.pow(2, saveRetries - 1), 30000)
        saveStatus = `Save failed. Retrying in ${(backoffDelay / 1000).toFixed(0)}s... (${saveRetries}/${MAX_SAVE_RETRIES})`
        await new Promise(resolve => setTimeout(resolve, backoffDelay))
        // Queue another save after backoff to pick up latest content
        saveQueuedContent = content
      } else if (saveRetries >= MAX_SAVE_RETRIES) {
        saveStatus = 'Save failed after multiple retries. Please save manually.'
        dispatch('save-error', { error: 'Max retries exceeded', fatal: true })
      }
    } finally {
      isSaving = false
    }
  }

  async function saveFile() {
    if (!filePath || (!hasUnsavedChanges && !isSaving)) return
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
  
  // Update editor when content prop changes externally
  $: if (editor && content !== editor.getValue()) {
    isUpdatingFromProp = true
    const position = editor.getPosition()
    editor.setValue(content)
    originalContent = content
    isDirty = false
    if (position) {
      editor.setPosition(position)
    }
    isUpdatingFromProp = false
  }
  
  // Export save function for parent components
  export function save() {
    return saveFile()
  }
  
  // Load file content with ETag
  export async function loadFile(path) {
    try {
      const response = await fetch(`/api/file?path=${encodeURIComponent(path)}`)
      if (response.ok) {
        const data = await response.json()

        // Backend returns the blake3 etag in the body (routes/files.rs)
        if (data.etag) {
          currentETag = data.etag
        }
        
        // Update content
        if (editor) {
          isUpdatingFromProp = true
          editor.setValue(data.content || '')
          originalContent = data.content || ''
          isDirty = false
          hasUnsavedChanges = false
          isUpdatingFromProp = false
        }
        
        return data
      }
    } catch (err) {
      console.error('Failed to load file with ETag:', err)
      throw err
    }
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
