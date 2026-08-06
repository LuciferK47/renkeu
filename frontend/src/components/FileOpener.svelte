<script>
  import Icon from './Icon.svelte'
  import { onMount, onDestroy } from 'svelte'
  import { createEventDispatcher } from 'svelte'

  export let visible = false
  export let recent = []
  export let pinned = []

  const dispatch = createEventDispatcher()

  let query = ''
  let results = []
  let loading = false
  let error = ''
  let debounceId
  let dialogEl

  onMount(() => {
    if (visible) {
      fetchResults()
    }

    const handleKey = (e) => {
      if (!visible) return
      if (e.key === 'Escape') {
        e.stopPropagation()
        close()
      }
      if (e.key === 'Tab' && dialogEl) {
        const focusable = dialogEl.querySelectorAll('button, input, select, textarea, [tabindex]:not([tabindex="-1"])')
        const first = focusable[0]
        const last = focusable[focusable.length - 1]
        if (!first || !last) return
        if (e.shiftKey && document.activeElement === first) {
          e.preventDefault()
          last.focus()
        } else if (!e.shiftKey && document.activeElement === last) {
          e.preventDefault()
          first.focus()
        }
      }
    }

    window.addEventListener('keydown', handleKey)

    return () => {
      window.removeEventListener('keydown', handleKey)
    }
  })

  onDestroy(() => {})

  $: if (visible) {
    fetchResults()
  }

  function close() {
    dispatch('close')
  }

  function onSelect(path) {
    dispatch('open', { path })
  }

  function onPin(path) {
    dispatch('pin', { path })
  }

  function onUnpin(path) {
    dispatch('unpin', { path })
  }

  function handleInput(e) {
    query = e.target.value
    if (debounceId) clearTimeout(debounceId)
    debounceId = setTimeout(() => fetchResults(), 200)
  }

  async function fetchResults() {
    if (!visible) return
    loading = true
    error = ''
    try {
      const params = new URLSearchParams({ limit: '40', offset: '0' })
      if (query.trim()) params.set('search', query.trim())
      const res = await fetch(`/api/files?${params.toString()}`)
      if (!res.ok) throw new Error('Failed to load files')
      const data = await res.json()
      results = data.files || []
    } catch (e) {
      error = e.message
    } finally {
      loading = false
    }
  }
</script>

{#if visible}
  <div class="overlay">
    <button
      class="overlay-backdrop"
      type="button"
      aria-label="Close file opener"
      on:click={close}
    ></button>
    <div class="dialog" role="dialog" aria-modal="true" aria-label="Quick file open" tabindex="-1" bind:this={dialogEl}>
      <div class="header">
        <input
          type="text"
          placeholder="Type to search files..."
          value={query}
          on:input={handleInput}
          class="search"
        />
        <button class="close" on:click={close} aria-label="Close file opener">×</button>
      </div>

      {#if pinned && pinned.length > 0}
        <div class="section">
          <div class="section-title">Pinned</div>
          {#each pinned as path}
            <div
              class="file"
              role="button"
              tabindex="0"
              on:click={() => onSelect(path)}
              on:keydown={(e) => (e.key === 'Enter' || e.key === ' ') && onSelect(path)}
            >
              <span>{path}</span>
              <button class="pin" type="button" aria-label={`Unpin ${path}`} on:click|stopPropagation={() => onUnpin(path)}><Icon name="star" /></button>
            </div>
          {/each}
        </div>
      {/if}

      {#if recent && recent.length > 0}
        <div class="section">
          <div class="section-title">Recent</div>
          {#each recent as path}
            <div
              class="file"
              role="button"
              tabindex="0"
              on:click={() => onSelect(path)}
              on:keydown={(e) => (e.key === 'Enter' || e.key === ' ') && onSelect(path)}
            >
              <span>{path}</span>
              <button class="pin" type="button" aria-label={`Pin ${path}`} on:click|stopPropagation={() => onPin(path)}><Icon name="star" /></button>
            </div>
          {/each}
        </div>
      {/if}

      <div class="section">
        <div class="section-title">Search Results</div>
        {#if loading}
          <div class="hint">Loading...</div>
        {:else if error}
          <div class="hint error">{error}</div>
        {:else if results.length === 0}
          <div class="hint">No files found</div>
        {:else}
          {#each results as file}
            <div
              class="file"
              role="button"
              tabindex="0"
              on:click={() => onSelect(file.path)}
              on:keydown={(e) => (e.key === 'Enter' || e.key === ' ') && onSelect(file.path)}
            >
              <span>{file.path}</span>
              <button class="pin" type="button" aria-label={`Pin ${file.path}`} on:click|stopPropagation={() => onPin(file.path)}><Icon name="star" /></button>
            </div>
          {/each}
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .overlay {
    position: fixed;
    inset: 0;
    display: flex;
    align-items: flex-start;
    justify-content: center;
    padding-top: 10vh;
    z-index: 900;
    overflow: hidden;
  }

  .overlay-backdrop {
    position: absolute;
    inset: 0;
    background: rgba(0,0,0,0.35);
    border: none;
    padding: 0;
    margin: 0;
    cursor: pointer;
    z-index: 0;
  }

  .dialog {
    position: relative;
    z-index: 1;
    width: 720px;
    max-height: 70vh;
    background: hsl(var(--surface-1));
    color: hsl(var(--foreground));
    border-radius: var(--radius-m);
    box-shadow: 0 24px 60px rgba(0,0,0,0.35);
    overflow: auto;
    padding: 16px;
  }

  .header {
    display: flex;
    gap: 8px;
    align-items: center;
    margin-bottom: 12px;
  }

  .search {
    flex: 1;
    padding: 12px 14px;
    border-radius: var(--radius-m);
    border: 1px solid hsl(var(--border));
    background: hsl(var(--surface-1));
    color: hsl(var(--foreground));
    font-size: 14px;
  }

  .close {
    background: transparent;
    color: hsl(var(--text-faint));
    border: none;
    font-size: 22px;
    cursor: pointer;
  }

  .section {
    margin-top: 10px;
    border-top: 1px solid hsl(var(--border));
    padding-top: 10px;
  }

  .section-title {
    font-size: 12px;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: hsl(var(--text-faint));
    margin-bottom: 6px;
  }

  .file {
    width: 100%;
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 10px 12px;
    background: hsl(var(--surface-1));
    border: 1px solid hsl(var(--border));
    border-radius: var(--radius-s);
    color: hsl(var(--foreground));
    cursor: pointer;
    margin-bottom: 6px;
    text-align: left;
  }

  .file:hover {
    border-color: hsl(var(--primary));
    background: hsl(var(--surface-2));
  }

  .pin {
    color: hsl(var(--warn));
    font-size: 14px;
    cursor: pointer;
  }

  .hint {
    color: hsl(var(--text-faint));
    padding: 6px;
  }

  .hint.error {
    color: hsl(var(--err));
  }
</style>
