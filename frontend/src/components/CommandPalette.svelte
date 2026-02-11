<script>
  import { createEventDispatcher, onMount } from 'svelte'
  import Fuse from 'fuse.js'

  const dispatch = createEventDispatcher()

  export let bulkMode = false
  export let selectedCount = 0
  export let dryRun = false

  let commandInput = ''
  let inputElement  // Reference to the input element
  let selectedCommand = null
  let suggestions = []
  let selectedIndex = -1
  let useAI = false
  let modelsLoading = false
  let availableModels = []
  let selectedModelId = ''
  let recentCommands = JSON.parse(localStorage.getItem('recent_commands') || '[]')
  let commandFrequency = JSON.parse(localStorage.getItem('command_frequency') || '{}')
  
  $: selectedModel = availableModels.find(m => m.id === selectedModelId)

  // OS-reserved keyboard shortcuts that should not be overridden
  const RESERVED_SHORTCUTS = [
    'Ctrl+S', 'Ctrl+C', 'Ctrl+V', 'Ctrl+X', 'Ctrl+Z', 'Ctrl+Y',
    'Ctrl+A', 'Ctrl+F', 'Ctrl+P', 'Ctrl+W', 'Ctrl+T', 'Ctrl+N',
    'Ctrl+Q', 'Alt+F4', 'Cmd+S', 'Cmd+C', 'Cmd+V', 'Cmd+X',
    'Cmd+Z', 'Cmd+Y', 'Cmd+A', 'Cmd+F', 'Cmd+P', 'Cmd+W',
    'Cmd+T', 'Cmd+N', 'Cmd+Q', 'F5', 'F11', 'F12'
  ]

  /**
   * Validate keyboard shortcut to prevent overriding OS/browser shortcuts
   * @param {KeyboardEvent} e - Keyboard event
   * @returns {boolean} True if shortcut is allowed, false if reserved
   */
  function isShortcutAllowed(e) {
    const key = e.key
    const ctrl = e.ctrlKey || e.metaKey
    const alt = e.altKey
    const shift = e.shiftKey
    
    // Build shortcut string
    let shortcut = ''
    if (ctrl) shortcut += (e.metaKey ? 'Cmd+' : 'Ctrl+')
    if (alt) shortcut += 'Alt+'
    if (shift) shortcut += 'Shift+'
    shortcut += key.toUpperCase()
    
    // Check if reserved
    if (RESERVED_SHORTCUTS.includes(shortcut)) {
      console.warn(`Shortcut ${shortcut} is reserved by the OS/browser`)
      return false
    }
    
    // Store in localStorage for custom shortcuts
    const customShortcuts = JSON.parse(localStorage.getItem('custom_shortcuts') || '{}')
    return true
  }

  const allCommands = [
    { name: 'expand-macros', description: 'Expand all macros', tags: ['editing', 'macros'] },
    { name: 'fix-citations', description: 'Fix citation formatting', tags: ['citations', 'references'] },
    { name: 'add-labels', description: 'Add figure and table labels', tags: ['references', 'labels'] },
    { name: 'remove-comments', description: 'Remove all comments', tags: ['cleanup', 'formatting'] },
    { name: 'fix-spacing', description: 'Fix inconsistent spacing', tags: ['formatting', 'whitespace'] },
    { name: 'fix-math-mode', description: 'Fix math mode issues', tags: ['math', 'formatting'] },
    { name: 'convert-to-documentclass', description: 'Convert document class', tags: ['document', 'structure'] },
    { name: 'add-packages', description: 'Add LaTeX packages', tags: ['packages', 'dependencies'] }
  ]

  let fuse

  onMount(async () => {
    // Initialize fuzzy search
    fuse = new Fuse(allCommands, {
      keys: ['name', 'description', 'tags'],
      threshold: 0.4,
      includeScore: true
    })

    try {
      modelsLoading = true
      const res = await fetch('/api/models')
      if (res.ok) {
        const data = await res.json()
        availableModels = data.available || []
        selectedModelId = data.current || (availableModels[0] ? availableModels[0].id : '')
      }
    } catch (e) {
      console.error('Failed to load models', e)
    } finally {
      modelsLoading = false
    }
  })

  function searchCommands(query) {
    if (!query) {
      // Show recent first, then frequent, then remaining
      const recent = allCommands.filter(c => recentCommands.includes(c.name))
        .sort((a, b) => recentCommands.indexOf(a.name) - recentCommands.indexOf(b.name))
      
      const frequent = allCommands
        .filter(c => !recentCommands.includes(c.name) && (commandFrequency[c.name] || 0) > 0)
        .sort((a, b) => (commandFrequency[b.name] || 0) - (commandFrequency[a.name] || 0))
        .slice(0, 5)
      
      const others = allCommands
        .filter(c => !recentCommands.includes(c.name) && !(commandFrequency[c.name] > 0))
        .slice(0, 3)
      
      return [...recent, ...frequent, ...others].slice(0, 10)
    }
    
    return fuse.search(query).map(result => result.item)
  }

  function onInput(e) {
    commandInput = e.target.value
    suggestions = searchCommands(commandInput)
    selectedIndex = -1
  }

  function selectCommand(cmd) {
    // Track usage
    recentCommands = [cmd.name, ...recentCommands.filter(c => c !== cmd.name)].slice(0, 10)
    commandFrequency[cmd.name] = (commandFrequency[cmd.name] || 0) + 1
    
    localStorage.setItem('recent_commands', JSON.stringify(recentCommands))
    localStorage.setItem('command_frequency', JSON.stringify(commandFrequency))
    
    commandInput = cmd.name
    suggestions = []
    selectedIndex = -1
    dispatch('command', cmd.name)
    commandInput = ''
  }

  function handleSubmit(e) {
    if (commandInput.trim()) {
      if (useAI) {
        dispatch('ai-plan', {
          request: commandInput,
          model: selectedModelId
        })
      } else {
        // Track as custom command
        commandFrequency[commandInput] = (commandFrequency[commandInput] || 0) + 1
        localStorage.setItem('command_frequency', JSON.stringify(commandFrequency))
        
        dispatch('command', commandInput)
      }
      commandInput = ''
      suggestions = []
      selectedIndex = -1
    }
  }

  function handleKeyDown(e) {
    // Validate shortcuts before processing
    if ((e.ctrlKey || e.metaKey || e.altKey) && e.key.length === 1) {
      if (!isShortcutAllowed(e)) {
        // Don't prevent default for reserved shortcuts
        return
      }
    }
    
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      selectedIndex = Math.min(selectedIndex + 1, suggestions.length - 1)
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      selectedIndex = Math.max(selectedIndex - 1, -1)
    } else if (e.key === 'Enter') {
      // Three-level keyboard shortcuts for command flow:
      // 1. Enter: Preview diff (open diff view without applying)
      // 2. Shift+Enter: Preview with auto-focus Apply button
      // 3. Ctrl+Enter: Apply immediately (skip diff view)
      
      if (selectedIndex >= 0 && suggestions[selectedIndex]) {
        selectCommand(suggestions[selectedIndex])
      } else if (commandInput.trim()) {
        if (e.ctrlKey) {
          // Ctrl+Enter: Apply immediately (skip preview)
          dispatch('apply-immediate', {
            request: commandInput,
            model: selectedModelId,
            useAI
          })
        } else if (e.shiftKey) {
          // Shift+Enter: Preview + auto-focus Apply button
          if (useAI) {
            dispatch('ai-plan', {
              request: commandInput,
              model: selectedModelId,
              autoFocus: 'apply'
            })
          } else {
            dispatch('command-preview', {
              command: commandInput,
              autoFocus: 'apply'
            })
          }
        } else {
          // Enter: Normal preview (default behavior)
          if (useAI) {
            dispatch('ai-plan', {
              request: commandInput,
              model: selectedModelId
            })
          } else {
            dispatch('command', commandInput)
          }
        }
        commandInput = ''
        suggestions = []
        selectedIndex = -1
      }
    } else if (e.key === 'Escape') {
      suggestions = []
      selectedIndex = -1
    }
  }

  // Export focus method for parent components
  export function focus() {
    if (inputElement) {
      inputElement.focus()
    }
  }
</script>

<div class="command-palette">
  <div class="palette-info">
    {#if bulkMode}
      <span class="bulk-indicator">[PACKAGE] Bulk mode: {selectedCount} files</span>
    {/if}
    <div class="keyboard-hints">
      <span title="Preview changes" class="hint">↩ Preview</span>
      <span title="Preview with Apply focused" class="hint">⇧↩ Apply-ready</span>
      <span title="Apply immediately" class="hint">⌃↩ Execute</span>
    </div>
  </div>
  <div class="input-wrapper" role="combobox" aria-expanded={suggestions.length > 0} aria-owns="command-suggestions" aria-haspopup="listbox" aria-controls="command-suggestions">
    <input
      bind:this={inputElement}
      type="text"
      placeholder={bulkMode ? `Enter command for ${selectedCount} files...` : (useAI ? "Describe changes for AI plan..." : "Enter command (e.g., 'expand-macros')...")}
      bind:value={commandInput}
      on:input={onInput}
      on:keydown={handleKeyDown}
      class="command-input"
      aria-label={bulkMode ? `Command input for ${selectedCount} files` : (useAI ? 'AI plan request input' : 'Command input')}
      aria-controls="command-suggestions"
    />
    <button
      on:click={handleSubmit}
      class="execute-btn"
      title={bulkMode ? `Apply to ${selectedCount} files` : 'Execute on current file'}
      aria-label={bulkMode ? `Execute command on ${selectedCount} files` : 'Execute command'}
    >
      {bulkMode ? `Bulk` : `Execute`}
    </button>
    <label class="dry-run-label">
      <input
        type="checkbox"
        bind:checked={dryRun}
        on:change={() => dispatch('toggle-dry-run')}
        aria-label="Toggle dry run mode"
      />
      Dry Run
    </label>
    
    <div class="model-display" title={selectedModel ? `Current model: ${selectedModel.label}` : 'Model: Loading...'}>
      <span class="model-label">Model:</span>
      {#if modelsLoading}
        <span class="model-name loading">Loading...</span>
      {:else if selectedModel}
        <span class="model-name">{selectedModel.label}</span>
        {#if selectedModel.tier}
          <span class="model-tier">{selectedModel.tier}</span>
        {/if}
      {:else}
        <span class="model-name">Default</span>
      {/if}
    </div>
    
    <label class="dry-run-label">
      <input
        type="checkbox"
        bind:checked={useAI}
        aria-label="Use AI planner"
      />
      Use AI
    </label>
    
    {#if useAI}
      <div class="model-select-wrapper">
        <div class="model-select">
          {#if modelsLoading}
            <span class="loading">Loading models...</span>
          {:else}
            <select
              bind:value={selectedModelId}
              aria-label="Select AI model for this run"
            >
              {#each availableModels as m}
                <option value={m.id}>{m.label} {m.tier ? `(${m.tier})` : ''}</option>
              {/each}
            </select>
          {/if}
        </div>
        {#if selectedModel}
          <div class="capabilities" aria-label="Model capabilities">
            {#if selectedModel.supports?.code}
              <span class="cap-pill">Code</span>
            {/if}
            {#if selectedModel.supports?.long_context}
              <span class="cap-pill">Long Context</span>
            {/if}
            {#if selectedModel.supports?.json}
              <span class="cap-pill">JSON</span>
            {/if}
            {#if selectedModel.provider}
              <span class="cap-pill provider">{selectedModel.provider}</span>
            {/if}
            {#if selectedModel.cost}
              <span class="cap-pill cost" title="Approximate cost">{selectedModel.cost}</span>
            {/if}
          </div>
        {/if}
      </div>
    {/if}
  </div>

  {#if suggestions.length > 0}
    <div id="command-suggestions" class="suggestions" role="listbox" aria-label="Command suggestions" aria-expanded="true">
      {#each suggestions as cmd, idx}
        <button
          on:click={() => selectCommand(cmd)}
          on:mouseenter={() => selectedIndex = idx}
          class="suggestion-item"
          class:selected={selectedIndex === idx}
          role="option"
          aria-selected={selectedIndex === idx}
          aria-label={`${cmd.name}: ${cmd.description}`}
          title={cmd.tags ? `Tags: ${cmd.tags.join(', ')}` : ''}
        >
          <div class="cmd-name">{cmd.name}</div>
          <div class="cmd-desc">{cmd.description}</div>
          {#if cmd.tags}
            <div class="cmd-tags">
              {#each cmd.tags as tag}
                <span class="tag">{tag}</span>
              {/each}
            </div>
          {/if}
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .command-palette {
    border-top: 1px solid #ddd;
    background: #fff;
    padding: 1rem;
  }

  .palette-info {
    margin-bottom: 0.5rem;
    min-height: 20px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
  }

  .bulk-indicator {
    display: inline-block;
    padding: 0.3rem 0.8rem;
    background: #fff3cd;
    color: #856404;
    border-radius: 3px;
    font-size: 12px;
    font-weight: bold;
  }

  .keyboard-hints {
    display: flex;
    gap: 0.75rem;
    font-size: 11px;
    color: #999;
  }

  .hint {
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
    padding: 0.2rem 0.4rem;
    background: #f5f5f5;
    border-radius: 2px;
    border: 1px solid #e0e0e0;
    cursor: help;
    white-space: nowrap;
  }

  .hint:hover {
    background: #efefef;
    color: #666;
  }

  .input-wrapper {
    display: flex;
    gap: 0.5rem;
    align-items: center;
  }

  .command-input {
    flex: 1;
    padding: 0.5rem;
    border: 1px solid #ccc;
    border-radius: 4px;
    font-size: 14px;
  }

  .command-input:focus {
    outline: none;
    border-color: #007bff;
    box-shadow: 0 0 0 3px rgba(0, 123, 255, 0.25);
  }

  .execute-btn {
    padding: 0.5rem 1rem;
    background: #007bff;
    color: white;
    border: none;
    border-radius: 4px;
    cursor: pointer;
    font-weight: bold;
  }

  .execute-btn:hover {
    background: #0056b3;
  }

  .dry-run-label {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    cursor: pointer;
    user-select: none;
  }

  .suggestions {
    margin-top: 0.5rem;
    border: 1px solid #ddd;
    border-radius: 4px;
    max-height: 200px;
    overflow-y: auto;
    background: #f9f9f9;
  }

  .suggestion-item {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    width: 100%;
    padding: 0.75rem 1rem;
    text-align: left;
    border: none;
    background: none;
    cursor: pointer;
    font-size: 14px;
    border-bottom: 1px solid #eee;
    transition: background 0.2s;
  }

  .suggestion-item:last-child {
    border-bottom: none;
  }

  .suggestion-item:hover,
  .suggestion-item.selected {
    background: #e8f0ff;
  }

  .suggestion-item:focus {
    outline: 2px solid #007bff;
    outline-offset: -2px;
  }

  .cmd-name {
    font-weight: 600;
    color: #333;
  }

  .cmd-desc {
    font-size: 12px;
    color: #666;
  }

  .cmd-tags {
    display: flex;
    gap: 0.25rem;
    flex-wrap: wrap;
    margin-top: 0.25rem;
  }

  .tag {
    display: inline-block;
    padding: 0.1rem 0.4rem;
    background: #f0f0f0;
    color: #555;
    border-radius: 2px;
    font-size: 11px;
  }

  .model-select select {
    padding: 0.4rem 0.5rem;
    border: 1px solid #ccc;
    border-radius: 4px;
    font-size: 13px;
  }

  .loading {
    font-size: 12px;
    color: #666;
  }

  .capabilities {
    display: flex;
    gap: 0.5rem;
    flex-wrap: wrap;
  }

  .cap-pill {
    display: inline-block;
    padding: 0.2rem 0.5rem;
    background: #eef2ff;
    color: #3730a3;
    border-radius: 999px;
    font-size: 12px;
  }

  .cap-pill.provider {
    background: #f3f4f6;
    color: #1f2937;
  }

  .cap-pill.cost {
    background: #fff7ed;
    color: #9a3412;
  }

  .model-display {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.3rem 0.6rem;
    background: #f0f0f0;
    border-radius: 3px;
    font-size: 12px;
    color: #555;
  }

  .model-label {
    font-weight: 600;
    color: #333;
  }

  .model-name {
    color: #007bff;
    font-weight: 500;
  }

  .model-name.loading {
    color: #999;
    font-style: italic;
  }

  .model-tier {
    display: inline-block;
    padding: 0.1rem 0.3rem;
    background: #fff3cd;
    color: #856404;
    border-radius: 2px;
    font-size: 10px;
    font-weight: bold;
  }

  .model-select-wrapper {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .model-select {
    display: flex;
    gap: 0.5rem;
    align-items: center;
  }</style>