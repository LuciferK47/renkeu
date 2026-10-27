<script>
  // What the assistant did for one request, step by step: each attempt, whether
  // the edits compiled, and how the run ended. Plain text throughout (nothing
  // from the model is ever rendered as HTML).
  let { run } = $props()

  const MARK = { active: '…', ok: '✓', failed: '✕' }
  const OUTCOME_TEXT = {
    success: 'Finished.',
    review: 'Ready for your review.',
    cancelled: 'Stopped.',
    failed: 'Stopped without a result.'
  }
</script>

<ol class="timeline" aria-label="What the assistant did">
  {#each run.steps as step (step.id)}
    <li class={step.state}>
      <span class="mark" aria-hidden="true">{MARK[step.state]}</span>
      <span class="body">
        <span class="text">{step.text}</span>
        {#if step.detail}<span class="detail">{step.detail}</span>{/if}
      </span>
      {#if step.state === 'active'}<span class="sr-only" role="status">in progress</span>{/if}
    </li>
  {/each}
  {#if run.outcome}
    <li class="outcome {run.outcome}">
      {OUTCOME_TEXT[run.outcome]}{#if run.note && (run.outcome === 'failed' || run.outcome === 'success')} {run.note}{/if}
    </li>
  {/if}
</ol>

<style>
  .timeline { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 0.35rem; font-size: 13px; }
  li { display: flex; gap: 0.5rem; align-items: baseline; }
  .mark { width: 1rem; text-align: center; flex-shrink: 0; color: hsl(var(--text-dim)); }
  .ok .mark { color: hsl(var(--ok, 142 60% 40%)); }
  .failed .mark { color: hsl(var(--err)); }
  .active .mark { animation: pulse 1s ease-in-out infinite; }
  .body { display: flex; flex-direction: column; min-width: 0; }
  .detail { color: hsl(var(--text-dim)); font-size: 12px; overflow-wrap: anywhere; white-space: pre-wrap; }
  .outcome { margin-top: 0.15rem; padding-left: 1.5rem; font-weight: 600; color: hsl(var(--text-dim)); }
  .outcome.failed { color: hsl(var(--err)); font-weight: 500; }
  .sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); }
  @keyframes pulse { 50% { opacity: 0.3; } }
  @media (prefers-reduced-motion: reduce) { .active .mark { animation: none; } }
</style>
