<script>
  // Shown once, on first launch. Two things a new user would otherwise trip
  // over: the first compile downloads the TeX support files (so do it now, in
  // the open), and the assistant needs either a key or a model on this computer.
  // Nothing here is required: "Start writing" works with the assistant off.
  import { onMount } from 'svelte'
  import { api } from '../lib/api/client.js'
  import LocalModelPicker from './LocalModelPicker.svelte'

  let { onclose = () => {} } = $props()

  let tex = $state('working') // 'working' | 'ready' | 'failed'
  let texMessage = $state('')
  let seconds = $state(0)
  let server = $state(null)
  let choice = $state('off') // 'off' | 'local' | 'key'
  let keyInput = $state('')
  let busy = $state(false)
  let problem = $state('')

  async function prepare() {
    tex = 'working'
    texMessage = ''
    seconds = 0
    const tick = setInterval(() => (seconds += 1), 1000)
    try {
      const r = await api.prepareTex()
      tex = r.ok ? 'ready' : 'failed'
      texMessage = r.message ?? ''
    } catch (e) {
      tex = 'failed'
      texMessage = e.message
    } finally {
      clearInterval(tick)
    }
  }

  async function saveKey() {
    busy = true
    problem = ''
    try {
      server = await api.setApiKey(keyInput.trim())
      keyInput = ''
    } catch (e) {
      problem = e.message
    } finally {
      busy = false
    }
  }

  async function finish() {
    try {
      await api.finishSetup()
    } catch {
      /* it will be offered again next time, which is harmless */
    }
    onclose()
  }

  onMount(async () => {
    try {
      server = await api.getConfig()
      if (server.ai_local) choice = 'local'
      else if (server.ai_enabled) choice = 'key'
    } catch {
      /* the choices still work once the server answers */
    }
    prepare()
  })
</script>

<div class="overlay" role="presentation">
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="welcome-title" tabindex="-1" onkeydown={(e) => e.key === 'Escape' && finish()}>
    <h2 id="welcome-title">Welcome to Renkeu</h2>
    <p class="help">Two quick things, and both are optional. You can change either later in Settings.</p>

    <section>
      <h3>TeX support files</h3>
      {#if tex === 'working'}
        <p class="help" role="status">Downloading the support files, once ({seconds} s). They are about 100 MB and are kept on this computer. You can keep going while this runs.</p>
      {:else if tex === 'ready'}
        <p class="ok" role="status">Ready. Compiling works, and later compiles do not need the internet.</p>
      {:else}
        <p class="bad" role="alert">{texMessage}</p>
        <button onclick={prepare}>Try again</button>
      {/if}
    </section>

    <section>
      <h3>The assistant</h3>
      <div class="choices" role="radiogroup" aria-label="Assistant">
        <label class:on={choice === 'off'}><input type="radio" name="ai" value="off" bind:group={choice} /> <span><b>No assistant</b><small>Compiling, layout, preflight and jumping between source and PDF all still work.</small></span></label>
        <label class:on={choice === 'local'}><input type="radio" name="ai" value="local" bind:group={choice} /> <span><b>A model on this computer</b><small>Uses Ollama. No account, no key, and nothing leaves your machine.</small></span></label>
        <label class:on={choice === 'key'}><input type="radio" name="ai" value="key" bind:group={choice} /> <span><b>My own key</b><small>A Groq key. Your requests go to that service.</small></span></label>
      </div>

      {#if choice === 'local'}
        <LocalModelPicker {server} onchoose={(config) => (server = config)} />
      {:else if choice === 'key'}
        {#if server?.key_source === 'env'}
          <p class="ok" role="status">A key is already set in the environment.</p>
        {:else}
          <div class="row">
            <label for="welcome-key" class="sr">Groq API key</label>
            <input id="welcome-key" type="password" autocomplete="off" spellcheck="false" placeholder="Groq API key" bind:value={keyInput} disabled={busy} />
            <button class="primary" onclick={saveKey} disabled={busy || !keyInput.trim()}>Save key</button>
          </div>
          {#if server?.key_source === 'file'}<p class="ok" role="status">A key is saved on this computer.</p>{/if}
        {/if}
      {/if}
      {#if problem}<p class="bad" role="alert">{problem}</p>{/if}
    </section>

    <footer>
      <button class="primary" onclick={finish}>Start writing</button>
    </footer>
  </div>
</div>

<style>
  .overlay { position: fixed; inset: 0; display: grid; place-items: center; background: rgb(0 0 0 / 0.55); z-index: 900; }
  .dialog { width: min(34rem, 94vw); max-height: 88vh; overflow: auto; display: flex; flex-direction: column; gap: 0.7rem; padding: 1.2rem 1.3rem; background: hsl(var(--popover)); color: hsl(var(--popover-foreground)); border: 1px solid hsl(var(--border)); border-radius: var(--radius-s); box-shadow: 0 12px 40px rgb(0 0 0 / 0.4); }
  h2 { margin: 0; font-size: 1.2rem; }
  h3 { margin: 0 0 0.3rem; font-size: 0.95rem; }
  section { display: flex; flex-direction: column; gap: 0.4rem; padding-top: 0.6rem; border-top: 1px solid hsl(var(--border)); }
  .choices { display: flex; flex-direction: column; gap: 0.35rem; }
  .choices label { display: flex; gap: 0.6rem; align-items: flex-start; padding: 0.45rem 0.6rem; border: 1px solid hsl(var(--border)); border-radius: var(--radius-xs); cursor: pointer; }
  .choices label.on { border-color: hsl(var(--primary)); }
  .choices span { display: flex; flex-direction: column; }
  .choices small { color: hsl(var(--text-dim)); font-size: 12px; }
  .row { display: flex; gap: 0.5rem; align-items: center; }
  .row input { flex: 1; min-width: 0; padding: 0.3rem 0.5rem; font: inherit; background: hsl(var(--surface-0)); color: inherit; border: 1px solid hsl(var(--border)); border-radius: var(--radius-xs); }
  .help { margin: 0; font-size: 13px; color: hsl(var(--text-dim)); }
  .ok { margin: 0; font-size: 13px; color: hsl(var(--ok, 142 60% 40%)); }
  .bad { margin: 0; font-size: 13px; color: hsl(var(--err)); }
  .sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); }
  footer { display: flex; justify-content: flex-end; padding-top: 0.4rem; }
  button { align-self: flex-start; padding: 0.3rem 0.9rem; font: inherit; border: 1px solid hsl(var(--border)); border-radius: var(--radius-xs); background: hsl(var(--secondary)); color: hsl(var(--secondary-foreground)); cursor: pointer; }
  button.primary { background: hsl(var(--primary)); color: hsl(var(--primary-foreground)); border-color: transparent; }
  button:disabled { opacity: 0.5; cursor: default; }
</style>
