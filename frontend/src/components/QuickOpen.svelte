<script>
  // Open a file by typing part of its name. Ranking is the server's (the same
  // fuzzy matcher as everything else); with nothing typed, recently opened
  // files come first.
  import { api } from '../lib/api/client.js'

  let { recent = [], onopen = () => {}, onclose = () => {} } = $props()

  let query = $state('')
  let hits = $state([])
  let active = $state(0)
  let error = $state('')
  /** Whether the server has answered at least once, so "nothing found" is true. */
  let loaded = $state(false)
  /** The query the current `hits` answer; until it matches what is typed they are stale. */
  let answered = ''
  /** Enter was pressed before the answer arrived: open the first hit when it does. */
  let openWhenReady = false
  let input = $state()
  let latest = 0

  const show = $derived.by(() => {
    if (query.trim()) return hits
    const rest = hits.filter((h) => !recent.includes(h))
    return [...recent.filter((r) => hits.includes(r)), ...rest]
  })

  $effect(() => input?.focus())

  // Ask the server, ignoring answers that arrive after a newer question.
  $effect(() => {
    const q = query.trim()
    const mine = ++latest
    const timer = setTimeout(async () => {
      try {
        const res = await api.searchWorkspace(q)
        if (mine !== latest) return
        hits = res.hits.map((h) => h.value).slice(0, 60)
        active = 0
        error = ''
        loaded = true
        answered = q
        if (openWhenReady) {
          openWhenReady = false
          if (show[0]) onopen(show[0])
        }
      } catch (e) {
        if (mine === latest) error = e.message
      }
    }, q ? 120 : 0)
    return () => clearTimeout(timer)
  })

  function keydown(e) {
    if (e.key === 'Escape') onclose()
    else if (e.key === 'ArrowDown') (e.preventDefault(), (active = Math.min(active + 1, show.length - 1)))
    else if (e.key === 'ArrowUp') (e.preventDefault(), (active = Math.max(active - 1, 0)))
    else if (e.key === 'Enter') {
      e.preventDefault()
      // The list still shows the previous query's answer: opening its top entry
      // would open a file that has nothing to do with what was typed.
      if (answered !== query.trim()) openWhenReady = true
      else if (show[active]) onopen(show[active])
    }
  }
</script>

<div class="overlay" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="box" role="dialog" aria-modal="true" aria-label="Open file" tabindex="-1" onkeydown={keydown}>
    <input
      bind:this={input}
      bind:value={query}
      placeholder="Type to find a file…"
      aria-label="Search files"
      role="combobox"
      aria-expanded="true"
      aria-controls="quick-open-list"
      autocomplete="off"
      spellcheck="false"
    />
    {#if error}<p class="bad" role="alert">{error}</p>{/if}
    <ul id="quick-open-list" role="listbox" aria-label="Files">
      {#each show as path, i (path)}
        <li role="option" aria-selected={i === active}>
          <button class:on={i === active} onclick={() => onopen(path)} onmousemove={() => (active = i)}>
            {path}
            {#if !query.trim() && recent.includes(path)}<span class="tag">recent</span>{/if}
          </button>
        </li>
      {:else}
        {#if loaded}<li class="empty">{query.trim() ? 'No matching file.' : 'No files yet.'}</li>{/if}
      {/each}
    </ul>
  </div>
</div>

<style>
  .overlay { position: fixed; inset: 0; display: flex; justify-content: center; padding-top: 12vh; background: rgb(0 0 0 / 0.35); z-index: 700; }
  .box { align-self: flex-start; width: min(34rem, 92vw); max-height: 70vh; display: flex; flex-direction: column; background: hsl(var(--popover)); color: hsl(var(--popover-foreground)); border: 1px solid hsl(var(--border)); border-radius: var(--radius-s); box-shadow: 0 12px 40px rgb(0 0 0 / 0.35); overflow: hidden; }
  input { margin: 0.6rem; padding: 0.5rem 0.65rem; font: inherit; font-size: 15px; background: hsl(var(--surface-0)); color: inherit; border: 1px solid hsl(var(--border)); border-radius: var(--radius-xs); }
  ul { list-style: none; margin: 0; padding: 0 0 0.4rem; overflow: auto; }
  li button { display: flex; justify-content: space-between; width: 100%; text-align: left; padding: 0.35rem 0.9rem; background: none; border: none; font: inherit; font-size: 13px; color: inherit; cursor: pointer; }
  li button.on { background: hsl(var(--accent)); }
  .tag { font-size: 11px; color: hsl(var(--text-dim)); }
  .empty { padding: 0.6rem 0.9rem; color: hsl(var(--text-dim)); font-size: 13px; }
  .bad { margin: 0 0.9rem 0.4rem; color: hsl(var(--err)); font-size: 13px; }
</style>
