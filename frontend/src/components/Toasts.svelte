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
    background: #111827;
    color: #f9fafb;
    font-size: 13px;
  }

  .toast.success { background: #065f46; }
  .toast.error { background: #991b1b; }
  .toast.info { background: #1f2937; }
  .toast.warn { background: #92400e; }

  .msg { display: block; }
</style>
