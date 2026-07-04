<script>
  import { Splitpanes, Pane } from 'svelte-splitpanes'
  import RightPanel from '../components/RightPanel.svelte'
  import Editor from '../components/Editor.svelte'
  import ChatPanel from '../components/ChatPanel.svelte'
  import StatusBar from '../components/StatusBar.svelte'
  import { createEventDispatcher } from 'svelte';
  const dispatch = createEventDispatcher();

  export let filePath = ''
  export let editorContent = ''
  export let editorRef = null
  export let autoSave = true
  export let autoSaveDelay = 30000
  export let editorSettings = {}
  export let themeSettings = {}
  export let layoutMode = 'overleaf' // 'overleaf' | 'fullscreen-editor' | 'fullscreen-pdf'
  export let isDirty = false
  export let compiling = false
  export let bulkMode = false
  export let bulkTotal = 0
  export let bulkProgress = 0
  export let editorHeightPercent = 60
  export let chatHeightPercent = 300
  export let isDraggingDivider = false
  export let contentWrapperRef = null
  export let showChatPanel = true
  export let pdfUrl = ''
  export let pdfPages = 0
  export let compileErrors = []
  export let compileWarnings = []
  export let diff = ''
  export let aiPlan = null
  export let compilationJobId = null
  export let activeRightTab = 'diff'
  export let chatMessages = []
  export let chatLoading = false
  export let chatStreamingMessage = ''

  function handleDividerMouseDown(e) { dispatch('divider-mousedown', e); }
  function handleChatDividerMouseDown(e) { dispatch('chat-divider-mousedown', e); }
</script>

<div class="content-wrapper" class:overleaf-layout={layoutMode === 'overleaf'} class:fullscreen-editor={layoutMode === 'fullscreen-editor'} class:fullscreen-pdf={layoutMode === 'fullscreen-pdf'} bind:this={contentWrapperRef}>
  <div class="editor-section" style={layoutMode === 'overleaf' ? `height: ${editorHeightPercent}%` : ''}>
    <div class="editor-header">
      <span class="section-label">[NOTE] Editor</span>
      <div class="editor-controls">
        <button on:click={() => layoutMode = layoutMode === 'fullscreen-editor' ? 'overleaf' : 'fullscreen-editor'} title="Toggle fullscreen" class="control-btn">
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
        {autoSaveDelay}
        {editorSettings}
        theme={themeSettings?.mode || 'light'}
        on:saved
        on:save-error
        on:dirty-change
        on:content-changed
        on:save-conflict
      />
    </div>
  </div>

  {#if layoutMode === 'overleaf'}
    <button type="button" class="divider" class:dragging={isDraggingDivider} aria-label="Resize editor and preview panels" on:mousedown={handleDividerMouseDown} on:keydown={(e) => (e.key === 'Enter' || e.key === ' ') && handleDividerMouseDown(e)}></button>
    
    <div class="pdf-section" style={`height: ${100 - editorHeightPercent}%`}>
      <div class="pdf-header">
        <span class="section-label">📄 Preview</span>
        <div class="pdf-controls">
          <button on:click={() => layoutMode = 'fullscreen-pdf'} title="Toggle fullscreen" class="control-btn">↗️</button>
        </div>
      </div>
      <div class="pdf-pane">
        {#if bulkMode}
          <div class="bulk-progress">
            <div class="progress-header">Applying command to {bulkTotal} files...</div>
            <div class="progress-bar"><div class="progress-fill" style="width: {(bulkProgress / bulkTotal) * 100}%"></div></div>
            <div class="progress-text">{bulkProgress} / {bulkTotal}</div>
          </div>
        {:else}
          <RightPanel
            {diff} {pdfUrl} {pdfPages} {compileErrors} {compileWarnings} {compiling} {aiPlan} {filePath} jobId={compilationJobId} bind:activeTab={activeRightTab}
            on:apply-step on:apply-all on:compiled on:compileFailed on:apply-selected on:apply-fix on:reject-all on:jump-to-line on:explain-errors on:get-suggestions on:code-review on:insert-template
          />
        {/if}
      </div>
    </div>
  {/if}
</div>

{#if showChatPanel && layoutMode === 'overleaf'}
  <button type="button" class="chat-divider" class:dragging={isDraggingDivider} aria-label="Resize chat panel" on:mousedown={handleChatDividerMouseDown} on:keydown={(e) => (e.key === 'Enter' || e.key === ' ') && handleChatDividerMouseDown(e)}></button>
  <div class="chat-section" style="height: {chatHeightPercent}px">
    <div class="chat-header">
      <span class="section-label">💬 AI Assistant</span>
      <div class="chat-controls">
        <button on:click={() => showChatPanel = false} title="Toggle chat panel" class="control-btn">▼</button>
      </div>
    </div>
    <ChatPanel messages={chatMessages} loading={chatLoading} streamingMessage={chatStreamingMessage} on:message-sent />
  </div>
{/if}
