<script>
  import Icon from './Icon.svelte'
  import { createEventDispatcher, onMount } from 'svelte'
  
  export let messages = []  // [{role: 'user'|'assistant', content: '...', timestamp: ...}]
  export let loading = false
  export let streamingMessage = ''  // Current streaming response
  
  const dispatch = createEventDispatcher()
  
  let inputValue = ''
  let messagesContainer
  let inputField
  
  onMount(() => {
    scrollToBottom()
    inputField?.focus()
  })
  
  function scrollToBottom() {
    setTimeout(() => {
      if (messagesContainer) {
        messagesContainer.scrollTop = messagesContainer.scrollHeight
      }
    }, 0)
  }
  
  // Auto-scroll when new messages arrive
  $: if (messages.length || streamingMessage) {
    scrollToBottom()
  }
  
  async function handleSubmit() {
    if (!inputValue.trim() || loading) return
    
    const userMessage = inputValue.trim()
    inputValue = ''
    
    // Add user message immediately
    dispatch('message-sent', {
      content: userMessage,
      timestamp: new Date().toISOString()
    })
  }
  
  function handleKeydown(e) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      handleSubmit()
    }
  }
  
  function formatTime(timestamp) {
    return new Date(timestamp).toLocaleTimeString([], { 
      hour: '2-digit', 
      minute: '2-digit' 
    })
  }
</script>

<div class="chat-panel">
  <!-- Chat History -->
  <div class="messages-container" bind:this={messagesContainer}>
    {#if messages.length === 0 && !streamingMessage}
      <div class="welcome-message">
        <h2>LaTeX Assistant</h2>
        <p>Start by describing what you want to create:</p>
        <ul class="examples">
          <li>"Create a research paper with 3 sections"</li>
          <li>"Make a resume with two columns"</li>
          <li>"Generate a presentation slide deck"</li>
          <li>"Add a table of contents to this document"</li>
        </ul>
      </div>
    {/if}
    
    <!-- Messages -->
    {#each messages as msg (msg.timestamp)}
      <div class="message" class:user={msg.role === 'user'} class:assistant={msg.role === 'assistant'}>
        <div class="message-avatar">
          {#if msg.role === 'user'}
            <Icon name="user" />
          {:else}
            <Icon name="sparkle" />
          {/if}
        </div>
        <div class="message-content">
          <div class="message-text">
            {#if msg.role === 'assistant'}
              <!-- Render LaTeX code blocks specially -->
              {#if msg.content.includes('```latex') || msg.content.includes('\\documentclass')}
                {@html formatAssistantMessage(msg.content)}
              {:else}
                {msg.content}
              {/if}
            {:else}
              {msg.content}
            {/if}
          </div>
          <div class="message-time">{formatTime(msg.timestamp)}</div>
        </div>
      </div>
    {/each}
    
    <!-- Streaming Response -->
    {#if streamingMessage}
      <div class="message assistant streaming">
        <div class="message-avatar"><Icon name="sparkle" /></div>
        <div class="message-content">
          <div class="message-text">
            {@html formatAssistantMessage(streamingMessage)}
            <span class="cursor">▌</span>
          </div>
        </div>
      </div>
    {/if}
    
    <!-- Loading Indicator -->
    {#if loading && !streamingMessage}
      <div class="message assistant thinking">
        <div class="message-avatar"><Icon name="sparkle" /></div>
        <div class="message-content">
          <div class="typing-indicator">
            <span></span>
            <span></span>
            <span></span>
          </div>
        </div>
      </div>
    {/if}
  </div>
  
  <!-- Input Area -->
  <div class="input-area">
    <div class="input-wrapper">
      <textarea
        bind:value={inputValue}
        bind:this={inputField}
        placeholder="What would you like to create or modify? (Shift+Enter for new line)"
        on:keydown={handleKeydown}
        disabled={loading}
        class="chat-input"
        rows="3"
      ></textarea>
      <button
        on:click={handleSubmit}
        disabled={!inputValue.trim() || loading}
        class="send-btn"
        aria-label="Send message"
      >
        {#if loading}
          <span class="loading-spinner"></span>
        {:else}
          <Icon name="send" label="Send message" />
        {/if}
      </button>
    </div>
    <div class="hint">Tip: Shift+Enter for new line, Enter to send</div>
  </div>
</div>

<style>
  .chat-panel {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: hsl(var(--background));
    border: 1px solid hsl(var(--border));
    border-radius: var(--radius-s);
    overflow: hidden;
  }
  
  .messages-container {
    flex: 1;
    overflow-y: auto;
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  
  .welcome-message {
    text-align: center;
    color: hsl(var(--muted-foreground));
    padding: 24px 16px;
  }
  
  .welcome-message h2 {
    font-size: 24px;
    margin-bottom: 8px;
    color: hsl(var(--foreground));
  }
  
  .welcome-message p {
    margin-bottom: 16px;
  }
  
  .examples {
    list-style: none;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  
  .examples li {
    padding: 8px 12px;
    background: hsl(var(--muted));
    border-radius: var(--radius-xs);
    cursor: pointer;
    transition: all 200ms;
  }
  
  .examples li:hover {
    background: hsl(var(--accent));
    color: hsl(var(--accent-foreground));
  }
  
  .message {
    display: flex;
    gap: 12px;
    animation: slideIn 200ms ease-out;
  }
  
  @keyframes slideIn {
    from {
      opacity: 0;
      transform: translateY(10px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }
  
  .message.user {
    flex-direction: row-reverse;
  }
  
  .message-avatar {
    font-size: 20px;
    flex-shrink: 0;
    width: 28px;
    height: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  
  .message-content {
    flex: 1;
    max-width: 85%;
  }
  
  .message-text {
    background: hsl(var(--card));
    padding: 12px 16px;
    border-radius: var(--radius-s);
    line-height: 1.5;
    word-wrap: break-word;
    font-size: 14px;
  }
  
  .message.user .message-text {
    background: hsl(var(--primary));
    color: hsl(var(--primary-foreground));
  }
  
  .message.assistant .message-text {
    background: hsl(var(--muted));
  }
  
  .message.streaming .message-text {
    background: hsl(var(--muted));
    border: 1px solid hsl(var(--accent));
  }
  
  .cursor {
    animation: blink 1s infinite;
    margin-left: 2px;
  }
  
  @keyframes blink {
    0%, 49% { opacity: 1; }
    50%, 100% { opacity: 0; }
  }
  
  .message-time {
    font-size: 12px;
    color: var(--text-secondary);
    margin-top: 4px;
    padding: 0 4px;
  }
  
  .typing-indicator {
    display: flex;
    gap: 4px;
    padding: 8px;
  }
  
  .typing-indicator span {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--text-secondary);
    animation: typing 1.4s infinite;
  }
  
  .typing-indicator span:nth-child(2) {
    animation-delay: 0.2s;
  }
  
  .typing-indicator span:nth-child(3) {
    animation-delay: 0.4s;
  }
  
  @keyframes typing {
    0%, 60%, 100% { opacity: 0.3; }
    30% { opacity: 1; }
  }
  
  .input-area {
    padding: 12px;
    border-top: 1px solid hsl(var(--border));
    background: hsl(var(--secondary));
  }
  
  .input-wrapper {
    display: flex;
    gap: 8px;
    align-items: flex-end;
  }
  
  .chat-input {
    flex: 1;
    padding: 10px 12px;
    border: 1px solid hsl(var(--border));
    border-radius: var(--radius-s);
    background: hsl(var(--background));
    color: hsl(var(--foreground));
    font-family: inherit;
    font-size: 14px;
    resize: none;
    max-height: 120px;
  }
  
  .chat-input:focus {
    outline: none;
    border-color: hsl(var(--ring));
    box-shadow: 0 0 0 2px hsl(var(--ring) / 0.2);
  }
  
  .chat-input:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }
  
  .send-btn {
    padding: 10px 16px;
    background: hsl(var(--primary));
    color: hsl(var(--primary-foreground));
    border: none;
    border-radius: var(--radius-s);
    cursor: pointer;
    font-size: 16px;
    transition: all 200ms;
    flex-shrink: 0;
  }
  
  .send-btn:hover:not(:disabled) {
    opacity: 0.9;
    transform: translateY(-2px);
    box-shadow: 0 4px 12px hsl(var(--primary) / 0.25);
  }
  
  .send-btn:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }
  
  .loading-spinner {
    display: inline-block;
    width: 14px;
    height: 14px;
    border: 2px solid hsl(var(--primary-foreground) / 0.3);
    border-top-color: hsl(var(--primary-foreground));
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  
  @keyframes spin {
    to { transform: rotate(360deg); }
  }
  
  .hint {
    font-size: 12px;
    color: var(--text-secondary);
    margin-top: 6px;
    padding: 0 4px;
  }

  /* Code block styling for LaTeX output */
  :global(.chat-input ::-webkit-input-placeholder) {
    color: var(--text-secondary);
  }
</style>

<script context="module">
  export function formatAssistantMessage(content) {
    // Format LaTeX code blocks with syntax highlighting
    return content
      .replace(/```latex\n?([\s\S]*?)```/g, '<div class="code-block"><pre>$1</pre></div>')
      .replace(/\\([\w]+)/g, '<span class="latex-command">\\$1</span>')
  }
</script>
