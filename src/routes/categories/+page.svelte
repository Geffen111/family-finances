<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  interface Category {
    id: number;
    name: string;
    parent_id: number | null;
    monthly_budget: number | null;
    created_at: string;
    exclude_from_budget: boolean;
    path: string;
  }

  interface BudgetStatus {
    category_id: number;
    name: string;
    path: string;
    monthly_budget: number;
    actual: number;
    percentage: number;
  }

  let categories = $state<Category[]>([]);
  let budgetStatus = $state<BudgetStatus[]>([]);
  let loading = $state(false);
  let importing = $state(false);

  let showAddModal = $state(false);
  let showEditModal = $state(false);
  let showDeleteConfirm = $state(false);

  let formName = $state("");
  let formParentId = $state<number | null>(null);
  let formBudget = $state<string>("");
  let formExclude = $state(false);

  let editId = $state<number>(0);
  let editName = $state("");
  let editParentId = $state<number | null>(null);
  let editBudget = $state<string>("");
  let editExclude = $state(false);

  let deleteId = $state<number>(0);
  let deleteName = $state("");

  let toastMsg = $state("");
  let toastType = $state<"success" | "error">("success");
  let toastVisible = $state(false);

  const currencyFormat = new Intl.NumberFormat("en-AU", {
    style: "currency",
    currency: "AUD",
  });

  $effect(() => {
    loadCategories();
  });

  async function loadCategories() {
    loading = true;
    try {
      categories = await invoke<Category[]>("get_categories");
      await loadBudgets();
    } catch (e) {
      showToast(String(e), "error");
    } finally {
      loading = false;
    }
  }

  async function loadBudgets() {
    // Budgets are tracked against the current calendar month.
    const now = new Date();
    const start = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-01`;
    try {
      budgetStatus = await invoke<BudgetStatus[]>("get_budget_status", { startDate: start });
    } catch (e) {
      budgetStatus = [];
    }
  }

  function budgetColor(pct: number): string {
    if (pct >= 100) return "var(--neg)";
    if (pct >= 80) return "var(--amber)";
    return "var(--pos)";
  }

  function showToast(msg: string, type: "success" | "error") {
    toastMsg = msg;
    toastType = type;
    toastVisible = true;
    setTimeout(() => { toastVisible = false; }, 4000);
  }

  async function handleImportCsv() {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".csv";
    input.addEventListener("change", async () => {
      const file = input.files?.[0];
      if (!file) return;
      const text = await file.text();
      importing = true;
      try {
        const count = await invoke<number>("upload_categories_csv", {
          csvContent: text,
        });
        showToast(`Imported ${count} categor${count === 1 ? "y" : "ies"}.`, "success");
        await loadCategories();
      } catch (e) {
        showToast(String(e), "error");
      } finally {
        importing = false;
      }
    });
    input.click();
  }

  function openAddModal() {
    formName = "";
    formParentId = null;
    formBudget = "";
    formExclude = false;
    showAddModal = true;
  }

  async function handleAdd() {
    if (!formName.trim()) return;
    try {
      const created = await invoke<Category>("create_category", {
        name: formName.trim(),
        parent_id: formParentId,
        monthly_budget: formBudget ? parseFloat(formBudget) : null,
      });
      if (formExclude) {
        await invoke("set_category_exclusion", { id: created.id, exclude: true });
      }
      showToast("Category created.", "success");
      showAddModal = false;
      await loadCategories();
    } catch (e) {
      showToast(String(e), "error");
    }
  }

  function openEditModal(cat: Category) {
    editId = cat.id;
    editName = cat.name;
    editParentId = cat.parent_id;
    editBudget = cat.monthly_budget != null ? String(cat.monthly_budget) : "";
    editExclude = cat.exclude_from_budget;
    showEditModal = true;
  }

  async function handleEdit() {
    if (!editName.trim()) return;
    try {
      await invoke("update_category", {
        id: editId,
        name: editName.trim() || null,
        parentId: editParentId,
        monthlyBudget: editBudget ? parseFloat(editBudget) : null,
      });
      await invoke("set_category_exclusion", { id: editId, exclude: editExclude });
      showToast("Category updated.", "success");
      showEditModal = false;
      await loadCategories();
    } catch (e) {
      showToast(String(e), "error");
    }
  }

  function openDeleteConfirm(cat: Category) {
    deleteId = cat.id;
    deleteName = cat.path;
    showDeleteConfirm = true;
  }

  async function handleDelete() {
    try {
      await invoke("delete_category", { id: deleteId });
      showToast(`Deleted "${deleteName}".`, "success");
      showDeleteConfirm = false;
      await loadCategories();
    } catch (e) {
      showToast(String(e), "error");
    }
  }

  function getParentName(parentId: number | null): string {
    if (parentId == null) return "";
    const parent = categories.find((c) => c.id === parentId);
    return parent ? parent.name : "";
  }

  let topLevel = $derived(categories.filter((c) => c.parent_id === null));

  function childrenOf(parentId: number) {
    return categories.filter((c) => c.parent_id === parentId);
  }

  function fmtBudget(val: number | null): string {
    if (val == null) return "-";
    return currencyFormat.format(val);
  }
</script>

<svelte:window onkeydown={(e) => {
  if (e.key !== "Escape") return;
  showAddModal = false;
  showEditModal = false;
  showDeleteConfirm = false;
}} />

<div class="page">
  <div class="header">
    <h1>Categories</h1>
    <div class="header-actions">
      <button class="btn btn-import" onclick={handleImportCsv} disabled={importing}>
        {importing ? "Importing\u2026" : "Import from CSV"}
      </button>
      <button class="btn btn-add" onclick={openAddModal}>Add Category</button>
    </div>
  </div>

  {#if toastVisible}
    <div class="toast" class:toast-error={toastType === "error"} class:toast-success={toastType === "success"}>
      {toastMsg}
    </div>
  {/if}

  {#if loading}
    <p class="loading">Loading categories\u2026</p>
  {:else if categories.length === 0}
    <div class="empty-state">
      <p>No categories yet. Import a CSV or add one manually.</p>
    </div>
  {:else}
    <div class="tree">
      {#each topLevel as parent (parent.id)}
        <div class="parent-row">
          <div class="parent-info">
            <span class="parent-name">{parent.name}</span>
            <span class="parent-budget">{fmtBudget(parent.monthly_budget)}</span>
            {#if parent.exclude_from_budget}<span class="excluded-badge">excluded</span>{/if}
          </div>
          <div class="actions">
            <button class="btn btn-sm btn-edit" onclick={() => openEditModal(parent)}>Edit</button>
            <button class="btn btn-sm btn-delete" onclick={() => openDeleteConfirm(parent)}>Delete</button>
          </div>
        </div>
        {#each childrenOf(parent.id) as child (child.id)}
          <div class="child-row">
            <div class="child-info">
              <span class="child-name">{child.name}</span>
              <span class="child-budget">{fmtBudget(child.monthly_budget)}</span>
              {#if child.exclude_from_budget}<span class="excluded-badge">excluded</span>{/if}
            </div>
            <div class="actions">
              <button class="btn btn-sm btn-edit" onclick={() => openEditModal(child)}>Edit</button>
              <button class="btn btn-sm btn-delete" onclick={() => openDeleteConfirm(child)}>Delete</button>
            </div>
          </div>
        {/each}
      {/each}
    </div>
  {/if}

  {#if budgetStatus.length > 0}
    <div class="budgets-section">
      <h2>Budgets &mdash; this month</h2>
      <div class="budget-list">
        {#each budgetStatus as b (b.category_id)}
          <div class="budget-item">
            <div class="budget-head">
              <span class="budget-name">{b.path}</span>
              <span class="budget-figures">{currencyFormat.format(b.actual)} / {currencyFormat.format(b.monthly_budget)}</span>
            </div>
            <div class="budget-track">
              <div class="budget-fill" style="width: {Math.min(b.percentage, 100)}%; background: {budgetColor(b.percentage)};"></div>
            </div>
            <span class="budget-pct" style="color: {budgetColor(b.percentage)};">
              {Math.round(b.percentage)}%{b.percentage >= 100 ? " — over budget" : ""}
            </span>
          </div>
        {/each}
      </div>
    </div>
  {/if}
</div>

<!-- Add Modal -->
{#if showAddModal}
  <div class="modal-overlay" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) showAddModal = false; }}>
    <div class="modal" role="dialog" aria-modal="true" tabindex="-1">
      <h2>Add Category</h2>
      <label>
        Name
        <input type="text" bind:value={formName} placeholder="Category name" />
      </label>
      <label>
        Parent
        <select bind:value={formParentId}>
          <option value={null}>None (top level)</option>
          {#each topLevel as parent (parent.id)}
            <option value={parent.id}>{parent.name}</option>
          {/each}
        </select>
      </label>
      <label>
        Monthly Budget
        <input type="number" bind:value={formBudget} placeholder="0.00" step="0.01" min="0" />
      </label>
      <label class="checkbox-label">
        <input type="checkbox" bind:checked={formExclude} />
        Exclude from budgets &amp; totals (e.g. internal transfers)
      </label>
      <div class="modal-actions">
        <button class="btn" onclick={() => { showAddModal = false; }}>Cancel</button>
        <button class="btn btn-primary" onclick={handleAdd} disabled={!formName.trim()}>Save</button>
      </div>
    </div>
  </div>
{/if}

<!-- Edit Modal -->
{#if showEditModal}
  <div class="modal-overlay" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) showEditModal = false; }}>
    <div class="modal" role="dialog" aria-modal="true" tabindex="-1">
      <h2>Edit Category</h2>
      <label>
        Name
        <input type="text" bind:value={editName} placeholder="Category name" />
      </label>
      <label>
        Parent
        <select bind:value={editParentId}>
          <option value={null}>None (top level)</option>
          {#each topLevel as parent (parent.id)}
            <option value={parent.id}>{parent.name}</option>
          {/each}
        </select>
      </label>
      <label>
        Monthly Budget
        <input type="number" bind:value={editBudget} placeholder="0.00" step="0.01" min="0" />
      </label>
      <label class="checkbox-label">
        <input type="checkbox" bind:checked={editExclude} />
        Exclude from budgets &amp; totals (e.g. internal transfers)
      </label>
      <div class="modal-actions">
        <button class="btn" onclick={() => { showEditModal = false; }}>Cancel</button>
        <button class="btn btn-primary" onclick={handleEdit} disabled={!editName.trim()}>Save</button>
      </div>
    </div>
  </div>
{/if}

<!-- Delete Confirmation -->
{#if showDeleteConfirm}
  <div class="modal-overlay" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) showDeleteConfirm = false; }}>
    <div class="modal modal-sm" role="dialog" aria-modal="true" tabindex="-1">
      <h2>Delete Category</h2>
      <p>Are you sure you want to delete <strong>{deleteName}</strong>?</p>
      <p class="delete-warning">Transactions in this category will be uncategorised.</p>
      <div class="modal-actions">
        <button class="btn" onclick={() => { showDeleteConfirm = false; }}>Cancel</button>
        <button class="btn btn-danger" onclick={handleDelete}>Delete</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .page { max-width: 1100px; margin: 0 auto; }

  .header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 1rem;
  }

  h1 { font-size: 1.75rem; font-weight: 700; color: var(--text-primary); }
  h2 { font-size: 1.25rem; font-weight: 600; margin-bottom: 1rem; }

  .header-actions {
    display: flex;
    gap: 0.5rem;
  }

  .btn {
    padding: 0.5rem 1rem;
    border: 1px solid var(--border-color);
    border-radius: 10px;
    background: var(--bg-card);
    color: var(--text-primary);
    font-size: 0.875rem;
    cursor: pointer;
    transition: background 0.15s;
  }
  .btn:hover { background: var(--bg-secondary); }
  .btn:disabled { opacity: 0.5; cursor: not-allowed; }
  .btn-sm { padding: 0.3rem 0.65rem; font-size: 0.8rem; }
  .btn-import { background: var(--accent); color: #fff; border-color: var(--accent); }
  .btn-import:hover { background: var(--accent); }
  .btn-add { background: var(--accent); color: #fff; border-color: var(--accent); }
  .btn-add:hover { background: #047857; }
  .btn-edit { background: var(--amber); color: #fff; border-color: var(--amber); }
  .btn-edit:hover { background: var(--amber); }
  .btn-delete { background: var(--neg); color: #fff; border-color: var(--neg); }
  .btn-delete:hover { background: var(--neg); }
  .btn-primary { background: var(--accent); color: #fff; border-color: var(--accent); }
  .btn-primary:hover { background: var(--accent); }
  .btn-danger { background: var(--neg); color: #fff; border-color: var(--neg); }
  .btn-danger:hover { background: var(--neg); }

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
  .toast-success { background: #d1fae5; color: #065f46; border: 1px solid #a7f3d0; }
  .toast-error { background: #fee2e2; color: #991b1b; border: 1px solid #fecaca; }

  .loading { color: var(--text-secondary); padding: 2rem 0; }

  .empty-state {
    border: 2px dashed var(--border-color);
    border-radius: 14px;
    padding: 3rem 2rem;
    text-align: center;
    color: var(--text-secondary);
    font-size: 1rem;
  }

  .tree {
    display: flex;
    flex-direction: column;
  }

  .parent-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 0.75rem 1rem;
    background: var(--bg-secondary);
    border-radius: 10px;
    margin-bottom: 0.25rem;
    font-weight: 600;
    color: var(--text-primary);
  }

  .child-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 0.6rem 1rem 0.6rem 2.5rem;
    margin-left: 0.5rem;
    border-left: 2px solid var(--border-color);
    border-radius: 0;
    color: var(--text-primary);
    transition: background 0.1s;
  }
  .child-row:hover {
    background: var(--bg-secondary);
  }

  .parent-info, .child-info {
    display: flex;
    align-items: center;
    gap: 1rem;
  }

  .parent-name { font-size: 0.95rem; }
  .child-name { font-size: 0.9rem; }

  .parent-budget, .child-budget {
    font-size: 0.8rem;
    color: var(--text-secondary);
    font-weight: 400;
  }

  .actions {
    display: flex;
    gap: 0.4rem;
  }

  /* Modal */
  .modal-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.4);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }

  .modal {
    background: var(--bg-card);
    border-radius: 14px;
    padding: 1.5rem;
    width: 420px;
    max-width: 90vw;
    box-shadow: 0 20px 60px rgba(0, 0, 0, 0.15);
  }

  .modal-sm {
    width: 360px;
  }

  .modal label {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    font-size: 0.85rem;
    font-weight: 500;
    color: var(--text-primary);
    margin-bottom: 0.75rem;
  }

  .modal input, .modal select {
    padding: 0.5rem 0.65rem;
    border: 1px solid var(--border-color);
    border-radius: 10px;
    font-size: 0.9rem;
  }

  .modal select { background: var(--bg-card); }

  .modal-actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
    margin-top: 1.25rem;
  }

  .delete-warning {
    font-size: 0.85rem;
    color: var(--text-secondary);
    margin-top: 0.3rem;
  }

  .checkbox-label {
    flex-direction: row !important;
    align-items: center;
    gap: 0.5rem !important;
    font-weight: 400 !important;
    color: var(--text-secondary) !important;
  }
  .checkbox-label input { width: auto; }

  .excluded-badge {
    font-size: 0.65rem;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    font-weight: 600;
    color: var(--text-secondary);
    background: var(--bg-secondary);
    border: 1px solid var(--border-color);
    border-radius: 4px;
    padding: 0.1rem 0.4rem;
  }

  .budgets-section { margin-top: 2.5rem; padding-top: 1.5rem; border-top: 1px solid var(--border-color); }
  .budgets-section h2 { font-size: 1.25rem; font-weight: 700; margin-bottom: 1rem; color: var(--text-primary); }
  .budget-list { display: grid; grid-template-columns: repeat(auto-fit, minmax(280px, 1fr)); gap: 1rem; }
  .budget-item {
    background: var(--bg-card);
    border: 1px solid var(--border-color);
    border-radius: var(--radius-card);
    padding: 0.9rem 1rem;
    box-shadow: var(--app-shadow);
  }
  .budget-head { display: flex; justify-content: space-between; align-items: baseline; gap: 0.5rem; margin-bottom: 0.5rem; }
  .budget-name { font-size: 0.88rem; font-weight: 600; color: var(--text-primary); }
  .budget-figures { font-size: 0.8rem; color: var(--text-secondary); font-variant-numeric: tabular-nums; white-space: nowrap; }
  .budget-track { height: 8px; background: var(--bg-secondary); border-radius: 999px; overflow: hidden; }
  .budget-fill { height: 100%; border-radius: 999px; transition: width 0.3s; }
  .budget-pct { display: inline-block; margin-top: 0.35rem; font-size: 0.75rem; font-weight: 600; }
</style>
