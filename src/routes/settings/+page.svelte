<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { showToast } from "$lib/stores/toast.svelte";
  import ModelCombobox, { type ModelInfo } from "$lib/components/ModelCombobox.svelte";

  let apiKey = $state("");
  let originalKeySet = $state(false);
  let saving = $state(false);

  // AI model state. `model` is what every AI feature sends; `fallbackModel`
  // (optional) is tried once when that model errors or replies with something
  // we can't parse, so a bad model id doesn't fail a whole categorisation run.
  let models = $state<ModelInfo[]>([]);
  let modelsLoading = $state(true);
  let modelsError = $state("");
  let model = $state("");
  let fallbackModel = $state("");
  let defaultModel = $state("");
  let savedModel = $state("");
  let savedFallback = $state("");
  let savingModel = $state(false);
  let testingModel = $state(false);
  let testResult = $state("");

  let modelDirty = $derived(model !== savedModel || fallbackModel !== savedFallback);

  // Household name state
  let householdName = $state("Our Home");
  let savingHousehold = $state(false);

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
    invoke<string | null>("get_household_name").then((name) => {
      if (name) householdName = name;
    });
    invoke<AiModelSettings>("get_ai_model_settings").then((s) => {
      model = s.model;
      savedModel = s.model;
      fallbackModel = s.fallback_model ?? "";
      savedFallback = s.fallback_model ?? "";
      defaultModel = s.default_model;
    });
    // The catalogue is a public endpoint, so this works before a key is saved.
    invoke<ModelInfo[]>("list_ai_models")
      .then((list) => {
        models = list;
      })
      .catch((e) => {
        modelsError = String(e);
      })
      .finally(() => {
        modelsLoading = false;
      });
  });

  interface AiModelSettings {
    model: string;
    fallback_model: string | null;
    default_model: string;
  }

  async function handleSaveModel() {
    savingModel = true;
    testResult = "";
    try {
      const saved = await invoke<AiModelSettings>("save_ai_model_settings", {
        model,
        fallbackModel: fallbackModel || null,
      });
      model = saved.model;
      savedModel = saved.model;
      fallbackModel = saved.fallback_model ?? "";
      savedFallback = saved.fallback_model ?? "";
      showToast("AI model saved.", "success");
    } catch (e) {
      showToast(String(e), "error");
    } finally {
      savingModel = false;
    }
  }

  async function handleTestModel() {
    testingModel = true;
    testResult = "";
    try {
      testResult = await invoke<string>("test_ai_model", { model });
    } catch (e) {
      showToast(String(e), "error");
      testResult = "";
    } finally {
      testingModel = false;
    }
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

  async function handleSaveHousehold() {
    if (!householdName.trim()) return;
    savingHousehold = true;
    try {
      await invoke("save_household_name", { name: householdName.trim() });
      showToast("Household name saved.", "success");
    } catch (e) {
      showToast(String(e), "error");
    } finally {
      savingHousehold = false;
    }
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

  <div class="setting-card">
    <label for="api-key">OpenRouter API Key</label>
    <p class="hint">Required for AI categorisation, Ask and insights.</p>
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
    <h2>AI Model</h2>
    <p class="hint">
      Used for transaction categorisation, Ask and insights. Models are listed
      live from OpenRouter &mdash; search by name, or type any model id.
    </p>

    {#if modelsLoading}
      <p class="hint">Loading models…</p>
    {:else if modelsError}
      <p class="model-warn">
        Couldn’t load the model list ({modelsError}). You can still type a
        model id into the picker.
      </p>
    {/if}

    <p class="field-label">Model</p>
    <ModelCombobox
      {models}
      value={model}
      onSelect={(id) => {
        model = id;
        testResult = "";
      }}
      placeholder={defaultModel || "Choose a model…"}
    />

    <p class="field-label stacked">Fallback model</p>
    <p class="hint">
      Tried once if the model above fails or returns an answer the app can’t
      read. Leave as None to see the error instead.
    </p>
    <ModelCombobox
      {models}
      value={fallbackModel}
      onSelect={(id) => (fallbackModel = id)}
      allowNone
      noneLabel="None"
    />

    <div class="btn-row">
      <button class="btn btn-primary" onclick={handleSaveModel} disabled={savingModel || !modelDirty}>
        {savingModel ? "Saving…" : modelDirty ? "Save" : "Saved"}
      </button>
      <button class="btn" onclick={handleTestModel} disabled={testingModel}>
        {testingModel ? "Testing…" : "Test"}
      </button>
      {#if defaultModel && model !== defaultModel}
        <button class="btn" onclick={() => (model = defaultModel)}>Use default</button>
      {/if}
    </div>
    {#if testResult}
      <p class="model-ok">{testResult}</p>
    {/if}
    {#if modelDirty}
      <p class="hint model-dirty">Unsaved changes &mdash; Test uses the model shown above.</p>
    {/if}
  </div>

  <div class="setting-card">
    <h2>Household Name</h2>
    <p class="hint">The name displayed in the sidebar for your household.</p>
    <input
      type="text"
      placeholder="Our Home"
      bind:value={householdName}
    />
    <div class="btn-row">
      <button class="btn btn-primary" onclick={handleSaveHousehold} disabled={savingHousehold}>
        {savingHousehold ? "Saving\u2026" : "Save"}
      </button>
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
    border-radius: var(--radius-card);
    padding: 1.5rem;
    margin-bottom: 1rem;
    box-shadow: var(--app-shadow);
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
    border-radius: 10px;
    font-size: 0.95rem;
    box-sizing: border-box;
    background: var(--bg-card);
    color: var(--text-primary);
  }
  input:focus {
    outline: none;
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-soft);
  }
  .btn-row {
    display: flex;
    gap: 0.5rem;
    margin-top: 1rem;
  }
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
  .field-label {
    font-weight: 600;
    color: var(--text-primary);
    margin: 0 0 0.25rem;
  }
  .stacked { margin-top: 1.25rem; }
  .model-warn {
    font-size: 0.8rem;
    color: var(--negative, #c0392b);
    margin: 0 0 0.75rem;
  }
  .model-ok {
    font-size: 0.8rem;
    color: var(--text-secondary);
    margin: 0.75rem 0 0;
    word-break: break-word;
  }
  .model-dirty { margin: 0.5rem 0 0; }
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