<script>
  // Use a model that runs on this computer (Ollama): no account and no key, and
  // nothing leaves the machine. Asks the server whether Ollama is running and
  // which models it has; choosing one sets the assistant's model.
  import { onMount } from 'svelte'
  import { api } from '../lib/api/client.js'

  let { server, disabled = false, onchoose = () => {} } = $props()

  let found = $state(null)
  let checking = $state(false)
  let picked = $state('')
  let problem = $state('')

  const gb = (bytes) => `${(bytes / 1e9).toFixed(1)} GB`
  const current = $derived(server?.ai_local ? server.model : '')

  async function check() {
    checking = true
    problem = ''
    try {
      found = await api.getLocalModels()
      const names = found.models.map((m) => m.name)
      if (!names.includes(picked)) picked = names.includes(server?.model) ? server.model : (names[0] ?? '')
    } catch (e) {
      problem = e.message
    } finally {
      checking = false
    }
  }

  async function use() {
    problem = ''
    try {
      onchoose(await api.saveConfig({ model: picked }))
    } catch (e) {
      problem = e.message
    }
  }

  onMount(check)
</script>

<div class="picker">
  {#if checking && !found}
    <p class="help">Looking for Ollama on this computer…</p>
  {:else if found?.running && found.models.length > 0}
    <p class="help">Ollama is running with {found.models.length} model{found.models.length === 1 ? '' : 's'}. Pick one for the assistant.</p>
    <div class="row">
      <label for="local-model" class="sr">Local model</label>
      <select id="local-model" bind:value={picked} {disabled}>
        {#each found.models as m (m.name)}<option value={m.name}>{m.name} ({gb(m.size_bytes)})</option>{/each}
      </select>
      <button class="primary" onclick={use} disabled={disabled || !picked || picked === current}>Use this model</button>
    </div>
    {#if current}<p class="ok" role="status">The assistant is using {current}, on this computer.</p>{/if}
    <p class="help">A small model on a laptop is slow and makes more mistakes than a hosted one. Every change is still compiled and shown to you before anything is written.</p>
  {:else if found?.running}
    <p class="help">Ollama is running but has no models. In a terminal, run <code>ollama pull qwen2.5:7b</code>, then check again.</p>
    <button onclick={check} disabled={checking}>Check again</button>
  {:else}
    <p class="help">Ollama is not running. Install it from ollama.com, start it, and pull a model (for example <code>ollama pull qwen2.5:7b</code>), then check again.</p>
    <button onclick={check} disabled={checking}>{checking ? 'Checking…' : 'Check again'}</button>
  {/if}
  {#if problem}<p class="bad" role="alert">{problem}</p>{/if}
</div>

<style>
  .picker { display: flex; flex-direction: column; gap: 0.5rem; }
  .row { display: flex; gap: 0.5rem; align-items: center; }
  .row select { flex: 1; min-width: 0; padding: 0.3rem 0.5rem; font: inherit; background: hsl(var(--surface-0)); color: inherit; border: 1px solid hsl(var(--border)); border-radius: var(--radius-xs); }
  .help { margin: 0; font-size: 13px; color: hsl(var(--text-dim)); }
  .ok { margin: 0; color: hsl(var(--ok, 142 60% 40%)); }
  .bad { margin: 0; color: hsl(var(--err)); }
  .sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); }
  code { font-size: 12px; }
  button { align-self: flex-start; padding: 0.3rem 0.9rem; font: inherit; border: 1px solid hsl(var(--border)); border-radius: var(--radius-xs); background: hsl(var(--secondary)); color: hsl(var(--secondary-foreground)); cursor: pointer; }
  button.primary { background: hsl(var(--primary)); color: hsl(var(--primary-foreground)); border-color: transparent; }
  button:disabled { opacity: 0.5; cursor: default; }
</style>
