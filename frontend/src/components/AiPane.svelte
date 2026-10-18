<script>
  // The assistant: the conversation, and — when the AI has proposed changes —
  // the per-hunk review above it.
  import { aiStore, clearProposal } from '../stores/aiStore.js'
  import ChatPanel from './ChatPanel.svelte'
  import DiffView from './DiffView.svelte'

  let { aiEnabled = true, filePath = '', onsend = () => {}, onapply = () => {} } = $props()

  const proposal = $derived($aiStore.currentProposal)
  const busy = $derived($aiStore.status === 'streaming' || $aiStore.status === 'thinking')
</script>

<section class="ai-pane" aria-label="Assistant">
  {#if proposal?.edits?.length}
    <div class="proposal">
      <header>
        <strong>{proposal.verified ? 'Proposed changes (compiles)' : 'Proposed changes'}</strong>
        {#if proposal.files?.length > 1}<span class="files">{proposal.files.length} files</span>{/if}
      </header>
      <DiffView edits={proposal.edits} filePath={proposal.file ?? filePath} {onapply} onreject={clearProposal} />
    </div>
  {/if}
  <div class="chat">
    <ChatPanel
      messages={$aiStore.messages}
      loading={busy}
      streamingMessage={$aiStore.streamingMessage}
      {aiEnabled}
      on:message-sent={(e) => onsend(e.detail.content)}
    />
  </div>
</section>

<style>
  .ai-pane { display: flex; flex-direction: column; height: 100%; min-width: 0; background: hsl(var(--surface-1)); }
  .proposal { flex: 0 1 55%; min-height: 0; overflow: auto; border-bottom: 1px solid hsl(var(--border)); }
  .proposal header { display: flex; gap: 0.5rem; align-items: baseline; padding: 0.4rem 0.75rem; font-size: 13px; }
  .files { color: hsl(var(--text-dim)); font-size: 12px; }
  .chat { flex: 1; min-height: 0; }
</style>
