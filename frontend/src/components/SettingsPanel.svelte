<script>
  import Icon from './Icon.svelte'
  import { paperStore } from '../stores/paperStore.js'
  import { onMount } from 'svelte'

  export let isOpen = false
  export let onClose = () => {}
  export let onSave = async (updates) => {}

  // Defaults for everything this panel binds to. /api/config answers with a
  // much smaller object than the panel reads -- it has no `editor` or `theme`
  // key at all -- so these are what the controls fall back to.
  const DEFAULT_CONFIG = {
    keyboard_shortcuts: {},
    theme: { mode: 'auto', primary_color: '#0b0b0c', accent_color: '#2a2a2a', error_color: '#f0616b' },
    editor: { font_size: 14, font_family: 'JetBrains Mono', tab_size: 2, word_wrap: true, minimap_enabled: false },
    auto_save: true,
    history_limit: 100,
  }

  let config = structuredClone(DEFAULT_CONFIG)

  let selectedTab = 'editor'
  let loading = true
  let modelsLoading = false
  let availableModels = []
  let modalContainer
  let previouslyFocused

  /**
   * Load configuration from server
   */
  async function loadConfig() {
    try {
      loading = true
      const res = await fetch('/api/config')
      if (!res.ok) throw new Error('Failed to load config')
      // Merge rather than replace. Assigning the response wholesale dropped
      // `editor`, `theme` and `keyboard_shortcuts`, and the very next thing the
      // markup does is read `config.editor.font_size` -- which threw, so the
      // whole panel failed to render and Settings appeared to do nothing.
      const fromServer = await res.json()
      config = {
        ...DEFAULT_CONFIG,
        ...fromServer,
        theme: { ...DEFAULT_CONFIG.theme, ...(fromServer.theme ?? {}) },
        editor: { ...DEFAULT_CONFIG.editor, ...(fromServer.editor ?? {}) },
        keyboard_shortcuts: {
          ...DEFAULT_CONFIG.keyboard_shortcuts,
          ...(fromServer.keyboard_shortcuts ?? {}),
        },
      }

      // One configured planner model (masterplan Q1) — reported by /api/config.
      modelsLoading = true
      if (config.model) {
        availableModels = [{ id: config.model, name: config.model }]
        if (!config.ai_model_default) config.ai_model_default = config.model
      }
    } catch (e) {
      console.error('Error loading config:', e)
    } finally {
      loading = false
      modelsLoading = false
    }
  }

  /**
   * Save configuration updates
   */
  async function saveConfig() {
    const updates = {}

    // Collect all changed values
    for (const [key, value] of Object.entries(config)) {
      if (typeof value === 'object') {
        for (const [subKey, subValue] of Object.entries(value)) {
          updates[`${key}.${subKey}`] = subValue
        }
      } else {
        updates[key] = value
      }
    }

    // Ensure AI model default is saved explicitly when present
    if (config.ai_model_default) {
      updates['ai_model_default'] = config.ai_model_default
    }

    await onSave(updates)
    onClose()
  }

  /**
   * Handle escape key to close modal
   */
  function handleKeyDown(e) {
    if (e.key === 'Escape') {
      onClose()
    }
    
    // Focus trap: prevent tab from escaping modal
    if (e.key === 'Tab' && modalContainer) {
      const focusable = modalContainer.querySelectorAll(
        'button, input, select, textarea, [tabindex]:not([tabindex="-1"])'
      )
      const first = focusable[0]
      const last = focusable[focusable.length - 1]
      
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault()
        last.focus()
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault()
        first.focus()
      }
    }
  }

  /**
   * Close and restore focus
   */
  function closeAndRestoreFocus() {
    onClose()
    if (previouslyFocused) {
      previouslyFocused.focus()
    }
  }

  onMount(() => {
    loadConfig()
    
    // Store currently focused element
    previouslyFocused = document.activeElement
    
    // Focus first interactive element in modal
    setTimeout(() => {
      if (modalContainer) {
        const firstFocusable = modalContainer.querySelector('button, input, select, textarea')
        if (firstFocusable) {
          firstFocusable.focus()
        }
      }
    }, 100)
  })
</script>

{#if isOpen}
  <div class="settings-overlay" role="presentation" aria-hidden="true" on:click={(e) => { if (e.target === e.currentTarget) closeAndRestoreFocus(); }} on:keydown={handleKeyDown}>
    <div 
      class="settings-panel" 
      
      bind:this={modalContainer}
      role="dialog"
      aria-modal="true"
      aria-labelledby="settings-title"
      tabindex="-1"
    >
      <div class="settings-header">
        <h2 id="settings-title">Settings</h2>
        <button 
          class="close-btn" 
          on:click={closeAndRestoreFocus}
          aria-label="Close settings"
        >
          <Icon name="close" label="Close settings" />
        </button>
      </div>

      {#if loading}
        <div class="settings-content">
          <p>Loading settings...</p>
        </div>
      {:else}
        <div class="settings-tabs">
          <button
            class="tab-btn"
            class:active={selectedTab === 'editor'}
            on:click={() => (selectedTab = 'editor')}
          >
            Editor
          </button>
          <button
            class="tab-btn"
            class:active={selectedTab === 'theme'}
            on:click={() => (selectedTab = 'theme')}
          >
            Theme
          </button>
          <button
            class="tab-btn"
            class:active={selectedTab === 'shortcuts'}
            on:click={() => (selectedTab = 'shortcuts')}
          >
            Keyboard
          </button>
          <button
            class="tab-btn"
            class:active={selectedTab === 'ai'}
            on:click={() => (selectedTab = 'ai')}
          >
            AI
          </button>
          <button
            class="tab-btn"
            class:active={selectedTab === 'general'}
            on:click={() => (selectedTab = 'general')}
          >
            General
          </button>
        </div>

        <div class="settings-content">
          {#if selectedTab === 'editor'}
            <div class="settings-section">
              <h3>Editor Settings</h3>

              <div class="setting-group">
                <label for="font-size">Font Size</label>
                <input
                  id="font-size"
                  type="number"
                  min="8"
                  max="32"
                  bind:value={config.editor.font_size}
                />
              </div>

              <div class="setting-group">
                <label for="font-family">Font Family</label>
                <select id="font-family" bind:value={config.editor.font_family}>
                  <option value="monospace">Monospace</option>
                  <option value="'Courier New'">'Courier New'</option>
                  <option value="'Fira Code'">'Fira Code'</option>
                  <option value="'JetBrains Mono'">'JetBrains Mono'</option>
                </select>
              </div>

              <div class="setting-group">
                <label for="tab-size">Tab Size</label>
                <input
                  id="tab-size"
                  type="number"
                  min="2"
                  max="8"
                  bind:value={config.editor.tab_size}
                />
              </div>

              <div class="setting-group">
                <label>
                  <input type="checkbox" bind:checked={config.editor.word_wrap} />
                  Word Wrap
                </label>
              </div>

              <div class="setting-group">
                <label>
                  <input type="checkbox" bind:checked={config.editor.minimap_enabled} />
                  Show Minimap
                </label>
              </div>
            </div>
          {/if}

          {#if selectedTab === 'theme'}
            <div class="settings-section">
              <h3>Theme Settings</h3>

              <div class="setting-group">
                <label for="theme-mode">Theme Mode</label>
                <select id="theme-mode" bind:value={config.theme.mode}>
                  <option value="light">Light</option>
                  <option value="dark">Dark</option>
                  <option value="auto">Auto (System)</option>
                </select>
              </div>

              <div class="setting-group">
                <label for="primary-color">Primary Color</label>
                <input id="primary-color" type="color" bind:value={config.theme.primary_color} />
                <code>{config.theme.primary_color}</code>
              </div>

              <div class="setting-group">
                <label for="accent-color">Accent Color</label>
                <input id="accent-color" type="color" bind:value={config.theme.accent_color} />
                <code>{config.theme.accent_color}</code>
              </div>

              <div class="setting-group">
                <label for="error-color">Error Color</label>
                <input id="error-color" type="color" bind:value={config.theme.error_color} />
                <code>{config.theme.error_color}</code>
              </div>
            </div>

            <div class="settings-section">
              <h3>Page appearance in dark mode</h3>
              <p class="setting-note">
                The compiled page is dimmed rather than inverted, so its colours
                stay true &mdash; a red warning box is still red. Hold
                <strong>Hold for true colours</strong> above the preview to see the
                page exactly as it will print.
              </p>

              <div class="setting-group">
                <label for="paper-lightness">Paper brightness</label>
                <input
                  id="paper-lightness"
                  type="range"
                  min="0.6"
                  max="1"
                  step="0.01"
                  value={$paperStore.paper}
                  on:input={(e) => paperStore.setPaper(e.currentTarget.value)}
                />
                <code>{Math.round($paperStore.paper * 100)}%</code>
              </div>

              <div class="setting-group">
                <label for="paper-warmth">Warmth</label>
                <input
                  id="paper-warmth"
                  type="range"
                  min="0"
                  max="40"
                  step="1"
                  value={$paperStore.warmth}
                  on:input={(e) => paperStore.setWarmth(e.currentTarget.value)}
                />
                <code>{$paperStore.warmth}</code>
              </div>

              <div class="setting-group">
                <button type="button" class="reset-tone" on:click={paperStore.reset}>
                  Reset to defaults
                </button>
              </div>
            </div>
          {/if}

          {#if selectedTab === 'shortcuts'}
            <div class="settings-section">
              <h3>Keyboard Shortcuts</h3>
              <p class="help-text">Edit keyboard shortcuts (e.g., ctrl+s, alt+enter)</p>

              {#each Object.entries(config.keyboard_shortcuts) as [action, shortcut]}
                <div class="setting-group">
                  <label for={`shortcut-${action}`}>{action.replace(/_/g, ' ')}</label>
                  <input
                    id={`shortcut-${action}`}
                    type="text"
                    placeholder="e.g., ctrl+s"
                    bind:value={config.keyboard_shortcuts[action]}
                  />
                </div>
              {/each}
            </div>
          {/if}

          {#if selectedTab === 'ai'}
            <div class="settings-section">
              <h3>AI Model</h3>
              <p class="help-text">Choose the global default model used for AI planning.</p>

              {#if modelsLoading}
                <div class="setting-group">
                  <p>Loading available models...</p>
                </div>
              {:else}
                <div class="setting-group">
                  <label for="ai-model-select">Default Model</label>
                  <select
                    id="ai-model-select"
                    aria-label="Select global default AI model"
                    bind:value={config.ai_model_default}
                  >
                    {#each availableModels as m}
                      <option value={m.id}>
                        {m.label} {m.tier ? `(${m.tier})` : ''}
                      </option>
                    {/each}
                  </select>
                </div>

                {#if config.ai_model_default}
                  {#each availableModels.filter(m => m.id === config.ai_model_default) as selected}
                    <div class="setting-group">
                      <label>Model Capabilities</label>
                      <div class="capabilities">
                        {#if selected.supports}
                          {#if selected.supports.streaming}
                            <span class="cap-badge streaming" title="Supports real-time streaming">
                              <Icon name="bolt" /> Streaming
                            </span>
                          {/if}
                          {#if selected.supports.vision}
                            <span class="cap-badge vision" title="Can analyze images">
                              <Icon name="eye" /> Vision
                            </span>
                          {/if}
                          {#if selected.supports.code}
                            <span class="cap-badge code" title="Optimized for code">
                              {'<>'} Code
                            </span>
                          {/if}
                          {#if selected.supports.json}
                            <span class="cap-badge json" title="Structured JSON output">
                              {'{ }'} JSON
                            </span>
                          {/if}
                          {#if selected.supports.long_context}
                            <span class="cap-badge context" title="Extended context window">
                              [DOCS] Long Context
                            </span>
                          {/if}
                          {#if selected.supports.function_calling}
                            <span class="cap-badge tools" title="Tool/function calling">
                              <Icon name="wrench" /> Tools
                            </span>
                          {/if}
                        {/if}
                        {#if selected.provider}
                          <span class="cap-provider">
                            Provider: {selected.provider}
                          </span>
                        {/if}
                        {#if selected.max_tokens}
                          <span class="cap-info">
                            Max: {selected.max_tokens.toLocaleString()} tokens
                          </span>
                        {/if}
                      </div>
                    </div>
                  {/each}
                {/if}
              {/if}
            </div>
          {/if}

          {#if selectedTab === 'general'}
            <div class="settings-section">
              <h3>General Settings</h3>

              <div class="setting-group">
                <label>
                  <input type="checkbox" bind:checked={config.auto_save} />
                  Auto Save
                </label>
              </div>

              <div class="setting-group">
                <label for="history-limit">History Limit</label>
                <input
                  id="history-limit"
                  type="number"
                  min="10"
                  max="1000"
                  bind:value={config.history_limit}
                />
                <p class="help-text">Maximum number of history entries to keep</p>
              </div>
            </div>
          {/if}
        </div>

        <div class="settings-footer">
          <button class="btn-cancel" on:click={onClose}>Cancel</button>
          <button class="btn-save" on:click={saveConfig}>Save Settings</button>
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .settings-overlay {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }

  .capabilities {
    display: flex;
    gap: 0.5rem;
    flex-wrap: wrap;
  }

  .cap-pill {
    display: inline-block;
    padding: 0.2rem 0.5rem;
    background: hsl(var(--primary) / 0.1);
    color: hsl(var(--primary));
    border-radius: var(--radius-pill);
    font-size: 12px;
  }

  .cap-pill.provider {
    background: hsl(var(--surface-2));
    color: hsl(var(--foreground));
  }

  .settings-panel {
    background: hsl(var(--surface-1));
    border-radius: var(--radius-s);
    width: 90%;
    max-width: 600px;
    max-height: 80vh;
    display: flex;
    flex-direction: column;
    box-shadow: 0 10px 40px rgba(0, 0, 0, 0.2);
  }

  .settings-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 1.5rem;
    border-bottom: 1px solid hsl(var(--border));
  }

  .settings-header h2 {
    margin: 0;
    font-size: 1.25rem;
    color: hsl(var(--foreground));
  }

  .close-btn {
    background: none;
    border: none;
    font-size: 1.5rem;
    cursor: pointer;
    color: hsl(var(--text-faint));
    padding: 0;
    width: 32px;
    height: 32px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius-xs);
  }

  .close-btn:hover {
    background: hsl(var(--surface-2));
    color: hsl(var(--foreground));
  }

  .settings-tabs {
    display: flex;
    border-bottom: 1px solid hsl(var(--border));
    background: hsl(var(--surface-2));
  }

  .tab-btn {
    flex: 1;
    padding: 1rem;
    background: none;
    border: none;
    color: hsl(var(--text-dim));
    border-bottom: 2px solid transparent;
    cursor: pointer;
    color: hsl(var(--text-dim));
    font-weight: 500;
    transition: all 0.2s ease;
  }

  .tab-btn:hover {
    color: hsl(var(--foreground));
    background: hsl(var(--surface-2));
  }

  .tab-btn.active {
    color: hsl(var(--primary));
    border-bottom-color: hsl(var(--primary));
  }

  .settings-content {
    flex: 1;
    overflow-y: auto;
    padding: 1.5rem;
  }

  .settings-section {
    margin-bottom: 2rem;
  }

  .settings-section h3 {
    margin-top: 0;
    margin-bottom: 1rem;
    color: hsl(var(--foreground));
    font-size: 1rem;
  }

  .setting-group {
    margin-bottom: 1rem;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .setting-group label {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-weight: 500;
    color: hsl(var(--text-dim));
  }

  .setting-group label input[type='checkbox'] {
    cursor: pointer;
  }

  .setting-group input[type='text'],
  .setting-group input[type='number'],
  .setting-group input[type='color'],
  .setting-group select {
    padding: 0.5rem;
    border: 1px solid hsl(var(--border));
    border-radius: var(--radius-xs);
    font-size: 0.9rem;
    font-family: inherit;
  }

  .setting-group input[type='color'] {
    width: 60px;
    height: 36px;
    cursor: pointer;
  }

  .setting-group code {
    font-family: 'JetBrains Mono', ui-monospace, SFMono-Regular, monospace;
    color: hsl(var(--text-dim));
    font-size: 0.85rem;
  }

  .help-text {
    margin: 0;
    font-size: 0.85rem;
    color: hsl(var(--text-faint));
  }

  .capabilities {
    display: flex;
    flex-wrap: wrap;
    gap: 0.75rem;
    margin-top: 0.5rem;
    align-items: center;
  }

  /* One chip for every capability, distinguished by its label.
   *
   * These were six pastel gradients — vision blue, code pink, context purple —
   * where the hue encoded nothing: they are static facts about a model, not
   * state to react to. Six competing colours also meant the three that do carry
   * state (compile error, warning, success) had to shout to be noticed. Spending
   * no colour here is what lets a failed compile read instantly.
   *
   * The scale-on-hover went with them: these are spans with a title, not
   * controls, and movement promised an interaction that does not exist. */
  .setting-note {
    margin: 0 0 1rem 0;
    font-size: 0.85rem;
    line-height: 1.5;
    color: hsl(var(--text-dim));
    max-width: 60ch;
  }

  .reset-tone {
    padding: 0.4rem 0.9rem;
    font-size: 0.85rem;
    border: 1px solid hsl(var(--border));
    border-radius: var(--radius-pill);
    background: transparent;
    color: hsl(var(--foreground));
    cursor: pointer;
  }

  .reset-tone:hover {
    background: hsl(var(--accent));
  }

  .cap-badge {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0.35rem 0.75rem;
    border-radius: var(--radius-pill);
    font-size: 0.8rem;
    font-weight: 500;
    white-space: nowrap;
    background: hsl(var(--foreground) / 0.06);
    border: 1px solid hsl(var(--border));
    color: hsl(var(--foreground));
  }

  .cap-provider {
    display: inline-block;
    padding: 0.35rem 0.75rem;
    background: hsl(var(--surface-2));
    border: 1px solid hsl(var(--border));
    border-radius: var(--radius-xs);
    font-size: 0.8rem;
    color: hsl(var(--text-dim));
  }

  .cap-info {
    display: inline-block;
    padding: 0.35rem 0.75rem;
    background: hsl(var(--surface-2));
    border: 1px solid hsl(var(--border));
    border-radius: var(--radius-xs);
    font-size: 0.8rem;
    color: hsl(var(--text-faint));
  }

  .settings-footer {
    display: flex;
    gap: 1rem;
    padding: 1.5rem;
    border-top: 1px solid hsl(var(--border));
    justify-content: flex-end;
  }

  .btn-cancel,
  .btn-save {
    padding: 0.5rem 1.5rem;
    border: none;
    border-radius: var(--radius-xs);
    cursor: pointer;
    font-weight: 500;
    font-size: 0.9rem;
    transition: all 0.2s ease;
  }

  .btn-cancel {
    background: hsl(var(--surface-2));
    color: hsl(var(--foreground));
  }

  .btn-cancel:hover {
    background: hsl(var(--muted));
  }

  .btn-save {
    background: hsl(var(--primary));
    color: hsl(var(--primary-foreground));
  }

  .btn-save:hover {
    background: hsl(var(--primary));
  }

</style>
