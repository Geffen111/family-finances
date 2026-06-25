<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  interface SavingsGoal {
    id: number;
    name: string;
    target_amount: number;
    current_amount: number;
    target_date: string | null;
    created_at: string;
  }

  let goals = $state<SavingsGoal[]>([]);
  let loading = $state(true);

  let showModal = $state(false);
  let editId = $state<number | null>(null);
  let formName = $state("");
  let formTarget = $state("");
  let formCurrent = $state("");
  let formDate = $state("");

  let showDeleteConfirm = $state(false);
  let deleteId = $state(0);
  let deleteName = $state("");

  let toastMsg = $state("");
  let toastType = $state<"success" | "error">("success");
  let toastVisible = $state(false);

  const currencyFormat = new Intl.NumberFormat("en-AU", { style: "currency", currency: "AUD" });
  function fmt(v: number): string { return currencyFormat.format(v); }

  $effect(() => { loadGoals(); });

  async function loadGoals() {
    loading = true;
    try {
      goals = await invoke<SavingsGoal[]>("list_savings_goals");
    } catch (e) {
      showToast(String(e), "error");
    } finally {
      loading = false;
    }
  }

  function showToast(msg: string, type: "success" | "error") {
    toastMsg = msg;
    toastType = type;
    toastVisible = true;
    setTimeout(() => { toastVisible = false; }, 4000);
  }

  function pct(g: SavingsGoal): number {
    if (g.target_amount <= 0) return 0;
    return Math.min((g.current_amount / g.target_amount) * 100, 100);
  }

  function openAdd() {
    editId = null;
    formName = "";
    formTarget = "";
    formCurrent = "";
    formDate = "";
    showModal = true;
  }

  function openEdit(g: SavingsGoal) {
    editId = g.id;
    formName = g.name;
    formTarget = String(g.target_amount);
    formCurrent = String(g.current_amount);
    formDate = g.target_date ?? "";
    showModal = true;
  }

  async function handleSave() {
    if (!formName.trim() || !formTarget) return;
    try {
      if (editId == null) {
        await invoke("create_savings_goal", {
          name: formName.trim(),
          targetAmount: parseFloat(formTarget),
          currentAmount: formCurrent ? parseFloat(formCurrent) : 0,
          targetDate: formDate || null,
        });
      } else {
        await invoke("update_savings_goal", {
          id: editId,
          name: formName.trim(),
          targetAmount: parseFloat(formTarget),
          currentAmount: formCurrent ? parseFloat(formCurrent) : 0,
          targetDate: formDate || null,
        });
      }
      showModal = false;
      await loadGoals();
      showToast("Goal saved.", "success");
    } catch (e) {
      showToast(String(e), "error");
    }
  }

  function openDelete(g: SavingsGoal) {
    deleteId = g.id;
    deleteName = g.name;
    showDeleteConfirm = true;
  }

  async function handleDelete() {
    try {
      await invoke("delete_savings_goal", { id: deleteId });
      showDeleteConfirm = false;
      await loadGoals();
      showToast(`Deleted "${deleteName}".`, "success");
    } catch (e) {
      showToast(String(e), "error");
    }
  }
</script>

<svelte:window onkeydown={(e) => { if (e.key !== "Escape") return; showModal = false; showDeleteConfirm = false; }} />

<div class="page">
  <div class="header">
    <h1>Savings Goals</h1>
    <button class="btn btn-primary" onclick={openAdd}>Add Goal</button>
  </div>

  {#if toastVisible}
    <div class="toast" class:toast-error={toastType === "error"} class:toast-success={toastType === "success"}>{toastMsg}</div>
  {/if}

  {#if loading}
    <p class="loading">Loading goals…</p>
  {:else if goals.length === 0}
    <div class="empty-state">
      <p>No savings goals yet. Add one to start tracking progress.</p>
      <button class="btn btn-primary" onclick={openAdd}>Add Goal</button>
    </div>
  {:else}
    <div class="goals-grid">
      {#each goals as g (g.id)}
        <div class="goal-card">
          <div class="goal-top">
            <span class="goal-name">{g.name}</span>
            <div class="goal-actions">
              <button class="btn btn-sm" onclick={() => openEdit(g)}>Edit</button>
              <button class="btn btn-sm btn-danger" onclick={() => openDelete(g)}>Delete</button>
            </div>
          </div>
          <div class="goal-figures">
            <span class="goal-current">{fmt(g.current_amount)}</span>
            <span class="goal-target">of {fmt(g.target_amount)}</span>
          </div>
          <div class="goal-track">
            <div class="goal-fill" style="width: {pct(g)}%;"></div>
          </div>
          <div class="goal-foot">
            <span class="goal-pct">{Math.round(pct(g))}%</span>
            {#if g.target_date}<span class="goal-date">by {g.target_date}</span>{/if}
            {#if g.current_amount < g.target_amount}
              <span class="goal-remaining">{fmt(g.target_amount - g.current_amount)} to go</span>
            {:else}
              <span class="goal-done">Reached 🎉</span>
            {/if}
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>

{#if showModal}
  <div class="modal-overlay" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) showModal = false; }}>
    <div class="modal" role="dialog" aria-modal="true" tabindex="-1">
      <h2>{editId == null ? "Add" : "Edit"} Goal</h2>
      <label>Name<input type="text" bind:value={formName} placeholder="e.g. Holiday fund" /></label>
      <label>Target Amount<input type="number" bind:value={formTarget} placeholder="0.00" step="0.01" min="0" /></label>
      <label>Current Amount<input type="number" bind:value={formCurrent} placeholder="0.00" step="0.01" min="0" /></label>
      <label>Target Date (optional)<input type="date" bind:value={formDate} /></label>
      <div class="modal-actions">
        <button class="btn" onclick={() => { showModal = false; }}>Cancel</button>
        <button class="btn btn-primary" onclick={handleSave} disabled={!formName.trim() || !formTarget}>Save</button>
      </div>
    </div>
  </div>
{/if}

{#if showDeleteConfirm}
  <div class="modal-overlay" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) showDeleteConfirm = false; }}>
    <div class="modal modal-sm" role="dialog" aria-modal="true" tabindex="-1">
      <h2>Delete Goal</h2>
      <p>Delete <strong>{deleteName}</strong>?</p>
      <div class="modal-actions">
        <button class="btn" onclick={() => { showDeleteConfirm = false; }}>Cancel</button>
        <button class="btn btn-danger" onclick={handleDelete}>Delete</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .page { max-width: 1320px; margin: 0 auto; }
  .header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 1.25rem; }
  h1 { font-size: 1.75rem; font-weight: 700; color: var(--text-primary); }
  h2 { font-size: 1.25rem; font-weight: 600; margin-bottom: 1rem; color: var(--text-primary); }

  .btn { padding: 0.5rem 1rem; border: 1px solid var(--border-color); border-radius: 10px; background: var(--bg-card); color: var(--text-primary); font-size: 0.875rem; cursor: pointer; transition: background 0.15s; }
  .btn:hover { background: var(--bg-secondary); }
  .btn:disabled { opacity: 0.5; cursor: not-allowed; }
  .btn-sm { padding: 0.3rem 0.65rem; font-size: 0.8rem; }
  .btn-primary { background: var(--accent); color: #fff; border-color: var(--accent); }
  .btn-primary:hover { background: var(--accent); }
  .btn-danger { background: var(--neg); color: #fff; border-color: var(--neg); }
  .btn-danger:hover { background: var(--neg); }

  .toast { position: fixed; top: 1rem; left: 50%; transform: translateX(-50%); z-index: 200; padding: 0.75rem 1.25rem; border-radius: 14px; font-size: 0.875rem; box-shadow: 0 8px 24px rgba(0, 0, 0, 0.15); animation: toast-in 0.2s ease-out; }
  @keyframes toast-in { from { opacity: 0; transform: translateX(-50%) translateY(-0.5rem); } to { opacity: 1; transform: translateX(-50%) translateY(0); } }
  .toast-success { background: var(--accent-soft); color: var(--nav-active-fg); border: 1px solid var(--accent); }
  .toast-error { background: var(--neg-soft); color: var(--neg); border: 1px solid var(--neg); }

  .loading { color: var(--text-secondary); padding: 2rem 0; }
  .empty-state { border: 2px dashed var(--border-color); border-radius: var(--radius-card); padding: 3rem 2rem; text-align: center; color: var(--text-secondary); }
  .empty-state .btn { margin-top: 1rem; }

  .goals-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(300px, 1fr)); gap: 1.25rem; }
  .goal-card { background: var(--bg-card); border: 1px solid var(--border-color); border-radius: var(--radius-card); padding: 1.25rem; box-shadow: var(--app-shadow); }
  .goal-top { display: flex; justify-content: space-between; align-items: center; gap: 0.5rem; margin-bottom: 0.75rem; }
  .goal-name { font-size: 1.05rem; font-weight: 700; color: var(--text-primary); }
  .goal-actions { display: flex; gap: 0.4rem; }
  .goal-figures { display: flex; align-items: baseline; gap: 0.4rem; margin-bottom: 0.6rem; }
  .goal-current { font-size: 1.4rem; font-weight: 700; color: var(--text-primary); font-variant-numeric: tabular-nums; }
  .goal-target { font-size: 0.85rem; color: var(--text-secondary); }
  .goal-track { height: 10px; background: var(--bg-secondary); border-radius: 999px; overflow: hidden; }
  .goal-fill { height: 100%; border-radius: 999px; background: var(--accent); transition: width 0.3s; }
  .goal-foot { display: flex; align-items: center; gap: 0.75rem; margin-top: 0.6rem; font-size: 0.78rem; color: var(--text-secondary); flex-wrap: wrap; }
  .goal-pct { font-weight: 700; color: var(--text-primary); }
  .goal-done { color: var(--pos); font-weight: 600; }

  .modal-overlay { position: fixed; inset: 0; background: rgba(0,0,0,0.4); display: flex; align-items: center; justify-content: center; z-index: 100; }
  .modal { background: var(--bg-card); border-radius: var(--radius-card); padding: 1.5rem; width: 420px; max-width: 90vw; box-shadow: 0 20px 60px rgba(0,0,0,0.15); }
  .modal-sm { width: 360px; }
  .modal label { display: flex; flex-direction: column; gap: 0.3rem; font-size: 0.85rem; font-weight: 500; color: var(--text-primary); margin-bottom: 0.75rem; }
  .modal input { padding: 0.5rem 0.65rem; border: 1px solid var(--border-color); border-radius: 10px; font-size: 0.9rem; background: var(--bg-card); color: var(--text-primary); }
  .modal-actions { display: flex; justify-content: flex-end; gap: 0.5rem; margin-top: 1.25rem; }
</style>
