<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import { darkMode } from "$lib/stores/theme.svelte";

  interface Scenario {
    id: number;
    name: string;
    description: string | null;
    horizon: string;
    base_start_date: string;
    base_end_date: string;
    created_at: string;
  }

  interface ScenarioAdjustment {
    id: number;
    scenario_id: number;
    category_id: number;
    adjustment_pct: number;
    fixed_amount: number | null;
  }

  interface ScenarioAdjustmentWithPath {
    id: number;
    scenario_id: number;
    category_id: number;
    category_path: string;
    adjustment_pct: number;
    fixed_amount: number | null;
  }

  interface ScenarioDefault {
    id: number;
    scenario_id: number;
    default_adjustment_pct: number;
    income_growth_pct: number;
  }

  interface CategoryWithPath {
    id: number;
    name: string;
    parent_id: number | null;
    monthly_budget: number | null;
    created_at: string;
    exclude_from_budget: boolean;
    path: string;
  }

  interface ForecastCategoryAmount {
    category_id: number;
    category_path: string;
    amount: number;
  }

  interface ForecastMonth {
    label: string;
    month_key: string;
    projected_income: number;
    projected_expenses: number;
    projected_net: number;
    categories: ForecastCategoryAmount[];
  }

  interface ForecastTotals {
    total_projected_income: number;
    total_projected_expenses: number;
    total_projected_net: number;
  }

  interface ForecastResult {
    scenario_name: string;
    scenario_id: number;
    months: ForecastMonth[];
    totals: ForecastTotals;
  }

  interface ForecastComparison {
    base: ForecastResult;
    scenarios: ForecastResult[];
    months_ahead: number;
  }

  let scenarios = $state<Scenario[]>([]);
  let categories = $state<CategoryWithPath[]>([]);
  // Categories excluded from budgets (e.g. internal transfers) never enter the
  // forecast baselines, so adjusting them does nothing — hide them here.
  let adjustableCategories = $derived(categories.filter((c) => !c.exclude_from_budget));
  let selectedScenario = $state<Scenario | null>(null);
  let adjustments = $state<ScenarioAdjustmentWithPath[]>([]);
  let defaults = $state<ScenarioDefault | null>(null);
  // Category IDs excluded from the selected scenario's projection (forecast-only,
  // independent of the global exclude_from_budget flag).
  let excludedCategoryIds = $state<number[]>([]);

  let loadingScenarios = $state(false);
  let loadingCategories = $state(false);
  let loadingAdjustments = $state(false);

  let showNewModal = $state(false);
  let newName = $state("");
  let newDesc = $state("");
  let newHorizon = $state("monthly");
  let newStart = $state("");
  let newEnd = $state("");

  // Forecast controls
  let monthsAhead = $state(12);
  let selectedForecastIds = $state<number[]>([]);
  let forecastResult = $state<ForecastComparison | null>(null);
  let forecastLoading = $state(false);
  let forecastError = $state("");

  // Adjustment editing
  let editingDefaults = $state(false);
  let editDefaultPct = $state(0);
  let editGrowthPct = $state(0);

  let toastMsg = $state("");
  let toastType = $state<"success" | "error">("success");
  let toastVisible = $state(false);

  const currencyFormat = new Intl.NumberFormat("en-AU", { style: "currency", currency: "AUD" });
  function fmt(val: number): string { return currencyFormat.format(val); }

  let lineChart: import("chart.js").Chart<"line"> | null = null;

  // Resolve Hearth theme tokens to concrete colors for Chart.js (which can't read var()).
  function themeVar(name: string, fallback = ""): string {
    if (typeof document === "undefined") return fallback;
    const el = document.querySelector(".app-layout") ?? document.documentElement;
    return getComputedStyle(el).getPropertyValue(name).trim() || fallback;
  }
  function chartSeries(): string[] {
    return ["--c1", "--c2", "--c3", "--c4", "--c5", "--c6"].map((n) => themeVar(n, "#7f9a6f"));
  }

  function showToast(msg: string, type: "success" | "error") {
    toastMsg = msg;
    toastType = type;
    toastVisible = true;
    setTimeout(() => { toastVisible = false; }, 4000);
  }

  $effect(() => {
    loadScenarios();
    loadCategories();
  });

  async function loadScenarios() {
    loadingScenarios = true;
    try {
      scenarios = await invoke<Scenario[]>("list_scenarios");
    } catch (e) {
      showToast(String(e), "error");
    } finally {
      loadingScenarios = false;
    }
  }

  async function loadCategories() {
    loadingCategories = true;
    try {
      categories = await invoke<CategoryWithPath[]>("get_categories");
    } catch (e) {
      showToast(String(e), "error");
    } finally {
      loadingCategories = false;
    }
  }

  async function selectScenario(s: Scenario) {
    selectedScenario = s;
    loadingAdjustments = true;
    try {
      const [adj, def, excl] = await Promise.all([
        invoke<ScenarioAdjustmentWithPath[]>("get_scenario_adjustments", { scenarioId: s.id }),
        invoke<ScenarioDefault | null>("get_scenario_defaults", { scenarioId: s.id }),
        invoke<number[]>("get_scenario_excluded_categories", { scenarioId: s.id }),
      ]);
      adjustments = adj;
      defaults = def;
      excludedCategoryIds = excl;
    } catch (e) {
      showToast(String(e), "error");
    } finally {
      loadingAdjustments = false;
    }
  }

  async function handleCreate() {
    if (!newName.trim() || !newStart || !newEnd) return;
    try {
      const s = await invoke<Scenario>("create_scenario", {
        name: newName.trim(),
        description: newDesc.trim() || null,
        horizon: newHorizon,
        baseStartDate: newStart,
        baseEndDate: newEnd,
      });
      showToast("Scenario created.", "success");
      showNewModal = false;
      await loadScenarios();
      await selectScenario(s);
    } catch (e) {
      showToast(String(e), "error");
    }
  }

  async function handleDelete(id: number) {
    if (!confirm("Delete this scenario?")) return;
    try {
      await invoke("delete_scenario", { id });
      showToast("Scenario deleted.", "success");
      if (selectedScenario?.id === id) {
        selectedScenario = null;
        adjustments = [];
        defaults = null;
        excludedCategoryIds = [];
      }
      await loadScenarios();
    } catch (e) {
      showToast(String(e), "error");
    }
  }

  async function saveAdjustment(catId: number, pct: number, fixed: number | null) {
    if (!selectedScenario) return;
    try {
      await invoke("save_scenario_adjustment", {
        scenarioId: selectedScenario.id,
        categoryId: catId,
        adjustmentPct: pct,
        fixedAmount: fixed,
      });
      await selectScenario(selectedScenario);
    } catch (e) {
      showToast(String(e), "error");
    }
  }

  async function toggleCategoryExclusion(catId: number, include: boolean) {
    if (!selectedScenario) return;
    try {
      await invoke("set_scenario_category_exclusion", {
        scenarioId: selectedScenario.id,
        categoryId: catId,
        excluded: !include,
      });
      await selectScenario(selectedScenario);
    } catch (e) {
      showToast(String(e), "error");
    }
  }

  function openDefaults() {
    if (!defaults) {
      editDefaultPct = 0;
      editGrowthPct = 0;
    } else {
      editDefaultPct = defaults.default_adjustment_pct;
      editGrowthPct = defaults.income_growth_pct;
    }
    editingDefaults = true;
  }

  async function saveDefaults() {
    if (!selectedScenario) return;
    try {
      await invoke("save_scenario_defaults", {
        scenarioId: selectedScenario.id,
        defaultAdjustmentPct: editDefaultPct,
        incomeGrowthPct: editGrowthPct,
      });
      showToast("Defaults saved.", "success");
      editingDefaults = false;
      await selectScenario(selectedScenario);
    } catch (e) {
      showToast(String(e), "error");
    }
  }

  function toggleForecastId(id: number) {
    const idx = selectedForecastIds.indexOf(id);
    if (idx >= 0) {
      selectedForecastIds = selectedForecastIds.filter((i) => i !== id);
    } else {
      if (selectedForecastIds.length >= 3) return;
      selectedForecastIds = [...selectedForecastIds, id];
    }
  }

  async function runForecast() {
    if (selectedForecastIds.length === 0) return;
    forecastLoading = true;
    forecastError = "";
    forecastResult = null;
    try {
      forecastResult = await invoke<ForecastComparison>("run_forecast", {
        scenarioIds: selectedForecastIds,
        monthsAhead,
      });
    } catch (e) {
      forecastError = String(e);
    } finally {
      forecastLoading = false;
    }
  }

  function addMonthsToDate(dateStr: string, n: number): string {
    const d = new Date(dateStr);
    d.setMonth(d.getMonth() + n);
    return d.toISOString().slice(0, 7);
  }

  let allDatasets = $derived.by<{ labels: string[]; datasets: any[] } | null>(() => {
    if (!forecastResult) return null;
    void $darkMode; // recompute colors when the theme flips
    const series = chartSeries();
    const ds: any[] = [];
    const labels = forecastResult.base.months.map((m) => m.label);

    ds.push({
      label: "Baseline (Net)",
      data: forecastResult.base.months.map((m) => m.projected_net),
      borderColor: themeVar("--text-muted", "#a89f90"),
      backgroundColor: "transparent",
      borderWidth: 2,
      borderDash: [6, 3],
      pointRadius: 2,
      tension: 0.3,
      fill: false,
    });

    forecastResult.scenarios.forEach((s, i) => {
      ds.push({
        label: `${s.scenario_name} (Net)`,
        data: s.months.map((m) => m.projected_net),
        borderColor: series[i % series.length],
        backgroundColor: "transparent",
        borderWidth: 2.5,
        pointRadius: 3,
        tension: 0.3,
        fill: false,
      });
    });

    return { labels, datasets: ds };
  });

  $effect(() => {
    if (!forecastResult || forecastLoading || !allDatasets) return;
    (async () => {
      const { default: Chart } = await import("chart.js/auto");
      const canvas = document.getElementById("forecastChart") as HTMLCanvasElement | null;
      if (!canvas) return;
      if (lineChart) lineChart.destroy();
      lineChart = new Chart(canvas, {
        type: "line",
        data: allDatasets,
        options: {
          responsive: true,
          maintainAspectRatio: true,
          interaction: { intersect: false, mode: "index" },
          scales: {
            y: {
              beginAtZero: true,
              border: { display: false },
              grid: { color: themeVar("--border-color", "#ece0cc") },
              ticks: { color: themeVar("--text-muted", "#a89f90"), callback: (v: any) => fmt(v) },
            },
            x: { border: { display: false }, grid: { display: false }, ticks: { color: themeVar("--text-muted", "#a89f90") } },
          },
          plugins: {
            legend: { position: "bottom", labels: { color: themeVar("--text-secondary", "#7b7468"), usePointStyle: true, pointStyle: "circle", boxWidth: 8 } },
            tooltip: {
              callbacks: {
                label: (ctx: any) => `${ctx.dataset.label}: ${fmt(ctx.parsed.y ?? ctx.parsed)}`,
              },
            },
          },
        },
      });
    })();
  });

  let summaryTable = $derived.by(() => {
    if (!forecastResult) return [];
    const rows: { scenario: string; income: string; expenses: string; net: string }[] = [];
    rows.push({
      scenario: "Baseline",
      income: fmt(forecastResult.base.totals.total_projected_income),
      expenses: fmt(forecastResult.base.totals.total_projected_expenses),
      net: fmt(forecastResult.base.totals.total_projected_net),
    });
    forecastResult.scenarios.forEach((s) => {
      rows.push({
        scenario: s.scenario_name,
        income: fmt(s.totals.total_projected_income),
        expenses: fmt(s.totals.total_projected_expenses),
        net: fmt(s.totals.total_projected_net),
      });
    });
    return rows;
  });

  let avgBaselines = $derived.by(() => {
    if (!forecastResult || forecastResult.base.months.length === 0) return [];
    const first = forecastResult.base.months[0];
    return first.categories.filter((c) => c.amount > 0);
  });

  // --- Debt payoff planner ---
  interface LiabilityAccount {
    account_id: number;
    name: string;
    balance: number | null;
    apr: number | null;
    min_payment: number | null;
  }
  interface DebtPayoffLine {
    account_id: number;
    name: string;
    starting_balance: number;
    interest_paid: number;
    payoff_month: number | null;
  }
  interface DebtPayoffPlan {
    strategy: string;
    extra_payment: number;
    months_to_debt_free: number | null;
    total_interest: number;
    total_paid: number;
    starting_balance: number;
    debts: DebtPayoffLine[];
    balance_trajectory: number[];
    underwater: boolean;
  }

  let liabilities = $state<LiabilityAccount[]>([]);
  let debtStrategy = $state<"avalanche" | "snowball">("avalanche");
  let debtExtra = $state<string>("0");
  let debtPlan = $state<DebtPayoffPlan | null>(null);
  let debtLoading = $state(false);
  let termEdits = $state<Record<number, { apr: string; min_payment: string }>>({});

  async function loadLiabilities() {
    try {
      liabilities = await invoke<LiabilityAccount[]>("list_liabilities");
      const edits: Record<number, { apr: string; min_payment: string }> = {};
      for (const l of liabilities) {
        edits[l.account_id] = {
          apr: l.apr != null ? String(l.apr) : "",
          min_payment: l.min_payment != null ? String(l.min_payment) : "",
        };
      }
      termEdits = edits;
    } catch (e) {
      liabilities = [];
    }
  }

  async function saveTerms(accountId: number) {
    const edit = termEdits[accountId];
    if (!edit) return;
    try {
      await invoke("update_account_debt_terms", {
        accountId,
        apr: edit.apr ? parseFloat(edit.apr) : null,
        minPayment: edit.min_payment ? parseFloat(edit.min_payment) : null,
      });
      showToast("Saved.", "success");
      await loadLiabilities();
      if (debtPlan) await runDebtPlan();
    } catch (e) {
      showToast(String(e), "error");
    }
  }

  async function runDebtPlan() {
    debtLoading = true;
    try {
      debtPlan = await invoke<DebtPayoffPlan>("simulate_debt_payoff", {
        extraPayment: debtExtra ? parseFloat(debtExtra) : 0,
        strategy: debtStrategy,
      });
    } catch (e) {
      debtPlan = null;
      showToast(String(e), "error");
    } finally {
      debtLoading = false;
    }
  }

  function monthsLabel(m: number | null): string {
    if (m == null) return "—";
    const y = Math.floor(m / 12);
    const mo = m % 12;
    if (y === 0) return `${mo} mo`;
    if (mo === 0) return `${y} yr`;
    return `${y} yr ${mo} mo`;
  }

  // Sparkline path for the remaining-balance trajectory.
  function trajectoryPath(traj: number[], w: number, h: number): string {
    if (traj.length < 2) return "";
    const max = Math.max(...traj, 1);
    const step = w / (traj.length - 1);
    return traj
      .map((v, i) => `${i === 0 ? "M" : "L"} ${(i * step).toFixed(1)} ${(h - (v / max) * h).toFixed(1)}`)
      .join(" ");
  }

  let hasLiabilityData = $derived(
    liabilities.some((l) => (l.balance ?? 0) > 0),
  );

  onMount(() => {
    loadLiabilities();
    return () => {
      if (lineChart) lineChart.destroy();
    };
  });
</script>

<svelte:window onkeydown={(e) => {
  if (e.key !== "Escape") return;
  showNewModal = false;
  editingDefaults = false;
}} />

<div class="page">
  <h1>Forecasting</h1>

  {#if toastVisible}
    <div class="toast" class:toast-error={toastType === "error"} class:toast-success={toastType === "success"}>
      {toastMsg}
    </div>
  {/if}

  <!-- Section 1: Scenario Management -->
  <section class="section">
    <div class="section-header">
      <h2>Scenarios</h2>
      <button class="btn btn-add" onclick={() => { showNewModal = true; }}>New Scenario</button>
    </div>

    {#if loadingScenarios}
      <div class="skeleton-row">
        {#each Array(2) as _}
          <div class="skeleton-card"><div class="skeleton-line skeleton-line-sm"></div><div class="skeleton-line skeleton-line-md"></div></div>
        {/each}
      </div>
    {:else if scenarios.length === 0}
      <div class="empty-state">
        <p>No scenarios yet. Create one to start forecasting.</p>
      </div>
    {:else}
      <div class="scenario-list">
        {#each scenarios as s (s.id)}
          <div
            class="scenario-card"
            class:selected={selectedScenario?.id === s.id}
            onclick={() => selectScenario(s)}
            onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') selectScenario(s); }}
            role="button"
            tabindex="0"
          >
            <div class="scenario-card-body">
              <span class="scenario-name">{s.name}</span>
              {#if s.description}
                <span class="scenario-desc">{s.description}</span>
              {/if}
              <span class="scenario-dates">{s.base_start_date} &rarr; {s.base_end_date}</span>
            </div>
            <button class="btn btn-sm btn-delete" onclick={(e) => { e.stopPropagation(); handleDelete(s.id); }}>Delete</button>
          </div>
        {/each}
      </div>
    {/if}
  </section>

  <!-- Section 2: Scenario Config -->
  {#if selectedScenario}
    <section class="section">
      <div class="section-header">
        <h2>Configuration: {selectedScenario.name}</h2>
        <button class="btn btn-sm" onclick={() => { selectedScenario = null; }}>Close</button>
      </div>

      <div class="config-grid">
        <div class="config-panel">
          <div class="panel-header">
            <h3>Category Adjustments</h3>
          </div>
          {#if loadingAdjustments}
            <p class="loading">Loading adjustments...</p>
          {:else if adjustableCategories.length === 0}
            <p class="empty-note">No categories defined.</p>
          {:else}
            <div class="adj-table-wrap">
              <table class="adj-table">
                <thead>
                  <tr>
                    <th class="incl-col" title="Include this category in this scenario's forecast">Incl.</th>
                    <th>Category</th>
                    <th class="num-col">% Adj</th>
                    <th class="num-col">Fixed Amount</th>
                    <th></th>
                  </tr>
                </thead>
                <tbody>
                  {#each adjustableCategories as cat (cat.id)}
                    {@const adj = adjustments.find((a) => a.category_id === cat.id)}
                    {@const excluded = excludedCategoryIds.includes(cat.id)}
                    <tr class:row-excluded={excluded}>
                      <td class="incl-col">
                        <input
                          type="checkbox"
                          checked={!excluded}
                          onchange={(e) => toggleCategoryExclusion(cat.id, (e.target as HTMLInputElement).checked)}
                          title={excluded ? "Excluded from this scenario" : "Included in this scenario"}
                        />
                      </td>
                      <td class="cat-cell">{cat.path}</td>
                      <td class="num-col">
                        <input
                          type="number"
                          class="adj-input"
                          value={adj?.adjustment_pct ?? 0}
                          disabled={excluded}
                          onchange={(e) => {
                            const val = parseFloat((e.target as HTMLInputElement).value) || 0;
                            saveAdjustment(cat.id, val, adj?.fixed_amount ?? null);
                          }}
                          step="0.1"
                        />
                      </td>
                      <td class="num-col">
                        <input
                          type="number"
                          class="adj-input"
                          value={adj?.fixed_amount ?? ""}
                          placeholder="auto"
                          disabled={excluded}
                          onchange={(e) => {
                            const raw = (e.target as HTMLInputElement).value;
                            const val = raw ? parseFloat(raw) : null;
                            saveAdjustment(cat.id, adj?.adjustment_pct ?? 0, val);
                          }}
                          step="0.01"
                        />
                      </td>
                      <td>
                        {#if excluded}
                          <span class="badge badge-excluded">Excluded</span>
                        {:else if adj?.fixed_amount != null}
                          <span class="badge badge-fixed">Fixed</span>
                        {:else if (adj?.adjustment_pct ?? 0) !== 0}
                          <span class="badge badge-pct">{adj!.adjustment_pct > 0 ? "+" : ""}{adj!.adjustment_pct}%</span>
                        {:else}
                          <span class="badge badge-default">Default</span>
                        {/if}
                      </td>
                    </tr>
                  {/each}
                </tbody>
              </table>
            </div>
          {/if}
        </div>

        <div class="config-panel">
          <div class="panel-header">
            <h3>Scenario Defaults</h3>
            <button class="btn btn-sm" onclick={openDefaults}>Edit</button>
          </div>
          {#if defaults}
            <div class="defaults-display">
              <div class="default-item">
                <span class="default-label">Default Adjustment</span>
                <span class="default-value">{defaults.default_adjustment_pct > 0 ? "+" : ""}{defaults.default_adjustment_pct}%</span>
              </div>
              <div class="default-item">
                <span class="default-label">Income Growth</span>
                <span class="default-value">{defaults.income_growth_pct > 0 ? "+" : ""}{defaults.income_growth_pct}%</span>
              </div>
            </div>
          {:else}
            <p class="empty-note">No defaults set. All categories will use 0% adjustment.</p>
            <button class="btn btn-sm" onclick={openDefaults}>Set Defaults</button>
          {/if}
        </div>
      </div>
    </section>
  {/if}

  <!-- Section 3: Forecast Results -->
  <section class="section">
    <div class="section-header">
      <h2>Forecast Projection</h2>
    </div>

    <div class="forecast-controls">
      <div class="control-group">
        <label class="control-label" for="months-ahead">Months Ahead: {monthsAhead}</label>
        <input id="months-ahead" type="range" min="1" max="24" bind:value={monthsAhead} class="slider" />
      </div>

      <div class="control-group">
        <span class="control-label">Scenarios (max 3):</span>
        <div class="checkbox-group">
          {#each scenarios as s (s.id)}
            <label class="checkbox-label">
              <input
                type="checkbox"
                checked={selectedForecastIds.includes(s.id)}
                onchange={() => toggleForecastId(s.id)}
                disabled={!selectedForecastIds.includes(s.id) && selectedForecastIds.length >= 3}
              />
              {s.name}
            </label>
          {/each}
        </div>
      </div>

      <button
        class="btn btn-primary"
        onclick={runForecast}
        disabled={selectedForecastIds.length === 0 || forecastLoading}
      >
        {forecastLoading ? "Generating..." : "Generate Forecast"}
      </button>
    </div>

    {#if forecastLoading}
      <div class="skeleton-chart">
        <div class="skeleton-block"></div>
      </div>
    {:else if forecastError}
      <div class="error-state">
        <p>Failed to generate forecast.</p>
        <p class="error-detail">{forecastError}</p>
        <button class="btn" onclick={runForecast}>Retry</button>
      </div>
    {:else if forecastResult}
      <div class="chart-card">
        <h3>Net Projection Comparison</h3>
        <div class="chart-wrap"><canvas id="forecastChart"></canvas></div>
      </div>

      <div class="summary-table-wrap">
        <h3>Totals over {forecastResult.months_ahead} months</h3>
        <table class="summary-table">
          <thead>
            <tr>
              <th>Scenario</th>
              <th class="num-col">Total Income</th>
              <th class="num-col">Total Expenses</th>
              <th class="num-col">Net</th>
            </tr>
          </thead>
          <tbody>
            {#each summaryTable as row}
              <tr>
                <td>{row.scenario}</td>
                <td class="num-col">{row.income}</td>
                <td class="num-col">{row.expenses}</td>
                <td class="num-col" class:positive={parseFloat(row.net.replace(/[^0-9.-]/g, "")) >= 0} class:negative={parseFloat(row.net.replace(/[^0-9.-]/g, "")) < 0}>
                  {row.net}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>

      {#if avgBaselines.length > 0}
        <details class="breakdown-details">
          <summary>Month 1 Category Breakdown (Baseline)</summary>
          <table class="summary-table">
            <thead>
              <tr><th>Category</th><th class="num-col">Amount</th></tr>
            </thead>
            <tbody>
              {#each avgBaselines as cat}
                <tr>
                  <td>{cat.category_path}</td>
                  <td class="num-col">{fmt(cat.amount)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </details>
      {/if}
    {:else}
      <div class="empty-state">
        <p>Select one or more scenarios and click Generate to see projections.</p>
      </div>
    {/if}
  </section>

  <section class="debt-section">
    <h2>Debt payoff planner</h2>
    {#if liabilities.length === 0}
      <div class="empty-state"><p>No liability accounts found.</p></div>
    {:else}
      <p class="debt-intro">
        Set an APR and minimum payment for each debt, choose a strategy, and add any extra you can
        put towards debt each month. Balances use each account's latest imported balance.
      </p>

      <div class="debt-terms">
        {#each liabilities as l (l.account_id)}
          <div class="debt-term-row">
            <span class="debt-term-name">{l.name}</span>
            <span class="debt-term-balance">
              {l.balance != null ? fmt(l.balance) : "no balance"}
            </span>
            <label class="debt-term-field">
              APR %
              <input type="number" step="0.01" min="0" bind:value={termEdits[l.account_id].apr} />
            </label>
            <label class="debt-term-field">
              Min/mo
              <input type="number" step="1" min="0" bind:value={termEdits[l.account_id].min_payment} />
            </label>
            <button class="btn btn-sm" onclick={() => saveTerms(l.account_id)}>Save</button>
          </div>
        {/each}
      </div>

      <div class="debt-controls">
        <div class="debt-strategy">
          <button class="btn btn-toggle" class:active={debtStrategy === "avalanche"} onclick={() => { debtStrategy = "avalanche"; }}>
            Avalanche
            <span class="debt-toggle-sub">highest APR first</span>
          </button>
          <button class="btn btn-toggle" class:active={debtStrategy === "snowball"} onclick={() => { debtStrategy = "snowball"; }}>
            Snowball
            <span class="debt-toggle-sub">smallest balance first</span>
          </button>
        </div>
        <label class="debt-extra">
          Extra / month
          <input type="number" step="10" min="0" bind:value={debtExtra} />
        </label>
        <button class="btn btn-primary" onclick={runDebtPlan} disabled={debtLoading || !hasLiabilityData}>
          {debtLoading ? "Calculating…" : "Calculate"}
        </button>
      </div>

      {#if debtPlan}
        {#if debtPlan.underwater}
          <div class="debt-warning">
            At these payments the debt never clears — the interest outweighs the payments.
            Increase the minimum payments or the extra amount.
          </div>
        {:else}
          <div class="debt-results">
            <div class="debt-stat">
              <span class="debt-stat-label">Debt-free in</span>
              <span class="debt-stat-value">{monthsLabel(debtPlan.months_to_debt_free)}</span>
            </div>
            <div class="debt-stat">
              <span class="debt-stat-label">Total interest</span>
              <span class="debt-stat-value">{fmt(debtPlan.total_interest)}</span>
            </div>
            <div class="debt-stat">
              <span class="debt-stat-label">Total paid</span>
              <span class="debt-stat-value">{fmt(debtPlan.total_paid)}</span>
            </div>
          </div>

          {#if debtPlan.balance_trajectory.length > 1}
            <svg class="debt-spark" viewBox="0 0 320 60" preserveAspectRatio="none" role="img" aria-label="Balance over time">
              <path d={trajectoryPath(debtPlan.balance_trajectory, 320, 56)} fill="none" stroke="var(--accent)" stroke-width="2" />
            </svg>
          {/if}

          <table class="debt-table">
            <thead>
              <tr><th>Debt</th><th class="num-col">Balance</th><th class="num-col">Interest</th><th class="num-col">Cleared</th></tr>
            </thead>
            <tbody>
              {#each debtPlan.debts as d (d.account_id)}
                <tr>
                  <td>{d.name}</td>
                  <td class="num-col">{fmt(d.starting_balance)}</td>
                  <td class="num-col">{fmt(d.interest_paid)}</td>
                  <td class="num-col">{monthsLabel(d.payoff_month)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      {/if}
    {/if}
  </section>
</div>

<!-- New Scenario Modal -->
{#if showNewModal}
  <div class="modal-overlay" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) showNewModal = false; }}>
    <div class="modal" role="dialog" aria-modal="true" tabindex="-1">
      <h2>New Scenario</h2>
      <label>
        Name
        <input type="text" bind:value={newName} placeholder="e.g. Optimistic Growth" />
      </label>
      <label>
        Description
        <input type="text" bind:value={newDesc} placeholder="Optional description" />
      </label>
      <label>
        Horizon
        <select bind:value={newHorizon}>
          <option value="monthly">Monthly</option>
          <option value="quarterly">Quarterly</option>
          <option value="yearly">Yearly</option>
        </select>
      </label>
      <div class="date-row">
        <label>
          Base Start
          <input type="date" bind:value={newStart} />
        </label>
        <label>
          Base End
          <input type="date" bind:value={newEnd} />
        </label>
      </div>
      <div class="modal-actions">
        <button class="btn" onclick={() => { showNewModal = false; }}>Cancel</button>
        <button class="btn btn-primary" onclick={handleCreate} disabled={!newName.trim() || !newStart || !newEnd}>Save</button>
      </div>
    </div>
  </div>
{/if}

<!-- Defaults Modal -->
{#if editingDefaults}
  <div class="modal-overlay" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) editingDefaults = false; }}>
    <div class="modal modal-sm" role="dialog" aria-modal="true" tabindex="-1">
      <h2>Scenario Defaults</h2>
      <label>
        Default Adjustment (%)
        <input type="number" bind:value={editDefaultPct} step="0.1" />
        <span class="hint">Applied to all categories without a specific adjustment</span>
      </label>
      <label>
        Income Growth (% p.a.)
        <input type="number" bind:value={editGrowthPct} step="0.1" />
        <span class="hint">Annual growth rate compounded monthly on income categories</span>
      </label>
      <div class="modal-actions">
        <button class="btn" onclick={() => { editingDefaults = false; }}>Cancel</button>
        <button class="btn btn-primary" onclick={saveDefaults}>Save</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .page { max-width: 1320px; margin: 0 auto; }
  h1 { font-size: 1.75rem; font-weight: 700; color: var(--text-primary); margin-bottom: 1rem; }
  h2 { font-size: 1.25rem; font-weight: 600; color: var(--text-primary); }
  h3 { font-size: 1rem; font-weight: 600; color: var(--text-primary); margin-bottom: 0.75rem; }

  .section { margin-bottom: 2rem; background: var(--bg-card); border: 1px solid var(--border-color); border-radius: var(--radius-card); padding: 1.25rem 1.5rem; box-shadow: var(--app-shadow); }
  .section-header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 1rem; flex-wrap: wrap; gap: 0.5rem; }

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
  .btn-primary { background: var(--accent); color: #fff; border-color: var(--accent); }
  .btn-primary:hover { background: var(--accent); }
  .btn-add { background: var(--accent); color: #fff; border-color: var(--accent); }
  .btn-add:hover { background: var(--accent); filter: brightness(0.95); }
  .btn-delete { background: var(--neg); color: #fff; border-color: var(--neg); }
  .btn-delete:hover { background: var(--neg); }

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

  .empty-state {
    border: 2px dashed var(--border-color);
    border-radius: 14px;
    padding: 2rem;
    text-align: center;
    color: var(--text-secondary);
    font-size: 0.95rem;
  }
  .empty-note { color: var(--text-muted); font-size: 0.85rem; padding: 0.5rem 0; }
  .loading { color: var(--text-secondary); padding: 1rem 0; font-size: 0.85rem; }
  .error-state { text-align: center; padding: 2rem; color: var(--neg); background: var(--bg-secondary); border: 1px solid var(--neg); border-radius: 14px; margin: 1rem 0; }
  .error-detail { font-size: 0.8rem; color: var(--text-secondary); margin: 0.5rem 0 1rem; word-break: break-all; }

  .skeleton-row { display: flex; flex-direction: column; gap: 0.75rem; }
  .skeleton-card { background: var(--bg-secondary); border-radius: 14px; padding: 1rem; animation: pulse 1.5s infinite; }
  .skeleton-line { background: var(--track); border-radius: 4px; }
  .skeleton-line-sm { width: 50%; height: 0.75rem; margin-bottom: 0.4rem; }
  .skeleton-line-md { width: 70%; height: 0.75rem; }
  .skeleton-chart { margin: 1rem 0; }
  .skeleton-block { width: 100%; height: 300px; background: var(--bg-secondary); border-radius: 14px; animation: pulse 1.5s infinite; }
  @keyframes pulse { 0%, 100% { opacity: 1; } 50% { opacity: 0.4; } }

  .scenario-list { display: flex; flex-direction: column; gap: 0.5rem; }
  .scenario-card {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 0.75rem 1rem;
    border: 1px solid var(--border-color);
    border-radius: 14px;
    background: var(--bg-card);
    cursor: pointer;
    transition: border-color 0.15s, box-shadow 0.15s;
    width: 100%;
    text-align: left;
    font: inherit;
  }
  .scenario-card:hover { border-color: var(--accent); }
  .scenario-card.selected { border-color: var(--accent); box-shadow: 0 0 0 2px rgba(127,154,111,0.22); }
  .scenario-card-body { display: flex; flex-direction: column; gap: 0.2rem; min-width: 0; }
  .scenario-name { font-weight: 600; color: var(--text-primary); font-size: 0.95rem; }
  .scenario-desc { font-size: 0.8rem; color: var(--text-secondary); }
  .scenario-dates { font-size: 0.75rem; color: var(--text-muted); }

  .config-grid { display: grid; grid-template-columns: 1fr 320px; gap: 1rem; }
  @media (max-width: 800px) { .config-grid { grid-template-columns: 1fr; } }
  .panel-header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 0.5rem; }

  .adj-table-wrap { max-height: 400px; overflow-y: auto; border: 1px solid var(--border-color); border-radius: 10px; }
  .adj-table { width: 100%; border-collapse: collapse; font-size: 0.8rem; }
  .adj-table th { text-align: left; padding: 0.5rem 0.65rem; background: var(--bg-secondary); border-bottom: 1px solid var(--border-color); font-weight: 600; color: var(--text-primary); position: sticky; top: 0; }
  .adj-table td { padding: 0.4rem 0.65rem; border-bottom: 1px solid var(--border-color); color: var(--text-primary); }
  .cat-cell { max-width: 200px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .incl-col { width: 1%; text-align: center; white-space: nowrap; }
  .row-excluded .cat-cell { color: var(--text-muted); text-decoration: line-through; }
  .num-col { text-align: right; }
  .adj-input { width: 80px; padding: 0.3rem 0.4rem; border: 1px solid var(--border-color); border-radius: 4px; font-size: 0.8rem; text-align: right; }

  .badge { font-size: 0.7rem; padding: 0.15rem 0.4rem; border-radius: 4px; font-weight: 500; }
  .badge-default { background: var(--bg-secondary); color: var(--text-secondary); }
  .badge-pct { background: var(--accent-soft); color: var(--accent); }
  .badge-fixed { background: var(--amber); color: #fff; }
  .badge-excluded { background: var(--neg); color: #fff; }

  .defaults-display { display: flex; flex-direction: column; gap: 0.5rem; }
  .default-item { display: flex; justify-content: space-between; padding: 0.4rem 0; border-bottom: 1px solid var(--border-color); font-size: 0.85rem; }
  .default-label { color: var(--text-secondary); }
  .default-value { font-weight: 600; color: var(--text-primary); }

  .forecast-controls {
    display: flex;
    align-items: center;
    gap: 1.5rem;
    flex-wrap: wrap;
    margin-bottom: 1rem;
    padding: 0.75rem 1rem;
    background: var(--bg-secondary);
    border-radius: 14px;
  }
  .control-group { display: flex; flex-direction: column; gap: 0.3rem; }
  .control-label { font-size: 0.85rem; font-weight: 500; color: var(--text-primary); }
  .slider { width: 160px; }
  .checkbox-group { display: flex; gap: 0.75rem; flex-wrap: wrap; }
  .checkbox-label { font-size: 0.85rem; color: var(--text-primary); display: flex; align-items: center; gap: 0.3rem; cursor: pointer; }

  .chart-card { background: var(--bg-card); border: 1px solid var(--border-color); border-radius: var(--radius-card); padding: 1.25rem; margin-bottom: 1rem; box-shadow: var(--app-shadow); }
  .chart-wrap { position: relative; width: 100%; max-height: 400px; display: flex; justify-content: center; }
  .chart-wrap canvas { max-width: 100%; max-height: 400px; }

  .summary-table-wrap { margin-bottom: 1rem; }
  .summary-table { width: 100%; border-collapse: collapse; font-size: 0.85rem; }
  .summary-table th { text-align: left; padding: 0.5rem 0.75rem; background: var(--bg-secondary); border-bottom: 1px solid var(--border-color); font-weight: 600; color: var(--text-primary); }
  .summary-table td { padding: 0.4rem 0.75rem; border-bottom: 1px solid var(--border-color); color: var(--text-primary); }
  .positive { color: var(--pos); font-weight: 600; }
  .negative { color: var(--neg); font-weight: 600; }

  .breakdown-details { margin-top: 0.5rem; }
  .breakdown-details summary { cursor: pointer; font-size: 0.85rem; color: var(--accent); padding: 0.3rem 0; }

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
  .modal-sm { width: 360px; }
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
  .hint { font-size: 0.75rem; color: var(--text-muted); font-weight: 400; }
  .date-row { display: flex; gap: 0.75rem; }
  .date-row label { flex: 1; }
  .modal-actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
    margin-top: 1.25rem;
  }

  /* Debt payoff planner */
  .debt-section { margin-top: 2.5rem; padding-top: 1.5rem; border-top: 1px solid var(--border-color); }
  .debt-section h2 { font-size: 1.25rem; font-weight: 700; margin-bottom: 0.75rem; color: var(--text-primary); }
  .debt-intro { font-size: 0.85rem; color: var(--text-secondary); margin-bottom: 1rem; max-width: 60ch; }
  .debt-terms { display: flex; flex-direction: column; gap: 0.5rem; margin-bottom: 1.25rem; }
  .debt-term-row {
    display: grid;
    grid-template-columns: 1.5fr 1fr auto auto auto;
    align-items: end;
    gap: 0.75rem;
    padding: 0.6rem 0.8rem;
    background: var(--bg-card);
    border: 1px solid var(--border-color);
    border-radius: var(--radius-card, 12px);
  }
  .debt-term-name { font-size: 0.9rem; font-weight: 600; color: var(--text-primary); align-self: center; }
  .debt-term-balance { font-size: 0.82rem; color: var(--text-secondary); font-variant-numeric: tabular-nums; align-self: center; }
  .debt-term-field { display: flex; flex-direction: column; gap: 0.2rem; font-size: 0.72rem; color: var(--text-secondary); }
  .debt-term-field input { width: 6rem; padding: 0.35rem 0.5rem; border: 1px solid var(--border-color); border-radius: 8px; background: var(--bg-card); color: var(--text-primary); }

  .debt-controls { display: flex; flex-wrap: wrap; align-items: flex-end; gap: 1rem; margin-bottom: 1.25rem; }
  .debt-strategy { display: flex; gap: 0.5rem; }
  .btn-toggle { display: flex; flex-direction: column; align-items: flex-start; line-height: 1.2; }
  .btn-toggle.active { background: var(--accent); color: #fff; border-color: var(--accent); }
  .debt-toggle-sub { font-size: 0.68rem; opacity: 0.8; font-weight: 400; }
  .debt-extra { display: flex; flex-direction: column; gap: 0.2rem; font-size: 0.72rem; color: var(--text-secondary); }
  .debt-extra input { width: 8rem; padding: 0.4rem 0.5rem; border: 1px solid var(--border-color); border-radius: 8px; background: var(--bg-card); color: var(--text-primary); }

  .debt-results { display: flex; gap: 1.5rem; flex-wrap: wrap; margin-bottom: 1rem; }
  .debt-stat { display: flex; flex-direction: column; }
  .debt-stat-label { font-size: 0.75rem; color: var(--text-secondary); }
  .debt-stat-value { font-family: "Bitter", Georgia, serif; font-size: 1.3rem; font-weight: 700; color: var(--text-primary); font-variant-numeric: tabular-nums; }
  .debt-spark { width: 100%; height: 60px; margin-bottom: 1rem; }
  .debt-warning { padding: 0.8rem 1rem; border-radius: 10px; background: var(--neg-soft, rgba(200,60,60,0.1)); color: var(--neg); font-size: 0.85rem; }
  .debt-table { width: 100%; border-collapse: collapse; font-size: 0.85rem; }
  .debt-table th, .debt-table td { padding: 0.5rem 0.6rem; border-bottom: 1px solid var(--border-color); text-align: left; color: var(--text-primary); }
  .debt-table th { font-size: 0.75rem; color: var(--text-secondary); font-weight: 600; }
</style>