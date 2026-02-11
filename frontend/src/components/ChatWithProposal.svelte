<script>
  import { onMount } from 'svelte'
  import { aiStore, startStreaming, updateStreamingMessage, setProposal, setError, addMessage, resetAI, clearProposal, setAutoApply } from '../stores/aiStore'
  import { fileStore } from '../stores/fileStore'
  import { compileStore } from '../stores/compileStore'
  import AgentProposal from './AgentProposal.svelte'
  
  let inputValue = ''
  let loading = false
  let messages = []
  let currentProposal = null
  let error = null
  let autoApply = false
  
  $: messages = $aiStore.messages
  $: currentProposal = $aiStore.currentProposal
  $: error = $aiStore.error
  $: autoApply = $aiStore.autoApply
  
  let scrollContainer
  
  $: if (scrollContainer) {
    scrollContainer.scrollTop = scrollContainer.scrollHeight
  }
  
  async function sendMessage() {
    if (!inputValue.trim() || loading) return
    
    const instruction = inputValue
    inputValue = ''
    
    if (!$fileStore.filePath) {
      error = 'No file selected'
      return
    }
    
    loading = true
    addMessage('user', instruction)
    startStreaming(instruction)
    
    try {
      // Stream agent-loop response
      const res = await fetch('/api/agent-loop/stream', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          file_path: $fileStore.filePath,
          instruction,
          max_iterations: 3,
          auto_compile: true
        })
      })
      
      if (!res.ok) {
        setError(`Request failed: ${res.statusText}`)
        loading = false
        return
      }
      
      const reader = res.body.getReader()
      const decoder = new TextDecoder()
      let buffer = ''
      
      while (true) {
        const {done, value} = await reader.read()
        if (done) break
        
        buffer += decoder.decode(value, {stream: true})
        const lines = buffer.split('\n\n')
        buffer = lines.pop()
        
        for (const line of lines) {
          if (line.startsWith('data: ')) {
            try {
              const event = JSON.parse(line.slice(6))
              
              if (event.type === 'thinking' || event.type === 'proposing') {
                updateStreamingMessage(`\n\n*${event.data.message}*`)
              } else if (event.type === 'proposal') {
                // When proposal arrives, set currentProposal and pause for user unless autoApply is enabled
                setProposal({
                  summary: event.data.summary,
                  edits: event.data.edits || [],
                  diffs: event.data.diffs || [],
                  iteration: event.iteration || 0
                })
                // If autoApply is enabled, continue automatically (message will be updated as applying continues)
                updateStreamingMessage(`\n\n[SPARKLE] **Proposal**: ${event.data.summary}\n(${event.data.edit_count} edits)${autoApply ? ' [Auto-applying...]' : ' [Awaiting your approval]'}`)
              } else if (event.type === 'applying') {
                updateStreamingMessage(`\n\n[EDIT] *Applying edits...*`)
              } else if (event.type === 'compiled') {
                if (event.data.success) {
                  updateStreamingMessage(`\n\n[SUCCESS] **Compilation successful** (${event.data.page_count} pages)`)
                } else {
                  updateStreamingMessage(`\n\n[ERROR] **Compilation failed** (${event.data.error_count} errors)`)
                }
              } else if (event.type === 'error') {
                updateStreamingMessage(`\n\n[WARNING] Error: ${event.data.message}`)
              } else if (event.type === 'done') {
                updateStreamingMessage(`\n\n---\n*Task ${event.data.success ? 'completed successfully' : 'ended'}*`)
                clearProposal()
              }
            } catch (e) {
              console.error('Error parsing event:', e)
            }
          }
        }
      }
      
      // Final message
      addMessage('assistant', $aiStore.streamingMessage)
    } catch (e) {
      setError(e.message)
    } finally {
      loading = false
    }
  }
  
  function handleProposalApplied(event) {
    addMessage('assistant', `[SUCCESS] Applied ${event.detail.results.length} edit(s). ${event.detail.compile_result?.success ? 'Compilation successful!' : 'Check compilation errors.'}`)
    clearProposal()
  }
  
  function handleProposalApproved() {
    // User approved the proposal; agent-loop continues automatically
    clearProposal()
  }
  
  function handleProposalRejected() {
    // User rejected; stop the agent-loop
    loading = false
    clearProposal()
    setError('Proposal rejected by user')
  }
  
  function handleFixRequested(event) {
    // Trigger another agent-loop iteration for fixes
    addMessage('user', `Fix the compilation errors`)
    // (agent-loop already handles this internally)
  }
  
  function handleProposalDismissed() {
    clearProposal()
  }
  
  function toggleAutoApply() {
    autoApply = !autoApply
    setAutoApply(autoApply)
  }
</script>

<div class="chat-with-proposal">
  <div class="chat-section">
    <div class="messages" bind:this={scrollContainer}>
      {#each messages as msg}
        <div class="message" class:user={msg.role === 'user'}>
          <div class="content">
            {#if msg.role === 'user'}
              <strong>{msg.content}</strong>
            {:else}
              {msg.content}
            {/if}
          </div>
          <div class="timestamp">
            {msg.timestamp?.toLocaleTimeString() || ''}
          </div>
        </div>
      {/each}
      
      {#if loading}
        <div class="message assistant">
          <div class="spinner"></div>
        </div>
      {/if}
      
      {#if error}
        <div class="message error">
          <strong>Error:</strong> {error}
        </div>
      {/if}
    </div>
    
    <div class="input-area">
      <div class="controls">
        <label class="auto-apply-toggle">
          <input type="checkbox" bind:checked={autoApply} on:change={toggleAutoApply} disabled={loading} />
          <span>Auto-apply proposals</span>
        </label>
      </div>
      <textarea
        placeholder="Describe what you want LaTeX-AI to do (e.g., 'Fix all citation formatting' or 'Make the title page look better')"
        bind:value={inputValue}
        on:keydown={(e) => e.key === 'Enter' && e.ctrlKey && sendMessage()}
        disabled={loading}
      ></textarea>
      <button on:click={sendMessage} disabled={loading || !inputValue.trim()}>
        {loading ? '[TIMER] Processing...' : '[SPARKLE] Ask AI'}
      </button>
    </div>
  </div>
  
  {#if currentProposal}
    <div class="proposal-section">
      <AgentProposal 
        {currentProposal}
        applying={loading}
        on:proposal-applied={handleProposalApplied}
        on:proposal-dismissed={handleProposalDismissed}
        on:fix-requested={handleFixRequested}
      />
    </div>
  {/if}
</div>

<style>
  .chat-with-proposal {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px;
    height: 100%;
    min-height: 400px;
  }
  
  @media (max-width: 1024px) {
    .chat-with-proposal {
      grid-template-columns: 1fr;
    }
    
    .proposal-section {
      border-top: 1px solid #2d3748;
      padding-top: 12px;
    }
  }
  
  .chat-section {
    display: flex;
    flex-direction: column;
    background: #0f172a;
    border: 1px solid #2d3748;
    border-radius: 8px;
    overflow: hidden;
  }
  
  .messages {
    flex: 1;
    overflow-y: auto;
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  
  .message {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 8px 12px;
    border-radius: 6px;
    max-width: 90%;
    align-self: flex-start;
    background: #1f2937;
    color: #e5e7eb;
  }
  
  .message.user {
    align-self: flex-end;
    background: #2563eb;
    color: white;
  }
  
  .message.assistant {
    background: #1f2937;
  }
  
  .message.error {
    background: #7c2d12;
    color: #fed7aa;
    border-left: 3px solid #dc2626;
  }
  
  .content {
    word-wrap: break-word;
    white-space: pre-wrap;
    font-size: 13px;
    line-height: 1.4;
  }
  
  .content strong {
    font-weight: 600;
  }
  
  .timestamp {
    font-size: 11px;
    color: #9ca3af;
  }
  
  .spinner {
    width: 16px;
    height: 16px;
    border: 2px solid #4b5563;
    border-top-color: #2563eb;
    border-radius: 50%;
    animation: spin 0.6s linear infinite;
  }
  
  @keyframes spin {
    to { transform: rotate(360deg); }
  }
  
  .input-area {
    border-top: 1px solid #2d3748;
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  
  .controls {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  
  .auto-apply-toggle {
    display: flex;
    align-items: center;
    gap: 6px;
    cursor: pointer;
    font-size: 13px;
    color: #e5e7eb;
  }
  
  .auto-apply-toggle input[type="checkbox"] {
    width: 16px;
    height: 16px;
    cursor: pointer;
    accent-color: #2563eb;
  }
  
  .auto-apply-toggle input[type="checkbox"]:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  
  textarea {
    padding: 8px 12px;
    border: 1px solid #2d3748;
    border-radius: 6px;
    background: #111827;
    color: #e5e7eb;
    font-family: inherit;
    font-size: 13px;
    resize: none;
    max-height: 100px;
  }
  
  textarea::placeholder {
    color: #6b7280;
  }
  
  textarea:focus {
    outline: none;
    border-color: #2563eb;
    box-shadow: 0 0 0 2px rgba(37, 99, 235, 0.1);
  }
  
  textarea:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  
  button {
    padding: 8px 16px;
    background: #2563eb;
    color: white;
    border: none;
    border-radius: 6px;
    font-weight: 500;
    cursor: pointer;
    font-size: 13px;
    white-space: nowrap;
    transition: background 0.2s;
  }
  
  button:hover:not(:disabled) {
    background: #1d4ed8;
  }
  
  button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  
  .proposal-section {
    overflow-y: auto;
  }
</style>
