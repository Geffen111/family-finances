<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  interface Category {
    id: number;
    name: string;
    parent_id: number | null;
    monthly_budget: number | null;
    created_at: string;
    path: string;
  }

  let categories = $state<Category[]>([]);
  let loading = $state(false);
  let importing = $state(false);

  let showAddModal = $state(false);
  let showEditModal = $state(false);
  let showDeleteConfirm = $state(false);

  let formName = $state("");
  let formParentId = $state<number | null>(null);
  let formBudget = $state<string>("");

  let editId = $state<number>(0);
  let editName = $state("");
  let editParentId = $state<number | null>(null);
  let editBudget = $state<string>("");

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
    showAddModal = true;
  }

  async function handleAdd() {
    if (!formName.trim()) return;
    try {
      await invoke("create_category", {
        name: formName.trim(),
        parent_id: formParentId,
        monthly_budget: formBudget ? parseFloat(formBudget) : null,
      });
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
  .page { max-width: 1000px; }

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
    border-radius: 6px;
    background: var(--bg-card);
    color: var(--text-primary);
    font-size: 0.875rem;
    cursor: pointer;
    transition: background 0.15s;
  }
  .btn:hover { background: var(--bg-secondary); }
  .btn:disabled { opacity: 0.5; cursor: not-allowed; }
  .btn-sm { padding: 0.3rem 0.65rem; font-size: 0.8rem; }
  .btn-import { background: #2563eb; color: #fff; border-color: #2563eb; }
  .btn-import:hover { background: #1d4ed8; }
  .btn-add { background: #059669; color: #fff; border-color: #059669; }
  .btn-add:hover { background: #047857; }
  .btn-edit { background: #f59e0b; color: #fff; border-color: #f59e0b; }
  .btn-edit:hover { background: #d97706; }
  .btn-delete { background: #ef4444; color: #fff; border-color: #ef4444; }
  .btn-delete:hover { background: #dc2626; }
  .btn-primary { background: #2563eb; color: #fff; border-color: #2563eb; }
  .btn-primary:hover { background: #1d4ed8; }
  .btn-danger { background: #ef4444; color: #fff; border-color: #ef4444; }
  .btn-danger:hover { background: #dc2626; }

  .toast {
    padding: 0.75rem 1rem;
    border-radius: 6px;
    margin-bottom: 1rem;
    font-size: 0.875rem;
  }
  .toast-success { background: #d1fae5; color: #065f46; border: 1px solid #a7f3d0; }
  .toast-error { background: #fee2e2; color: #991b1b; border: 1px solid #fecaca; }

  .loading { color: var(--text-secondary); padding: 2rem 0; }

  .empty-state {
    border: 2px dashed var(--border-color);
    border-radius: 8px;
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
    border-radius: 6px;
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
    border-radius: 10px;
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
    border-radius: 6px;
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
</style>
