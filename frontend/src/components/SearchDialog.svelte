<script>
  // Find in the project: every text file, literal or regular expression,
  // matches grouped by file. Clicking one opens the file at that line. "Replace
  // all" rewrites the files through the server, so each change is in History.
  import { api } from '../lib/api/client.js'

  let { onopen = () => {}, onreplaced = () => {}, onclose = () => {} } = $props()

  let query = $state('')
  let replacement = $state('')
  let regex = $state(false)
  let caseSensitive = $state(false)
  let showReplace = $state(false)
  let result = $state(null)
  let problem = $state('')
  let busy = $state(false)
  let input = $state()

  $effect(() => input?.focus())

  // Search shortly after typing stops; an older answer never replaces a newer one.
  let ticket = 0
  $effect(() => {
    const body = { query, regex, case_sensitive: caseSensitive }
    const mine = ++ticket
    if (!query) {
      result = null
      problem = ''
      return
    }
    const timer = setTimeout(async () => {
      try {
        const found = await api.searchProject(body)
        if (mine === ticket) (result = found, (problem = ''))
      } catch (e) {
        if (mine === ticket) (result = null, (problem = e.message))
      }
    }, 250)
    return () => clearTimeout(timer)
  })

  const parts = (hit) => {
    const chars = [...hit.text]
    return [chars.slice(0, hit.start).join(''), chars.slice(hit.start, hit.end).join(''), chars.slice(hit.end).join('')]
  }

  const fileCount = $derived(result?.files.length ?? 0)

  async function replaceAll() {
    if (!result || result.total === 0 || busy) return
    const what = `${result.total} match${result.total === 1 ? '' : 'es'} in ${fileCount} file${fileCount === 1 ? '' : 's'}`
    if (!window.confirm(`Replace ${what} with "${replacement}"? Each file's previous text stays in History.`)) return
    busy = true
    try {
      const done = await api.replaceProject({ query, regex, case_sensitive: caseSensitive, replacement })
      onreplaced(done)
      result = null
      problem = done.skipped.length ? `Not changed (edited meanwhile): ${done.skipped.join(', ')}` : ''
      query = ''
    } catch (e) {
      problem = e.message
    } finally {
      busy = false
    }
  }
</script>

<div class="overlay" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="box" role="dialog" aria-modal="true" aria-label="Find in project" tabindex="-1" onkeydown={(e) => e.key === 'Escape' && onclose()}>
    <div class="bar">
      <input bind:this={input} bind:value={query} placeholder="Find in all files" aria-label="Find in project" autocomplete="off" spellcheck="false" />
      <button type="button" class="toggle" aria-pressed={caseSensitive} aria-label="Match case" title="Match case" onclick={() => (caseSensitive = !caseSensitive)}>Aa</button>
      <button type="button" class="toggle" aria-pressed={regex} aria-label="Regular expression" title="Regular expression" onclick={() => (regex = !regex)}>.*</button>
      <button type="button" class="toggle" aria-pressed={showReplace} title="Replace" aria-label="Show replace" onclick={() => (showReplace = !showReplace)}>⇄</button>
    </div>
    {#if showReplace}
      <div class="bar">
        <input bind:value={replacement} placeholder={regex ? 'Replace with ($1 for groups)' : 'Replace with'} aria-label="Replace with" autocomplete="off" spellcheck="false" />
        <button type="button" class="primary" disabled={!result || result.total === 0 || busy} onclick={replaceAll}>Replace all</button>
      </div>
    {/if}

    {#if problem}<p class="note bad" role="alert">{problem}</p>{/if}
    {#if result}
      <p class="note" aria-live="polite">
        {result.total === 0 ? 'No matches.' : `${result.total} match${result.total === 1 ? '' : 'es'} in ${fileCount} file${fileCount === 1 ? '' : 's'}${result.truncated ? ' (first 500 shown)' : ''}`}
      </p>
      <div class="results">
        {#each result.files as file (file.path)}
          <section>
            <h3>{file.path}</h3>
            <ul>
              {#each file.hits as hit (`${hit.line}:${hit.column}`)}
                {@const [a, m, z] = parts(hit)}
                <li>
                  <button type="button" onclick={() => onopen({ file: file.path, line: hit.line })}>
                    <span class="pos">{hit.line}</span><span class="text">{a}<mark>{m}</mark>{z}</span>
                  </button>
                </li>
              {/each}
            </ul>
          </section>
        {/each}
      </div>
    {/if}
  </div>
</div>

<style>
  .overlay { position: fixed; inset: 0; display: flex; justify-content: center; align-items: flex-start; padding-top: 8vh; background: rgb(0 0 0 / 0.35); z-index: 700; }
  .box { width: min(46rem, 94vw); max-height: 78vh; display: flex; flex-direction: column; gap: 0.5rem; padding: 0.8rem; background: hsl(var(--popover)); border: 1px solid hsl(var(--border)); border-radius: var(--radius-s); box-shadow: 0 12px 40px rgb(0 0 0 / 0.35); }
  .bar { display: flex; gap: 0.4rem; }
  input { flex: 1; min-width: 0; padding: 0.45rem 0.65rem; font: inherit; font-size: 14px; color: inherit; background: hsl(var(--surface-0)); border: 1px solid hsl(var(--border)); border-radius: var(--radius-xs); }
  button { padding: 0.3rem 0.7rem; font: inherit; color: hsl(var(--secondary-foreground)); background: hsl(var(--secondary)); border: 1px solid hsl(var(--border)); border-radius: var(--radius-xs); cursor: pointer; }
  button.toggle[aria-pressed='true'] { color: hsl(var(--primary-foreground)); background: hsl(var(--primary)); border-color: transparent; }
  button.primary { color: hsl(var(--primary-foreground)); background: hsl(var(--primary)); border-color: transparent; }
  button:disabled { opacity: 0.5; cursor: default; }
  .note { margin: 0; font-size: 12px; color: hsl(var(--text-dim)); }
  .note.bad { color: hsl(var(--err)); }
  .results { overflow-y: auto; min-height: 0; }
  h3 { margin: 0.5rem 0 0.15rem; font-size: 12px; color: hsl(var(--text-dim)); font-weight: 600; }
  ul { list-style: none; margin: 0; padding: 0; }
  li button { display: flex; gap: 0.6rem; width: 100%; padding: 0.15rem 0.4rem; text-align: left; font-size: 13px; background: none; border: none; color: inherit; }
  li button:hover, li button:focus-visible { background: hsl(var(--accent)); outline: none; }
  .pos { width: 2.5rem; flex-shrink: 0; text-align: right; color: hsl(var(--text-faint)); font-variant-numeric: tabular-nums; }
  .text { font-family: 'JetBrains Mono', monospace; font-size: 12px; white-space: pre; overflow: hidden; text-overflow: ellipsis; }
  mark { background: hsl(var(--primary) / 0.35); color: inherit; border-radius: 2px; }
</style>
