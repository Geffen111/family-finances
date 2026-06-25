<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  interface Category {
    id: number;
    name: string;
    parent_id: number | null;
    monthly_budget: number | null;
    created_at: string;
    exclude_from_budget: boolean;
    rollover: boolean;
    path: string;
  }

  interface BudgetStatus {
    category_id: number;
    name: string;
    path: string;
    monthly_budget: number;
    actual: number;
    percentage: number;
    rollover: boolean;
    carryover: number;
    available: number;
  }

  interface BudgetSuggestion {
    category_id: number;
    path: string;
    suggested: number;
    current_budget: number | null;
  }

  interface CategoryRule {
    id: number;
    match_type: string;
    pattern: string;
    category_id: number;
    category_name: string | null;
    priority: number;
    active: boolean;
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
  let formRollover = $state(false);

  let editId = $state<number>(0);
  let editName = $state("");
  let editParentId = $state<number | null>(null);
  let editBudget = $state<string>("");
  let editExclude = $state(false);
  let editRollover = $state(false);

  let deleteId = $state<number>(0);
  let deleteName = $state("");

  let showSuggestModal = $state(false);
  let suggestions = $state<BudgetSuggestion[]>([]);
  let suggestLoading = $state(false);
  let suggestMonths = $state(3);

  let rules = $state<CategoryRule[]>([]);
  let ruleMatchType = $state("contains");
  let rulePattern = $state("");
  let ruleCategoryId = $state<number | null>(null);
  let applyingRules = $state(false);

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
      await loadRules();
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

  async function loadRules() {
    try {
      rules = await invoke<CategoryRule[]>("list_category_rules");
    } catch (e) {
      rules = [];
    }
  }

  async function addRule() {
    if (!rulePattern.trim() || ruleCategoryId == null) return;
    try {
      await invoke("create_category_rule", {
        matchType: ruleMatchType,
        pattern: rulePattern.trim(),
        categoryId: ruleCategoryId,
        priority: 0,
        active: true,
      });
      rulePattern = "";
      showToast("Rule added.", "success");
      await loadRules();
    } catch (e) {
      showToast(String(e), "error");
    }
  }

  async function deleteRule(id: number) {
    try {
      await invoke("delete_category_rule", { id });
      await loadRules();
    } catch (e) {
      showToast(String(e), "error");
    }
  }

  async function applyRulesNow() {
    applyingRules = true;
    try {
      const n = await invoke<number>("apply_category_rules", { onlyUncategorised: true });
      showToast(`Applied rules to ${n} transaction${n === 1 ? "" : "s"}.`, "success");
    } catch (e) {
      showToast(String(e), "error");
    } finally {
      applyingRules = false;
    }
  }

  function ruleMatchLabel(t: string): string {
    if (t === "equals") return "equals";
    if (t === "starts_with") return "starts with";
    return "contains";
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

  async function openSuggestModal() {
    showSuggestModal = true;
    await loadSuggestions();
  }

  async function loadSuggestions() {
    suggestLoading = true;
    try {
      suggestions = await invoke<BudgetSuggestion[]>("get_budget_suggestions", {
        months: suggestMonths,
      });
    } catch (e) {
      showToast(String(e), "error");
      suggestions = [];
    } finally {
      suggestLoading = false;
    }
  }

  async function applySuggestion(s: BudgetSuggestion) {
    try {
      await invoke("update_category", { id: s.category_id, monthlyBudget: s.suggested });
      showToast(`Set ${s.path} to ${currencyFormat.format(s.suggested)}.`, "success");
      suggestions = suggestions.map((x) =>
        x.category_id === s.category_id ? { ...x, current_budget: s.suggested } : x,
      );
      await loadCategories();
    } catch (e) {
      showToast(String(e), "error");
    }
  }

  async function applyAllSuggestions(onlyEmpty: boolean) {
    const targets = onlyEmpty
      ? suggestions.filter((s) => s.current_budget == null || s.current_budget === 0)
      : suggestions;
    if (targets.length === 0) {
      showToast("Nothing to apply.", "success");
      return;
    }
    try {
      for (const s of targets) {
        await invoke("update_category", { id: s.category_id, monthlyBudget: s.suggested });
      }
      showToast(`Updated ${targets.length} budget${targets.length === 1 ? "" : "s"}.`, "success");
      showSuggestModal = false;
      await loadCategories();
    } catch (e) {
      showToast(String(e), "error");
    }
  }

  function openAddModal() {
    formName = "";
    formParentId = null;
    formBudget = "";
    formExclude = false;
    formRollover = false;
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
      if (formRollover) {
        await invoke("set_category_rollover", { id: created.id, rollover: true });
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
    editRollover = cat.rollover;
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
      await invoke("set_category_rollover", { id: editId, rollover: editRollover });
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
  showSuggestModal = false;
}} />

<div class="page">
  <div class="header">
    <h1>Categories</h1>
    <div class="header-actions">
      <button class="btn btn-import" onclick={handleImportCsv} disabled={importing}>
        {importing ? "Importing\u2026" : "Import from CSV"}
      </button>
      <button class="btn btn-suggest" onclick={openSuggestModal}>Suggest budgets</button>
      <button class="btn btn-add" onclick={openAddModal}>Add Category</button>
    </div>
  </div>

  {#if toastVisible}
    <div class="toast" class:toast-error={toastType === "error"} class:toast-success={toastType === "success"}>
      {toastMsg}
    </div>
  {/if}

  {#if loading}
    <p class="loading">Loading categories…</p>
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
              <span class="budget-name">
                {b.path}
                {#if b.rollover}<span class="rollover-badge" title="Unspent budget rolls over">rollover</span>{/if}
              </span>
              <span class="budget-figures">{currencyFormat.format(b.actual)} / {currencyFormat.format(b.rollover ? b.available : b.monthly_budget)}</span>
            </div>
            <div class="budget-track">
              <div class="budget-fill" style="width: {Math.min(b.percentage, 100)}%; background: {budgetColor(b.percentage)};"></div>
            </div>
            <span class="budget-pct" style="color: {budgetColor(b.percentage)};">
              {Math.round(b.percentage)}%{b.percentage >= 100 ? " — over budget" : ""}
            </span>
            {#if b.rollover && b.carryover !== 0}
              <span class="budget-carry" class:carry-neg={b.carryover < 0}>
                {b.carryover > 0 ? "+" : ""}{currencyFormat.format(b.carryover)} rolled over
                ({currencyFormat.format(b.monthly_budget)}/mo)
              </span>
            {/if}
          </div>
        {/each}
      </div>
    </div>
  {/if}

  <div class="rules-section">
    <div class="rules-head">
      <div>
        <h2>Auto-categorisation rules</h2>
        <p class="rules-intro">Rules run on import (before the AI) and can be re-applied to uncategorised transactions.</p>
      </div>
      <button class="btn" onclick={applyRulesNow} disabled={applyingRules || rules.length === 0}>
        {applyingRules ? "Applying…" : "Apply rules now"}
      </button>
    </div>

    <div class="rule-form">
      <span class="rule-when">If description</span>
      <select bind:value={ruleMatchType}>
        <option value="contains">contains</option>
        <option value="starts_with">starts with</option>
        <option value="equals">equals</option>
      </select>
      <input type="text" placeholder="e.g. WOOLWORTHS" bind:value={rulePattern} />
      <span class="rule-then">→</span>
      <select bind:value={ruleCategoryId}>
        <option value={null}>Select category…</option>
        {#each categories.filter((c) => c.parent_id !== null || childrenOf(c.id).length === 0) as cat (cat.id)}
          <option value={cat.id}>{cat.path}</option>
        {/each}
      </select>
      <button class="btn btn-primary" onclick={addRule} disabled={!rulePattern.trim() || ruleCategoryId == null}>Add rule</button>
    </div>

    {#if rules.length > 0}
      <div class="rule-list">
        {#each rules as r (r.id)}
          <div class="rule-row">
            <span class="rule-text">
              <span class="rule-match">{ruleMatchLabel(r.match_type)}</span>
              <strong>{r.pattern}</strong>
              <span class="rule-arrow">→</span>
              <span class="rule-cat">{r.category_name ?? "?"}</span>
            </span>
            <button class="btn btn-sm btn-delete" onclick={() => deleteRule(r.id)}>Delete</button>
          </div>
        {/each}
      </div>
    {/if}
  </div>
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
      <label class="checkbox-label">
        <input type="checkbox" bind:checked={formRollover} />
        Roll unspent budget over to the next month
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
      <label class="checkbox-label">
        <input type="checkbox" bind:checked={editRollover} />
        Roll unspent budget over to the next month
      </label>
      <div class="modal-actions">
        <button class="btn" onclick={() => { showEditModal = false; }}>Cancel</button>
        <button class="btn btn-primary" onclick={handleEdit} disabled={!editName.trim()}>Save</button>
      </div>
    </div>
  </div>
{/if}

<!-- Suggest Budgets Modal -->
{#if showSuggestModal}
  <div class="modal-overlay" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) showSuggestModal = false; }}>
    <div class="modal modal-lg" role="dialog" aria-modal="true" tabindex="-1">
      <h2>Suggested budgets</h2>
      <p class="suggest-intro">
        Based on average spending per category. Adjust the look-back window, then apply individually or in bulk.
      </p>
      <label class="suggest-window">
        Look back
        <select bind:value={suggestMonths} onchange={loadSuggestions}>
          <option value={3}>3 months</option>
          <option value={6}>6 months</option>
          <option value={12}>12 months</option>
        </select>
      </label>

      {#if suggestLoading}
        <p class="loading">Calculating…</p>
      {:else if suggestions.length === 0}
        <p class="loading">No spending found in this window.</p>
      {:else}
        <div class="suggest-list">
          {#each suggestions as s (s.category_id)}
            <div class="suggest-row">
              <span class="suggest-name">{s.path}</span>
              <span class="suggest-figures">
                <span class="suggest-current">{s.current_budget != null ? currencyFormat.format(s.current_budget) : "—"}</span>
                <span class="suggest-arrow">→</span>
                <span class="suggest-new">{currencyFormat.format(s.suggested)}</span>
              </span>
              <button class="btn btn-sm btn-primary" onclick={() => applySuggestion(s)}>Apply</button>
            </div>
          {/each}
        </div>
      {/if}

      <div class="modal-actions">
        <button class="btn" onclick={() => { showSuggestModal = false; }}>Close</button>
        <button class="btn" onclick={() => applyAllSuggestions(true)} disabled={suggestions.length === 0}>Fill empty only</button>
        <button class="btn btn-primary" onclick={() => applyAllSuggestions(false)} disabled={suggestions.length === 0}>Apply all</button>
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
  .btn-add:hover { background: var(--accent); filter: brightness(0.95); }
  .btn-suggest { background: var(--bg-card); color: var(--text-primary); }
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
  .toast-success { background: var(--accent-soft); color: var(--nav-active-fg); border: 1px solid var(--accent); }
  .toast-error { background: var(--neg-soft); color: var(--neg); border: 1px solid var(--neg); }

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

  .modal-lg {
    width: 560px;
  }

  .suggest-intro { font-size: 0.85rem; color: var(--text-secondary); margin-bottom: 0.75rem; }
  .suggest-window { flex-direction: row !important; align-items: center; gap: 0.5rem !important; font-size: 0.85rem; margin-bottom: 1rem; }
  .suggest-window select { padding: 0.35rem 0.5rem; border: 1px solid var(--border-color); border-radius: 8px; background: var(--bg-card); color: var(--text-primary); }
  .suggest-list { display: flex; flex-direction: column; gap: 0.4rem; max-height: 50vh; overflow-y: auto; }
  .suggest-row { display: flex; align-items: center; gap: 0.75rem; padding: 0.45rem 0.6rem; border: 1px solid var(--border-color); border-radius: 8px; }
  .suggest-name { flex: 1; font-size: 0.85rem; color: var(--text-primary); }
  .suggest-figures { display: flex; align-items: center; gap: 0.4rem; font-size: 0.8rem; font-variant-numeric: tabular-nums; white-space: nowrap; }
  .suggest-current { color: var(--text-secondary); }
  .suggest-arrow { color: var(--text-secondary); }
  .suggest-new { color: var(--text-primary); font-weight: 600; }

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
  .rules-section { margin-top: 2.5rem; padding-top: 1.5rem; border-top: 1px solid var(--border-color); }
  .rules-head { display: flex; justify-content: space-between; align-items: flex-start; gap: 1rem; margin-bottom: 1rem; }
  .rules-head h2 { font-size: 1.25rem; font-weight: 700; color: var(--text-primary); margin-bottom: 0.25rem; }
  .rules-intro { font-size: 0.8rem; color: var(--text-secondary); max-width: 55ch; }
  .rule-form { display: flex; flex-wrap: wrap; align-items: center; gap: 0.5rem; margin-bottom: 1rem; }
  .rule-form select, .rule-form input { padding: 0.4rem 0.55rem; border: 1px solid var(--border-color); border-radius: 8px; background: var(--bg-card); color: var(--text-primary); font-size: 0.85rem; }
  .rule-form input[type="text"] { flex: 1; min-width: 8rem; }
  .rule-when, .rule-then { font-size: 0.85rem; color: var(--text-secondary); }
  .rule-list { display: flex; flex-direction: column; gap: 0.4rem; }
  .rule-row { display: flex; justify-content: space-between; align-items: center; gap: 0.75rem; padding: 0.45rem 0.7rem; border: 1px solid var(--border-color); border-radius: 8px; }
  .rule-text { font-size: 0.85rem; color: var(--text-primary); display: flex; align-items: center; gap: 0.4rem; flex-wrap: wrap; }
  .rule-match { color: var(--text-secondary); font-size: 0.78rem; }
  .rule-arrow, .rule-cat { color: var(--text-secondary); }

  .budget-pct { display: inline-block; margin-top: 0.35rem; font-size: 0.75rem; font-weight: 600; }
  .budget-carry { display: block; margin-top: 0.2rem; font-size: 0.72rem; color: var(--pos); font-variant-numeric: tabular-nums; }
  .budget-carry.carry-neg { color: var(--neg); }
  .rollover-badge {
    font-size: 0.6rem;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    font-weight: 600;
    color: var(--accent);
    background: var(--bg-secondary);
    border: 1px solid var(--border-color);
    border-radius: 4px;
    padding: 0.05rem 0.35rem;
    margin-left: 0.4rem;
    vertical-align: middle;
  }
</style>
