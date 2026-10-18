<script>
  // Shown when a save was refused because the file changed on disk after it was
  // opened. Both ways forward lose something, so the user chooses, and nothing
  // is decided for them.
  let { doc, onresolve = () => {}, ondismiss = () => {} } = $props()
</script>

<div class="overlay" role="presentation">
  <div class="dialog" role="alertdialog" aria-modal="true" aria-labelledby="conflict-title" aria-describedby="conflict-body">
    <h2 id="conflict-title">{doc.path} changed on disk</h2>
    <p id="conflict-body">
      The file was modified outside this editor after you opened it, and you have unsaved edits.
      Nothing has been overwritten.
    </p>
    <div class="actions">
      <button class="danger" onclick={() => onresolve('overwrite')}>Keep my version</button>
      <button onclick={() => onresolve('reload')}>Load the file from disk</button>
      <button class="quiet" onclick={ondismiss}>Decide later</button>
    </div>
  </div>
</div>

<style>
  .overlay { position: fixed; inset: 0; display: grid; place-items: center; background: rgb(0 0 0 / 0.45); z-index: 900; }
  .dialog { width: min(32rem, 92vw); padding: 1.25rem 1.5rem; background: hsl(var(--popover)); color: hsl(var(--popover-foreground)); border: 1px solid hsl(var(--border)); border-radius: var(--radius-s); box-shadow: 0 12px 40px rgb(0 0 0 / 0.35); }
  h2 { margin: 0 0 0.5rem; font-size: 1.05rem; }
  p { margin: 0 0 1rem; color: hsl(var(--text-dim)); font-size: 14px; }
  .actions { display: flex; gap: 0.5rem; flex-wrap: wrap; }
  button { padding: 0.4rem 0.9rem; font: inherit; border-radius: var(--radius-xs); border: 1px solid hsl(var(--border)); background: hsl(var(--secondary)); color: hsl(var(--secondary-foreground)); cursor: pointer; }
  .danger { background: hsl(var(--primary)); color: hsl(var(--primary-foreground)); border-color: transparent; }
  .quiet { background: none; border-color: transparent; color: hsl(var(--text-dim)); }
</style>
