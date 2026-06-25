<script lang="ts">
  import { toast } from "$lib/stores/toast.svelte";
</script>

{#if toast.visible}
  <div class="toast" class:toast-error={toast.kind === "error"} class:toast-success={toast.kind === "success"}>
    {toast.message}
  </div>
{/if}

<style>
  .toast {
    position: fixed;
    top: 1rem;
    left: 50%;
    transform: translateX(-50%);
    z-index: 200;
    padding: 0.75rem 1.25rem;
    border-radius: 14px;
    font-size: 0.875rem;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.15);
    animation: toast-in 0.2s ease-out;
  }
  @keyframes toast-in {
    from { opacity: 0; transform: translateX(-50%) translateY(-0.5rem); }
    to { opacity: 1; transform: translateX(-50%) translateY(0); }
  }
  .toast-success { background: var(--accent-soft); color: var(--nav-active-fg); border: 1px solid var(--accent); }
  .toast-error { background: var(--neg-soft); color: var(--neg); border: 1px solid var(--neg); }
</style>
