<script>
  // Every registered command with its shortcut, filterable. This is the
  // registry's own listing, so it can never disagree with what the keys do.
  import { formatChord } from '../lib/commands.js'

  let { commands, onclose = () => {} } = $props()

  let query = $state('')
  let index = $state(0)
  let input = $state()

  const items = $derived(
    commands
      .list()
      .filter((c) => c.title.toLowerCase().includes(query.trim().toLowerCase()))
  )

  $effect(() => {
    input?.focus()
  })
  $effect(() => {
    void query
    index = 0
  })

  async function run(item) {
    if (!item?.enabled) return
    onclose()
    await commands.run(item.id)
  }

  function onkeydown(e) {
    if (e.key === 'Escape') onclose()
    else if (e.key === 'ArrowDown') (e.preventDefault(), (index = Math.min(index + 1, items.length - 1)))
    else if (e.key === 'ArrowUp') (e.preventDefault(), (index = Math.max(index - 1, 0)))
    else if (e.key === 'Enter') (e.preventDefault(), run(items[index]))
  }
</script>

<div class="overlay" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="menu" role="dialog" aria-modal="true" aria-label="Command menu">
    <input
      bind:this={input}
      bind:value={query}
      {onkeydown}
      placeholder="Type a command…"
      aria-label="Search commands"
      role="combobox"
      aria-expanded="true"
      aria-controls="command-list"
    />
    <ul id="command-list" role="listbox">
      {#each items as item, i (item.id)}
        <li role="option" aria-selected={i === index} aria-disabled={!item.enabled}>
          <button class:active={i === index} disabled={!item.enabled} onclick={() => run(item)}>
            <span>{item.title}</span>
            {#if item.keys}<kbd>{formatChord(item.keys)}</kbd>{/if}
          </button>
        </li>
      {:else}
        <li class="none">No command matches “{query}”.</li>
      {/each}
    </ul>
  </div>
</div>

<style>
  .overlay { position: fixed; inset: 0; background: rgb(0 0 0 / 0.35); display: flex; justify-content: center; padding-top: 12vh; z-index: 800; }
  .menu { width: min(34rem, 92vw); max-height: 60vh; display: flex; flex-direction: column; background: hsl(var(--popover)); border: 1px solid hsl(var(--border)); border-radius: var(--radius-s); box-shadow: 0 12px 40px rgb(0 0 0 / 0.35); overflow: hidden; align-self: flex-start; }
  input { padding: 0.7rem 0.9rem; font: inherit; background: none; border: none; border-bottom: 1px solid hsl(var(--border)); color: hsl(var(--foreground)); outline: none; }
  ul { list-style: none; margin: 0; padding: 0.25rem; overflow: auto; }
  button { display: flex; justify-content: space-between; width: 100%; padding: 0.4rem 0.7rem; font: inherit; background: none; border: none; border-radius: var(--radius-xs); color: hsl(var(--foreground)); cursor: pointer; text-align: left; }
  button.active { background: hsl(var(--accent)); }
  button:disabled { color: hsl(var(--text-faint)); cursor: default; }
  kbd { font-size: 11px; color: hsl(var(--text-dim)); }
  .none { padding: 0.7rem; color: hsl(var(--text-dim)); }
</style>
