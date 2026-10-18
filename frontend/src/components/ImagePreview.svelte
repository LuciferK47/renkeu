<script>
  import { api } from '../lib/api/client.js'
  let { path, onclose = () => {} } = $props()
  let failed = $state(false)
</script>

<div class="overlay" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="box" role="dialog" aria-modal="true" aria-label={`Preview of ${path}`} tabindex="-1" onkeydown={(e) => e.key === 'Escape' && onclose()}>
    <header>
      <span class="name">{path}</span>
      <button onclick={onclose} aria-label="Close preview">✕</button>
    </header>
    {#if failed}
      <p class="bad" role="alert">This file could not be displayed.</p>
    {:else}
      <img src={api.rawUrl(path)} alt={path} onerror={() => (failed = true)} />
    {/if}
  </div>
</div>

<style>
  .overlay { position: fixed; inset: 0; display: grid; place-items: center; background: rgb(0 0 0 / 0.6); z-index: 850; }
  .box { max-width: 92vw; max-height: 90vh; display: flex; flex-direction: column; background: hsl(var(--popover)); border: 1px solid hsl(var(--border)); border-radius: var(--radius-s); overflow: hidden; }
  header { display: flex; justify-content: space-between; align-items: center; gap: 1rem; padding: 0.4rem 0.75rem; font-size: 13px; color: hsl(var(--text-dim)); }
  header button { background: none; border: none; color: inherit; cursor: pointer; font-size: 14px; }
  img { max-width: 90vw; max-height: 80vh; object-fit: contain; background: repeating-conic-gradient(hsl(var(--surface-1)) 0% 25%, hsl(var(--surface-0)) 0% 50%) 50% / 16px 16px; }
  .bad { padding: 1rem; color: hsl(var(--err)); }
</style>
