<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { formatDate } from "$lib/format";
  import { showToast } from "$lib/stores/toast.svelte";

  interface RecurringCost {
    id: number;
    name: string;
    amount: number;
    frequency: string;
    category_id: number | null;
    category_name: string | null;
    next_due_date: string | null;
    active: boolean;
    notes: string | null;
    created_at: string;
    monthly_cost: number;
  }

  interface Category {
    id: number;
    path: string;
  }

  // Keep in sync with `payments_per_month` in src-tauri/src/commands/recurring.rs.
  const FREQUENCIES = [
    "Weekly",
    "Fortnightly",
    "Monthly",
    "Every 2 months",
    "Quarterly",
    "Half-yearly",
    "Yearly",
  ];

  let costs = $state<RecurringCost[]>([]);
  let categories = $state<Category[]>([]);
  let loading = $state(true);

  let showModal = $state(false);
  let editId = $state<number | null>(null);
  let formName = $state("");
  let formAmount = $state("");
  let formFrequency = $state("Monthly");
  let formCategory = $state<string>("");
  let formDate = $state("");
  let formActive = $state(true);
  let formNotes = $state("");

  let showDeleteConfirm = $state(false);
  let deleteId = $state(0);
  let deleteName = $state("");

  const currencyFormat = new Intl.NumberFormat("en-AU", { style: "currency", currency: "AUD" });
  function fmt(v: number): string { return currencyFormat.format(v); }

  let monthlyTotal = $derived(
    costs.filter((c) => c.active).reduce((s, c) => s + c.monthly_cost, 0),
  );
  let annualTotal = $derived(monthlyTotal * 12);

  $effect(() => { load(); });

  async function load() {
    loading = true;
    try {
      const [c, cats] = await Promise.all([
        invoke<RecurringCost[]>("list_recurring_costs"),
        invoke<Category[]>("get_categories"),
      ]);
      costs = c;
      categories = cats;
    } catch (e) {
      showToast(String(e), "error");
    } finally {
      loading = false;
    }
  }

  function openAdd() {
    editId = null;
    formName = "";
    formAmount = "";
    formFrequency = "Monthly";
    formCategory = "";
    formDate = "";
    formActive = true;
    formNotes = "";
    showModal = true;
  }

  function openEdit(c: RecurringCost) {
    editId = c.id;
    formName = c.name;
    formAmount = String(c.amount);
    formFrequency = c.frequency;
    formCategory = c.category_id == null ? "" : String(c.category_id);
    formDate = c.next_due_date ?? "";
    formActive = c.active;
    formNotes = c.notes ?? "";
    showModal = true;
  }

  async function handleSave() {
    if (!formName.trim() || !formAmount) return;
    const args = {
      name: formName.trim(),
      amount: parseFloat(formAmount),
      frequency: formFrequency,
      categoryId: formCategory ? parseInt(formCategory, 10) : null,
      nextDueDate: formDate || null,
      active: formActive,
      notes: formNotes.trim() || null,
    };
    try {
      if (editId == null) {
        await invoke("create_recurring_cost", args);
      } else {
        await invoke("update_recurring_cost", { id: editId, ...args });
      }
      showModal = false;
      await load();
      showToast("Recurring cost saved.", "success");
    } catch (e) {
      showToast(String(e), "error");
    }
  }

  function openDelete(c: RecurringCost) {
    deleteId = c.id;
    deleteName = c.name;
    showDeleteConfirm = true;
  }

  async function handleDelete() {
    try {
      await invoke("delete_recurring_cost", { id: deleteId });
      showDeleteConfirm = false;
      await load();
      showToast(`Deleted "${deleteName}".`, "success");
    } catch (e) {
      showToast(String(e), "error");
    }
  }
</script>

<svelte:window onkeydown={(e) => { if (e.key !== "Escape") return; showModal = false; showDeleteConfirm = false; }} />

<div class="page">
  <div class="header">
    <div>
      <h1>Recurring &amp; Subscriptions</h1>
      {#if !loading && costs.length > 0}
        <p class="subtitle">{fmt(monthlyTotal)} / month · {fmt(annualTotal)} / year</p>
      {/if}
    </div>
    <button class="btn btn-primary" onclick={openAdd}>Add Recurring</button>
  </div>

  {#if loading}
    <p class="loading">Loading…</p>
  {:else if costs.length === 0}
    <div class="empty-state">
      <p>No recurring costs yet. Add subscriptions and regular bills to track what you spend each month.</p>
      <button class="btn btn-primary" onclick={openAdd}>Add Recurring</button>
    </div>
  {:else}
    <div class="grid">
      {#each costs as c (c.id)}
        <div class="card" class:inactive={!c.active}>
          <div class="card-top">
            <span class="card-name">{c.name}</span>
            <div class="card-actions">
              <button class="btn btn-sm" onclick={() => openEdit(c)}>Edit</button>
              <button class="btn btn-sm btn-danger" onclick={() => openDelete(c)}>Delete</button>
            </div>
          </div>
          <div class="card-figures">
            <span class="card-amount">{fmt(c.amount)}</span>
            <span class="card-freq">{c.frequency}</span>
          </div>
          <div class="card-foot">
            <span class="card-monthly">{fmt(c.monthly_cost)}<span class="per">/mo</span></span>
            {#if c.category_name}<span class="chip">{c.category_name}</span>{/if}
            {#if !c.active}<span class="chip chip-muted">Paused</span>{/if}
            {#if c.next_due_date}<span class="card-due">next {formatDate(c.next_due_date)}</span>{/if}
          </div>
          {#if c.notes}<p class="card-notes">{c.notes}</p>{/if}
        </div>
      {/each}
    </div>
  {/if}
</div>

{#if showModal}
  <div class="modal-overlay" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) showModal = false; }}>
    <div class="modal" role="dialog" aria-modal="true" tabindex="-1">
      <h2>{editId == null ? "Add" : "Edit"} Recurring Cost</h2>
      <label>Name<input type="text" bind:value={formName} placeholder="e.g. Netflix" /></label>
      <label>Amount<input type="number" bind:value={formAmount} placeholder="0.00" step="0.01" min="0" /></label>
      <label>Frequency
        <select bind:value={formFrequency}>
          {#each FREQUENCIES as f}<option value={f}>{f}</option>{/each}
        </select>
      </label>
      <label>Category (optional)
        <select bind:value={formCategory}>
          <option value="">— None —</option>
          {#each categories as cat}<option value={String(cat.id)}>{cat.path}</option>{/each}
        </select>
      </label>
      <label>Next Due Date (optional)<input type="date" bind:value={formDate} /></label>
      <label>Notes (optional)<input type="text" bind:value={formNotes} placeholder="e.g. shared plan" /></label>
      <label class="checkbox"><input type="checkbox" bind:checked={formActive} /> Active</label>
      <div class="modal-actions">
        <button class="btn" onclick={() => { showModal = false; }}>Cancel</button>
        <button class="btn btn-primary" onclick={handleSave} disabled={!formName.trim() || !formAmount}>Save</button>
      </div>
    </div>
  </div>
{/if}

{#if showDeleteConfirm}
  <div class="modal-overlay" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) showDeleteConfirm = false; }}>
    <div class="modal modal-sm" role="dialog" aria-modal="true" tabindex="-1">
      <h2>Delete Recurring Cost</h2>
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
  .header { display: flex; justify-content: space-between; align-items: flex-start; margin-bottom: 1.25rem; gap: 1rem; }
  h1 { font-size: 1.75rem; font-weight: 700; color: var(--text-primary); }
  .subtitle { margin-top: 0.25rem; font-size: 0.9rem; color: var(--text-secondary); font-variant-numeric: tabular-nums; }
  h2 { font-size: 1.25rem; font-weight: 600; margin-bottom: 1rem; color: var(--text-primary); }

  .loading { color: var(--text-secondary); padding: 2rem 0; }
  .empty-state { border: 2px dashed var(--border-color); border-radius: var(--radius-card); padding: 3rem 2rem; text-align: center; color: var(--text-secondary); }
  .empty-state .btn { margin-top: 1rem; }

  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(300px, 1fr)); gap: 1.25rem; }
  .card { background: var(--bg-card); border: 1px solid var(--border-color); border-radius: var(--radius-card); padding: 1.25rem; box-shadow: var(--app-shadow); }
  .card.inactive { opacity: 0.6; }
  .card-top { display: flex; justify-content: space-between; align-items: center; gap: 0.5rem; margin-bottom: 0.75rem; }
  .card-name { font-size: 1.05rem; font-weight: 700; color: var(--text-primary); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .card-actions { display: flex; gap: 0.4rem; flex-shrink: 0; }
  .card-figures { display: flex; align-items: baseline; gap: 0.5rem; margin-bottom: 0.6rem; }
  .card-amount { font-size: 1.4rem; font-weight: 700; color: var(--text-primary); font-variant-numeric: tabular-nums; }
  .card-freq { font-size: 0.7rem; text-transform: uppercase; letter-spacing: 0.04em; font-weight: 600; color: var(--text-secondary); background: var(--bg-secondary); border: 1px solid var(--border-color); border-radius: 4px; padding: 0.1rem 0.4rem; }
  .card-foot { display: flex; align-items: center; gap: 0.6rem; font-size: 0.78rem; color: var(--text-secondary); flex-wrap: wrap; }
  .card-monthly { font-weight: 700; color: var(--text-primary); font-variant-numeric: tabular-nums; }
  .per { font-size: 0.7rem; font-weight: 500; color: var(--text-secondary); }
  .chip { font-size: 0.72rem; background: var(--bg-secondary); border: 1px solid var(--border-color); border-radius: 999px; padding: 0.1rem 0.5rem; }
  .chip-muted { color: var(--text-muted); }
  .card-due { font-size: 0.72rem; color: var(--text-secondary); }
  .card-notes { margin-top: 0.6rem; font-size: 0.78rem; color: var(--text-secondary); }

  .modal-overlay { position: fixed; inset: 0; background: rgba(0,0,0,0.4); display: flex; align-items: center; justify-content: center; z-index: 100; }
  .modal { background: var(--bg-card); border-radius: var(--radius-card); padding: 1.5rem; width: 420px; max-width: 90vw; max-height: 90vh; overflow-y: auto; box-shadow: 0 20px 60px rgba(0,0,0,0.15); }
  .modal-sm { width: 360px; }
  .modal label { display: flex; flex-direction: column; gap: 0.3rem; font-size: 0.85rem; font-weight: 500; color: var(--text-primary); margin-bottom: 0.75rem; }
  .modal label.checkbox { flex-direction: row; align-items: center; gap: 0.5rem; }
  .modal input, .modal select { padding: 0.5rem 0.65rem; border: 1px solid var(--border-color); border-radius: 10px; font-size: 0.9rem; background: var(--bg-card); color: var(--text-primary); }
  .modal label.checkbox input { width: auto; }
  .modal-actions { display: flex; justify-content: flex-end; gap: 0.5rem; margin-top: 1.25rem; }
</style>
