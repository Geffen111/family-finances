<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { format, startOfMonth, endOfMonth, subMonths, startOfYear } from "date-fns";
  import { onMount } from "svelte";
  import { darkMode } from "$lib/stores/theme.svelte";

  interface DashboardSummary {
    total_income: number;
    total_expenses: number;
    net: number;
    top_category: string;
    top_category_amount: number;
    transaction_count: number;
  }

  interface CategorySpending {
    category_name: string;
    category_path: string;
    total: number;
    percentage: number;
    transaction_count: number;
  }

  interface MonthlyTrend {
    month: string;
    label: string;
    income: number;
    expenses: number;
    net: number;
  }

  interface CategoryTrend {
    category_id: number | null;
    category_name: string;
    month: string;
    label: string;
    amount: number;
  }

  interface NetWorthPoint {
    month: string;
    label: string;
    net_worth: number;
  }

  interface CategorySpendingChild {
    category_id: number;
    name: string;
    total: number;
    transaction_count: number;
  }

  interface CategorySpendingGroup {
    category_id: number | null;
    name: string;
    direct_total: number;
    total: number;
    transaction_count: number;
    children: CategorySpendingChild[];
  }

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

  let summary = $state<DashboardSummary | null>(null);
  let categorySpending = $state<CategorySpending[]>([]);
  let monthlyTrends = $state<MonthlyTrend[]>([]);
  let categoryTrends = $state<CategoryTrend[]>([]);
  let categoryTree = $state<CategorySpendingGroup[]>([]);
  let expandedCategories = $state<Set<number>>(new Set());
  let recurring = $state<RecurringCost[]>([]);
  let netWorth = $state<NetWorthPoint[]>([]);
  let loading = $state(true);
  let error = $state("");

  type DatePreset = "thisMonth" | "lastMonth" | "last3Months" | "last6Months" | "last24Months" | "ytd" | "all";
  let activePreset = $state<DatePreset>("last6Months");
  let customStart = $state("");
  let customEnd = $state("");
  let showCustom = $state(false);

  let selectedTrendCategory = $state<string>("top3");

  const currencyFormat = new Intl.NumberFormat("en-AU", { style: "currency", currency: "AUD" });
  function fmt(val: number): string { return currencyFormat.format(val); }

  type ChartType = import("chart.js").Chart;
  let doughnutChart: ChartType | null = null;
  let barChart: ChartType | null = null;
  let lineChart: ChartType | null = null;

  // Read Hearth theme tokens off a live element so charts follow light/dark mode.
  function themeVar(name: string, fallback = ""): string {
    if (typeof document === "undefined") return fallback;
    const el = document.querySelector(".app-layout") ?? document.documentElement;
    const v = getComputedStyle(el).getPropertyValue(name).trim();
    return v || fallback;
  }
  function chartSeries(): string[] {
    return ["--c1", "--c2", "--c3", "--c4", "--c5", "--c6"].map((n) => themeVar(n, "#7f9a6f"));
  }
  // hex (#rgb/#rrggbb) → rgba string with given alpha
  function withAlpha(hex: string, alpha: number): string {
    const h = hex.replace("#", "");
    const f = h.length === 3 ? h.split("").map((c) => c + c).join("") : h;
    const r = parseInt(f.slice(0, 2), 16);
    const g = parseInt(f.slice(2, 4), 16);
    const b = parseInt(f.slice(4, 6), 16);
    return `rgba(${r}, ${g}, ${b}, ${alpha})`;
  }

  function getDateRange(preset: DatePreset): { start: string; end: string } {
    const today = new Date();
    switch (preset) {
      case "thisMonth":
        return { start: format(startOfMonth(today), "yyyy-MM-dd"), end: "" };
      case "lastMonth": {
        const last = subMonths(today, 1);
        return { start: format(startOfMonth(last), "yyyy-MM-dd"), end: format(endOfMonth(last), "yyyy-MM-dd") };
      }
      case "last3Months":
        return { start: format(startOfMonth(subMonths(today, 2)), "yyyy-MM-dd"), end: "" };
      case "last6Months":
        return { start: format(startOfMonth(subMonths(today, 5)), "yyyy-MM-dd"), end: "" };
      case "last24Months":
        return { start: format(startOfMonth(subMonths(today, 23)), "yyyy-MM-dd"), end: "" };
      case "ytd":
        return { start: format(startOfYear(today), "yyyy-MM-dd"), end: "" };
      case "all":
        return { start: "", end: "" };
    }
  }

  function getParams(): Record<string, unknown> {
    const p: Record<string, unknown> = {};
    if (showCustom) {
      if (customStart) p.startDate = customStart;
      if (customEnd) p.endDate = customEnd;
    } else {
      const r = getDateRange(activePreset);
      if (r.start) p.startDate = r.start;
      if (r.end) p.endDate = r.end;
    }
    return p;
  }

  async function fetchData() {
    loading = true;
    error = "";
    const params = getParams();
    try {
      const [s, c, m, tree] = await Promise.all([
        invoke<DashboardSummary>("get_dashboard_summary", params),
        invoke<CategorySpending[]>("get_spending_by_category", params),
        invoke<MonthlyTrend[]>("get_monthly_trends", params),
        invoke<CategorySpendingGroup[]>("get_category_spending_tree", params),
      ]);
      summary = s;
      categorySpending = c;
      monthlyTrends = m;
      categoryTree = tree;
      const trendParams = { ...params };
      categoryTrends = await invoke<CategoryTrend[]>("get_spending_trend_by_category", trendParams);
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  function setPreset(preset: DatePreset) {
    activePreset = preset;
    showCustom = false;
    fetchData();
  }

  function applyCustom() {
    showCustom = true;
    fetchData();
  }

  function getFilteredTrends(): CategoryTrend[] {
    if (selectedTrendCategory === "top3") {
      const topNames = new Set(categorySpending.slice(0, 3).map((cs) => cs.category_name));
      return categoryTrends.filter((ct) => topNames.has(ct.category_name));
    }
    return categoryTrends.filter((ct) => ct.category_name === selectedTrendCategory);
  }

  function toggleCategory(id: number | null) {
    if (id == null) return;
    const next = new Set(expandedCategories);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    expandedCategories = next;
  }

  let categoryTreeTotal = $derived(categoryTree.reduce((s, g) => s + g.total, 0));
  function treePct(v: number): number {
    return categoryTreeTotal > 0 ? (v / categoryTreeTotal) * 100 : 0;
  }

  let doughnutData = $derived.by(() => {
    if (categorySpending.length === 0) return null;
    const top8 = categorySpending.slice(0, 8);
    const other = categorySpending.slice(8);
    const labels = top8.map((c) => c.category_name);
    const data = top8.map((c) => c.total);
    if (other.length > 0) {
      labels.push("Other");
      data.push(other.reduce((s, c) => s + c.total, 0));
    }
    return { labels, data };
  });

  $effect(() => {
    if (loading || !doughnutData) return;
    const dark = $darkMode; // track theme so the chart recolors on toggle
    (async () => {
      void dark;
      const { default: Chart } = await import("chart.js/auto");
      const canvas = document.getElementById("doughnutChart") as HTMLCanvasElement | null;
      if (!canvas) return;
      if (doughnutChart) doughnutChart.destroy();
      const series = chartSeries();
      const total = doughnutData.data.reduce((a, b) => a + b, 0);
      doughnutChart = new Chart(canvas, {
        type: "doughnut",
        data: {
          labels: doughnutData.labels,
          datasets: [{
            data: doughnutData.data,
            backgroundColor: doughnutData.labels.map((_, i) => series[i % series.length]),
            borderColor: themeVar("--bg-card", "#fff"),
            borderWidth: 2,
          }],
        },
        options: {
          responsive: true,
          maintainAspectRatio: true,
          cutout: "62%",
          plugins: {
            legend: { position: "bottom", labels: { color: themeVar("--text-secondary", "#7b7468"), usePointStyle: true, pointStyle: "circle", boxWidth: 8 } },
            tooltip: {
              callbacks: {
                label: (ctx: any) => {
                  const val = fmt(ctx.parsed);
                  const pct = ((ctx.parsed / total) * 100).toFixed(1);
                  return `${ctx.label}: ${val} (${pct}%)`;
                },
              },
            },
          },
        },
      });
    })();
  });

  $effect(() => {
    if (loading || monthlyTrends.length === 0) return;
    const dark = $darkMode;
    (async () => {
      void dark;
      const { default: Chart } = await import("chart.js/auto");
      const canvas = document.getElementById("barChart") as HTMLCanvasElement | null;
      if (!canvas) return;
      if (barChart) barChart.destroy();
      const pos = themeVar("--pos", "#6f9466");
      const neg = themeVar("--neg", "#c77a5a");
      const accent = themeVar("--accent", "#7f9a6f");
      const grid = themeVar("--border-color", "#ece0cc");
      const tick = themeVar("--text-muted", "#a89f90");
      const labels = monthlyTrends.map((m) => m.label);
      barChart = new Chart(canvas, {
        type: "bar",
        data: {
          labels,
          datasets: [
            {
              label: "Income",
              data: monthlyTrends.map((m) => m.income),
              backgroundColor: pos,
              borderWidth: 0,
              borderRadius: 5,
              borderSkipped: false,
              barPercentage: 0.6,
              categoryPercentage: 0.6,
            },
            {
              label: "Expenses",
              data: monthlyTrends.map((m) => m.expenses),
              backgroundColor: neg,
              borderWidth: 0,
              borderRadius: 5,
              borderSkipped: false,
              barPercentage: 0.6,
              categoryPercentage: 0.6,
            },
            {
              label: "Net",
              data: monthlyTrends.map((m) => m.net),
              type: "line",
              borderColor: accent,
              backgroundColor: "transparent",
              borderWidth: 2,
              pointBackgroundColor: accent,
              pointRadius: 3,
              tension: 0.35,
            },
          ],
        },
        options: {
          responsive: true,
          maintainAspectRatio: true,
          interaction: { intersect: false, mode: "index" },
          scales: {
            y: {
              beginAtZero: true,
              border: { display: false },
              grid: { color: grid },
              ticks: { color: tick, callback: (v: any) => fmt(v) },
            },
            x: { border: { display: false }, grid: { display: false }, ticks: { color: tick } },
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

  $effect(() => {
    if (loading || categoryTrends.length === 0) return;
    // Read reactive state synchronously so the effect re-runs when the category
    // dropdown (or underlying data) changes — anything read only after the
    // `await` below would not be tracked as a dependency.
    const filtered = getFilteredTrends();
    const dark = $darkMode;
    (async () => {
      void dark;
      const { default: Chart } = await import("chart.js/auto");
      const canvas = document.getElementById("lineChart") as HTMLCanvasElement | null;
      if (!canvas) return;
      if (lineChart) lineChart.destroy();
      const series = chartSeries();
      const grid = themeVar("--border-color", "#ece0cc");
      const tick = themeVar("--text-muted", "#a89f90");

      const monthSet = new Set(filtered.map((ct) => ct.label));
      const labels = [...monthSet].sort();
      const grouped = new Map<string, { data: Map<string, number> }>();
      for (const ct of filtered) {
        if (!grouped.has(ct.category_name)) {
          grouped.set(ct.category_name, { data: new Map() });
        }
        grouped.get(ct.category_name)!.data.set(ct.label, ct.amount);
      }

      const datasets: any[] = [];
      let ci = 0;
      for (const [name, g] of grouped) {
        datasets.push({
          label: name,
          data: labels.map((l) => g.data.get(l) ?? 0),
          borderColor: series[ci % series.length],
          backgroundColor: withAlpha(series[ci % series.length], 0.13),
          borderWidth: 2,
          pointRadius: 3,
          tension: 0.35,
          fill: false,
        });
        ci++;
      }

      lineChart = new Chart(canvas, {
        type: "line",
        data: { labels, datasets },
        options: {
          responsive: true,
          maintainAspectRatio: true,
          interaction: { intersect: false, mode: "index" },
          scales: {
            y: {
              beginAtZero: true,
              border: { display: false },
              grid: { color: grid },
              ticks: { color: tick, callback: (v: any) => fmt(v) },
            },
            x: { border: { display: false }, grid: { display: false }, ticks: { color: tick } },
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

  let recurringMonthlyTotal = $derived(recurring.filter((r) => r.active).reduce((s, r) => s + r.monthly_cost, 0));

  async function loadRecurring() {
    try {
      recurring = await invoke<RecurringCost[]>("list_recurring_costs");
    } catch (e) {
      recurring = [];
    }
  }

  async function loadNetWorth() {
    try {
      netWorth = await invoke<NetWorthPoint[]>("get_net_worth_trend");
    } catch (e) {
      netWorth = [];
    }
  }

  let netWorthChart: ChartType | null = null;

  $effect(() => {
    if (loading || netWorth.length === 0) return;
    const dark = $darkMode;
    (async () => {
      void dark;
      const { default: Chart } = await import("chart.js/auto");
      const canvas = document.getElementById("netWorthChart") as HTMLCanvasElement | null;
      if (!canvas) return;
      if (netWorthChart) netWorthChart.destroy();
      const accent = themeVar("--accent", "#7f9a6f");
      const grid = themeVar("--border-color", "#ece0cc");
      const tick = themeVar("--text-muted", "#a89f90");
      const ctx2d = canvas.getContext("2d");
      let fill: string | CanvasGradient = withAlpha(accent, 0.16);
      if (ctx2d) {
        const grad = ctx2d.createLinearGradient(0, 0, 0, canvas.height || 300);
        grad.addColorStop(0, withAlpha(accent, 0.18));
        grad.addColorStop(1, withAlpha(accent, 0));
        fill = grad;
      }
      netWorthChart = new Chart(canvas, {
        type: "line",
        data: {
          labels: netWorth.map((p) => p.label),
          datasets: [{
            label: "Net Worth",
            data: netWorth.map((p) => p.net_worth),
            borderColor: accent,
            backgroundColor: fill,
            borderWidth: 2.5,
            pointRadius: 0,
            tension: 0.35,
            fill: true,
          }],
        },
        options: {
          responsive: true,
          maintainAspectRatio: true,
          interaction: { intersect: false, mode: "index" },
          scales: {
            y: { border: { display: false }, grid: { color: grid }, ticks: { color: tick, callback: (v: any) => fmt(v) } },
            x: { border: { display: false }, grid: { display: false }, ticks: { color: tick } },
          },
          plugins: {
            legend: { display: false },
            tooltip: { callbacks: { label: (ctx: any) => `Net worth: ${fmt(ctx.parsed.y ?? ctx.parsed)}` } },
          },
        },
      });
    })();
  });

  onMount(() => {
    fetchData();
    loadRecurring();
    loadNetWorth();
    return () => {
      if (netWorthChart) netWorthChart.destroy();
      if (doughnutChart) doughnutChart.destroy();
      if (barChart) barChart.destroy();
      if (lineChart) lineChart.destroy();
    };
  });
</script>

<div class="page">
  <div class="page-header">
    <div>
      <h1>Welcome back</h1>
      <p class="page-subtitle">Here's how the household is tracking</p>
    </div>
    <div class="filter-bar">
      <div class="period-switch">
        <button class="preset-btn" class:active={!showCustom && activePreset === "thisMonth"} onclick={() => setPreset("thisMonth")}>This Month</button>
        <button class="preset-btn" class:active={!showCustom && activePreset === "lastMonth"} onclick={() => setPreset("lastMonth")}>Last</button>
        <button class="preset-btn" class:active={!showCustom && activePreset === "last3Months"} onclick={() => setPreset("last3Months")}>3M</button>
        <button class="preset-btn" class:active={!showCustom && activePreset === "last6Months"} onclick={() => setPreset("last6Months")}>6M</button>
        <button class="preset-btn" class:active={!showCustom && activePreset === "last24Months"} onclick={() => setPreset("last24Months")}>24M</button>
        <button class="preset-btn" class:active={!showCustom && activePreset === "ytd"} onclick={() => setPreset("ytd")}>YTD</button>
        <button class="preset-btn" class:active={!showCustom && activePreset === "all"} onclick={() => setPreset("all")}>All</button>
        <button class="preset-btn" class:active={showCustom} onclick={() => { showCustom = !showCustom; if (!showCustom) fetchData(); }}>Custom</button>
      </div>
      {#if showCustom}
        <div class="custom-dates">
          <label>From <input type="date" bind:value={customStart} /></label>
          <label>To <input type="date" bind:value={customEnd} /></label>
          <button class="btn btn-sm" onclick={applyCustom}>Apply</button>
        </div>
      {/if}
    </div>
  </div>

  {#if loading}
    <div class="loading-grid">
      <div class="summary-cards">
        {#each Array(4) as _}
          <div class="skeleton-card"><div class="skeleton-line skeleton-line-sm"></div><div class="skeleton-line skeleton-line-lg"></div></div>
        {/each}
      </div>
      <div class="charts-grid">
        {#each Array(3) as _}
          <div class="chart-card"><div class="skeleton-line skeleton-line-md" style="margin-bottom:1rem;"></div><div class="skeleton-block"></div></div>
        {/each}
      </div>
    </div>
  {:else if error}
    <div class="error-state">
      <p>Error loading dashboard data.</p>
      <p class="error-detail">{error}</p>
      <button class="btn" onclick={fetchData}>Retry</button>
    </div>
  {:else if summary && summary.transaction_count === 0}
    <div class="empty-state">
      <p>No transactions yet. Import a CSV to get started.</p>
      <a href="/transactions" class="btn btn-primary">Go to Transactions</a>
    </div>
  {:else if summary}
    <div class="summary-cards">
      <div class="card">
        <div class="card-top">
          <span class="card-label">Total Income</span>
          <span class="card-icon card-icon-income">&#x1F4C8;</span>
        </div>
        <div class="card-value card-income">{fmt(summary.total_income)}</div>
      </div>
      <div class="card">
        <div class="card-top">
          <span class="card-label">Total Expenses</span>
          <span class="card-icon card-icon-expenses">&#x1F4C9;</span>
        </div>
        <div class="card-value card-expenses">{fmt(summary.total_expenses)}</div>
      </div>
      <div class="card">
        <div class="card-top">
          <span class="card-label">Net</span>
          <span class="card-icon" class:card-icon-income={summary.net >= 0} class:card-icon-expenses={summary.net < 0}>
            {summary.net >= 0 ? "\u{1F4C8}" : "\u{1F4C9}"}
          </span>
        </div>
        <div class="card-value" class:card-income={summary.net >= 0} class:card-expenses={summary.net < 0}>
          {summary.net >= 0 ? "+" : "-"}{fmt(Math.abs(summary.net))}
        </div>
      </div>
      <div class="card">
        <div class="card-top">
          <span class="card-label">Top Category</span>
          <span class="card-icon card-icon-top">&#x1F3C6;</span>
        </div>
        <div class="card-value card-value-sm">{summary.top_category}</div>
        <div class="card-sub-value">{fmt(summary.top_category_amount)}</div>
      </div>
    </div>

    <div class="charts-grid">
      {#if netWorth.length > 1}
        <div class="chart-card chart-card-wide">
          <h3>Net Worth Over Time</h3>
          <div class="chart-wrap"><canvas id="netWorthChart"></canvas></div>
        </div>
      {/if}
      <div class="chart-card">
        <h3>Spending Breakdown</h3>
        <div class="chart-wrap"><canvas id="doughnutChart"></canvas></div>
      </div>
      <div class="chart-card">
        <h3>Monthly Income vs Expenses</h3>
        <div class="chart-wrap"><canvas id="barChart"></canvas></div>
      </div>
      <div class="chart-card chart-card-wide">
        <div class="chart-header">
          <h3>Category Spending Trend</h3>
          <select class="trend-select" bind:value={selectedTrendCategory}>
            <option value="top3">Top 3 Categories</option>
            {#each categorySpending as cs}
              <option value={cs.category_name}>{cs.category_name}</option>
            {/each}
          </select>
        </div>
        <div class="chart-wrap"><canvas id="lineChart"></canvas></div>
      </div>
    </div>

    {#if categoryTree.length > 0}
      <div class="cat-table-card">
        <div class="chart-header">
          <h3>Category Spending</h3>
          <span class="cat-table-total">{fmt(categoryTreeTotal)}</span>
        </div>
        <table class="cat-table">
          <thead>
            <tr>
              <th>Category</th>
              <th class="num-col">Transactions</th>
              <th class="num-col">% of total</th>
              <th class="num-col">Amount</th>
            </tr>
          </thead>
          <tbody>
            {#each categoryTree as g (g.category_id ?? "uncat")}
              {@const hasChildren = g.children.length > 0}
              {@const expanded = g.category_id != null && expandedCategories.has(g.category_id)}
              <tr
                class="parent-row"
                class:expandable={hasChildren}
                onclick={() => hasChildren && toggleCategory(g.category_id)}
              >
                <td class="cat-name-cell">
                  <span class="twisty" class:invisible={!hasChildren}>{expanded ? "▾" : "▸"}</span>
                  <span class="cat-name">{g.name}</span>
                </td>
                <td class="num-col">{g.transaction_count}</td>
                <td class="num-col">{treePct(g.total).toFixed(1)}%</td>
                <td class="num-col amount">{fmt(g.total)}</td>
              </tr>
              {#if expanded}
                {#if g.direct_total > 0 && hasChildren}
                  <tr class="child-row">
                    <td class="cat-name-cell child"><span class="cat-name muted">{g.name} (direct)</span></td>
                    <td class="num-col"></td>
                    <td class="num-col muted">{treePct(g.direct_total).toFixed(1)}%</td>
                    <td class="num-col amount muted">{fmt(g.direct_total)}</td>
                  </tr>
                {/if}
                {#each g.children as child (child.category_id)}
                  <tr class="child-row">
                    <td class="cat-name-cell child"><span class="cat-name">{child.name}</span></td>
                    <td class="num-col">{child.transaction_count}</td>
                    <td class="num-col muted">{treePct(child.total).toFixed(1)}%</td>
                    <td class="num-col amount">{fmt(child.total)}</td>
                  </tr>
                {/each}
              {/if}
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  {/if}

  {#if recurring.length > 0}
    <a href="/recurring" class="recurring-summary-card">
      <div class="recurring-summary-left">
        <span class="recurring-summary-icon">&#x1F504;</span>
        <div>
          <span class="recurring-summary-label">Expected next month</span>
          <span class="recurring-summary-count">{recurring.length} recurring items</span>
        </div>
      </div>
      <div class="recurring-summary-right">
        <span class="recurring-summary-amount">{fmt(recurringMonthlyTotal)}</span>
        <span class="recurring-summary-per">/month</span>
      </div>
    </a>
  {/if}

</div>

<style>
  .page { max-width: 1320px; margin: 0 auto; }
  h1 { font-size: 28px; color: var(--text-primary); }
  .page-header { display: flex; justify-content: space-between; align-items: flex-start; gap: 1rem; margin-bottom: 1.75rem; flex-wrap: wrap; }
  .page-subtitle { font-size: 0.9rem; color: var(--text-secondary); margin-top: 0.25rem; }

  .filter-bar {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    flex-wrap: wrap;
  }
  .period-switch {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    padding: 4px;
    border: 1px solid var(--border-color);
    border-radius: var(--radius-pill);
    background: var(--bg-card);
  }
  .preset-btn {
    padding: 6px 13px;
    border: none;
    border-radius: var(--radius-pill);
    background: transparent;
    color: var(--text-secondary);
    font-size: 0.8rem;
    font-weight: 500;
    cursor: pointer;
    transition: background 0.15s, color 0.15s;
  }
  .preset-btn:hover { color: var(--text-primary); }
  .preset-btn.active { background: var(--accent); color: #fff; }
  .custom-dates { display: flex; align-items: center; gap: 0.5rem; }
  .custom-dates label { font-size: 0.8rem; color: var(--text-secondary); display: flex; align-items: center; gap: 0.3rem; }
  .custom-dates input[type="date"] { padding: 0.3rem 0.5rem; border: 1px solid var(--border-color); border-radius: 4px; font-size: 0.8rem; }

  .btn {
    padding: 0.5rem 1.1rem;
    border: 1px solid var(--border-color);
    border-radius: var(--radius-pill);
    background: var(--bg-card);
    color: var(--text-primary);
    font-size: 0.875rem;
    font-weight: 600;
    cursor: pointer;
  }
  .btn:hover { background: var(--bg-secondary); }
  .btn-sm { padding: 0.35rem 0.85rem; font-size: 0.8rem; }
  .btn-primary { background: var(--accent); color: #fff; border-color: var(--accent); }
  .btn-primary:hover { filter: brightness(0.95); background: var(--accent); }

  .loading-grid { display: flex; flex-direction: column; gap: 1.5rem; }
  .skeleton-card { background: var(--bg-card); border: 1px solid var(--track); border-radius: 14px; padding: 1.25rem; }
  .skeleton-line { background: var(--track); border-radius: 4px; animation: pulse 1.5s infinite; }
  .skeleton-line-sm { width: 60%; height: 0.75rem; margin-bottom: 0.5rem; }
  .skeleton-line-md { width: 40%; height: 1rem; }
  .skeleton-line-lg { width: 80%; height: 1.5rem; }
  .skeleton-block { width: 100%; height: 200px; background: var(--bg-secondary); border-radius: 4px; animation: pulse 1.5s infinite; }
  @keyframes pulse { 0%, 100% { opacity: 1; } 50% { opacity: 0.4; } }

  .error-state { text-align: center; padding: 3rem 2rem; color: #991b1b; background: #fee2e2; border: 1px solid #fecaca; border-radius: 14px; }
  .error-detail { font-size: 0.8rem; color: var(--text-secondary); margin: 0.5rem 0 1rem; word-break: break-all; }
  .empty-state { border: 2px dashed var(--border-color); border-radius: 14px; padding: 3rem 2rem; text-align: center; color: var(--text-secondary); font-size: 1rem; }
  .empty-state p { line-height: 1.6; margin-bottom: 1rem; }
  .empty-state .btn { margin-top: 0; }

  .summary-cards { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 1.25rem; margin-bottom: 1.5rem; }
  .card { background: var(--bg-card); border: 1px solid var(--border-color); border-radius: var(--radius-card); padding: 1.25rem 1.4rem; display: flex; flex-direction: column; gap: 0.7rem; box-shadow: var(--app-shadow); transition: box-shadow 0.15s, transform 0.15s; }
  .card:hover { transform: translateY(-1px); }
  .card-top { display: flex; align-items: center; justify-content: space-between; gap: 0.5rem; }
  .card-icon { font-size: 0.95rem; width: 2.2rem; height: 2.2rem; display: flex; align-items: center; justify-content: center; border-radius: 50%; flex-shrink: 0; background: var(--accent-soft); }
  .card-icon-income, .card-icon-expenses, .card-icon-top { background: var(--accent-soft); }
  .card-label { font-size: 0.78rem; color: var(--text-secondary); font-weight: 600; }
  .card-value { font-family: "Bitter", Georgia, serif; font-size: 1.7rem; font-weight: 600; font-variant-numeric: tabular-nums; color: var(--text-primary); line-height: 1.1; letter-spacing: -0.01em; }
  .card-value-sm { font-size: 1.1rem; line-height: 1.25; }
  .card-sub-value { font-size: 0.9rem; color: var(--text-secondary); font-variant-numeric: tabular-nums; }
  .card-income { color: var(--pos); }
  .card-expenses { color: var(--neg); }

  .charts-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(320px, 1fr)); gap: 1.25rem; }
  .chart-card { background: var(--bg-card); border: 1px solid var(--border-color); border-radius: var(--radius-card); padding: 1.5rem; box-shadow: var(--app-shadow); }
  .chart-card-wide { grid-column: 1 / -1; }
  .chart-card h3 { font-size: 1.05rem; font-weight: 600; color: var(--text-primary); margin-bottom: 1rem; }
  .chart-header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 1rem; }
  .chart-header h3 { margin-bottom: 0; }
  .trend-select { padding: 0.4rem 0.7rem; border: 1px solid var(--border-color); border-radius: var(--radius-pill); font-size: 0.8rem; background: var(--bg-card); color: var(--text-primary); }
  .chart-wrap { position: relative; width: 100%; max-height: 350px; display: flex; justify-content: center; }
  .chart-wrap canvas { max-width: 100%; max-height: 350px; }

  .cat-table-card { background: var(--bg-card); border: 1px solid var(--border-color); border-radius: var(--radius-card); padding: 1.5rem; box-shadow: var(--app-shadow); margin-top: 1.25rem; }
  .cat-table-card h3 { font-size: 1rem; font-weight: 600; color: var(--text-primary); margin-bottom: 0; }
  .cat-table-total { font-size: 1.1rem; font-weight: 700; color: var(--text-primary); font-variant-numeric: tabular-nums; }
  .cat-table { width: 100%; border-collapse: collapse; font-size: 0.85rem; }
  .cat-table th { text-align: left; padding: 0.5rem 0.75rem; background: var(--bg-secondary); border-bottom: 1px solid var(--border-color); font-weight: 600; color: var(--text-secondary); font-size: 0.72rem; text-transform: uppercase; letter-spacing: 0.04em; }
  .cat-table td { padding: 0.5rem 0.75rem; border-bottom: 1px solid var(--border-color); color: var(--text-primary); }
  .cat-table .num-col { text-align: right; font-variant-numeric: tabular-nums; }
  .cat-table .amount { font-weight: 600; color: var(--text-primary); }
  .cat-table .muted { color: var(--text-secondary); font-weight: 400; }
  .parent-row.expandable { cursor: pointer; }
  .parent-row.expandable:hover { background: var(--bg-secondary); }
  .parent-row .cat-name { font-weight: 600; color: var(--text-primary); }
  .cat-name-cell { display: flex; align-items: center; gap: 0.5rem; }
  .cat-name-cell.child { padding-left: 1.9rem; }
  .twisty { display: inline-block; width: 1rem; text-align: center; color: var(--text-secondary); font-size: 0.7rem; }
  .twisty.invisible { visibility: hidden; }
  .child-row { background: var(--bg-secondary); }
  .child-row .cat-name { color: var(--text-primary); }

  .recurring-summary-card {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-top: 1.25rem;
    padding: 1rem 1.4rem;
    background: var(--bg-card);
    border: 1px solid var(--border-color);
    border-radius: var(--radius-card);
    text-decoration: none;
    box-shadow: var(--app-shadow);
    transition: box-shadow 0.15s, transform 0.15s;
  }
  .recurring-summary-card:hover { transform: translateY(-1px); box-shadow: 0 4px 18px rgba(0,0,0,0.1); }
  .recurring-summary-left { display: flex; align-items: center; gap: 0.8rem; }
  .recurring-summary-icon { font-size: 1.2rem; width: 2.4rem; height: 2.4rem; display: flex; align-items: center; justify-content: center; border-radius: 50%; background: var(--accent-soft); }
  .recurring-summary-label { display: block; font-size: 0.85rem; font-weight: 600; color: var(--text-primary); }
  .recurring-summary-count { display: block; font-size: 0.75rem; color: var(--text-secondary); margin-top: 0.1rem; }
  .recurring-summary-right { text-align: right; }
  .recurring-summary-amount { font-family: "Bitter", Georgia, serif; font-size: 1.3rem; font-weight: 700; color: var(--text-primary); font-variant-numeric: tabular-nums; letter-spacing: -0.01em; }
  .recurring-summary-per { font-size: 0.78rem; color: var(--text-secondary); margin-left: 0.15rem; }

</style>