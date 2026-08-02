<script>
  import Icon from './Icon.svelte'
  import DiffView from './DiffView.svelte'
  import PdfViewer from './PdfViewer.svelte'
  import { createEventDispatcher } from 'svelte'
  
  export let diff = ''
  export let pdfUrl = ''; export let filePath = ''
  export let pdfPages = 0
  export let compileErrors = []
  export let compileWarnings = []
  export let compiling = false
  export let activeTab = 'diff'  // 'diff', 'pdf', 'errors', 'ai'
  export let aiPlan = null
  export let jobId = null  // Compilation job ID for async status
  export let showSuggestionsButton = true  // Show AI suggestions button
  export let suggestedFixes = []  // AI-suggested fixes from error explanation [{command, description, rationale}]

  const templates = [
    {
      name: 'Research Article',
      description: 'IMRaD structure with abstract, keywords, figures, and references.',
      content: `\\documentclass[11pt]{article}
\\usepackage[utf8]{inputenc}
\\usepackage{graphicx}
\\usepackage{hyperref}
\\title{Your Title}
\\author{Your Name}
\\date{\\today}

\\begin{document}
\\maketitle
\\begin{abstract}
Brief abstract summarizing objectives, methods, results, conclusions.
\\end{abstract}

\\section{Introduction}
Context and motivation.

\\section{Methods}
Describe methodology and assumptions.

\\section{Results}
Present key findings with tables/figures.

\\section{Discussion}
Interpret results and limitations.

\\section{Conclusion}
Summarize contributions and future work.

\\bibliographystyle{plain}
\\bibliography{references}
\\end{document}`
    },
    {
      name: 'Beamer Slides',
      description: 'Minimal slide deck with title, agenda, and bullets.',
      content: `\\documentclass{beamer}
\\usetheme{Madrid}
\\title{Talk Title}
\\author{Your Name}
\\date{\\today}

\\begin{document}

\\begin{frame}
  \\titlepage
\\end{frame}

\\begin{frame}{Agenda}
  \\tableofcontents
\\end{frame}

\\section{Topic One}
\\begin{frame}{Key Points}
  \\begin{itemize}
    \\item Point A
    \\item Point B
    \\item Point C
  \\end{itemize}
\\end{frame}

\\end{document}`
    },
    {
      name: 'IEEE Conference',
      description: 'IEEEtran conference format with abstract and keywords.',
      content: `\\documentclass[conference]{IEEEtran}
\\usepackage[utf8]{inputenc}
\\usepackage{graphicx}
\\usepackage{amsmath}
\\usepackage{hyperref}

\\title{Paper Title}
\\author{Author Name\\thanks{Affiliation and email.}}

\\begin{document}
\\maketitle

\\begin{abstract}
Summary of the paper.
\\end{abstract}

\\begin{IEEEkeywords}
keyword1, keyword2, keyword3
\\end{IEEEkeywords}

\\section{Introduction}
Introduce the problem and contributions.

\\section{Method}
Describe the proposed approach.

\\section{Results}
Show experiments and analysis.

\\section{Conclusion}
Wrap up and future work.

\\bibliographystyle{IEEEtran}
\\bibliography{references}
\\end{document}`
    }
  ]
  
  const dispatch = createEventDispatcher()

  function applyStep(step) {
    if (!step || !step.command) return
    dispatch('apply-step', { step })
  }

  function applyAll() {
    dispatch('apply-all', { plan: aiPlan })
  }

  function applyFix(fix) {
    if (!fix || !fix.command) return
    dispatch('apply-fix', { fix })
  }

  function insertTemplate(template) {
    if (!template?.content) return
    dispatch('insert-template', { template })
  }
  
  $: hasErrors = compileErrors.length > 0 || compileWarnings.length > 0
</script>

<div class="right-panel">
  <!-- Tab Headers -->
  <div class="tabs" role="tablist" aria-label="Result panels">
    <button
      class="tab"
      class:active={activeTab === 'diff'}
      on:click={() => activeTab = 'diff'}
      role="tab"
      aria-selected={activeTab === 'diff'}
      aria-controls="diff-panel"
      id="tab-diff"
    >
      <Icon name="document" /> Diff
      {#if diff}
        <span class="badge" aria-label="Unseen diff changes">•</span>
      {/if}
    </button>
    <button
      class="tab"
      class:active={activeTab === 'pdf'}
      on:click={() => activeTab = 'pdf'}
      role="tab"
      aria-selected={activeTab === 'pdf'}
      aria-controls="pdf-panel"
      id="tab-pdf"
    >
      <Icon name="document" /> PDF
      {#if pdfPages > 0}
        <span class="badge-text" aria-label={`PDF has ${pdfPages} pages`}>({pdfPages})</span>
      {/if}
    </button>
    <button
      class="tab"
      class:active={activeTab === 'errors'}
      on:click={() => activeTab = 'errors'}
      role="tab"
      aria-selected={activeTab === 'errors'}
      aria-controls="errors-panel"
      id="tab-errors"
    >
      {#if compileErrors.length > 0}
        <Icon name="alert" /> Errors ({compileErrors.length})
      {:else if compileWarnings.length > 0}
        [WARNING] Warnings ({compileWarnings.length})
      {:else}
        <Icon name="check-circle" /> Issues
      {/if}
    </button>
    <button
      class="tab"
      class:active={activeTab === 'ai'}
      on:click={() => activeTab = 'ai'}
      role="tab"
      aria-selected={activeTab === 'ai'}
      aria-controls="ai-panel"
      id="tab-ai"
    >
      <Icon name="sparkle" /> AI Plan
      {#if aiPlan}
        <span class="badge" aria-label="AI plan pending review">•</span>
      {/if}
    </button>
    <button
      class="tab"
      class:active={activeTab === 'suggestions'}
      on:click={() => activeTab = 'suggestions'}
      role="tab"
      aria-selected={activeTab === 'suggestions'}
      aria-controls="suggestions-panel"
      id="tab-suggestions"
      title="Get AI suggestions for improving your document"
    >
      <Icon name="sparkle" /> Suggestions
    </button>
  </div>

  <!-- Tab Content -->
  <div class="tab-content">
    {#if activeTab === 'diff'}
      <div id="diff-panel" role="tabpanel" aria-labelledby="tab-diff">
        {#if aiPlan?.edits?.length}
          <DiffView
            edits={aiPlan.edits}
            {filePath}
            onapply={(payload) => dispatch('apply-selected', payload)}
            onreject={() => dispatch('reject-all')}
          />
        {:else}
        <div class="empty-state">
          <p>No changes yet</p>
          <p class="hint">Apply a command to see the diff</p>
        </div>
      {/if}
      </div>
    {:else if activeTab === 'pdf'}
      <div id="pdf-panel" role="tabpanel" aria-labelledby="tab-pdf">
        <PdfViewer 
          pdfPath={pdfUrl} {filePath}
          numPages={pdfPages}
          {jobId}
          loading={compiling}
          on:compiled={(e) => dispatch('compiled', e.detail)}
          on:compileFailed={(e) => dispatch('compileFailed', e.detail)}
        />
      </div>
    {:else if activeTab === 'errors'}
      <div id="errors-panel" role="tabpanel" aria-labelledby="tab-errors" class="errors-panel">
        {#if compileErrors.length > 0}
          <div class="error-section" role="region" aria-label="Compilation errors">
            <h3><Icon name="alert" /> Errors ({compileErrors.length})</h3>
            {#each compileErrors as error}
              <button
                class="error-item error"
                type="button"
                aria-label={`Go to error at line ${error.line || 'unknown'} in ${error.file || 'current file'}`}
                on:click={() => dispatch('jump-to-line', { line: error.line, file: error.file })}
              >
                <div class="error-line">Line {error.line || '?'}</div>
                <div class="error-file">{error.file || 'Current file'}</div>
                <div class="error-message">{error.message}</div>
              </button>
            {/each}
          </div>
        {/if}
        
        {#if compileWarnings.length > 0}
          <div class="error-section" role="region" aria-label="Compilation warnings">
            <h3>[WARNING] Warnings ({compileWarnings.length})</h3>
            {#each compileWarnings as warning}
              <button
                class="error-item warning"
                on:click={() => dispatch('jump-to-line', { line: warning.line, file: warning.file })}
              >
                <div class="error-line">Line {warning.line || '?'}</div>
                <div class="error-file">{warning.file || 'Current file'}</div>
                <div class="error-message">{warning.message}</div>
              </button>
            {/each}
          </div>
        {/if}

        {#if compileErrors.length === 0 && compileWarnings.length === 0}
          <div class="empty-state" role="status">
            <p><Icon name="check-circle" /> No compilation issues</p>
            <p class="hint">Your document compiled successfully</p>
          </div>
        {/if}

        {#if hasErrors}
          <div class="explain-actions">
            <button
              class="explain-btn"
              on:click={() => dispatch('explain-errors', { errors: compileErrors, warnings: compileWarnings })}
              aria-label="Explain issues and suggest fixes with AI"
            >
              <Icon name="sparkle" /> Explain &amp; suggest fixes
            </button>
          </div>

          {#if suggestedFixes.length > 0}
            <div class="suggested-fixes" role="region" aria-label="AI-suggested fixes">
              <h3><Icon name="wrench" /> Suggested fixes</h3>
              <div class="fixes-list">
                {#each suggestedFixes as fix, i}
                  <div class="fix-item">
                    <div class="fix-header">
                      <span class="fix-number">{i + 1}</span>
                      <span class="fix-desc">{fix.description || fix.command}</span>
                    </div>
                    {#if fix.rationale}
                      <div class="fix-rationale">{fix.rationale}</div>
                    {/if}
                    <div class="fix-command"><code>{fix.command}</code></div>
                    <button
                      class="apply-fix-btn"
                      on:click={() => applyFix(fix)}
                      aria-label={`Apply fix: ${fix.description || fix.command}`}
                    >
                      <Icon name="check" /> Apply
                    </button>
                  </div>
                {/each}
              </div>
            </div>
          {/if}
        {/if}
      </div>
    {:else if activeTab === 'ai'}
      <div id="ai-panel" role="tabpanel" aria-labelledby="tab-ai">
        {#if aiPlan}
          <div class="ai-panel">
            <h3>Proposed Plan</h3>
            {#if aiPlan.edits}
              <div class="ai-actions">
                <button 
                  class="apply-all-btn" 
                  on:click={() => dispatch('apply-selected', { edits: aiPlan.edits, selectedHunkIds: aiPlan.edits.map((e) => e.hunk_id).filter(Boolean) })} 
                  aria-label="Apply all AI edits"
                >
                  Apply All Edits
                </button>
              </div>
              <DiffView
                edits={aiPlan.edits}
                {filePath}
                onapply={(payload) => dispatch('apply-selected', payload)}
                onreject={() => dispatch('reject-all')}
              />
            {:else if aiPlan.steps}
              <div class="ai-actions">
                <button 
                  class="apply-all-btn" 
                  on:click={applyAll} 
                  aria-label="Apply all AI steps"
                >
                  Apply All Steps
                </button>
              </div>
              <div role="list" aria-label="AI plan steps">
                {#each aiPlan.steps as step, i}
                  <div class="ai-step" role="listitem">
                    <div class="ai-step-index" aria-label={`Step ${i + 1}`}>{i + 1}</div>
                    <div class="ai-step-content">
                      <div class="ai-step-title">{step.title || 'Step'}</div>
                      {#if step.description}
                        <div class="ai-step-desc">{step.description}</div>
                      {/if}
                      {#if step.command}
                        <div class="ai-step-command"><code>{step.command}</code></div>
                        <div class="ai-step-actions">
                          <button 
                            class="apply-btn" 
                            on:click={() => applyStep(step)} 
                            aria-label={`Apply step ${i + 1}: ${step.title || 'untitled'}`}
                          >
                            Apply
                          </button>
                        </div>
                      {/if}
                    </div>
                  </div>
                {/each}
              </div>
            {:else}
              <pre class="ai-json">{JSON.stringify(aiPlan, null, 2)}</pre>
            {/if}
          </div>
        {:else}
          <div class="empty-state">
            <p>No AI plan yet</p>
            <p class="hint">Use the palette to request a plan</p>
          </div>
        {/if}
      </div>
    {:else if activeTab === 'suggestions'}
      <div id="suggestions-panel" role="tabpanel" aria-labelledby="tab-suggestions">
        <div class="suggestions-panel">
          <div class="suggestions-intro">
            <p>Get AI-powered suggestions and code review for your LaTeX document.</p>
            <button
              class="get-suggestions-btn"
              on:click={() => dispatch('get-suggestions')}
              aria-label="Analyze document and get improvement suggestions"
            >
              [ANALYSIS] Analyze Document
            </button>
            <button
              class="review-btn"
              on:click={() => dispatch('code-review')}
              aria-label="Get comprehensive code review"
            >
              <Icon name="search" /> Code review
            </button>
          </div>
        </div>
      </div>
    {/if}
  </div>
</div>

<style>
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

  .right-panel {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: hsl(var(--surface-2));
  }

  .tabs {
    display: flex;
    background: hsl(var(--surface-1));
    border-bottom: 2px solid hsl(var(--border));
  }

  .tab {
    flex: 1;
    padding: 0.75rem 1rem;
    background: none;
    border: none;
    border-bottom: 3px solid transparent;
    cursor: pointer;
    font-size: 13px;
    font-weight: 500;
    color: hsl(var(--text-dim));
    transition: all 0.2s;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 0.25rem;
  }

  .tab:hover {
    background: hsl(var(--surface-2));
    color: hsl(var(--foreground));
  }

  .tab.active {
    color: hsl(var(--primary));
    border-bottom-color: hsl(var(--primary));
    background: hsl(var(--surface-2));
  }

  .badge {
    color: hsl(var(--ok));
    font-size: 16px;
  }

  .badge-text {
    font-size: 11px;
    color: hsl(var(--text-faint));
  }

  .tab-content {
    flex: 1;
    overflow: auto;
  }

  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100%;
    color: hsl(var(--text-faint));
    text-align: center;
    padding: 2rem;
  }

  .empty-state p {
    margin: 0.5rem 0;
  }

  .hint {
    font-size: 12px;
    color: hsl(var(--text-dim));
  }

  .loading-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100%;
    gap: 1rem;
  }

  .spinner {
    width: 40px;
    height: 40px;
    /* A rotating arc, not a side accent: the thick ring is the spinner. */
    border: 4px solid hsl(var(--line-soft));
    border-top: 4px solid hsl(var(--primary));
    border-radius: 50%;
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    0% { transform: rotate(0deg); }
    100% { transform: rotate(360deg); }
  }

  .errors-panel {
    padding: 1rem;
  }

  .error-section {
    margin-bottom: 1.5rem;
  }

  .error-section h3 {
    margin: 0 0 0.75rem 0;
    font-size: 14px;
    font-weight: 600;
  }

  .error-item {
    padding: 0.75rem;
    margin-bottom: 0.5rem;
    border-radius: var(--radius-xs);
    width: 100%;
    text-align: left;
    cursor: pointer;
    border: none;
  }

  .error-item.error {
    background: hsl(var(--err) / 0.1);
    border-left-color: hsl(var(--err));
  }

  .error-item.warning {
    background: hsl(var(--warn) / 0.1);
    border-left-color: hsl(var(--warn));
  }

  .error-line {
    font-weight: 600;
    font-size: 11px;
    color: hsl(var(--text-dim));
    margin-bottom: 0.25rem;
    text-transform: uppercase;
  }

  .error-file {
    font-size: 12px;
    color: hsl(var(--text-dim));
    margin-bottom: 0.25rem;
    word-break: break-all;
  }

  .error-message {
    font-size: 13px;
    line-height: 1.4;
  }

  .ai-panel {
    padding: 1rem;
  }

  .explain-actions {
    margin-top: 1rem;
    display: flex;
    justify-content: flex-end;
  }

  .explain-btn {
    padding: 0.4rem 0.75rem;
    background: hsl(var(--primary));
    color: hsl(var(--primary-foreground));
    border: none;
    border-radius: var(--radius-xs);
    cursor: pointer;
    font-size: 13px;
  }

  .ai-step {
    display: flex;
    gap: 0.75rem;
    padding: 0.75rem;
    border: 1px solid hsl(var(--border));
    border-radius: var(--radius-s);
    background: hsl(var(--surface-1));
    margin-bottom: 0.5rem;
  }

  .ai-actions {
    margin-bottom: 0.75rem;
  }

  .apply-all-btn {
    padding: 0.4rem 0.75rem;
    background: hsl(var(--ok));
    color: hsl(var(--primary-foreground));
    border: none;
    border-radius: var(--radius-xs);
    cursor: pointer;
    font-size: 13px;
  }

  .ai-step-actions {
    margin-top: 0.4rem;
  }

  .apply-btn {
    padding: 0.3rem 0.6rem;
    background: hsl(var(--primary));
    color: hsl(var(--primary-foreground));
    border: none;
    border-radius: var(--radius-xs);
    cursor: pointer;
    font-size: 12px;
  }

  .ai-step-index {
    width: 28px;
    height: 28px;
    border-radius: 50%;
    background: hsl(var(--primary) / 0.1);
    color: hsl(var(--primary));
    display: flex;
    align-items: center;
    justify-content: center;
    font-weight: 600;
  }

  .ai-step-title {
    font-weight: 600;
    margin-bottom: 0.25rem;
  }

  .ai-step-desc {
    color: hsl(var(--text-dim));
    font-size: 13px;
    margin-bottom: 0.25rem;
  }

  .ai-step-command {
    font-size: 12px;
    color: hsl(var(--foreground));
    background: hsl(var(--surface-2));
    padding: 0.2rem 0.4rem;
    border-radius: var(--radius-xs);
  }

  .ai-json {
    background: hsl(var(--surface-1));
    color: hsl(var(--foreground));
    padding: 0.75rem;
    border-radius: var(--radius-s);
    font-size: 12px;
  }

  .suggestions-panel {
    padding: 1rem;
  }

  .suggestions-intro {
    display: flex;
    flex-direction: column;
    gap: 1rem;
    align-items: center;
    justify-content: center;
    padding: 2rem;
    text-align: center;
    color: hsl(var(--text-dim));
  }

  .suggestions-intro p {
    margin: 0;
    line-height: 1.5;
  }

  .get-suggestions-btn {
    padding: 0.5rem 1rem;
    background: hsl(var(--primary));
    color: hsl(var(--primary-foreground));
    border: none;
    border-radius: var(--radius-xs);
    cursor: pointer;
    font-size: 13px;
    font-weight: 600;
  }

  .get-suggestions-btn:hover {
    background: hsl(var(--primary));
  }

  .review-btn {
    padding: 0.5rem 1rem;
    background: hsl(var(--ok));
    color: hsl(var(--primary-foreground));
    border: none;
    border-radius: var(--radius-xs);
    cursor: pointer;
    font-size: 13px;
    font-weight: 600;
  }

  .review-btn:hover {
    background: hsl(var(--ok));
  }

  .suggested-fixes {
    margin-top: 1rem;
    padding: 0.75rem;
    background: hsl(var(--ok) / 0.08);
    border: 1px solid hsl(var(--ok) / 0.35);
    border-radius: var(--radius-s);
  }

  .suggested-fixes h3 {
    margin: 0 0 0.75rem 0;
    font-size: 14px;
    color: hsl(var(--ok));
    font-weight: 600;
  }

  .fixes-list {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .fix-item {
    padding: 0.5rem;
    background: hsl(var(--surface-1));
    border: 1px solid hsl(var(--ok));
    border-radius: var(--radius-xs);
  }

  .fix-header {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin-bottom: 0.25rem;
  }

  .fix-number {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    background-color: hsl(var(--ok));
    color: hsl(var(--primary-foreground));
    border-radius: 50%;
    font-size: 0.7rem;
    font-weight: 600;
  }

  .fix-desc {
    color: hsl(var(--foreground));
    font-weight: 500;
    font-size: 13px;
  }

  .fix-rationale {
    margin-bottom: 0.4rem;
    font-size: 12px;
    color: hsl(var(--text-dim));
    line-height: 1.3;
    padding-left: 24px;
  }

  .fix-command {
    margin-bottom: 0.4rem;
    padding: 0.3rem 0.4rem;
    background: hsl(var(--surface-2));
    border-radius: var(--radius-xs);
    overflow-x: auto;
    margin-left: 24px;
  }

  .fix-command code {
    font-family: 'JetBrains Mono', ui-monospace, SFMono-Regular, monospace;
    font-size: 11px;
    color: hsl(var(--err));
    white-space: nowrap;
  }

  .apply-fix-btn {
    padding: 0.3rem 0.6rem;
    background-color: hsl(var(--ok));
    color: hsl(var(--primary-foreground));
    border: none;
    border-radius: var(--radius-xs);
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    transition: background-color 0.2s;
    margin-left: 24px;
  }

  .apply-fix-btn:hover {
    background-color: hsl(var(--ok));
  }

  .apply-fix-btn:active {
    transform: scale(0.98);
  }

  .templates-panel {
    margin-top: 1.5rem;
    padding: 1rem;
    background: hsl(var(--surface-2));
    border: 1px solid hsl(var(--border));
    border-radius: var(--radius-s);
  }

  .templates-panel h4 {
    margin: 0 0 0.75rem 0;
    font-size: 14px;
    color: hsl(var(--foreground));
  }

  .templates-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
    gap: 0.75rem;
  }

  .template-card {
    padding: 0.75rem;
    border: 1px solid hsl(var(--border));
    border-radius: var(--radius-s);
    background: hsl(var(--surface-1));
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
  }

  .template-title {
    font-weight: 600;
    color: hsl(var(--foreground));
  }

  .template-desc {
    font-size: 12px;
    color: hsl(var(--text-dim));
  }

  .template-btn {
    margin-top: 0.25rem;
    align-self: flex-start;
    padding: 0.4rem 0.8rem;
    background: hsl(var(--primary));
    color: hsl(var(--primary-foreground));
    border: none;
    border-radius: var(--radius-xs);
    cursor: pointer;
    font-size: 12px;
    font-weight: 600;
  }

  .template-btn:hover {
    background: hsl(var(--primary));
  }
</style>
