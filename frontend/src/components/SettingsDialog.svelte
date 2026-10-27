<script>
  // Settings, in five tabs. Appearance and Editor are preferences kept in this
  // browser or app; Assistant is the server's configuration. A value pinned by
  // an environment variable is shown but locked, and the API key is write-only:
  // the server never sends it back, only where it comes from.
  import { onMount } from 'svelte'
  import { api } from '../lib/api/client.js'
  import { formatChord } from '../lib/commands.js'
  import { FEEDBACK_URL } from '../lib/feedback.js'
  import LocalModelPicker from './LocalModelPicker.svelte'

  let { ui, documents, commands, paper, onclose = () => {}, onchange = () => {} } = $props()

  const TABS = [
    ['appearance', 'Appearance'],
    ['editor', 'Editor'],
    ['assistant', 'Assistant'],
    ['privacy', 'Privacy'],
    ['shortcuts', 'Shortcuts']
  ]
  let tab = $state('appearance')

  // ---- assistant (server) -----------------------------------------------
  let server = $state(null)
  let serverError = $state('')
  let form = $state({ model: '', vision_model: '', timeout_secs: 120 })
  let keyInput = $state('')
  let busy = $state(false)
  let message = $state('')
  let problem = $state('')

  const pinned = (field) => server?.env_overridden?.includes(field) ?? false
  const ENV = { model: 'LATEX_AI_MODEL', vision_model: 'LATEX_AI_VISION_MODEL' }

  function adopt(config) {
    server = config
    form = { model: config.model, vision_model: config.vision_model ?? '', timeout_secs: config.timeout_secs }
  }

  onMount(async () => {
    try {
      adopt(await api.getConfig())
    } catch (e) {
      serverError = e.message
    }
  })

  async function guarded(work, done) {
    busy = true
    message = ''
    problem = ''
    try {
      adopt(await work())
      message = done
      onchange()
    } catch (e) {
      problem = e.message
    } finally {
      busy = false
    }
  }

  function saveAssistant() {
    // Send only what changed and is not locked: an unchanged field is not a request.
    const patch = {}
    if (!pinned('model') && form.model.trim() !== server.model) patch.model = form.model.trim()
    if (!pinned('vision_model') && form.vision_model.trim() !== (server.vision_model ?? ''))
      patch.vision_model = form.vision_model.trim()
    if (Number(form.timeout_secs) !== server.timeout_secs) patch.timeout_secs = Number(form.timeout_secs)
    if (Object.keys(patch).length === 0) {
      message = 'Nothing to save.'
      return
    }
    return guarded(() => api.saveConfig(patch), 'Saved.')
  }

  async function saveKey() {
    const key = keyInput.trim()
    await guarded(() => api.setApiKey(key), 'Key saved.')
    if (!problem) keyInput = ''
  }

  const removeKey = () => guarded(() => api.clearApiKey(), 'Key removed.')

  // ---- privacy -----------------------------------------------------------
  let report = $state('')
  let reportNote = $state('')

  async function copyReport() {
    reportNote = ''
    try {
      report = await api.getDiagnostics()
    } catch (e) {
      reportNote = `Could not build the report: ${e.message}`
      return
    }
    try {
      await navigator.clipboard.writeText(report)
      reportNote = 'Copied. Paste it into your bug report.'
    } catch {
      reportNote = 'Copy it from the box below.'
    }
  }

  const chords = $derived(commands.list().filter((c) => c.keys))
</script>

<div class="overlay" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="settings-title" tabindex="-1" onkeydown={(e) => e.key === 'Escape' && onclose()}>
    <header>
      <h2 id="settings-title">Settings</h2>
      <button class="x" onclick={onclose} aria-label="Close settings">✕</button>
    </header>

    <div class="tabs" role="tablist">
      {#each TABS as [id, label] (id)}
        <button role="tab" aria-selected={tab === id} class:on={tab === id} onclick={() => (tab = id)}>{label}</button>
      {/each}
    </div>

    <div class="content" role="tabpanel">
      {#if tab === 'appearance'}
        <div class="field">
          <label for="theme">Theme</label>
          <select id="theme" value={ui.theme} onchange={(e) => ui.setTheme(e.currentTarget.value)}>
            <option value="auto">Follow the system</option>
            <option value="light">Light</option>
            <option value="dark">Dark</option>
          </select>
        </div>
        <h3>Page in dark mode</h3>
        <p class="help">Tones the compiled page so it is not a white sheet beside a dark editor. Hold “True colours” in the PDF bar to see it as printed.</p>
        <div class="field">
          <label for="paper">Brightness</label>
          <input id="paper" type="range" min="0.6" max="1" step="0.01" value={paper.paper} oninput={(e) => paper.setPaper(e.currentTarget.value)} />
          <span class="val">{Math.round(paper.paper * 100)}%</span>
        </div>
        <div class="field">
          <label for="warmth">Warmth</label>
          <input id="warmth" type="range" min="0" max="40" step="1" value={paper.warmth} oninput={(e) => paper.setWarmth(e.currentTarget.value)} />
          <span class="val">{paper.warmth}</span>
        </div>
        <button onclick={() => paper.reset()}>Reset page tone</button>
      {:else if tab === 'editor'}
        <div class="field">
          <label for="font-size">Font size</label>
          <input id="font-size" type="number" min="10" max="28" value={ui.editor.fontSize} onchange={(e) => ui.setEditor({ fontSize: e.currentTarget.value })} />
        </div>
        <div class="field">
          <label for="tab-size">Tab width</label>
          <select id="tab-size" value={ui.editor.tabSize} onchange={(e) => ui.setEditor({ tabSize: e.currentTarget.value })}>
            {#each [2, 4, 8] as n (n)}<option value={n}>{n} spaces</option>{/each}
          </select>
        </div>
        <label class="check"><input type="checkbox" checked={ui.editor.wordWrap} onchange={(e) => ui.setEditor({ wordWrap: e.currentTarget.checked })} /> Wrap long lines</label>
        <label class="check"><input type="checkbox" checked={ui.spellcheck} onchange={() => ui.toggle('spellcheck')} /> Check spelling (English)</label>
        <label class="check"><input type="checkbox" checked={ui.compileOnSave} onchange={() => ui.toggle('compileOnSave')} /> Compile after each save</label>
        <label class="check"><input type="checkbox" checked={documents.autosave} onchange={(e) => (documents.autosave = e.currentTarget.checked)} /> Save automatically after a pause</label>
      {:else if tab === 'assistant'}
        {#if serverError}
          <p class="bad" role="alert">Could not reach the server: {serverError}</p>
        {:else if !server}
          <p class="help">Loading…</p>
        {:else}
          <p class="help">
            {#if server.ai_local}
              The assistant is on, using a model on this computer. No key is needed.
            {:else if server.ai_enabled}
              The assistant is on.
            {:else}
              The assistant is off until there is an API key or a local model.
            {/if}
            {#if server.env_overridden.length > 0} Fields set by an environment variable are locked.{/if}
          </p>
          <div class="field">
            <label for="model">Model</label>
            <input id="model" bind:value={form.model} disabled={pinned('model') || busy} spellcheck="false" />
          </div>
          {#if pinned('model')}<p class="help">Set by {ENV.model}.</p>{/if}
          <div class="field">
            <label for="vision">Vision model (optional)</label>
            <input id="vision" bind:value={form.vision_model} disabled={pinned('vision_model') || busy} spellcheck="false" />
          </div>
          {#if pinned('vision_model')}<p class="help">Set by {ENV.vision_model}.</p>{/if}
          <div class="field">
            <label for="timeout">Request timeout (seconds)</label>
            <input id="timeout" type="number" min="5" max="600" bind:value={form.timeout_secs} disabled={busy} />
          </div>
          <button class="primary" onclick={saveAssistant} disabled={busy}>Save</button>

          <h3>Model on this computer</h3>
          <LocalModelPicker {server} disabled={busy || pinned('model')} onchoose={(config) => { adopt(config); message = 'Saved.'; onchange() }} />

          <h3>API key</h3>
          {#if server.key_source === 'env'}
            <p class="help">The key comes from the GROQ_API_KEY environment variable and cannot be changed here.</p>
          {:else}
            <p class="help">
              {server.key_source === 'file' ? 'A key is stored on this computer.' : 'No key is stored.'}
              It is kept in a private file and never shown again.
            </p>
            <div class="field">
              <label for="key">{server.key_source === 'file' ? 'Replace key' : 'Groq API key'}</label>
              <input id="key" type="password" autocomplete="off" spellcheck="false" bind:value={keyInput} disabled={busy} />
            </div>
            <div class="row">
              <button class="primary" onclick={saveKey} disabled={busy || !keyInput.trim()}>Save key</button>
              {#if server.key_source === 'file'}<button onclick={removeKey} disabled={busy}>Remove key</button>{/if}
            </div>
          {/if}
          {#if message}<p class="ok" role="status">{message}</p>{/if}
          {#if problem}<p class="bad" role="alert">{problem}</p>{/if}
        {/if}
      {:else if tab === 'privacy'}
        <p class="help">Renkeu has no account, no analytics and no telemetry. Your project stays on this computer.</p>
        <h3>Stays here</h3>
        <p class="help">Compiling, the page preview, jumping between source and PDF, the layout report and the submission checks all run on this computer and send nothing.</p>
        <h3>Sent when you ask the assistant</h3>
        <p class="help">
          Only when you ask it something: your request, the file it is working on (a long file as the parts around the lines that matter), the other project files it needs, and the errors from its trial compiles. The visual fix also sends a picture of the page to the vision model.
        </p>
        <p class="help">
          Where that goes follows the model name under Assistant. A hosted model's name sends it to that service with your key. A name the app does not recognise as a hosted model goes to Ollama on this computer, and nothing leaves it.
        </p>
        <h3>Other network use</h3>
        <p class="help">The first compile downloads the TeX support files (no document text). Looking up a citation by DOI asks doi.org for that DOI, only when you use it.</p>
        <h3>Bug reports</h3>
        <p class="help">The report lists the version, your system, the assistant and bundle settings, and the project folder's name. It has no project text and no key.</p>
        <p class="help">
          To report a problem or say how it went, write to
          <a href={FEEDBACK_URL} target="_blank" rel="noreferrer noopener">{FEEDBACK_URL}</a>
          and paste the report there.
        </p>
        <button onclick={copyReport}>Copy diagnostics report</button>
        {#if reportNote}<p class="ok" role="status">{reportNote}</p>{/if}
        {#if report}<textarea class="report" readonly rows="6" aria-label="Diagnostics report" value={report}></textarea>{/if}
      {:else}
        <p class="help">These are fixed for now. F1 lists every command, including those without a shortcut.</p>
        <table>
          <tbody>
            {#each chords as c (c.id)}
              <tr><td>{c.title}</td><td><kbd>{formatChord(c.keys)}</kbd></td></tr>
            {/each}
          </tbody>
        </table>
      {/if}
    </div>
  </div>
</div>

<style>
  .overlay { position: fixed; inset: 0; display: grid; place-items: center; background: rgb(0 0 0 / 0.45); z-index: 800; }
  .dialog { width: min(36rem, 94vw); max-height: 84vh; display: flex; flex-direction: column; background: hsl(var(--popover)); color: hsl(var(--popover-foreground)); border: 1px solid hsl(var(--border)); border-radius: var(--radius-s); box-shadow: 0 12px 40px rgb(0 0 0 / 0.35); }
  header { display: flex; justify-content: space-between; align-items: center; padding: 0.8rem 1rem 0.4rem; }
  h2 { margin: 0; font-size: 1.1rem; }
  h3 { margin: 1rem 0 0.3rem; font-size: 0.95rem; }
  .x { background: none; border: none; color: inherit; font-size: 15px; cursor: pointer; }
  .tabs { display: flex; gap: 0.2rem; padding: 0 1rem; border-bottom: 1px solid hsl(var(--border)); }
  .tabs button { padding: 0.4rem 0.8rem; background: none; border: none; border-bottom: 2px solid transparent; color: hsl(var(--text-dim)); font: inherit; cursor: pointer; }
  .tabs button.on { color: inherit; border-bottom-color: hsl(var(--primary)); }
  .content { padding: 1rem; overflow: auto; font-size: 14px; display: flex; flex-direction: column; gap: 0.6rem; }
  .field { display: flex; align-items: center; gap: 0.6rem; }
  .field label { width: 12rem; flex-shrink: 0; }
  .field input:not([type='range']), .field select { flex: 1; min-width: 0; padding: 0.3rem 0.5rem; font: inherit; background: hsl(var(--surface-0)); color: inherit; border: 1px solid hsl(var(--border)); border-radius: var(--radius-xs); }
  .field input[type='range'] { flex: 1; }
  .val { width: 3rem; text-align: right; color: hsl(var(--text-dim)); }
  .check { display: flex; align-items: center; gap: 0.5rem; }
  .row { display: flex; gap: 0.5rem; }
  .help { margin: 0; font-size: 13px; color: hsl(var(--text-dim)); }
  .ok { margin: 0; color: hsl(var(--ok, 142 60% 40%)); }
  .bad { margin: 0; color: hsl(var(--err)); }
  button { align-self: flex-start; padding: 0.3rem 0.9rem; font: inherit; border: 1px solid hsl(var(--border)); border-radius: var(--radius-xs); background: hsl(var(--secondary)); color: hsl(var(--secondary-foreground)); cursor: pointer; }
  button.primary { background: hsl(var(--primary)); color: hsl(var(--primary-foreground)); border-color: transparent; }
  button:disabled { opacity: 0.5; cursor: default; }
  .report { width: 100%; box-sizing: border-box; font: 12px/1.4 var(--font-mono, monospace); background: hsl(var(--surface-0)); color: inherit; border: 1px solid hsl(var(--border)); border-radius: var(--radius-xs); padding: 0.4rem; resize: vertical; }
  table { border-collapse: collapse; width: 100%; font-size: 13px; }
  td { padding: 0.25rem 0; border-bottom: 1px solid hsl(var(--border)); }
  td:last-child { text-align: right; }
  kbd { padding: 0 0.35rem; border: 1px solid hsl(var(--border)); border-radius: var(--radius-xs); font-size: 12px; }
</style>
