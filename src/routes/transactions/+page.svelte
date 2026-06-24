<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { format, startOfMonth, subMonths } from "date-fns";

  interface Account {
    id: number;
    name: string;
    type: string;
    created_at: string;
  }

  interface Transaction {
    id: number;
    account_id: number;
    category_id: number | null;
    date: string;
    description: string;
    debit: number;
    credit: number;
    balance: number | null;
    ai_category: string | null;
    ai_category_conf: number | null;
    ai_categorised_at: string | null;
    notes: string | null;
    created_at: string;
  }

  interface Category {
    id: number;
    name: string;
    parent_id: number | null;
    monthly_budget: number | null;
    created_at: string;
    path: string;
  }

  interface CategorisationSuggestion {
    transaction_id: number;
    date: string;
    description: string;
    debit: number;
    credit: number;
    suggested_category: string;
    category_id: number | null;
    confidence: number;
    reasoning: string;
  }

  type SortKey = "date" | "description" | "debit" | "credit" | "balance";
  type SortDir = "asc" | "desc";

  let accounts = $state<Account[]>([]);
  let selectedAccountId = $state<number>(0);
  let transactions = $state<Transaction[]>([]);
  let categories = $state<Category[]>([]);
  let loading = $state(false);
  let importing = $state(false);

  let sortKey = $state<SortKey>("date");
  let sortDir = $state<SortDir>("desc");

  let filterStart = $state("");
  let filterEnd = $state("");

  let toastMsg = $state("");
  let toastType = $state<"success" | "error">("success");
  let toastVisible = $state(false);

  let searchText = $state("");

  let aiProcessing = $state(false);
  let aiSuggestions = $state<CategorisationSuggestion[]>([]);
  let acceptedSet = $state<Set<number>>(new Set());
  let showAiModal = $state(false);
  let hasApiKey = $state<boolean>(false);
  let uncategorisedCount = $state(0);

  let lastImport = $state<string | null>(null);

  // "Import for which account?" prompt before the file picker.
  let showImportModal = $state(false);
  let importAccountId = $state<number>(0);

  // Bulk "move to another account" target.
  let moveAccountId = $state<string>("");

  const currencyFormat = new Intl.NumberFormat("en-AU", {
    style: "currency",
    currency: "AUD",
  });

  $effect(() => {
    invoke<Account[]>("get_accounts").then((accs) => {
      accounts = accs;
      if (accs.length > 0 && selectedAccountId === 0) {
        selectedAccountId = accs[0].id;
      }
    });
    invoke<Category[]>("get_categories").then((cats) => {
      categories = cats;
    });
    checkApiKey();
  });

  $effect(() => {
    if (selectedAccountId === 0) return;
    loadTransactions();
  });

  async function checkApiKey() {
    const key = await invoke<string | null>("get_api_key");
    hasApiKey = key != null && key.length > 0;
  }

  async function loadTransactions() {
    loading = true;
    try {
      const params: Record<string, unknown> = { accountId: selectedAccountId };
      if (filterStart) params.startDate = filterStart;
      if (filterEnd) params.endDate = filterEnd;
      transactions = await invoke<Transaction[]>("get_transactions", params);
      uncategorisedCount = transactions.filter((t) => t.category_id == null).length;
      selectedTxIds = new Set();
      loadLastImport();
    } catch (e) {
      showToast(String(e), "error");
    } finally {
      loading = false;
    }
  }

  async function loadLastImport() {
    try {
      lastImport = await invoke<string | null>("get_last_import", {
        accountId: selectedAccountId,
      });
    } catch {
      lastImport = null;
    }
  }

  // Format the stored "YYYY-MM-DD HH:MM:SS" (UTC) timestamp for display.
  function formatLastImport(ts: string): string {
    const d = new Date(ts.replace(" ", "T") + "Z");
    if (isNaN(d.getTime())) return ts;
    return format(d, "d MMM yyyy, h:mm a");
  }

  function showToast(msg: string, type: "success" | "error") {
    toastMsg = msg;
    toastType = type;
    toastVisible = true;
    setTimeout(() => { toastVisible = false; }, 4000);
  }

  async function handleAiCategorise() {
    if (!hasApiKey) {
      showToast("Configure your OpenRouter API key in Settings first.", "error");
      return;
    }
    aiProcessing = true;
    try {
      const all = await invoke<CategorisationSuggestion[]>("categorise_transactions");
      if (all.length === 0) {
        showToast("No uncategorised transactions found.", "success");
        return;
      }

      // Auto-apply very high-confidence matches (mostly history matches at
      // 0.99) without bothering the user; only the rest go to the modal.
      const auto = all.filter((s) => s.confidence >= 0.95 && s.category_id != null);
      const review = all.filter((s) => !(s.confidence >= 0.95 && s.category_id != null));

      let autoApplied = 0;
      if (auto.length > 0) {
        autoApplied = await invoke<number>("accept_categorisations", { suggestions: auto });
      }

      if (review.length > 0) {
        aiSuggestions = review;
        acceptedSet = new Set(review.map((s) => s.transaction_id));
        showAiModal = true;
        if (autoApplied > 0) {
          showToast(`Auto-applied ${autoApplied}; ${review.length} to review.`, "success");
        }
      } else {
        await loadTransactions();
        showToast(`Auto-applied ${autoApplied} categorisation${autoApplied === 1 ? "" : "s"}.`, "success");
      }
    } catch (e) {
      showToast(String(e), "error");
    } finally {
      aiProcessing = false;
    }
  }

  function toggleAccept(txId: number) {
    const next = new Set(acceptedSet);
    if (next.has(txId)) {
      next.delete(txId);
    } else {
      next.add(txId);
    }
    acceptedSet = next;
  }

  function acceptAll() {
    acceptedSet = new Set(aiSuggestions.map((s) => s.transaction_id));
  }

  // Override the AI's suggested category for one row. Picking a category also
  // marks the row as accepted so it's included in "Accept Selected".
  function overrideSuggestion(txId: number, categoryId: number | null) {
    aiSuggestions = aiSuggestions.map((s) =>
      s.transaction_id === txId
        ? {
            ...s,
            category_id: categoryId,
            suggested_category: categoryId === null ? "Uncategorised" : getCategoryPath(categoryId),
          }
        : s,
    );
    const next = new Set(acceptedSet);
    if (categoryId === null) next.delete(txId);
    else next.add(txId);
    acceptedSet = next;
  }

  async function handleAcceptSelected() {
    const selected = aiSuggestions.filter((s) => acceptedSet.has(s.transaction_id));
    if (selected.length === 0) return;
    try {
      const count = await invoke<number>("accept_categorisations", { suggestions: selected });
      showToast(`Accepted ${count} categorisation${count === 1 ? "" : "s"}.`, "success");
      showAiModal = false;
      await loadTransactions();
    } catch (e) {
      showToast(String(e), "error");
    }
  }

  function confidenceColor(conf: number): string {
    if (conf >= 0.8) return "var(--pos)";
    if (conf >= 0.5) return "var(--amber)";
    return "var(--neg)";
  }

  function openImportModal() {
    importAccountId = selectedAccountId || (accounts.length > 0 ? accounts[0].id : 0);
    showImportModal = true;
  }

  // Called once the user has confirmed which account to import into.
  function handleImportCsv(accountId: number) {
    showImportModal = false;
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".csv";
    input.addEventListener("change", async () => {
      const file = input.files?.[0];
      if (!file) return;
      const text = await file.text();
      importing = true;
      try {
        const res = await invoke<{ imported: number; skipped_duplicate: number }>("csv_import", {
          csvContent: text,
          accountId,
        });
        const dupNote = res.skipped_duplicate > 0 ? ` (${res.skipped_duplicate} duplicate${res.skipped_duplicate === 1 ? "" : "s"} skipped)` : "";
        const acctName = accounts.find((a) => a.id === accountId)?.name ?? "account";
        showToast(`Imported ${res.imported} transaction${res.imported === 1 ? "" : "s"} into ${acctName}${dupNote}.`, "success");
        // Jump to the account we imported into so the user sees the result.
        if (accountId !== selectedAccountId) {
          selectedAccountId = accountId;
        } else {
          await loadTransactions();
        }
      } catch (e) {
        showToast(String(e), "error");
      } finally {
        importing = false;
      }
    });
    input.click();
  }

  function setDatePreset(preset: string) {
    const today = new Date();
    switch (preset) {
      case "thisMonth":
        filterStart = format(startOfMonth(today), "yyyy-MM-dd");
        filterEnd = "";
        break;
      case "last3Months":
        filterStart = format(startOfMonth(subMonths(today, 2)), "yyyy-MM-dd");
        filterEnd = "";
        break;
      case "all":
        filterStart = "";
        filterEnd = "";
        break;
    }
  }

  function applyFilterAndLoad() {
    loadTransactions();
  }

  function toggleSort(key: SortKey) {
    if (sortKey === key) {
      sortDir = sortDir === "asc" ? "desc" : "asc";
    } else {
      sortKey = key;
      sortDir = "asc";
    }
  }

  function sortIcon(key: SortKey): string {
    if (sortKey !== key) return "\u2195";
    return sortDir === "asc" ? "\u2191" : "\u2193";
  }

  let sortedTransactions = $derived(
    [...transactions].sort((a, b) => {
      let cmp = 0;
      switch (sortKey) {
        case "date":
          cmp = a.date.localeCompare(b.date);
          break;
        case "description":
          cmp = a.description.localeCompare(b.description);
          break;
        case "debit":
          cmp = a.debit - b.debit;
          break;
        case "credit":
          cmp = a.credit - b.credit;
          break;
        case "balance":
          cmp = (a.balance ?? 0) - (b.balance ?? 0);
          break;
      }
      return sortDir === "asc" ? cmp : -cmp;
    })
  );

  // Client-side search across description and amounts.
  let visibleTransactions = $derived.by(() => {
    const q = searchText.trim().toLowerCase();
    if (!q) return sortedTransactions;
    return sortedTransactions.filter(
      (t) =>
        t.description.toLowerCase().includes(q) ||
        String(t.debit).includes(q) ||
        String(t.credit).includes(q),
    );
  });

  let totalDebits = $derived(visibleTransactions.reduce((sum, t) => sum + t.debit, 0));
  let totalCredits = $derived(visibleTransactions.reduce((sum, t) => sum + t.credit, 0));
  let net = $derived(totalCredits - totalDebits);

  let subcategories = $derived(categories.filter((c) => c.parent_id !== null));

  // Bulk selection + re-categorise.
  let selectedTxIds = $state<Set<number>>(new Set());
  let bulkCategoryId = $state<string>("");

  let allVisibleSelected = $derived(
    visibleTransactions.length > 0 && visibleTransactions.every((t) => selectedTxIds.has(t.id)),
  );

  function toggleSelect(id: number) {
    const next = new Set(selectedTxIds);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    selectedTxIds = next;
  }

  function toggleSelectAll() {
    if (allVisibleSelected) {
      selectedTxIds = new Set();
    } else {
      selectedTxIds = new Set(visibleTransactions.map((t) => t.id));
    }
  }

  async function applyBulkCategory() {
    if (selectedTxIds.size === 0) return;
    try {
      const count = await invoke<number>("assign_categories_bulk", {
        transactionIds: [...selectedTxIds],
        categoryId: bulkCategoryId ? Number(bulkCategoryId) : null,
      });
      showToast(`Updated ${count} transaction${count === 1 ? "" : "s"}.`, "success");
      selectedTxIds = new Set();
      bulkCategoryId = "";
      await loadTransactions();
    } catch (e) {
      showToast(String(e), "error");
    }
  }

  async function applyMoveAccount() {
    if (selectedTxIds.size === 0 || !moveAccountId) return;
    const targetId = Number(moveAccountId);
    const targetName = accounts.find((a) => a.id === targetId)?.name ?? "account";
    try {
      const res = await invoke<{ moved: number; skipped_duplicate: number }>("move_transactions", {
        transactionIds: [...selectedTxIds],
        accountId: targetId,
      });
      const dupNote = res.skipped_duplicate > 0
        ? ` (${res.skipped_duplicate} duplicate${res.skipped_duplicate === 1 ? "" : "s"} already there, removed)`
        : "";
      showToast(`Moved ${res.moved} transaction${res.moved === 1 ? "" : "s"} to ${targetName}${dupNote}.`, "success");
      selectedTxIds = new Set();
      moveAccountId = "";
      await loadTransactions();
    } catch (e) {
      showToast(String(e), "error");
    }
  }

  function getCategoryPath(categoryId: number | null): string {
    if (categoryId == null) return "";
    const cat = categories.find((c) => c.id === categoryId);
    return cat ? cat.path : "";
  }

  async function handleAssignCategory(transactionId: number, categoryId: number | null) {
    try {
      await invoke("assign_category", {
        transactionId,
        categoryId,
      });
      await loadTransactions();
    } catch (e) {
      showToast(String(e), "error");
    }
  }

  function fmt(val: number | null | undefined): string {
    if (val == null) return "-";
    return currencyFormat.format(val);
  }
</script>

<svelte:window onkeydown={(e) => { if (e.key === "Escape") { showAiModal = false; showImportModal = false; } }} />

<div class="page">
  <div class="header">
    <h1>Transactions</h1>
    <div class="header-actions">
      {#if uncategorisedCount > 0}
        {#if !hasApiKey}
          <a href="/settings" class="btn btn-warning">
            {uncategorisedCount} uncategorised \u2014 Set API Key
          </a>
        {:else}
          <button class="btn btn-ai" onclick={handleAiCategorise} disabled={aiProcessing}>
            {aiProcessing ? "AI Processing\u2026" : `${uncategorisedCount} uncategorised \u2014 AI Categorise`}
          </button>
        {/if}
      {/if}
      <button class="btn btn-import" onclick={openImportModal} disabled={importing}>
        {importing ? "Importing\u2026" : "Import CSV"}
      </button>
    </div>
  </div>

  {#if toastVisible}
    <div class="toast" class:toast-error={toastType === "error"} class:toast-success={toastType === "success"}>
      {toastMsg}
    </div>
  {/if}

  <div class="account-selector">
    {#each accounts as acc}
      <button
        class="account-btn"
        class:active={acc.id === selectedAccountId}
        onclick={() => { selectedAccountId = acc.id; }}
      >
        {acc.name}
      </button>
    {/each}
  </div>

  <p class="last-import">
    {#if lastImport}
      Last import for this account: {formatLastImport(lastImport)}
    {:else}
      No transactions imported for this account yet.
    {/if}
  </p>

  <div class="filters">
    <label>
      From
      <input type="date" bind:value={filterStart} onchange={applyFilterAndLoad} />
    </label>
    <label>
      To
      <input type="date" bind:value={filterEnd} onchange={applyFilterAndLoad} />
    </label>
    <button class="btn btn-sm" onclick={() => { setDatePreset("thisMonth"); applyFilterAndLoad(); }}>This Month</button>
    <button class="btn btn-sm" onclick={() => { setDatePreset("last3Months"); applyFilterAndLoad(); }}>Last 3 Months</button>
    <button class="btn btn-sm" onclick={() => { setDatePreset("all"); applyFilterAndLoad(); }}>All Time</button>
    <input class="search-input" type="search" placeholder="Search description or amount…" bind:value={searchText} />
  </div>

  {#if selectedTxIds.size > 0}
    <div class="bulk-bar">
      <span class="bulk-count">{selectedTxIds.size} selected</span>
      <select class="cat-select" bind:value={bulkCategoryId}>
        <option value="">Uncategorised</option>
        {#each subcategories as cat (cat.id)}
          <option value={cat.id}>{cat.path}</option>
        {/each}
      </select>
      <button class="btn btn-sm btn-primary" onclick={applyBulkCategory}>Apply to selected</button>
      <span class="bulk-sep"></span>
      <span class="bulk-label">Move to</span>
      <select class="cat-select" bind:value={moveAccountId}>
        <option value="">Choose account…</option>
        {#each accounts as acc (acc.id)}
          {#if acc.id !== selectedAccountId}
            <option value={acc.id}>{acc.name}</option>
          {/if}
        {/each}
      </select>
      <button class="btn btn-sm" onclick={applyMoveAccount} disabled={!moveAccountId}>Move account</button>
      <button class="btn btn-sm" onclick={() => { selectedTxIds = new Set(); }}>Clear</button>
    </div>
  {/if}

  {#if loading}
    <p class="loading">Loading transactions\u2026</p>
  {:else if transactions.length === 0}
    <div class="empty-state">
      <p>No transactions yet. Import a CSV to get started.</p>
    </div>
  {:else}
    <div class="table-wrap">
      <table class="tx-table">
        <thead>
          <tr>
            <th class="cell-check">
              <input type="checkbox" checked={allVisibleSelected} onchange={toggleSelectAll} aria-label="Select all" />
            </th>
            <th class="sortable" onclick={() => toggleSort("date")}>
              Date <span class="sort-icon">{sortIcon("date")}</span>
            </th>
            <th class="sortable" onclick={() => toggleSort("description")}>
              Description <span class="sort-icon">{sortIcon("description")}</span>
            </th>
            <th class="sortable" onclick={() => toggleSort("debit")}>
              Debit <span class="sort-icon">{sortIcon("debit")}</span>
            </th>
            <th class="sortable" onclick={() => toggleSort("credit")}>
              Credit <span class="sort-icon">{sortIcon("credit")}</span>
            </th>
            <th class="sortable" onclick={() => toggleSort("balance")}>
              Balance <span class="sort-icon">{sortIcon("balance")}</span>
            </th>
            <th>Categorise</th>
          </tr>
        </thead>
        <tbody>
          {#each visibleTransactions as tx (tx.id)}
            <tr class:row-selected={selectedTxIds.has(tx.id)}>
              <td class="cell-check">
                <input type="checkbox" checked={selectedTxIds.has(tx.id)} onchange={() => toggleSelect(tx.id)} aria-label="Select transaction" />
              </td>
              <td class="cell-date">{tx.date}</td>
              <td class="cell-desc">{tx.description}</td>
              <td class="cell-debit">{tx.debit > 0 ? fmt(tx.debit) : "-"}</td>
              <td class="cell-credit">{tx.credit > 0 ? fmt(tx.credit) : "-"}</td>
              <td class="cell-balance">{fmt(tx.balance)}</td>
              <td class="cell-category">
                <select
                  class="cat-select"
                  value={tx.category_id ?? ""}
                  onchange={(e) => {
                    const val = (e.target as HTMLSelectElement).value;
                    handleAssignCategory(tx.id, val ? Number(val) : null);
                  }}
                >
                  <option value="">Uncategorised</option>
                  {#each subcategories as cat (cat.id)}
                    <option value={cat.id}>{cat.path}</option>
                  {/each}
                </select>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>

    <div class="summary">
      <div class="summary-item">
        <span class="summary-label">Total Debits</span>
        <span class="summary-value summary-debit">{fmt(totalDebits)}</span>
      </div>
      <div class="summary-item">
        <span class="summary-label">Total Credits</span>
        <span class="summary-value summary-credit">{fmt(totalCredits)}</span>
      </div>
      <div class="summary-item">
        <span class="summary-label">Net</span>
        <span class="summary-value" class:summary-debit={net < 0} class:summary-credit={net >= 0}>{fmt(net)}</span>
      </div>
    </div>
  {/if}
</div>

{#if showAiModal}
  <div class="modal-overlay" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) showAiModal = false; }}>
    <div class="modal" role="dialog" aria-modal="true" tabindex="-1">
      <div class="modal-header">
        <h2>AI Categorisation Review</h2>
        <button class="modal-close" onclick={() => { showAiModal = false; }}>&times;</button>
      </div>
      <div class="modal-body">
        <p class="modal-hint">
          Review the AI suggestions below. Transactions with high confidence are pre-accepted.
        </p>
        {#each aiSuggestions as s}
          <div class="suggestion-row" class:suggestion-accepted={acceptedSet.has(s.transaction_id)}>
            <div class="suggestion-check">
              <input
                type="checkbox"
                checked={acceptedSet.has(s.transaction_id)}
                onchange={() => toggleAccept(s.transaction_id)}
              />
            </div>
            <div class="suggestion-details">
              <div class="suggestion-meta">
                <span class="suggestion-date">{s.date}</span>
                <span class="suggestion-desc">{s.description}</span>
                <span class="suggestion-amount">
                  {s.debit > 0 ? fmt(s.debit) + " dr" : fmt(s.credit) + " cr"}
                </span>
              </div>
              <div class="suggestion-category">
                <select
                  class="suggestion-select"
                  value={s.category_id ?? ""}
                  onchange={(e) => overrideSuggestion(s.transaction_id, e.currentTarget.value ? Number(e.currentTarget.value) : null)}
                >
                  <option value="">&mdash; Uncategorised &mdash;</option>
                  {#each subcategories as cat (cat.id)}
                    <option value={cat.id}>{cat.path}</option>
                  {/each}
                </select>
                <span class="suggestion-reasoning">{s.reasoning}</span>
              </div>
              <div class="suggestion-confidence">
                <div class="conf-bar-track">
                  <div
                    class="conf-bar-fill"
                    style="width: {Math.round(s.confidence * 100)}%; background: {confidenceColor(s.confidence)};"
                  ></div>
                </div>
                <span class="conf-label" style="color: {confidenceColor(s.confidence)};">
                  {Math.round(s.confidence * 100)}%
                </span>
              </div>
            </div>
          </div>
        {/each}
      </div>
      <div class="modal-footer">
        <div class="modal-footer-left">
          <span class="selected-count">{acceptedSet.size} of {aiSuggestions.length} selected</span>
        </div>
        <div class="modal-footer-right">
          <button class="btn" onclick={acceptAll}>Accept All</button>
          <button class="btn btn-primary" onclick={handleAcceptSelected} disabled={acceptedSet.size === 0}>
            Accept Selected ({acceptedSet.size})
          </button>
        </div>
      </div>
    </div>
  </div>
{/if}

{#if showImportModal}
  <div class="modal-overlay" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) showImportModal = false; }}>
    <div class="modal modal-sm" role="dialog" aria-modal="true" tabindex="-1">
      <div class="modal-header">
        <h2>Import transactions for which account?</h2>
        <button class="modal-close" onclick={() => { showImportModal = false; }}>&times;</button>
      </div>
      <div class="modal-body">
        <p class="modal-hint">Choose the account these transactions belong to, then pick your CSV file.</p>
        <select class="cat-select import-account-select" bind:value={importAccountId}>
          {#each accounts as acc (acc.id)}
            <option value={acc.id}>{acc.name}</option>
          {/each}
        </select>
      </div>
      <div class="modal-footer">
        <div class="modal-footer-left"></div>
        <div class="modal-footer-right">
          <button class="btn" onclick={() => { showImportModal = false; }}>Cancel</button>
          <button class="btn btn-primary" onclick={() => handleImportCsv(importAccountId)} disabled={!importAccountId}>
            Choose CSV…
          </button>
        </div>
      </div>
    </div>
  </div>
{/if}

<style>
  .page { max-width: 1320px; margin: 0 auto; }
  .header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 1rem; }
  h1 { font-size: 1.75rem; font-weight: 700; color: var(--text-primary); }
  .header-actions { display: flex; gap: 0.5rem; align-items: center; }
  .btn { padding: 0.5rem 1rem; border: 1px solid var(--border-color); border-radius: 10px; background: var(--bg-card); color: var(--text-primary); font-size: 0.875rem; cursor: pointer; transition: background 0.15s; }
  .btn:hover { background: var(--bg-secondary); }
  .btn:disabled { opacity: 0.5; cursor: not-allowed; }
  .btn-sm { padding: 0.3rem 0.65rem; font-size: 0.8rem; }
  .btn-primary { background: var(--accent); color: #fff; border-color: var(--accent); }
  .btn-primary:hover { background: var(--accent); }
  .btn-import { background: var(--accent); color: #fff; border-color: var(--accent); }
  .btn-import:hover { background: var(--accent); }
  .btn-ai { background: #7c3aed; color: #fff; border-color: #7c3aed; }
  .btn-ai:hover { background: #6d28d9; }
  .btn-warning { background: var(--amber); color: #fff; border-color: var(--amber); text-decoration: none; font-size: 0.875rem; padding: 0.5rem 1rem; border-radius: 10px; }
  .btn-warning:hover { background: #b45309; }
  .toast { position: fixed; top: 1rem; left: 50%; transform: translateX(-50%); z-index: 200; padding: 0.75rem 1.25rem; border-radius: 14px; font-size: 0.875rem; box-shadow: 0 8px 24px rgba(0, 0, 0, 0.15); animation: toast-in 0.2s ease-out; }
  @keyframes toast-in { from { opacity: 0; transform: translateX(-50%) translateY(-0.5rem); } to { opacity: 1; transform: translateX(-50%) translateY(0); } }
  .toast-success { background: #d1fae5; color: #065f46; border: 1px solid #a7f3d0; }
  .toast-error { background: #fee2e2; color: #991b1b; border: 1px solid #fecaca; }
  .account-selector { display: flex; gap: 0.5rem; margin-bottom: 1rem; }
  .account-btn { padding: 0.5rem 1rem; border: 1px solid var(--border-color); border-radius: 10px; background: var(--bg-card); color: var(--text-primary); font-size: 0.875rem; cursor: pointer; transition: background 0.15s, border-color 0.15s; }
  .account-btn:hover { background: var(--bg-secondary); }
  .account-btn.active { background: var(--accent); color: #fff; border-color: var(--accent); }
  .last-import { font-size: 0.8rem; color: var(--text-secondary); margin: -0.5rem 0 1rem; }
  .filters { display: flex; align-items: center; gap: 0.75rem; margin-bottom: 1rem; flex-wrap: wrap; }
  .filters label { font-size: 0.8rem; color: var(--text-secondary); display: flex; align-items: center; gap: 0.3rem; }
  .filters input[type="date"] { padding: 0.3rem 0.5rem; border: 1px solid var(--border-color); border-radius: 4px; font-size: 0.8rem; }
  .search-input { margin-left: auto; min-width: 220px; padding: 0.4rem 0.6rem; border: 1px solid var(--border-color); border-radius: 10px; font-size: 0.85rem; background: var(--bg-card); color: var(--text-primary); }

  .bulk-bar { display: flex; align-items: center; gap: 0.75rem; margin-bottom: 0.75rem; padding: 0.6rem 0.9rem; background: var(--accent-soft); border: 1px solid var(--border-color); border-radius: 14px; flex-wrap: wrap; }
  .bulk-count { font-size: 0.85rem; font-weight: 600; color: var(--accent); }
  .bulk-bar .cat-select { width: auto; min-width: 200px; }
  .bulk-sep { width: 1px; align-self: stretch; background: var(--border-color); margin: 0 0.25rem; }
  .bulk-label { font-size: 0.85rem; color: var(--text-secondary); }

  .cell-check { width: 2.2rem; text-align: center; }
  .cell-check input { cursor: pointer; }
  .row-selected { background: var(--accent-soft); }
  .loading { color: var(--text-secondary); padding: 2rem 0; }
  .empty-state { border: 2px dashed var(--border-color); border-radius: 14px; padding: 3rem 2rem; text-align: center; color: var(--text-secondary); font-size: 1rem; }
  .table-wrap { max-height: 60vh; overflow-y: auto; border: 1px solid var(--border-color); border-radius: var(--radius-card); box-shadow: var(--app-shadow); }
  .tx-table { width: 100%; border-collapse: collapse; font-size: 0.875rem; }
  .tx-table thead { position: sticky; top: 0; z-index: 1; }
  .tx-table th { background: var(--bg-secondary); padding: 0.6rem 0.75rem; text-align: left; font-weight: 600; color: var(--text-primary); border-bottom: 2px solid var(--border-color); white-space: nowrap; user-select: none; }
  .tx-table th.sortable { cursor: pointer; }
  .tx-table th.sortable:hover { background: var(--border-color); }
  .sort-icon { font-size: 0.7rem; margin-left: 0.25rem; color: var(--text-muted); }
  .tx-table td { padding: 0.5rem 0.75rem; border-bottom: 1px solid var(--bg-secondary); }
  .cell-date { white-space: nowrap; color: var(--text-secondary); }
  .cell-desc { max-width: 300px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text-primary); }
  .cell-debit { text-align: right; color: var(--neg); font-variant-numeric: tabular-nums; }
  .cell-credit { text-align: right; color: var(--pos); font-variant-numeric: tabular-nums; }
  .cell-balance { text-align: right; font-variant-numeric: tabular-nums; color: var(--text-primary); }
  .cell-category { min-width: 200px; }
  .cat-select { width: 100%; padding: 0.3rem 0.4rem; font-size: 0.8rem; border: 1px solid var(--border-color); border-radius: 4px; background: var(--bg-card); color: var(--text-primary); cursor: pointer; }
  .cat-select:hover { border-color: var(--text-muted); }
  .summary { display: flex; gap: 2rem; margin-top: 1rem; padding: 0.75rem 1rem; background: var(--bg-secondary); border: 1px solid var(--border-color); border-radius: var(--radius-card); }
  .summary-item { display: flex; flex-direction: column; gap: 0.15rem; }
  .summary-label { font-size: 0.75rem; color: var(--text-secondary); text-transform: uppercase; letter-spacing: 0.05em; }
  .summary-value { font-size: 1.05rem; font-weight: 700; font-variant-numeric: tabular-nums; }
  .summary-debit { color: var(--neg); }
  .summary-credit { color: var(--pos); }

  .modal-overlay {
    position: fixed; inset: 0; background: rgba(0,0,0,0.5);
    display: flex; align-items: center; justify-content: center;
    z-index: 1000;
  }
  .modal {
    background: var(--bg-card); border-radius: var(--radius-card); width: min(700px, 90vw);
    max-height: 85vh; display: flex; flex-direction: column;
    box-shadow: 0 20px 60px rgba(0,0,0,0.2);
  }
  .modal-sm { width: min(440px, 90vw); }
  .import-account-select { width: 100%; padding: 0.5rem; font-size: 0.9rem; }
  .modal-header {
    display: flex; justify-content: space-between; align-items: center;
    padding: 1rem 1.5rem; border-bottom: 1px solid var(--border-color);
  }
  .modal-header h2 { margin: 0; font-size: 1.2rem; font-weight: 700; color: var(--text-primary); }
  .modal-close {
    background: none; border: none; font-size: 1.5rem; color: var(--text-secondary);
    cursor: pointer; padding: 0; line-height: 1;
  }
  .modal-close:hover { color: var(--text-primary); }
  .modal-body {
    padding: 1rem 1.5rem; overflow-y: auto; flex: 1;
  }
  .modal-hint {
    font-size: 0.85rem; color: var(--text-secondary); margin: 0 0 1rem 0;
  }
  .suggestion-row {
    display: flex; gap: 0.75rem; padding: 0.75rem;
    border: 1px solid var(--border-color); border-radius: 14px; margin-bottom: 0.5rem;
    align-items: flex-start;
  }
  .suggestion-accepted { background: #f0fdf4; border-color: #bbf7d0; }
  .suggestion-check { padding-top: 0.25rem; }
  .suggestion-check input { width: 1.1rem; height: 1.1rem; cursor: pointer; }
  .suggestion-details { flex: 1; min-width: 0; }
  .suggestion-meta {
    display: flex; gap: 0.75rem; align-items: baseline;
    font-size: 0.85rem; margin-bottom: 0.3rem;
  }
  .suggestion-date { color: var(--text-secondary); white-space: nowrap; }
  .suggestion-desc { font-weight: 500; color: var(--text-primary); flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .suggestion-amount { font-weight: 600; white-space: nowrap; font-variant-numeric: tabular-nums; color: var(--text-primary); }
  .suggestion-category { margin-bottom: 0.3rem; display: flex; align-items: center; gap: 0.5rem; flex-wrap: wrap; }
  .suggestion-select {
    font-size: 0.82rem;
    font-weight: 600;
    color: var(--accent);
    padding: 0.25rem 0.4rem;
    border: 1px solid var(--border-color);
    border-radius: 5px;
    background: var(--bg-card);
    max-width: 100%;
  }
  .suggestion-reasoning { font-size: 0.78rem; color: var(--text-secondary); }
  .suggestion-confidence { display: flex; align-items: center; gap: 0.5rem; }
  .conf-bar-track { flex: 1; height: 6px; background: var(--border-color); border-radius: 3px; max-width: 120px; }
  .conf-bar-fill { height: 100%; border-radius: 3px; transition: width 0.3s; }
  .conf-label { font-size: 0.78rem; font-weight: 700; white-space: nowrap; }
  .modal-footer {
    display: flex; justify-content: space-between; align-items: center;
    padding: 1rem 1.5rem; border-top: 1px solid var(--border-color);
  }
  .modal-footer-left { font-size: 0.85rem; color: var(--text-secondary); }
  .modal-footer-right { display: flex; gap: 0.5rem; }
  .selected-count { font-weight: 500; }
</style>