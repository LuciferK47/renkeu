<script>
  // Problems (errors and warnings, each a link to its source line) and the raw
  // TeX log of the build they came from.
  import { api } from '../lib/api/client.js'
  import LayoutReport from './LayoutReport.svelte'
  import SubmissionReport from './SubmissionReport.svelte'

  let { compile, onreveal = () => {}, onshow = () => {}, onfit = null, onclose = () => {} } = $props()

  let tab = $state('problems')
  let log = $state({ buildId: null, text: '', loading: false, error: null })

  const count = $derived(compile.errors.length + compile.warnings.length)

  // Fetch the log for the current build when the Log tab is open.
  $effect(() => {
    const id = compile.buildId
    if (tab !== 'log' || !id || log.buildId === id) return
    log = { buildId: id, text: '', loading: true, error: null }
    api
      .getBuildLog(id)
      .then((text) => {
        if (log.buildId === id) log = { buildId: id, text: String(text), loading: false, error: null }
      })
      .catch((e) => {
        if (log.buildId === id) log = { buildId: id, text: '', loading: false, error: e.message }
      })
  })

  const where = (p) => (p.file ? `${p.file}${p.line ? `:${p.line}` : ''}` : '')
</script>

<section class="problems" aria-label="Problems">
  <header>
    <div role="tablist">
      <button role="tab" aria-selected={tab === 'problems'} class:active={tab === 'problems'} onclick={() => (tab = 'problems')}>
        Problems
        {#if count > 0}<span class="count" class:bad={compile.errors.length > 0}>{count}</span>{/if}
      </button>
      <button role="tab" aria-selected={tab === 'layout'} class:active={tab === 'layout'} onclick={() => (tab = 'layout')}>Layout</button>
      <button role="tab" aria-selected={tab === 'submission'} class:active={tab === 'submission'} onclick={() => (tab = 'submission')}>Submission</button>
      <button role="tab" aria-selected={tab === 'log'} class:active={tab === 'log'} onclick={() => (tab = 'log')}>Log</button>
    </div>
    <button class="close" aria-label="Hide problems panel" onclick={onclose}>×</button>
  </header>

  <div class="body">
    {#if tab === 'problems'}
      {#if compile.candidates.length > 0}
        <p class="note">Several files could be the main document: {compile.candidates.join(', ')}. Choose one in the toolbar.</p>
      {/if}
      {#each [...compile.errors.map((p) => ({ p, kind: 'error' })), ...compile.warnings.map((p) => ({ p, kind: 'warning' }))] as { p, kind }, i (i)}
        <button
          class="item {kind}"
          disabled={!p.file}
          onclick={() => onreveal({ file: p.file, line: p.line })}
          aria-label={`${kind}: ${p.message}${p.file ? ` at ${where(p)}` : ''}`}
        >
          <span class="sev" aria-hidden="true">{kind === 'error' ? '✕' : '▲'}</span>
          <span class="msg">{p.message}{#if p.context}<small class="ctx"> — {p.context}</small>{/if}</span>
          {#if p.file}<span class="loc">{where(p)}</span>{/if}
        </button>
      {:else}
        {#if compile.status === 'success'}
          <p class="note ok">Compiled with no problems.</p>
        {:else if compile.status === 'idle'}
          <p class="note">Nothing compiled yet.</p>
        {/if}
      {/each}
    {:else if tab === 'layout'}
      <LayoutReport {compile} {onshow} {onreveal} {onfit} />
    {:else if tab === 'submission'}
      <SubmissionReport {compile} {onreveal} />
    {:else if !compile.buildId}
      <p class="note">No build yet — compile to see the log.</p>
    {:else if log.loading}
      <p class="note">Loading log…</p>
    {:else if log.error}
      <p class="note bad">Could not load the log: {log.error}</p>
    {:else}
      <pre class="log">{log.text}</pre>
    {/if}
  </div>
</section>

<style>
  .problems { display: flex; flex-direction: column; height: 100%; background: hsl(var(--surface-1)); border-top: 1px solid hsl(var(--border)); font-size: 13px; }
  header { display: flex; justify-content: space-between; align-items: center; border-bottom: 1px solid hsl(var(--border)); flex-shrink: 0; }
  [role='tablist'] { display: flex; }
  [role='tab'] { padding: 0.35rem 0.8rem; background: none; border: none; color: hsl(var(--text-dim)); font: inherit; cursor: pointer; }
  [role='tab'].active { color: hsl(var(--foreground)); box-shadow: inset 0 -2px 0 hsl(var(--primary)); }
  .count { margin-left: 0.3rem; padding: 0 0.4rem; border-radius: var(--radius-pill); background: hsl(var(--accent)); font-size: 11px; }
  .count.bad { background: hsl(var(--err)); color: hsl(var(--destructive-foreground)); }
  .close { background: none; border: none; cursor: pointer; color: hsl(var(--text-dim)); font-size: 16px; padding: 0 0.6rem; }
  .body { flex: 1; overflow: auto; }
  .item { display: flex; gap: 0.5rem; align-items: baseline; width: 100%; text-align: left; padding: 0.3rem 0.75rem; background: none; border: none; font: inherit; color: hsl(var(--foreground)); cursor: pointer; }
  .item:hover:not(:disabled) { background: hsl(var(--accent)); }
  .item:disabled { cursor: default; }
  .item.error .sev { color: hsl(var(--err)); }
  .item.warning .sev { color: hsl(var(--warn)); }
  .msg { flex: 1; }
  .ctx { font-family: 'JetBrains Mono', monospace; color: hsl(var(--text-dim)); }
  .loc { color: hsl(var(--text-dim)); font-family: 'JetBrains Mono', monospace; font-size: 12px; white-space: nowrap; }
  .note { padding: 0.5rem 0.75rem; color: hsl(var(--text-dim)); }
  .note.ok { color: hsl(var(--ok)); }
  .note.bad { color: hsl(var(--err)); }
  .log { margin: 0; padding: 0.5rem 0.75rem; font-family: 'JetBrains Mono', monospace; font-size: 12px; white-space: pre-wrap; word-break: break-word; }
</style>
