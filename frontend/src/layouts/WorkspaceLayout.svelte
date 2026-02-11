<!--
  WorkspaceLayout - Content area with resizable editor/PDF sections and chat
  Responsible for: layout mode switching, divider resizing, section visibility
-->
<script>
  import { Splitpanes, Pane } from 'svelte-splitpanes'
  import RightPanel from '../components/RightPanel.svelte'
  import Editor from '../components/Editor.svelte'
  import ChatPanel from '../components/ChatPanel.svelte'
  import StatusBar from '../components/StatusBar.svelte'

  export let filePath = ''
  export let isDirty = false
  export let compiling = false
  export let layoutMode = 'overleaf' // 'overleaf' | 'fullscreen-editor' | 'fullscreen-pdf'
  export let selectedFiles = []
  export let bulkMode = false
  export let status = 'Ready'

  // Layout state
  let editorSize = 60
  let chatSize = 25

  let editorRef = null
  let editorContent = ''
  let activeRightTab = 'diff'
  let pdfUrl = ''
  let pdfPages = 0
  let compileErrors = []
  let compileWarnings = []
  let diff = ''
  let aiPlan = null
  let compilationJobId = null
  let bulkProgress = 0
  let bulkTotal = 0
  let showChatPanel = true
  let chatMessages = []
  let chatLoading = false
  let chatStreamingMessage = ''
</script>

<div
  class="workspace-layout"
  class:overleaf-layout={layoutMode === 'overleaf'}
  class:fullscreen-editor={layoutMode === 'fullscreen-editor'}
  class:fullscreen-pdf={layoutMode === 'fullscreen-pdf'}
>
  {#if layoutMode === 'overleaf'}
    <Splitpanes horizontal class="workspace-splitpanes">
      <Pane size={editorSize} minSize={20}>
        <div class="editor-section">
          <div class="editor-header">
            <span class="section-label">[NOTE] Editor</span>
            <div class="editor-controls">
              <button
                on:click={() => (layoutMode = 'fullscreen-editor')}
                title="Toggle fullscreen"
                class="control-btn"
              >
                ↗️
              </button>
            </div>
          </div>
          <div class="editor-pane">
            <slot name="editor">
              <Editor
                bind:this={editorRef}
                bind:content={editorContent}
                {filePath}
                autoSave={true}
                autoSaveDelay={30000}
                editorSettings={{}}
                theme="light"
                on:saved
                on:save-error
                on:dirty-change
                on:content-changed
                on:save-conflict
              />
            </slot>
          </div>
        </div>
      </Pane>
      
      <Pane size={100 - editorSize - (showChatPanel ? chatSize : 0)} minSize={20}>
        <div class="pdf-section">
          <div class="pdf-header">
            <span class="section-label">📄 Preview</span>
            <div class="pdf-controls">
              <button
                on:click={() => (layoutMode = 'fullscreen-pdf')}
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
                  <div class="progress-fill" style="width: {(bulkProgress / bulkTotal) * 100}%"/>
                </div>
                <div class="progress-text">{bulkProgress} / {bulkTotal}</div>
              </div>
            {:else}
              <slot name="rightpanel">
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
                  on:apply-step
                  on:apply-all
                  on:compiled
                  on:compileFailed
                  on:apply-selected
                  on:apply-fix
                  on:reject-all
                  on:jump-to-line
                  on:explain-errors
                  on:get-suggestions
                  on:code-review
                  on:insert-template
                />
              </slot>
            {/if}
          </div>
        </div>
      </Pane>
      
      {#if showChatPanel}
        <Pane size={chatSize} minSize={15}>
          <div class="chat-section">
            <div class="chat-header">
              <span class="section-label">💬 AI Assistant</span>
              <div class="chat-controls">
                <button
                  on:click={() => (showChatPanel = !showChatPanel)}
                  title="Toggle chat panel"
                  class="control-btn"
                >
                  ▼
                </button>
              </div>
            </div>
            <slot name="chat">
              <ChatPanel
                messages={chatMessages}
                loading={chatLoading}
                streamingMessage={chatStreamingMessage}
                on:message-sent
              />
            </slot>
          </div>
        </Pane>
      {/if}
    </Splitpanes>
  {:else if layoutMode === 'fullscreen-editor'}
    <div class="editor-section">
      <div class="editor-header">
        <span class="section-label">[NOTE] Editor</span>
        <div class="editor-controls">
          <button
            on:click={() => (layoutMode = 'overleaf')}
            title="Exit fullscreen"
            class="control-btn"
          >
            ↙️
          </button>
        </div>
      </div>
      <div class="editor-pane">
        <slot name="editor">
          <Editor
            bind:this={editorRef}
            bind:content={editorContent}
            {filePath}
            autoSave={true}
            autoSaveDelay={30000}
            editorSettings={{}}
            theme="light"
            on:saved
            on:save-error
            on:dirty-change
            on:content-changed
            on:save-conflict
          />
        </slot>
      </div>
    </div>
  {:else if layoutMode === 'fullscreen-pdf'}
    <div class="pdf-section">
      <div class="pdf-header">
        <span class="section-label">📄 Preview</span>
        <div class="pdf-controls">
          <button
            on:click={() => (layoutMode = 'overleaf')}
            title="Exit fullscreen"
            class="control-btn"
          >
            ↙️
          </button>
        </div>
      </div>
      <div class="pdf-pane">
        <slot name="rightpanel">
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
            on:apply-step
            on:apply-all
            on:compiled
            on:compileFailed
            on:apply-selected
            on:apply-fix
            on:reject-all
            on:jump-to-line
            on:explain-errors
            on:get-suggestions
            on:code-review
            on:insert-template
          />
        </slot>
      </div>
    </div>
  {/if}
</div>

<!-- Status Bar -->
<StatusBar {status} />

<style>
  .workspace-layout {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    min-height: 0;
  }

  :global(.workspace-splitpanes) {
    height: 100%;
  }

  :global(.splitpanes__splitter) {
    background-color: hsl(var(--border));
    position: relative;
    transition: background-color 0.2s;
  }

  :global(.splitpanes__splitter:hover) {
    background-color: hsl(var(--primary));
  }

  :global(.splitpanes--horizontal > .splitpanes__splitter) {
    height: 4px;
    cursor: row-resize;
  }

  .editor-section,
  .pdf-section,
  .chat-section {
    display: flex;
    flex-direction: column;
    overflow: hidden;
    height: 100%;
  }

  .editor-header,
  .pdf-header,
  .chat-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.5rem 1rem;
    border-bottom: 1px solid hsl(var(--border));
    background: hsl(var(--secondary));
    flex-shrink: 0;
  }

  .section-label {
    font-weight: 600;
    user-select: none;
  }

  .editor-controls,
  .pdf-controls,
  .chat-controls {
    display: flex;
    gap: 0.5rem;
  }

  .control-btn {
    background: none;
    border: none;
    cursor: pointer;
    padding: 0.25rem 0.5rem;
    font-size: 1rem;
    color: hsl(var(--foreground));
  }

  .control-btn:hover {
    opacity: 0.7;
  }

  .editor-pane,
  .pdf-pane {
    flex: 1;
    overflow: auto;
    min-height: 0;
  }

  .bulk-progress {
    padding: 2rem;
    text-align: center;
  }

  .progress-header {
    margin-bottom: 1rem;
    font-weight: 600;
  }

  .progress-bar {
    height: 4px;
    background: hsl(var(--secondary));
    border-radius: 2px;
    overflow: hidden;
    margin-bottom: 0.5rem;
  }

  .progress-fill {
    height: 100%;
    background: hsl(var(--primary));
    transition: width 0.2s;
  }

  .progress-text {
    font-size: 0.875rem;
    color: hsl(var(--muted-foreground));
  }
</style>
