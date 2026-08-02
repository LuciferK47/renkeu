<script>
  import { createEventDispatcher, onMount } from 'svelte'
  import VirtualList from 'svelte-virtual-list'

  export let filePath = ''
  export let agentOnly = false

  const dispatch = createEventDispatcher()
  let history = []
  let filterAgentOnly = agentOnly

  onMount(async () => {
    if (filePath) await loadHistory()
  })

  // Reactive: reload history when filePath or filter changes
  $: if (filePath) {
    loadHistory()
  }

  async function loadHistory() {
    try {
      const params = new URLSearchParams({ limit: '1000' })
      if (filePath) params.set('path', filePath)
      if (filterAgentOnly) params.set('agent_only', 'true')
      const res = await fetch(`/api/history?${params.toString()}`)
      if (!res.ok) return
      history = await res.json()
    } catch (e) {
      console.error('Failed to load history:', e)
    }
  }

  function setFilter(agentOnly) {
    filterAgentOnly = agentOnly
  }

  function formatDate(timestamp) {
    return new Date(timestamp).toLocaleString()
  }

  function handleRestore(id) {
    dispatch('restore', id)
  }
</script>

<div class="history-sidebar">
  <h3>History</h3>
  <div class="filter-toggle" role="group" aria-label="History filter">
    <button
      class:active={!filterAgentOnly}
      on:click={() => setFilter(false)}
      aria-pressed={!filterAgentOnly}
    >
      All
    </button>
    <button
      class:active={filterAgentOnly}
      on:click={() => setFilter(true)}
      aria-pressed={filterAgentOnly}
    >
      Agent
    </button>
  </div>
  {#if history.length === 0}
    <p class="no-history">No history yet</p>
  {:else}
    <div class="history-list">
      <VirtualList items={history} let:item height="calc(100vh - 140px)">
        <div class="history-item">
          <div class="command">{item.command}</div>
          <div class="timestamp">{formatDate(item.timestamp)}</div>
          <button
            on:click={() => handleRestore(item.id)}
            class="restore-btn"
          >
            Restore
          </button>
        </div>
      </VirtualList>
    </div>
  {/if}
</div>

<style>
  .history-sidebar {
    padding: 1rem;
    height: 100%;
    overflow-y: auto;
  }

  h3 {
    margin: 0 0 1rem 0;
    font-size: 16px;
  }

  .filter-toggle {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.5rem;
    margin-bottom: 0.75rem;
  }

  .filter-toggle button {
    padding: 0.4rem;
    border: 1px solid hsl(var(--border));
    background: hsl(var(--surface-2));
    border-radius: 4px;
    cursor: pointer;
    font-size: 12px;
  }

  .filter-toggle button.active {
    background: hsl(var(--primary) / 0.12);
    border-color: hsl(var(--primary));
    color: hsl(var(--primary));
  }

  .no-history {
    color: hsl(var(--text-faint));
    text-align: center;
    padding: 2rem 0;
  }

  .history-list {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }

  .history-item {
    padding: 0.75rem;
    background: hsl(var(--surface-2));
    border: 1px solid hsl(var(--border));
    border-radius: 4px;
  }

  .command {
    font-weight: bold;
    font-size: 13px;
    margin-bottom: 0.25rem;
  }

  .timestamp {
    font-size: 11px;
    color: hsl(var(--text-faint));
    margin-bottom: 0.5rem;
  }

  .restore-btn {
    width: 100%;
    padding: 0.4rem;
    background: hsl(var(--ok));
    color: hsl(var(--primary-foreground));
    border: none;
    border-radius: 3px;
    cursor: pointer;
    font-size: 12px;
  }

  .restore-btn:hover {
    background: hsl(var(--ok));
  }
</style>
