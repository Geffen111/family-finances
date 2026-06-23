<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  let apiKey = $state("");
  let originalKeySet = $state(false);
  let saving = $state(false);
  let toastMsg = $state("");
  let toastType = $state<"success" | "error">("success");
  let toastVisible = $state(false);

  // Export state
  let exportingTransactions = $state(false);
  let exportingSummary = $state(false);
  let summaryStartDate = $state("");
  let summaryEndDate = $state("");

  // Historical import state
  let importingHistory = $state(false);
  let historyResult = $state<HistoricalImportSummary | null>(null);
  let historyFileInput = $state<HTMLInputElement | null>(null);

  interface HistoricalImportSummary {
    imported: number;
    skipped_duplicate: number;
    skipped_invalid: number;
    uncategorised: number;
    accounts_created: number;
    categories_created: number;
  }

  $effect(() => {
    invoke<string | null>("get_api_key").then((key) => {
      if (key) {
        originalKeySet = true;
        apiKey = "\u2022".repeat(20);
      }
    });
  });

  function showToast(msg: string, type: "success" | "error") {
    toastMsg = msg;
    toastType = type;
    toastVisible = true;
    setTimeout(() => { toastVisible = false; }, 4000);
  }

  async function handleSave() {
    if (!apiKey || apiKey === "\u2022".repeat(20)) {
      showToast("Please enter an API key.", "error");
      return;
    }
    saving = true;
    try {
      await invoke("save_api_key", { key: apiKey });
      showToast("API key saved.", "success");
      originalKeySet = true;
      apiKey = "\u2022".repeat(20);
    } catch (e) {
      showToast(String(e), "error");
    } finally {
      saving = false;
    }
  }

  function handleClear() {
    apiKey = "";
    originalKeySet = false;
  }

  function downloadCsv(csv: string, filename: string) {
    const blob = new Blob([csv], { type: "text/csv;charset=utf-8;" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = filename;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
  }

  async function handleExportTransactions() {
    exportingTransactions = true;
    try {
      const csv = await invoke<string>("export_transactions_csv");
      downloadCsv(csv, `transactions_${new Date().toISOString().slice(0, 10)}.csv`);
      showToast("Transactions exported.", "success");
    } catch (e) {
      showToast(String(e), "error");
    } finally {
      exportingTransactions = false;
    }
  }

  async function handleExportSummary() {
    exportingSummary = true;
    try {
      const params: Record<string, unknown> = {};
      if (summaryStartDate) params.startDate = summaryStartDate;
      if (summaryEndDate) params.endDate = summaryEndDate;
      const csv = await invoke<string>("export_summary_csv", params);
      downloadCsv(csv, `spending_summary_${new Date().toISOString().slice(0, 10)}.csv`);
      showToast("Summary exported.", "success");
    } catch (e) {
      showToast(String(e), "error");
    } finally {
      exportingSummary = false;
    }
  }

  async function handleHistoryImport(event: Event) {
    const input = event.target as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;

    importingHistory = true;
    historyResult = null;
    try {
      const text = await file.text();
      const summary = await invoke<HistoricalImportSummary>("import_historical_csv", {
        csvContent: text,
      });
      historyResult = summary;
      showToast(`Imported ${summary.imported} transactions.`, "success");
    } catch (e) {
      showToast(String(e), "error");
    } finally {
      importingHistory = false;
      // Allow re-selecting the same file later.
      if (historyFileInput) historyFileInput.value = "";
    }
  }
</script>

<div class="page">
  <h1>Settings</h1>

  {#if toastVisible}
    <div class="toast" class:toast-error={toastType === "error"} class:toast-success={toastType === "success"}>
      {toastMsg}
    </div>
  {/if}

  <div class="setting-card">
    <label for="api-key">OpenRouter API Key</label>
    <p class="hint">Required for AI categorisation and insights (Phases 4-5)</p>
    <input
      id="api-key"
      type="password"
      placeholder="sk-or-..."
      bind:value={apiKey}
    />
    <div class="btn-row">
      <button class="btn btn-primary" onclick={handleSave} disabled={saving}>
        {saving ? "Saving\u2026" : "Save"}
      </button>
      {#if originalKeySet}
        <button class="btn" onclick={handleClear}>Clear</button>
      {/if}
    </div>
  </div>

  <div class="setting-card">
    <h2>Import Historical Data</h2>
    <p class="hint">
      One-time import of your old spreadsheet export (with Category, Subcategory
      and notes preserved). Safe to run more than once &mdash; rows already
      present are skipped. Imported categories also train future
      auto-categorisation.
    </p>
    <input
      type="file"
      accept=".csv"
      bind:this={historyFileInput}
      onchange={handleHistoryImport}
      style="display: none;"
    />
    <div class="btn-row">
      <button
        class="btn btn-primary"
        onclick={() => historyFileInput?.click()}
        disabled={importingHistory}
      >
        {importingHistory ? "Importing…" : "Choose CSV & Import"}
      </button>
    </div>
    {#if historyResult}
      <div class="import-summary">
        <span><strong>{historyResult.imported}</strong> imported</span>
        <span><strong>{historyResult.skipped_duplicate}</strong> already present</span>
        <span><strong>{historyResult.uncategorised}</strong> left uncategorised</span>
        <span><strong>{historyResult.categories_created}</strong> categories created</span>
        <span><strong>{historyResult.accounts_created}</strong> accounts created</span>
        {#if historyResult.skipped_invalid > 0}
          <span><strong>{historyResult.skipped_invalid}</strong> skipped (invalid)</span>
        {/if}
      </div>
    {/if}
  </div>

  <div class="setting-card">
    <h2>Export Data</h2>
    <div class="export-section">
      <div class="export-row">
        <div>
          <p class="export-label">Export All Transactions</p>
          <p class="hint">Download all transactions as CSV with account and category names.</p>
        </div>
        <button class="btn btn-primary" onclick={handleExportTransactions} disabled={exportingTransactions}>
          {exportingTransactions ? "Exporting\u2026" : "Export CSV"}
        </button>
      </div>
      <div class="export-divider"></div>
      <div class="export-row">
        <div>
          <p class="export-label">Export Spending Summary</p>
          <p class="hint">Download spending by category as CSV for a date range.</p>
        </div>
        <div class="export-controls">
          <label class="date-label">
            From
            <input type="date" bind:value={summaryStartDate} />
          </label>
          <label class="date-label">
            To
            <input type="date" bind:value={summaryEndDate} />
          </label>
          <button class="btn btn-primary" onclick={handleExportSummary} disabled={exportingSummary}>
            {exportingSummary ? "Exporting\u2026" : "Export CSV"}
          </button>
        </div>
      </div>
    </div>
  </div>
</div>

<style>
  .page { max-width: 600px; margin: 0 auto; }
  h1 { font-size: 1.75rem; font-weight: 700; color: var(--text-primary); margin-bottom: 1.5rem; }
  h2 { font-size: 1.1rem; font-weight: 600; color: var(--text-primary); margin: 0 0 0.5rem; }
  .setting-card {
    background: var(--bg-card);
    border: 1px solid var(--border-color);
    border-radius: 12px;
    padding: 1.5rem;
    margin-bottom: 1rem;
    box-shadow: 0 1px 2px rgba(0,0,0,0.04);
  }
  label {
    display: block;
    font-weight: 600;
    color: var(--text-primary);
    margin-bottom: 0.25rem;
  }
  .hint {
    font-size: 0.85rem;
    color: var(--text-secondary);
    margin-bottom: 0.75rem;
  }
  input {
    width: 100%;
    padding: 0.625rem 0.75rem;
    border: 1px solid var(--border-color);
    border-radius: 6px;
    font-size: 0.95rem;
    box-sizing: border-box;
    background: var(--bg-card);
    color: var(--text-primary);
  }
  input:focus {
    outline: none;
    border-color: #3b82f6;
    box-shadow: 0 0 0 3px rgba(59, 130, 246, 0.15);
  }
  .btn-row {
    display: flex;
    gap: 0.5rem;
    margin-top: 1rem;
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
  .btn-primary {
    background: #2563eb;
    color: #fff;
    border-color: #2563eb;
  }
  .btn-primary:hover { background: #1d4ed8; }
  .toast {
    position: fixed;
    top: 1rem;
    left: 50%;
    transform: translateX(-50%);
    z-index: 200;
    padding: 0.75rem 1.25rem;
    border-radius: 8px;
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
  .export-section {
    display: flex;
    flex-direction: column;
    gap: 0;
  }
  .export-row {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 1rem;
    padding: 0.5rem 0;
  }
  .export-label {
    font-weight: 500;
    color: var(--text-primary);
    margin: 0 0 0.25rem;
  }
  .export-controls {
    display: flex;
    align-items: flex-end;
    gap: 0.5rem;
    flex-wrap: wrap;
  }
  .date-label {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    font-size: 0.75rem;
    color: var(--text-secondary);
    font-weight: 500;
  }
  .date-label input[type="date"] {
    width: auto;
    padding: 0.35rem 0.5rem;
    font-size: 0.8rem;
  }
  .export-divider {
    height: 1px;
    background: var(--border-color);
    margin: 0.25rem 0;
  }
  .import-summary {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem 1.25rem;
    margin-top: 1rem;
    font-size: 0.85rem;
    color: var(--text-secondary);
  }
  .import-summary strong { color: var(--text-primary); }
</style>