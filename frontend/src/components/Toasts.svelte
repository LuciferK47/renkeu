<script>
  export let toasts = []
</script>

<div class="toast-stack" aria-live="polite" aria-atomic="true">
  {#each toasts as toast (toast.id)}
    <div class={`toast ${toast.type}`} role="status">
      <span class="msg">{toast.message}</span>
    </div>
  {/each}
</div>

<style>
  .toast-stack {
    position: fixed;
    top: 16px;
    right: 16px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    z-index: 1000;
  }

  .toast {
    min-width: 220px;
    padding: 10px 14px;
    border-radius: 6px;
    box-shadow: 0 4px 14px rgba(0,0,0,0.12);
    background: hsl(var(--surface-1));
    color: hsl(var(--foreground));
    font-size: 13px;
  }

  /* Tinted ground plus matching text, rather than a solid fill. A solid
     dark-green toast was invisible against a light theme and shouted against a
     dark one; this reads at the same weight on both. */
  .toast.success {
    background: hsl(var(--ok) / 0.16);
    border: 1px solid hsl(var(--ok) / 0.4);
    color: hsl(var(--ok));
  }
  .toast.error {
    background: hsl(var(--err) / 0.16);
    border: 1px solid hsl(var(--err) / 0.4);
    color: hsl(var(--err));
  }
  .toast.info {
    background: hsl(var(--foreground) / 0.08);
    border: 1px solid hsl(var(--border));
  }
  .toast.warn {
    background: hsl(var(--warn) / 0.16);
    border: 1px solid hsl(var(--warn) / 0.4);
    color: hsl(var(--warn));
  }

  .msg { display: block; }
</style>
