<script>
  import Icon from '../components/Icon.svelte'
  import RightPanel from '../components/RightPanel.svelte'
  import Editor from '../components/Editor.svelte'
  import ChatPanel from '../components/ChatPanel.svelte'
  import StatusBar from '../components/StatusBar.svelte'
  import { createEventDispatcher } from 'svelte';
  const dispatch = createEventDispatcher();

  export let filePath = ''
  export let etag = null
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
  export let aiEnabled = true
  export let chatStreamingMessage = ''

  function handleDividerMouseDown(e) { dispatch('divider-mousedown', e); }
  function handleChatDividerMouseDown(e) { dispatch('chat-divider-mousedown', e); }
</script>

<div class="content-wrapper" class:overleaf-layout={layoutMode === 'overleaf'} class:fullscreen-editor={layoutMode === 'fullscreen-editor'} class:fullscreen-pdf={layoutMode === 'fullscreen-pdf'} bind:this={contentWrapperRef}>
  <div class="editor-section" style={layoutMode === 'overleaf' ? `height: ${editorHeightPercent}%` : ''}>
    <div class="editor-header">
      <span class="section-label"><Icon name="document" /> Editor</span>
      <div class="editor-controls">
        <button on:click={() => layoutMode = layoutMode === 'fullscreen-editor' ? 'overleaf' : 'fullscreen-editor'} title="Toggle fullscreen" class="control-btn">
          <Icon name={layoutMode === 'fullscreen-editor' ? 'collapse' : 'expand'} />
        </button>
      </div>
    </div>
    <div class="editor-pane">
      <Editor
        bind:this={editorRef}
        bind:content={editorContent}
        {filePath}
        {etag}
        {autoSave}
        {autoSaveDelay}
        {editorSettings}
        theme={themeSettings?.mode || 'light'}
        on:saved={(e) => dispatch('saved', e.detail)}
        on:save-error={(e) => dispatch('save-error', e.detail)}
        on:dirty-change={(e) => dispatch('dirty-change', e.detail)}
        on:content-changed={(e) => dispatch('content-changed', e.detail)}
        on:save-conflict={(e) => dispatch('save-conflict', e.detail)}
      />
    </div>
  </div>

  {#if layoutMode === 'overleaf'}
    <button type="button" class="divider" class:dragging={isDraggingDivider} aria-label="Resize editor and preview panels" on:mousedown={handleDividerMouseDown} on:keydown={(e) => (e.key === 'Enter' || e.key === ' ') && handleDividerMouseDown(e)}></button>
    
    <div class="pdf-section" style={`height: ${100 - editorHeightPercent}%`}>
      <div class="pdf-header">
        <span class="section-label"><Icon name="eye" /> Preview</span>
        <div class="pdf-controls">
          <button on:click={() => layoutMode = 'fullscreen-pdf'} title="Toggle fullscreen" class="control-btn"><Icon name="expand" /></button>
        </div>
      </div>
      <div class="pdf-pane">
        {#if bulkMode}
          <div class="bulk-progress">
            <div class="progress-header">Applying command to {bulkTotal} files...</div>
            <div class="progress-bar" role="progressbar" aria-valuemin="0" aria-valuemax={bulkTotal} aria-valuenow={bulkProgress}>
              <div class="progress-fill" style="transform: scaleX({bulkTotal ? bulkProgress / bulkTotal : 0})"></div>
            </div>
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
      <span class="section-label"><Icon name="message" /> AI assistant</span>
      <div class="chat-controls">
        <button on:click={() => showChatPanel = false} title="Hide chat panel" class="control-btn"><Icon name="chevron-down" /></button>
      </div>
    </div>
    <ChatPanel messages={chatMessages} loading={chatLoading} {aiEnabled} streamingMessage={chatStreamingMessage} on:message-sent />
  </div>
{/if}

<style>
  /* This component had no styles at all. Every class below was in the markup
     and matched nothing, so the panes had no heights: `.editor-section` got its
     inline `height: 60%` against an auto-height parent, and the editor
     collapsed to five pixels with the document loaded inside it. */

  .content-wrapper {
    display: flex;
    flex-direction: column;
    flex: 1;
    /* Without this a flex child refuses to shrink below its content, and the
       percentage heights below never get a definite box to resolve against. */
    min-height: 0;
  }

  .editor-section,
  .pdf-section {
    display: flex;
    flex-direction: column;
    min-height: 0;
    overflow: hidden;
  }

  /* One pane owns the space in the fullscreen modes. */
  .fullscreen-editor .editor-section,
  .fullscreen-pdf .pdf-section {
    flex: 1;
  }

  .editor-header,
  .pdf-header,
  .chat-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    padding: 0.4rem 0.75rem;
    background: hsl(var(--surface-1));
    border-bottom: 1px solid hsl(var(--border));
    flex: none;
  }

  .section-label {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.85rem;
    font-weight: 500;
    color: hsl(var(--text-dim));
  }

  .editor-controls,
  .pdf-controls,
  .chat-controls {
    display: flex;
    gap: 0.25rem;
  }

  .control-btn {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border: 1px solid transparent;
    border-radius: var(--radius-xs);
    background: transparent;
    color: hsl(var(--text-dim));
    cursor: pointer;
    transition: background-color 0.16s ease, color 0.16s ease;
  }

  .control-btn:hover {
    background: hsl(var(--accent));
    color: hsl(var(--foreground));
  }

  .editor-pane,
  .pdf-pane {
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }

  .pdf-pane {
    overflow: auto;
  }

  /* A grab handle, not decoration: it is the control that resizes the panes. */
  .divider,
  .chat-divider {
    flex: none;
    height: 5px;
    padding: 0;
    border: 0;
    background: hsl(var(--border));
    cursor: row-resize;
    transition: background-color 0.16s ease;
  }

  .divider:hover,
  .chat-divider:hover,
  .divider.dragging,
  .chat-divider.dragging {
    background: hsl(var(--text-faint));
  }

  .divider:focus-visible,
  .chat-divider:focus-visible {
    outline: 2px solid hsl(var(--ring));
    outline-offset: -2px;
  }

  /* `chatHeightPercent` is a pixel value despite the name; it is applied inline
     as `height: {n}px`, so this only needs to stop it growing or shrinking. */
  .chat-section {
    display: flex;
    flex-direction: column;
    flex: none;
    min-height: 0;
    overflow: hidden;
    border-top: 1px solid hsl(var(--border));
  }

  .bulk-progress {
    padding: 1.25rem;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }

  .progress-header {
    font-size: 0.9rem;
    color: hsl(var(--foreground));
  }

  .progress-bar {
    height: 6px;
    border-radius: var(--radius-pill);
    background: hsl(var(--surface-2));
    overflow: hidden;
  }

  .progress-fill {
    height: 100%;
    width: 100%;
    background: hsl(var(--primary));
    /* Scaled rather than resized: animating `width` re-runs layout on every
       frame, while a transform stays on the compositor. */
    transform-origin: left center;
    transition: transform 0.2s ease;
  }

  .progress-text {
    font-size: 0.8rem;
    color: hsl(var(--text-dim));
    font-variant-numeric: tabular-nums;
  }
</style>
