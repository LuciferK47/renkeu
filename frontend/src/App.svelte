<script>
  import { onMount } from 'svelte'
  import Editor from './components/Editor.svelte'
  import DiffView from './components/DiffView.svelte'
  import RightPanel from './components/RightPanel.svelte'
  import CommandPalette from './components/CommandPalette.svelte'
  import StatusBar from './components/StatusBar.svelte'
  // Lazy load heavy components - only loaded when needed
  let HistorySidebar
  let SettingsPanel
  import FileTree from './components/FileTree.svelte'
  import KeyboardShortcuts from './components/KeyboardShortcuts.svelte'
  import ThemeToggle from './components/ThemeToggle.svelte'
  import FileOpener from './components/FileOpener.svelte'
  import Toasts from './components/Toasts.svelte'
  import SearchResults from './components/SearchResults.svelte'
  import ChatPanel from './components/ChatPanel.svelte'
  import ChatWithProposal from './components/ChatWithProposal.svelte'
  import * as fileService from './services/fileService.js'
  import * as compileService from './services/compileService.js'
  import * as aiService from './services/aiService.js'
  import * as configService from './services/configService.js'
  import { fileStore, setFile, markDirty, markSaved } from './stores/fileStore.js'
  import { compileStore, setCompileJob, updateCompileProgress, setCompileSuccess, setCompileError, resetCompile } from './stores/compileStore.js'
  import { aiStore, initSession, addMessage, startStreaming, updateStreamingMessage, setProposal, setError, resetAI } from './stores/aiStore.js'
  import './globals.css'
  
  // Lazy load heavy components
  let historyLoaded = false
  let settingsLoaded = false
  
  async function loadHistorySidebar() {
    if (!historyLoaded) {
      const module = await import('./components/HistorySidebar.svelte')
      HistorySidebar = module.default
      historyLoaded = true
    }
  }
  
  async function loadSettingsPanel() {
    if (!settingsLoaded) {
      const module = await import('./components/SettingsPanel.svelte')
      SettingsPanel = module.default
      settingsLoaded = true
    }
  }

  let filePath = ''
  let editorContent = ''
  let editorRef
  let commandPaletteRef
  let diff = ''
  let status = 'Ready'
  let showHistory = false
  let showFileTree = true
  let dryRun = false
  let bulkMode = false
  let selectedFiles = []
  let bulkProgress = 0
  let bulkTotal = 0
  let showSettings = false
  let aiPlan = null
  let autoSave = true
  let autoSaveInterval = 30
  let shortcuts = {}
  let editorSettings = {}
  let themeSettings = {}
  
  // PDF compilation state
  let pdfUrl = ''
  let pdfPages = 0
  let compiling = false
  let compileQueued = false
  let compileController = null
  let compileErrors = []
  let compileWarnings = []
  let activeRightTab = 'diff'
  let compilationJobId = null  // Track async compilation job
  let lastDiffBase = ''
  let isDirty = false
  let autoCompile = true  // Enable/disable auto-compile
  let compileDebounceTimer = null  // Debounce timer for auto-compile
  let suggestionsDebounceTimer = null  // Debounce timer for auto-suggestions
  let autoSuggest = true  // Enable/disable auto-suggestions

  // Quick open + favorites
  let showFileOpener = false
  let recentFiles = JSON.parse(localStorage.getItem('recent_files') || '[]')
  let pinnedFiles = JSON.parse(localStorage.getItem('pinned_files') || '[]')

  // Toasts
  let toasts = []
  let toastId = 0
  
  // Conflict resolution
  let showConflictDialog = false
  let conflictData = null
  
  // Search
  let showSearch = false
  let searchResults = []
  let searchLoading = false
  let searchQuery = ''

  // Chat Panel
  let chatMessages = []
  let chatLoading = false
  let chatStreamingMessage = ''
  let showChatPanel = true
  let layoutMode = 'overleaf'  // 'overleaf' | 'fullscreen-editor' | 'fullscreen-pdf'
  let sessionId = generateSessionId()  // Unique ID for conversation tracking

  function generateSessionId() {
    return `session_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`
  }

  // Resizable layout state
  let editorHeightPercent = 60  // Editor takes 60% of vertical space (when in overleaf mode)
  let chatHeightPercent = 300  // Chat panel height in pixels
  let isDraggingDivider = false
  let contentWrapperRef = null
  
  function handleDividerMouseDown(e) {
    isDraggingDivider = true
    const startY = e.clientY
    const startHeight = contentWrapperRef?.offsetHeight || 800
    const startEditorHeight = (editorHeightPercent / 100) * startHeight
    
    const handleMouseMove = (moveEvent) => {
      const deltaY = moveEvent.clientY - startY
      const newEditorHeight = Math.max(200, Math.min(startEditorHeight + deltaY, startHeight - 300))
      editorHeightPercent = (newEditorHeight / startHeight) * 100
    }
    
    const handleMouseUp = () => {
      isDraggingDivider = false
      document.removeEventListener('mousemove', handleMouseMove)
      document.removeEventListener('mouseup', handleMouseUp)
      // Save user preference
      localStorage.setItem('layout:editorHeight', editorHeightPercent.toString())
    }
    
    document.addEventListener('mousemove', handleMouseMove)
    document.addEventListener('mouseup', handleMouseUp)
  }
  
  function handleChatDividerMouseDown(e) {
    isDraggingDivider = true
    const startY = e.clientY
    const startHeight = chatHeightPercent
    
    const handleMouseMove = (moveEvent) => {
      const deltaY = moveEvent.clientY - startY
      const newHeight = Math.max(150, Math.min(startHeight - deltaY, 600))
      chatHeightPercent = newHeight
    }
    
    const handleMouseUp = () => {
      isDraggingDivider = false
      document.removeEventListener('mousemove', handleMouseMove)
      document.removeEventListener('mouseup', handleMouseUp)
      // Save user preference
      localStorage.setItem('layout:chatHeight', chatHeightPercent.toString())
    }
    
    document.addEventListener('mousemove', handleMouseMove)
    document.addEventListener('mouseup', handleMouseUp)
  }
  
  // Load saved layout preferences on mount
  onMount(() => {
    const savedEditorHeight = localStorage.getItem('layout:editorHeight')
    const savedChatHeight = localStorage.getItem('layout:chatHeight')
    
    if (savedEditorHeight) editorHeightPercent = parseFloat(savedEditorHeight)
    if (savedChatHeight) chatHeightPercent = parseFloat(savedChatHeight)
  })

  function handleCompiled(e) {
    const { pdf_url, diagnostics, page_count } = e.detail
    compiling = false
    pdfUrl = pdf_url
    pdfPages = page_count
    compileErrors = diagnostics?.errors || []
    compileWarnings = diagnostics?.warnings || []
    status = `Compiled successfully (${page_count} pages)`
    activeRightTab = 'pdf'
    pushToast('Compile succeeded', 'success')
    
    // Auto-trigger suggestions after successful compilation (debounced)
    scheduleAutoSuggestions()
  }

  function handleCompileFailed(e) {
    const { error, diagnostics } = e.detail
    compiling = false
    compileErrors = diagnostics?.errors || [{ message: error }]
    compileWarnings = diagnostics?.warnings || []
    status = `Compilation failed: ${error}`
    activeRightTab = 'errors'
    pushToast('Compile failed', 'error')
  }

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

  async function loadFile() {
    if (!filePath) return
    
    status = 'Loading...'
    try {
      const data = await fileService.loadFile(filePath)
      editorContent = data.content
      status = 'Loaded'
      
      // Auto-trigger suggestions after file load (debounced)
      scheduleAutoSuggestions()
    } catch (e) {
      status = `Error: ${e.message}`
    }
  }

  async function applyCommand(command) {
    if (!filePath) {
      status = 'Please select a file first'
      return
    }

    lastDiffBase = editorContent
    status = 'Applying...'
    try {
      const data = await fileService.applyCommand(filePath, command, dryRun)

      if (!data.success) throw new Error(data.message)

      diff = data.diff || ''
      status = data.message
      activeRightTab = 'diff'  // Show diff tab after applying command
      pushToast(data.message || 'Command applied', 'info')

      if (!dryRun && data.changed) {
        await loadFile()
      }
    } catch (e) {
      status = `Error: ${e.message}`
      pushToast(`Command error: ${e.message}`, 'error')
    }
  }

  async function requestAIPlan({ request, model }) {
    if (!filePath) {
      status = 'Please select a file first'
      return
    }

    status = 'Planning with AI...'
    try {
      const data = await aiService.requestAIPlan(filePath, request, model)

      aiPlan = data
      status = 'AI plan ready'
      activeRightTab = 'ai'
    } catch (e) {
      status = `AI error: ${e.message}`
    }
  }

  async function applyPlanStep(step) {
    if (!filePath || !step || !step.command) {
      status = 'No command to apply'
      return
    }
    await applyCommand(step.command)
  }

  async function applyPlanAll(plan) {
    if (!plan || !plan.steps || plan.steps.length === 0) {
      status = 'No steps to apply'
      return
    }
    status = 'Applying AI steps...'
    for (const step of plan.steps) {
      if (step.command) {
        await applyCommand(step.command)
      }
    }
    status = 'Applied all AI steps'
  }

  async function applyFix(fix) {
    if (!filePath || !fix || !fix.command) {
      status = 'No fix command to apply'
      return
    }
    status = `Applying fix: ${fix.description || fix.command}...`
    try {
      await applyCommand(fix.command)
      pushToast(`Applied: ${fix.description || 'Fix'}`, 'success')
      // Trigger recompilation after applying fix
      await triggerCompile()
    } catch (e) {
      status = `Error applying fix: ${e.message}`
      pushToast(`Error: ${e.message}`, 'error')
    }
  }

  async function applyBulkCommand(command) {
    if (selectedFiles.length === 0) {
      status = 'No files selected'
      pushToast('No files selected for bulk operation', 'warning')
      return
    }

    bulkMode = true
    bulkTotal = selectedFiles.length
    bulkProgress = 0
    status = `Applying command to ${selectedFiles.length} files...`

    try {
      const data = await fileService.bulkApply(selectedFiles, command)

      // Process results
      let successes = 0
      let failures = 0
      
      if (data.results) {
        for (const result of data.results) {
          if (result.success) {
            successes++
          } else {
            failures++
          }
          bulkProgress++
        }
      }

      status = `Applied to ${successes}/${selectedFiles.length} files${failures > 0 ? ` (${failures} failed)` : ''}`
      pushToast(`Bulk operation complete: ${successes} succeeded${failures > 0 ? `, ${failures} failed` : ''}`, successes > 0 ? 'success' : 'error')
      
      // Reload current file if it was in the selection
      if (selectedFiles.includes(filePath)) {
        await loadFile()
      }
    } catch (e) {
      status = `Bulk operation error: ${e.message}`
      pushToast(`Bulk operation failed: ${e.message}`, 'error')
    } finally {
      bulkMode = false
      bulkProgress = 0
      bulkTotal = 0
    }
  }

  async function handleChatMessage(e) {
    const userMessage = e.detail.content
    const timestamp = e.detail.timestamp

    // Add user message to chat
    chatMessages = [...chatMessages, {
      role: 'user',
      content: userMessage,
      timestamp
    }]

    chatLoading = true
    chatStreamingMessage = ''
    status = 'AI is thinking...'

    try {
      let fullResponse = ''
      await aiService.streamAIPlan(
        filePath || 'main.tex',
        userMessage,
        chatMessages,
        sessionId,
        (data) => {
          if (data.type === 'progress') {
            chatStreamingMessage = data.message
          } else if (data.type === 'step') {
            chatStreamingMessage += `\n${data.step.reasoning}\n\`\`\`latex\n${data.step.command}\n\`\`\`\n`
            fullResponse += `${data.step.reasoning}\n`
          } else if (data.type === 'complete') {
            fullResponse = chatStreamingMessage
          }
        },
        () => {
          // completion handled after stream
        },
        (error) => {
          throw error
        }
      )

      // Add assistant response to chat
      if (fullResponse || chatStreamingMessage) {
        chatMessages = [...chatMessages, {
          role: 'assistant',
          content: fullResponse || chatStreamingMessage,
          timestamp: new Date().toISOString()
        }]
      }

      status = 'AI response ready'
      activeRightTab = 'ai'
    } catch (e) {
      console.error('Chat error:', e)
      status = `Chat error: ${e.message}`
    } finally {
      chatLoading = false
      chatStreamingMessage = ''
    }
  }

  async function handleGetSuggestions(e) {
    if (!filePath) {
      status = 'Please select a file first'
      return
    }

    // Add system message to chat
    chatMessages = [...chatMessages, {
      role: 'system',
      content: 'Analyzing document for improvement suggestions...',
      timestamp: new Date().toISOString()
    }]

    chatLoading = true
    chatStreamingMessage = ''
    status = 'Generating suggestions...'

    try {
      let suggestionText = ''
      let assessmentText = ''

      await aiService.streamSuggestions(
        filePath,
        sessionId,
        chatMessages,
        ({ event, data }) => {
          if (event === 'progress') {
            chatStreamingMessage = data.message || 'Analyzing...'
          } else if (event === 'assessment') {
            assessmentText = data.text || ''
            chatStreamingMessage = `[DOCS] Assessment:\n${assessmentText}`
          } else if (event === 'strength') {
            chatStreamingMessage += `\n[SUCCESS] Strength: ${data.text}`
          } else if (event === 'suggestion') {
            const priority = data.priority === 'high' ? '[HIGH]' : data.priority === 'medium' ? '[MEDIUM]' : '[LOW]'
            const suggestion = `\n${priority} [${data.category}] ${data.issue}\n   → ${data.suggestion}\n   (Impact: ${data.impact})`
            suggestionText += suggestion
            chatStreamingMessage = assessmentText + '\n\nSuggestions:\n' + suggestionText
          } else if (event === 'complete') {
            if (!chatStreamingMessage) {
              chatStreamingMessage = assessmentText || 'Analysis complete.'
            }
          }
        },
        () => {},
        (error) => { throw error }
      )

      // Append assistant message
      if (chatStreamingMessage) {
        chatMessages = [...chatMessages, {
          role: 'assistant',
          content: chatStreamingMessage,
          timestamp: new Date().toISOString()
        }]
      }

      status = 'Suggestions ready'
      activeRightTab = 'suggestions'
    } catch (e) {
      console.error('Suggestions error:', e)
      status = `Suggestions error: ${e.message}`
    } finally {
      chatLoading = false
      chatStreamingMessage = ''
    }
  }

  function handleInsertTemplate(e) {
    const tpl = e.detail?.template
    if (!tpl?.content) return
    editorContent = tpl.content
    status = `Inserted template: ${tpl.name || 'Template'}`
    pushToast(status, 'success')
  }

  async function handleCodeReview(e) {
    if (!filePath) {
      status = 'Please select a file first'
      return
    }

    // Add system message to chat
    chatMessages = [...chatMessages, {
      role: 'system',
      content: 'Starting comprehensive code review...',
      timestamp: new Date().toISOString()
    }]

    chatLoading = true
    chatStreamingMessage = ''
    status = 'Performing code review...'

    try {
      let reviewScore = 0
      let summaryText = ''
      let sectionsText = ''
      let actionsText = ''

      await aiService.streamCodeReview(
        filePath,
        sessionId,
        ({ event, data }) => {
          if (event === 'progress') {
            chatStreamingMessage = data.message || 'Analyzing...'
          } else if (event === 'score') {
            reviewScore = data.score || 0
            chatStreamingMessage = `[SCORE] **Review Score: ${reviewScore}/100**\n\n`
          } else if (event === 'summary') {
            summaryText = data.text || ''
            chatStreamingMessage += `[NOTE] **Summary:**\n${summaryText}\n\n`
          } else if (event === 'section') {
            const section = `**${data.name}** (${data.rating})\n`
            let details = ''
            if (data.findings?.length) {
              details += `  Findings: ${data.findings.join(', ')}\n`
            }
            if (data.issues?.length) {
              details += `  Issues: ${data.issues.join(', ')}\n`
            }
            if (data.violations?.length) {
              details += `  Violations: ${data.violations.join(', ')}\n`
            }
            if (data.missing_packages?.length) {
              details += `  Missing: ${data.missing_packages.join(', ')}\n`
            }
            if (data.recommendations?.length) {
              details += `  Recommendations:\n`
              data.recommendations.forEach(r => {
                details += `    - ${r}\n`
              })
            }
            sectionsText += section + details + '\n'
            chatStreamingMessage = `[SCORE] **Review Score: ${reviewScore}/100**\n\n[SUMMARY] **Summary:**\n${summaryText}\n\n[ANALYSIS] **Detailed Analysis:**\n${sectionsText}`
          } else if (event === 'action') {
            const priority = data.priority === 'critical' ? '[CRITICAL]' : data.priority === 'high' ? '[HIGH]' : data.priority === 'medium' ? '[MEDIUM]' : '[LOW]'
            let action = `${priority} [${data.priority.toUpperCase()}] ${data.action}\n   → ${data.reason}\n`
            if (data.code) {
              action += `   \`${data.code}\`\n`
            }
            actionsText += action + '\n'
          } else if (event === 'complete') {
            if (actionsText) {
              chatStreamingMessage += `\n🎬 **Priority Actions:**\n${actionsText}`
            }
          }
        },
        () => {},
        (error) => { throw error }
      )

      // Append assistant message
      if (chatStreamingMessage) {
        chatMessages = [...chatMessages, {
          role: 'assistant',
          content: chatStreamingMessage,
          timestamp: new Date().toISOString()
        }]
      }

      status = 'Code review complete'
      activeRightTab = 'suggestions'
    } catch (e) {
      console.error('Code review error:', e)
      status = `Code review error: ${e.message}`
    } finally {
      chatLoading = false
      chatStreamingMessage = ''
    }
  }

  async function handleExplainErrors(e) {
    const { errors = [], warnings = [] } = e.detail || {}

    // Prepare diagnostics payload
    const diagnostics = [
      ...errors.map(err => ({ ...err, type: 'error' })),
      ...warnings.map(w => ({ ...w, type: 'warning' }))
    ]

    // Add a system message to chat
    chatMessages = [...chatMessages, {
      role: 'system',
      content: 'Analyzing compilation issues...',
      timestamp: new Date().toISOString()
    }]

    chatLoading = true
    chatStreamingMessage = ''
    status = 'AI explaining errors...'

    try {
      let explanationText = ''
      await aiService.streamExplainErrors(
        filePath,
        diagnostics,
        chatMessages,
        sessionId,
        ({ event, data }) => {
          if (event === 'progress') {
            chatStreamingMessage = data.message || 'Analyzing...'
          } else if (event === 'explain') {
            const causes = (data.causes || []).map(c => `- ${c}`).join('\n')
            explanationText += `${data.text || ''}\n${causes ? '\nLikely causes:\n' + causes + '\n' : ''}`
            chatStreamingMessage = explanationText
          } else if (event === 'step') {
            const cmd = data.command || ''
            const rationale = data.rationale || ''
            chatStreamingMessage += `${rationale ? rationale + '\n' : ''}\n\n\`\`\`latex\n${cmd}\n\`\`\`\n`
          } else if (event === 'complete') {
            if (!chatStreamingMessage) {
              chatStreamingMessage = explanationText || 'Explanation complete.'
            }
          }
        },
        () => {},
        (error) => { throw error }
      )

      // Append assistant message
      if (chatStreamingMessage) {
        chatMessages = [...chatMessages, {
          role: 'assistant',
          content: chatStreamingMessage,
          timestamp: new Date().toISOString()
        }]
      }

      status = 'AI explanation ready'
      activeRightTab = 'errors'
    } catch (e) {
      console.error('Explain errors error:', e)
      status = `Explain errors error: ${e.message}`
    } finally {
      chatLoading = false
      chatStreamingMessage = ''
    }
  }

  async function autoTriggerSuggestions() {
    if (!autoSuggest || !filePath) return
    
    try {
      status = 'Analyzing document for suggestions...'
      chatStreamingMessage = ''

      await aiService.streamSuggestions(
        filePath,
        sessionId,
        chatMessages,
        ({ event, data }) => {
          if (event === 'progress') {
            chatStreamingMessage = data.message || 'Analyzing...'
          } else if (event === 'assessment') {
            const assessment = data.text || data.assessment || ''
            chatStreamingMessage = `[ANALYSIS] **Analysis:** ${assessment}`
          } else if (event === 'suggestion') {
            const suggestion = data.suggestion || data.text || ''
            if (suggestion) {
              chatStreamingMessage += `\n[IDEA] ${suggestion}`
            }
          } else if (event === 'complete' && !chatStreamingMessage) {
            chatStreamingMessage = 'Analysis complete.'
          }
        },
        () => {},
        (error) => { throw error }
      )

      // Add auto-triggered suggestion to chat as assistant message
      if (chatStreamingMessage) {
        chatMessages = [...chatMessages, {
          role: 'assistant',
          content: chatStreamingMessage.trim(),
          timestamp: new Date().toISOString(),
          isAutoTriggered: true
        }]
        chatStreamingMessage = ''
      }

      status = 'Auto-analysis complete'
    } catch (e) {
      console.error('Auto-suggest error:', e)
      // Silently fail - don't disrupt user experience
    } finally {
      chatStreamingMessage = ''
    }
  }

  function scheduleAutoSuggestions() {
    // Debounced auto-suggestions trigger after file load or user pause
    if (suggestionsDebounceTimer) clearTimeout(suggestionsDebounceTimer)
    
    suggestionsDebounceTimer = setTimeout(() => {
      autoTriggerSuggestions()
    }, 3000)  // 3 seconds after compile finishes
  }

  function handleEditorContentChanged(e) {
    const { isDirty } = e.detail
    
    // Auto-compile with debounce (wait 2 seconds after user stops typing)
    if (autoCompile && isDirty && filePath && !compiling) {
      if (compileDebounceTimer) clearTimeout(compileDebounceTimer)
      
      compileDebounceTimer = setTimeout(() => {
        compileDocument()
      }, 2000)  // Wait 2 seconds after typing stops
    }
  }

  async function compileDocument() {
    if (!filePath) {
      status = 'Please select a file first'
      return
    }

    // If a compile is already running, queue the latest request and exit.
    if (compiling) {
      compileQueued = true
      return
    }

    compileQueued = false
    compileController?.abort()
    compileController = new AbortController()

    compiling = true
    compileErrors = []
    compileWarnings = []
    status = 'Submitting compilation job...'
    activeRightTab = 'pdf'  // Switch to PDF tab
    pushToast('Compilation started', 'info')

    try {
      // Submit async compilation job
      const data = await compileService.startCompilation(filePath, 'pdflatex', compileController.signal)

      // Pass job ID to PdfViewer for status polling
      compilationJobId = data.job_id
      status = 'Compilation queued...'
      
      // PdfViewer will handle polling and updating status
    } catch (e) {
      if (e?.name === 'AbortError') {
        status = 'Compilation superseded by newer request'
      } else {
        status = `Compilation error: ${e.message}`
        compileErrors = [{ message: e.message }]
        activeRightTab = 'errors'
      }
      compiling = false
    } finally {
      compileController = null
      compiling = false
      if (compileQueued) {
        compileQueued = false
        // Run the latest queued compile once current finishes/aborts.
        compileDocument()
      }
    }
  }

  async function toggleHistory() {
    if (!showHistory) {
      await loadHistorySidebar()
    }
    showHistory = !showHistory
  }

  async function restoreHistoryEntry(id) {
    try {
      status = 'Restoring selected version...'
      await fileService.restoreHistoryEntry(id, filePath)
      await loadFile()
      status = 'History entry restored'
      pushToast('History entry restored', 'success')
    } catch (e) {
      status = `Failed to restore history: ${e.message}`
      pushToast(`Failed to restore history: ${e.message}`, 'error')
    }
  }

  function handleFileTreeSelection(e) {
    selectedFiles = e.detail
  }

  function handleFileClick(e) {
    filePath = e.detail
    loadFile()
    rememberRecent(filePath)
  }

  function handleDirtyChange(e) {
    isDirty = e.detail?.isDirty
  }
  
  function handleSaveConflict(e) {
    // Show conflict dialog when save fails due to concurrent edit
    conflictData = e.detail
    showConflictDialog = true
    pushToast('File conflict detected', 'warning')
  }
  
  async function resolveConflict(action) {
    if (!conflictData) return
    
    if (action === 'overwrite') {
      // Force save by updating ETag to server's version first
      if (editorRef && editorRef.currentETag !== undefined) {
        editorRef.currentETag = conflictData.serverETag
        await editorRef.save()
        pushToast('Changes saved (overwrite)', 'success')
      }
    } else if (action === 'reload') {
      // Discard local changes and reload from server
      await loadFile()
      pushToast('File reloaded from server', 'info')
    }
    
    showConflictDialog = false
    conflictData = null
  }

  async function handleJumpToLine(e) {
    const { line, file } = e.detail || {}
    if (!line) return

    const targetFile = file || filePath
    if (!targetFile) return

    if (targetFile !== filePath) {
      filePath = targetFile
      await loadFile()
    }

    if (editorRef && typeof editorRef.goToLine === 'function') {
      // Small delay ensures Monaco is ready after file load
      setTimeout(() => editorRef.goToLine(line), 10)
    }
  }

  // Handle keyboard shortcuts from KeyboardShortcuts component
  function handleShortcut(e) {
    const { action } = e.detail

    switch (action) {
      case 'save':
        // Save current file (could be implemented in Editor component)
        if (editorRef && typeof editorRef.save === 'function') {
          editorRef.save()
        }
        break
      case 'apply_command':
        // Focus command palette (would need to expose palette ref)
        if (commandPaletteRef && typeof commandPaletteRef.focus === 'function') {
          commandPaletteRef.focus()
        }
        break
      case 'toggle_history':
        // Toggle history sidebar
        toggleHistory()
        break
      case 'toggle_file_tree':
        // Toggle file tree
        showFileTree = !showFileTree
        break
      case 'quick_open':
        showFileOpener = true
        break
      case 'search':
        showSearch = !showSearch
        break
      default:
        break
    }
  }

  // Handle theme change
  async function handleThemeChange(theme) {
    await configService
      .updateConfigField('theme.mode', theme)
      .catch(e => console.error('Error saving theme:', e))
  }

  // Handle settings save
  async function handleSettingsSave(updates) {
    try {
      await configService.saveConfig(updates)
      status = 'Settings saved'
      // Reload config to pick up changes
      await loadConfig()
    } catch (e) {
      status = `Error saving settings: ${e.message}`
    }
  }

  // Load config on mount
  onMount(() => {
    loadConfig()
    const beforeUnload = (event) => {
      if (isDirty) {
        event.preventDefault()
        event.returnValue = ''
      }
    }
    window.addEventListener('beforeunload', beforeUnload)
    return () => window.removeEventListener('beforeunload', beforeUnload)
  })

  function rememberRecent(path) {
    recentFiles = [path, ...recentFiles.filter(p => p !== path)].slice(0, 12)
    localStorage.setItem('recent_files', JSON.stringify(recentFiles))
  }

  function togglePin(path) {
    if (pinnedFiles.includes(path)) {
      pinnedFiles = pinnedFiles.filter(p => p !== path)
    } else {
      pinnedFiles = [path, ...pinnedFiles].slice(0, 20)
    }
    localStorage.setItem('pinned_files', JSON.stringify(pinnedFiles))
  }

  function handleOpenFromPalette(path) {
    showFileOpener = false
    filePath = path
    loadFile()
    rememberRecent(path)
  }
  
  function handleSearchOpen(detail) {
    const { file, lineNumber } = detail
    
    // Load the file
    filePath = file
    loadFile().then(() => {
      // Jump to line after file loads
      if (editorRef && lineNumber) {
        setTimeout(() => {
          editorRef.goToLine(lineNumber)
        }, 100)
      }
    })
    
    // Close search panel
    showSearch = false
  }

  function pushToast(message, type = 'info') {
    const id = ++toastId
    toasts = [...toasts, { id, message, type }]
    setTimeout(() => {
      toasts = toasts.filter(t => t.id !== id)
    }, 4000)
  }

  // Apply selected diff hunks
  function applyHunks(baseText, hunks, selection) {
    const baseLines = baseText.split('\n')
    const output = []
    let cursor = 0

    const parseHeader = (header) => {
      const m = /@@ -([0-9]+)(?:,([0-9]+))? \+([0-9]+)(?:,([0-9]+))? @@/.exec(header)
      if (!m) return null
      return {
        startOld: parseInt(m[1], 10),
        lenOld: m[2] ? parseInt(m[2], 10) : 1
      }
    }

    for (let i = 0; i < hunks.length; i++) {
      const hunk = hunks[i]
      const meta = parseHeader(hunk.header)
      if (!meta) continue
      const startIdx = meta.startOld - 1
      // add untouched lines before hunk
      while (cursor < startIdx) {
        output.push(baseLines[cursor] ?? '')
        cursor++
      }

      if (!selection[i]) {
        // keep original segment
        for (let k = 0; k < meta.lenOld; k++) {
          output.push(baseLines[cursor] ?? '')
          cursor++
        }
        continue
      }

      // apply hunk
      for (const line of hunk.lines) {
        if (line.type === 'add') {
          output.push(line.text)
        } else if (line.type === 'remove') {
          cursor++
        } else {
          output.push(baseLines[cursor] ?? '')
          cursor++
        }
      }
    }

    // append tail
    while (cursor < baseLines.length) {
      output.push(baseLines[cursor] ?? '')
      cursor++
    }

    return output.join('\n')
  }

  async function handleApplyHunks(e) {
    const { hunks, selected } = e.detail || {}
    if (!hunks || !selected) {
      pushToast('No hunks to apply', 'error')
      return
    }
    if (!lastDiffBase) {
      pushToast('No baseline for patch; re-run command with dry run', 'error')
      return
    }

    const patched = applyHunks(lastDiffBase, hunks, selected)
    editorContent = patched
    status = 'Applying selected changes...'
    try {
      await fileService.saveFile(filePath, patched, editorRef?.currentETag)
      status = 'Selected hunks applied'
      pushToast('Selected changes applied', 'success')
    } catch (err) {
      if (err.status === 409) {
        conflictData = err.conflictData
        showConflictDialog = true
        status = 'Patch failed: conflict detected'
        pushToast('Patch conflict detected', 'warning')
      } else {
        status = `Patch failed: ${err.message}`
        pushToast(`Patch failed: ${err.message}`, 'error')
      }
    }
  }

  function handleRejectAll() {
    diff = ''
    pushToast('Changes discarded', 'info')
  }
</script>

<div class="app" role="application" aria-label="LaTeX AI Editor">
  <!-- Keyboard shortcuts handler (invisible) -->
  <KeyboardShortcuts {shortcuts} on:shortcut={handleShortcut} />

  <div class="sidebar" role="navigation" aria-label="Document navigation">
    {#if showHistory && !showFileTree && historyLoaded && HistorySidebar}
      <svelte:component this={HistorySidebar}
        {filePath}
        agentOnly={true}
        on:restore={e => restoreHistoryEntry(e.detail)}
      />
    {/if}
    {#if showFileTree}
      <FileTree
        on:selection-changed={handleFileTreeSelection}
        on:file-selected={handleFileClick}
      />
    {/if}
  </div>

  <div class="main" role="main">
    <div class="top-bar" role="toolbar" aria-label="Document controls">
      <label for="file-path-input" class="sr-only">File path</label>
      <input
        id="file-path-input"
        type="text"
        placeholder="File path..."
        bind:value={filePath}
        on:change={loadFile}
        class="flex-1 px-3 py-2 border rounded"
        aria-label="Current file path"
      />
      {#if isDirty}
        <span aria-live="polite" class="text-red-600" title="Unsaved changes">•</span>
      {/if}
      <button
        on:click={() => { showFileTree = !showFileTree; showHistory = false }}
        class="ml-2 px-4 py-2 bg-blue-500 text-white rounded hover:bg-blue-600"
        aria-label={showFileTree ? 'Hide file tree' : 'Show file tree'}
        aria-pressed={showFileTree}
      >
        {showFileTree ? 'Hide Tree' : 'Files'}
      </button>
      <button
        on:click={() => { showHistory = !showHistory; showFileTree = false }}
        class="ml-2 px-4 py-2 bg-blue-500 text-white rounded hover:bg-blue-600"
        aria-label={showHistory ? 'Hide history' : 'Show file history'}
        aria-pressed={showHistory}
      >
        {showHistory ? 'Hide' : 'History'}
      </button>
      <button
        on:click={compileDocument}
        disabled={!filePath || compiling}
        class="ml-2 px-4 py-2 bg-green-500 text-white rounded hover:bg-green-600 disabled:bg-gray-400 disabled:cursor-not-allowed"
        title="Compile to PDF (Ctrl+Enter)"
        aria-busy={compiling}
        aria-label={compiling ? 'Compiling document' : 'Compile document to PDF'}
      >
        {compiling ? '⏳ Compiling...' : '📄 Compile'}
      </button>
      <button
        on:click={async () => { 
          if (!showSettings) await loadSettingsPanel()
          showSettings = !showSettings 
        }}
        class="ml-2 px-4 py-2 bg-gray-500 text-white rounded hover:bg-gray-600"
        aria-label={showSettings ? 'Close settings' : 'Open settings'}
        aria-pressed={showSettings}
      >
        ⚙️ Settings
      </button>
      {#if selectedFiles.length > 0}
        <span 
          class="ml-2 px-3 py-2 bg-green-100 text-green-800 rounded"
          role="status"
          aria-live="polite"
          aria-atomic="true"
        >
          {selectedFiles.length} {selectedFiles.length === 1 ? 'file' : 'files'} selected
        </span>
      {/if}
      <div class="ml-auto">
        <ThemeToggle onThemeChange={handleThemeChange} />
      </div>
    </div>

    <div class="content-wrapper" class:overleaf-layout={layoutMode === 'overleaf'} class:fullscreen-editor={layoutMode === 'fullscreen-editor'} class:fullscreen-pdf={layoutMode === 'fullscreen-pdf'} bind:this={contentWrapperRef}>
      <div class="editor-section" style={layoutMode === 'overleaf' ? `height: ${editorHeightPercent}%` : ''}>
        <div class="editor-header">
          <span class="section-label">[NOTE] Editor</span>
          <div class="editor-controls">
            <button 
              on:click={() => layoutMode = layoutMode === 'fullscreen-editor' ? 'overleaf' : 'fullscreen-editor'}
              title="Toggle fullscreen"
              class="control-btn"
            >
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
            autoSaveDelay={autoSaveInterval * 1000}
            editorSettings={editorSettings}
            theme={themeSettings?.mode || 'light'}
            on:saved={() => pushToast('Saved', 'success')}
            on:save-error={(e) => pushToast(`Save failed: ${e.detail?.error || 'Unknown'}`, 'error')}
            on:dirty-change={handleDirtyChange}
            on:content-changed={handleEditorContentChanged}
            on:save-conflict={handleSaveConflict}
          />
        </div>
      </div>

      {#if layoutMode === 'overleaf'}
        <button 
          type="button"
          class="divider"
          class:dragging={isDraggingDivider}
          aria-label="Resize editor and preview panels"
          on:mousedown={handleDividerMouseDown}
          on:keydown={(e) => (e.key === 'Enter' || e.key === ' ') && handleDividerMouseDown(e)}
        ></button>
        
        <div class="pdf-section" style={layoutMode === 'overleaf' ? `height: ${100 - editorHeightPercent}%` : ''}>
          <div class="pdf-header">
            <span class="section-label">📄 Preview</span>
            <div class="pdf-controls">
              <button 
                on:click={() => layoutMode = 'fullscreen-pdf'}
                title="Toggle fullscreen"
                class="control-btn"
              >
                ↗️
              </button>
            </div>
          </div>
          <div class="pdf-pane">
            {#if bulkMode}
              <div class="bulk-progress">
                <div class="progress-header">Applying command to {bulkTotal} files...</div>
                <div class="progress-bar">
                  <div class="progress-fill" style="width: {(bulkProgress / bulkTotal) * 100}%"></div>
                </div>
                <div class="progress-text">{bulkProgress} / {bulkTotal}</div>
              </div>
            {:else}
              <RightPanel
                {diff}
                {pdfUrl}
                {pdfPages}
                {compileErrors}
                {compileWarnings}
                {compiling}
                {aiPlan}
                jobId={compilationJobId}
                bind:activeTab={activeRightTab}
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
              />
            {/if}
          </div>
        </div>
      {/if}
    </div>

    <!-- Chat Panel (bottom, always visible when showChatPanel) -->
    {#if showChatPanel && layoutMode === 'overleaf'}
      <button 
        type="button"
        class="chat-divider"
        class:dragging={isDraggingDivider}
        aria-label="Resize chat panel"
        on:mousedown={handleChatDividerMouseDown}
        on:keydown={(e) => (e.key === 'Enter' || e.key === ' ') && handleChatDividerMouseDown(e)}
      ></button>
      <div class="chat-section" style="height: {chatHeightPercent}px">
        <div class="chat-header">
          <span class="section-label">💬 AI Assistant</span>
          <div class="chat-controls">
            <button 
              on:click={() => showChatPanel = !showChatPanel}
              title="Toggle chat panel"
              class="control-btn"
            >
              {showChatPanel ? '▼' : '▲'}
            </button>
          </div>
        </div>
        <ChatPanel
          messages={chatMessages}
          loading={chatLoading}
          streamingMessage={chatStreamingMessage}
          on:message-sent={handleChatMessage}
        />
      </div>
    {/if}

    <CommandPalette
      bind:this={commandPaletteRef}
      {dryRun}
      bulkMode={selectedFiles.length > 0}
      selectedCount={selectedFiles.length}
      on:command={e => bulkMode ? applyBulkCommand(e.detail) : applyCommand(e.detail)}
      on:ai-plan={e => requestAIPlan(e.detail)}
      on:toggle-dry-run={() => dryRun = !dryRun}
    />

    <StatusBar {status} />
    <Toasts {toasts} />
    
    <!-- Conflict Resolution Dialog -->
    {#if showConflictDialog && conflictData}
      <div class="conflict-dialog-overlay" role="dialog" aria-modal="true" aria-labelledby="conflict-title">
        <div class="conflict-dialog">
          <h2 id="conflict-title">File Conflict Detected</h2>
          <p>The file has been modified by another process. How would you like to proceed?</p>
          
          <div class="conflict-actions">
            <button
              class="conflict-btn conflict-btn-primary"
              on:click={() => resolveConflict('overwrite')}
            >
              Overwrite Server Version
            </button>
            <button
              class="conflict-btn conflict-btn-secondary"
              on:click={() => resolveConflict('reload')}
            >
              Reload from Server
            </button>
            <button
              class="conflict-btn conflict-btn-cancel"
              on:click={() => {
                showConflictDialog = false
                conflictData = null
              }}
            >
              Cancel
            </button>
          </div>
        </div>
      </div>
    {/if}
  </div>

  <!-- Settings Panel -->
  {#if showSettings && settingsLoaded && SettingsPanel}
    <svelte:component this={SettingsPanel}
      isOpen={showSettings}
      onClose={() => {
        showSettings = false
      }}
      onSave={handleSettingsSave}
    />
  {/if}

  <FileOpener
    visible={showFileOpener}
    recent={recentFiles}
    pinned={pinnedFiles}
    on:close={() => showFileOpener = false}
    on:open={(e) => handleOpenFromPalette(e.detail.path)}
    on:pin={(e) => togglePin(e.detail.path)}
    on:unpin={(e) => togglePin(e.detail.path)}
  />
  
  <SearchResults
    bind:visible={showSearch}
    bind:searchQuery={searchQuery}
    bind:results={searchResults}
    bind:loading={searchLoading}
    on:open={(e) => handleSearchOpen(e.detail)}
    on:close={() => showSearch = false}
  />
</div>

<style>
  :global(body) {
    margin: 0;
    padding: 0;
  }

  .app {
    display: flex;
    height: 100vh;
    background: #f5f5f5;
  }

  .sidebar {
    width: 280px;
    background: #fff;
    border-right: 1px solid #ddd;
    overflow-y: auto;
  }

  .main {
    flex: 1;
    display: flex;
    flex-direction: column;
  }

  .top-bar {
    padding: 1rem;
    background: #fff;
    border-bottom: 1px solid #ddd;
    display: flex;
    gap: 0.5rem;
    align-items: center;
  }

  .content {
    flex: 1;
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0;
  }

  .editor-pane,
  .right-pane {
    overflow: auto;
    border-right: 1px solid #ddd;
  }

  .right-pane {
    border-right: none;
    background: #fafafa;
  }

  /* New Overleaf-style layout */
  .content-wrapper {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 0;
  }

  .content-wrapper.overleaf-layout {
    flex-direction: column;
  }

  .editor-section,
  .pdf-section {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .pdf-section {
    border-top: 1px solid #ddd;
  }

  .editor-header,
  .pdf-header,
  .chat-header {
    padding: 0.75rem 1rem;
    background: #f0f0f0;
    border-bottom: 1px solid #ddd;
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-weight: 600;
    font-size: 13px;
    color: #333;
  }

  .section-label {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .editor-controls,
  .pdf-controls,
  .chat-controls {
    display: flex;
    gap: 0.5rem;
  }

  .control-btn {
    padding: 0.25rem 0.5rem;
    background: transparent;
    border: 1px solid #ccc;
    border-radius: 3px;
    cursor: pointer;
    font-size: 12px;
    transition: all 200ms;
  }

  .control-btn:hover {
    background: #fff;
    border-color: #999;
  }

  .editor-pane,
  .pdf-pane {
    flex: 1;
    overflow: auto;
  }

  .divider {
    width: 100%;
    height: 8px;
    background: linear-gradient(to bottom, #ddd, #f0f0f0);
    border: 1px solid #ccc;
    cursor: row-resize;
    transition: background 200ms;
    user-select: none;
    position: relative;
  }

  .divider:hover {
    background: linear-gradient(to bottom, #999, #ddd);
  }

  .divider.dragging {
    background: linear-gradient(to bottom, #0078d4, #005a9e);
    box-shadow: 0 2px 8px rgba(0, 120, 212, 0.3);
  }

  .chat-divider {
    width: 100%;
    height: 8px;
    background: linear-gradient(to bottom, #ddd, #f0f0f0);
    border: 1px solid #ccc;
    cursor: row-resize;
    transition: background 200ms;
    user-select: none;
  }

  .chat-divider:hover {
    background: linear-gradient(to bottom, #999, #ddd);
  }

  .chat-divider.dragging {
    background: linear-gradient(to bottom, #0078d4, #005a9e);
    box-shadow: 0 -2px 8px rgba(0, 120, 212, 0.3);
  }

  .chat-section {
    border-top: 1px solid #ddd;
    display: flex;
    flex-direction: column;
    background: #fff;
    overflow: hidden;
  }

  /* Fullscreen modes */
  .content-wrapper.fullscreen-editor {
    grid-template-columns: 1fr;
  }

  .content-wrapper.fullscreen-editor .pdf-section,
  .content-wrapper.fullscreen-pdf .editor-section {
    display: none;
  }

  .content-wrapper.fullscreen-pdf .chat-section {
    display: none;
  }

  /* Responsive tweaks */
  @media (max-width: 1024px) {
    .content-wrapper.overleaf-layout {
      grid-template-columns: 1fr;
    }

    .pdf-section {
      max-height: 40%;
    }

    .editor-section {
      max-height: 60%;
    }
  }

  .bulk-progress {
    padding: 2rem;
    text-align: center;
  }

  .progress-header {
    font-weight: bold;
    margin-bottom: 1rem;
    color: #333;
  }

  .progress-bar {
    width: 100%;
    height: 24px;
    background: #e0e0e0;
    border-radius: 4px;
    overflow: hidden;
    margin-bottom: 0.5rem;
  }

  .progress-fill {
    height: 100%;
    background: linear-gradient(90deg, #007bff, #0056b3);
    transition: width 0.3s ease;
  }

  .progress-text {
    color: #666;
    font-size: 13px;
  }
  
  /* Conflict dialog styles */
  .conflict-dialog-overlay {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 10000;
  }
  
  .conflict-dialog {
    background: white;
    border-radius: 8px;
    padding: 2rem;
    max-width: 500px;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
  }
  
  .conflict-dialog h2 {
    margin: 0 0 1rem 0;
    font-size: 1.5rem;
    color: #333;
  }
  
  .conflict-dialog p {
    margin: 0 0 1.5rem 0;
    color: #666;
    line-height: 1.5;
  }
  
  .conflict-actions {
    display: flex;
    gap: 0.75rem;
    justify-content: flex-end;
  }
  
  .conflict-btn {
    padding: 0.5rem 1rem;
    border: none;
    border-radius: 4px;
    cursor: pointer;
    font-size: 0.95rem;
    transition: background 0.2s;
  }
  
  .conflict-btn-primary {
    background: #dc3545;
    color: white;
  }
  
  .conflict-btn-primary:hover {
    background: #c82333;
  }
  
  .conflict-btn-secondary {
    background: #007bff;
    color: white;
  }
  
  .conflict-btn-secondary:hover {
    background: #0056b3;
  }
  
  .conflict-btn-cancel {
    background: #6c757d;
    color: white;
  }
  
  .conflict-btn-cancel:hover {
    background: #545b62;
  }
</style>
