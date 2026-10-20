<script>
  // The assistant: the conversation (each request with what the assistant did
  // for it), the proposal to review, and the box to ask for something.
  import { tick } from 'svelte'
  import RunTimeline from './RunTimeline.svelte'
  import ProposalReview from './ProposalReview.svelte'
  import Bot from '../lib/Bot.svelte'

  let { ai, aiEnabled = true, hasFile = false, onsend = null, onsettings = () => {}, onopenfile = null } = $props()

  const SUGGESTIONS = [
    'Fix the compile errors in this file',
    'Make the table fit the page width',
    'Add a table of contents'
  ]

  let draft = $state('')
  let log = $state()
  const canSend = $derived(aiEnabled && hasFile && !ai.running && draft.trim().length > 0)

  function send(text = draft) {
    const content = text.trim()
    if (!content || !aiEnabled || !hasFile || ai.running) return
    draft = ''
    ;(onsend ?? ((c) => ai.send(c)))(content)
  }

  function keydown(e) {
    if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) {
      e.preventDefault()
      send()
    }
  }

  // Keep the newest step in view while a run is going.
  $effect(() => {
    void ai.messages.length
    void ai.messages.at(-1)?.run?.steps.length
    tick().then(() => log?.scrollTo?.({ top: log.scrollHeight }))
  })
</script>

<section class="pane" aria-label="Assistant">
  <header>
    <strong>Assistant</strong>
    <button onclick={() => ai.newConversation()} disabled={ai.messages.length === 0 && !ai.proposal}>New chat</button>
  </header>

  {#if ai.proposal?.edits?.length}
    <div class="proposal">
      <ProposalReview
        proposal={ai.proposal}
        busy={ai.status === 'applying'}
        onapply={(payload) => ai.apply(payload)}
        ondiscard={() => ai.discard()}
        {onopenfile}
      />
    </div>
  {/if}

  <div class="log" bind:this={log} aria-live="polite">
    {#if !aiEnabled}
      <div class="notice" role="note">
        <Bot mood="asleep" scale={5} />
        <p>The assistant is off: it needs an API key.</p>
        <button onclick={onsettings}>Open settings</button>
      </div>
    {:else if ai.messages.length === 0}
      <div class="empty">
        <Bot mood={ai.running ? 'working' : 'idle'} scale={4} />
        <p>Ask for a change to the open file. It tries the edits on a copy, checks they compile, then shows them for you to accept.</p>
        <ul>
          {#each SUGGESTIONS as s (s)}
            <li><button class="chip" onclick={() => send(s)} disabled={!hasFile}>{s}</button></li>
          {/each}
        </ul>
      </div>
    {/if}

    {#each ai.messages as m (m.id)}
      {#if m.role === 'user'}
        <p class="user">{m.text}</p>
      {:else}
        <div class="assistant"><RunTimeline run={m.run} /></div>
      {/if}
    {/each}
  </div>

  <form class="composer" onsubmit={(e) => (e.preventDefault(), send())}>
    <textarea
      bind:value={draft}
      onkeydown={keydown}
      rows="2"
      placeholder={hasFile ? 'What should change? (Shift+Enter for a new line)' : 'Open a file to ask for changes'}
      disabled={!aiEnabled || !hasFile}
      aria-label="Message the assistant"
    ></textarea>
    {#if ai.running}
      <button type="button" class="stop" onclick={() => ai.cancel()}>Stop</button>
    {:else}
      <button type="submit" class="send" disabled={!canSend}>Send</button>
    {/if}
  </form>
</section>

<style>
  .pane { display: flex; flex-direction: column; height: 100%; min-width: 0; background: hsl(var(--surface-1)); }
  header { display: flex; justify-content: space-between; align-items: center; padding: 0.4rem 0.75rem; border-bottom: 1px solid hsl(var(--border)); font-size: 13px; }
  .proposal { flex: 0 1 55%; min-height: 0; overflow: auto; border-bottom: 1px solid hsl(var(--border)); }
  .log { flex: 1; min-height: 0; overflow: auto; padding: 0.75rem; display: flex; flex-direction: column; gap: 0.75rem; }
  .user { align-self: flex-end; max-width: 90%; margin: 0; padding: 0.4rem 0.7rem; background: hsl(var(--primary) / 0.12); border-radius: var(--radius-s); white-space: pre-wrap; overflow-wrap: anywhere; font-size: 13px; }
  .assistant { align-self: stretch; }
  .empty, .notice { color: hsl(var(--text-dim)); font-size: 13px; }
  .empty ul { list-style: none; padding: 0; display: flex; flex-direction: column; gap: 0.4rem; align-items: flex-start; }
  .chip { text-align: left; padding: 0.3rem 0.6rem; background: hsl(var(--secondary)); color: hsl(var(--secondary-foreground)); border: 1px solid hsl(var(--border)); border-radius: var(--radius-s); font: inherit; cursor: pointer; }
  .notice { display: flex; flex-direction: column; align-items: flex-start; gap: 0.4rem; padding: 0.6rem; border: 1px solid hsl(var(--border)); border-radius: var(--radius-s); }
  .notice p { margin: 0 0 0.5rem; }
  .composer { display: flex; gap: 0.5rem; align-items: flex-end; padding: 0.5rem 0.75rem; border-top: 1px solid hsl(var(--border)); }
  textarea { flex: 1; resize: none; padding: 0.4rem 0.5rem; font: inherit; font-size: 13px; background: hsl(var(--surface-0)); color: inherit; border: 1px solid hsl(var(--border)); border-radius: var(--radius-xs); }
  button { padding: 0.3rem 0.8rem; font: inherit; border: 1px solid hsl(var(--border)); border-radius: var(--radius-xs); background: hsl(var(--secondary)); color: hsl(var(--secondary-foreground)); cursor: pointer; }
  button.send { background: hsl(var(--primary)); color: hsl(var(--primary-foreground)); border-color: transparent; }
  button.stop { background: hsl(var(--err)); color: white; border-color: transparent; }
  button:disabled { opacity: 0.5; cursor: default; }
</style>
