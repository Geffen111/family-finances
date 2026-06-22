<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { format, startOfMonth, endOfMonth, subMonths, startOfYear } from "date-fns";
  import { onMount } from "svelte";

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

  interface RecurringItem {
    description: string;
    category: string;
    frequency: string;
    occurrences: number;
    avg_amount: number;
    monthly_cost: number;
    last_date: string;
  }

  interface InsightItem {
    title: string;
    detail: string;
    severity: string;
    icon: string;
  }

  interface SpendingInsights {
    summary: string;
    spending_patterns: InsightItem[];
    anomalies: InsightItem[];
    recommendations: InsightItem[];
    period_label: string;
    generated_at: string;
  }

  let summary = $state<DashboardSummary | null>(null);
  let categorySpending = $state<CategorySpending[]>([]);
  let monthlyTrends = $state<MonthlyTrend[]>([]);
  let categoryTrends = $state<CategoryTrend[]>([]);
  let recurring = $state<RecurringItem[]>([]);
  let loading = $state(true);
  let error = $state("");

  type DatePreset = "thisMonth" | "lastMonth" | "last3Months" | "ytd" | "all";
  let activePreset = $state<DatePreset>("thisMonth");
  let customStart = $state("");
  let customEnd = $state("");
  let showCustom = $state(false);

  let selectedTrendCategory = $state<string>("top3");

  let insights = $state<SpendingInsights | null>(null);
  let insightsLoading = $state(false);
  let insightsError = $state("");
  let insightsGenerated = $state(false);

  const currencyFormat = new Intl.NumberFormat("en-AU", { style: "currency", currency: "AUD" });
  function fmt(val: number): string { return currencyFormat.format(val); }

  type ChartType = import("chart.js").Chart;
  let doughnutChart: ChartType | null = null;
  let barChart: ChartType | null = null;
  let lineChart: ChartType | null = null;

  const CHART_COLORS = ["#3b82f6", "#ef4444", "#10b981", "#f59e0b", "#8b5cf6", "#ec4899", "#14b8a6", "#f97316"];

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
      const [s, c, m] = await Promise.all([
        invoke<DashboardSummary>("get_dashboard_summary", params),
        invoke<CategorySpending[]>("get_spending_by_category", params),
        invoke<MonthlyTrend[]>("get_monthly_trends", params),
      ]);
      summary = s;
      categorySpending = c;
      monthlyTrends = m;
      const trendParams = { ...params };
      categoryTrends = await invoke<CategoryTrend[]>("get_spending_trend_by_category", trendParams);
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  async function generateInsights() {
    insightsLoading = true;
    insightsError = "";
    const params = getParams();
    try {
      insights = await invoke<SpendingInsights>("get_insights", params);
      insightsGenerated = true;
    } catch (e) {
      insightsError = String(e);
    } finally {
      insightsLoading = false;
    }
  }

  async function refreshInsights() {
    insightsLoading = true;
    insightsError = "";
    const params = getParams();
    try {
      insights = await invoke<SpendingInsights>("refresh_insights", params);
      insightsGenerated = true;
    } catch (e) {
      insightsError = String(e);
    } finally {
      insightsLoading = false;
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
    (async () => {
      const { default: Chart } = await import("chart.js/auto");
      const canvas = document.getElementById("doughnutChart") as HTMLCanvasElement | null;
      if (!canvas) return;
      if (doughnutChart) doughnutChart.destroy();
      const total = doughnutData.data.reduce((a, b) => a + b, 0);
      doughnutChart = new Chart(canvas, {
        type: "doughnut",
        data: {
          labels: doughnutData.labels,
          datasets: [{
            data: doughnutData.data,
            backgroundColor: CHART_COLORS.slice(0, doughnutData.labels.length),
            borderWidth: 1,
          }],
        },
        options: {
          responsive: true,
          maintainAspectRatio: true,
          plugins: {
            legend: { position: "bottom" },
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
    (async () => {
      const { default: Chart } = await import("chart.js/auto");
      const canvas = document.getElementById("barChart") as HTMLCanvasElement | null;
      if (!canvas) return;
      if (barChart) barChart.destroy();
      const labels = monthlyTrends.map((m) => m.label);
      barChart = new Chart(canvas, {
        type: "bar",
        data: {
          labels,
          datasets: [
            {
              label: "Income",
              data: monthlyTrends.map((m) => m.income),
              backgroundColor: "rgba(16, 185, 129, 0.7)",
              borderColor: "#10b981",
              borderWidth: 1,
            },
            {
              label: "Expenses",
              data: monthlyTrends.map((m) => m.expenses),
              backgroundColor: "rgba(239, 68, 68, 0.7)",
              borderColor: "#ef4444",
              borderWidth: 1,
            },
            {
              label: "Net",
              data: monthlyTrends.map((m) => m.net),
              type: "line",
              borderColor: "#3b82f6",
              backgroundColor: "transparent",
              borderWidth: 2,
              pointBackgroundColor: "#3b82f6",
              tension: 0.3,
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
              ticks: { callback: (v: any) => fmt(v) },
            },
          },
          plugins: {
            legend: { position: "bottom" },
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
    (async () => {
      const { default: Chart } = await import("chart.js/auto");
      const canvas = document.getElementById("lineChart") as HTMLCanvasElement | null;
      if (!canvas) return;
      if (lineChart) lineChart.destroy();

      const filtered = getFilteredTrends();
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
          borderColor: CHART_COLORS[ci % CHART_COLORS.length],
          backgroundColor: CHART_COLORS[ci % CHART_COLORS.length] + "22",
          borderWidth: 2,
          pointRadius: 3,
          tension: 0.3,
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
              ticks: { callback: (v: any) => fmt(v) },
            },
          },
          plugins: {
            legend: { position: "bottom" },
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

  let recurringMonthlyTotal = $derived(recurring.reduce((s, r) => s + r.monthly_cost, 0));

  async function loadRecurring() {
    try {
      recurring = await invoke<RecurringItem[]>("get_recurring_transactions");
    } catch (e) {
      recurring = [];
    }
  }

  onMount(() => {
    fetchData();
    loadRecurring();
    return () => {
      if (doughnutChart) doughnutChart.destroy();
      if (barChart) barChart.destroy();
      if (lineChart) lineChart.destroy();
    };
  });
</script>

<div class="page">
  <h1>Dashboard</h1>

  <div class="filter-bar">
    <button class="preset-btn" class:active={!showCustom && activePreset === "thisMonth"} onclick={() => setPreset("thisMonth")}>This Month</button>
    <button class="preset-btn" class:active={!showCustom && activePreset === "lastMonth"} onclick={() => setPreset("lastMonth")}>Last Month</button>
    <button class="preset-btn" class:active={!showCustom && activePreset === "last3Months"} onclick={() => setPreset("last3Months")}>Last 3 Months</button>
    <button class="preset-btn" class:active={!showCustom && activePreset === "ytd"} onclick={() => setPreset("ytd")}>Year to Date</button>
    <button class="preset-btn" class:active={!showCustom && activePreset === "all"} onclick={() => setPreset("all")}>All Time</button>
    <button class="preset-btn" class:active={showCustom} onclick={() => { showCustom = !showCustom; if (!showCustom) fetchData(); }}>
      Custom
    </button>
    {#if showCustom}
      <div class="custom-dates">
        <label>From <input type="date" bind:value={customStart} /></label>
        <label>To <input type="date" bind:value={customEnd} /></label>
        <button class="btn btn-sm" onclick={applyCustom}>Apply</button>
      </div>
    {/if}
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
  {/if}

  {#if recurring.length > 0}
    <div class="recurring-section">
      <div class="recurring-header">
        <h2>Recurring &amp; Subscriptions</h2>
        <span class="recurring-total">{fmt(recurringMonthlyTotal)}<span class="recurring-total-label"> / month</span></span>
      </div>
      <div class="recurring-grid">
        {#each recurring as r}
          <div class="recurring-card">
            <div class="recurring-card-top">
              <span class="recurring-desc">{r.description}</span>
              <span class="recurring-freq">{r.frequency}</span>
            </div>
            <div class="recurring-card-bottom">
              <span class="recurring-amount">{fmt(r.monthly_cost)}<span class="recurring-per">/mo</span></span>
              <span class="recurring-meta">{fmt(r.avg_amount)} × {r.occurrences} · {r.category}</span>
            </div>
          </div>
        {/each}
      </div>
    </div>
  {/if}

  <div class="insights-section">
    <div class="insights-header">
      <h2>AI Insights</h2>
      <div class="insights-actions">
        {#if !insightsGenerated && !insightsLoading}
          <button class="btn btn-primary" onclick={generateInsights}>Generate Insights</button>
        {:else if insightsGenerated && !insightsLoading}
          <button class="btn" onclick={refreshInsights}>Refresh</button>
        {/if}
        {#if insights}
          <span class="cached-note">Last generated: {insights.generated_at}</span>
        {/if}
      </div>
    </div>

    {#if insightsLoading}
      <div class="insights-loading">
        <div class="insights-skeleton insight-summary-skeleton"></div>
        <div class="insights-grid-skeleton">
          {#each Array(3) as _}
            <div class="insights-skeleton insight-card-skeleton"></div>
          {/each}
        </div>
      </div>
    {:else if insightsError}
      <div class="insights-error">
        <p>Failed to generate insights.</p>
        <p class="error-detail">{insightsError}</p>
        <button class="btn" onclick={generateInsights}>Retry</button>
      </div>
    {:else if insights}
      <div class="insights-summary-card">
        <p>{insights.summary}</p>
      </div>

      {#if insights.spending_patterns.length > 0}
        <h3 class="insights-subheading">Spending Patterns</h3>
        <div class="insights-grid">
          {#each insights.spending_patterns as item}
            <div class="insight-card" class:severity-positive={item.severity === "positive"} class:severity-warning={item.severity === "warning"} class:severity-critical={item.severity === "critical"}>
              <span class="insight-icon">{item.icon}</span>
              <div class="insight-body">
                <div class="insight-title">{item.title}</div>
                <div class="insight-detail">{item.detail}</div>
              </div>
            </div>
          {/each}
        </div>
      {/if}

      {#if insights.anomalies.length > 0}
        <h3 class="insights-subheading">Anomalies</h3>
        <div class="insights-grid">
          {#each insights.anomalies as item}
            <div class="insight-card" class:severity-positive={item.severity === "positive"} class:severity-warning={item.severity === "warning"} class:severity-critical={item.severity === "critical"}>
              <span class="insight-icon">{item.icon}</span>
              <div class="insight-body">
                <div class="insight-title">{item.title}</div>
                <div class="insight-detail">{item.detail}</div>
              </div>
            </div>
          {/each}
        </div>
      {/if}

      {#if insights.recommendations.length > 0}
        <h3 class="insights-subheading">Recommendations</h3>
        <div class="insights-grid">
          {#each insights.recommendations as item}
            <div class="insight-card insight-card-recommendation" class:severity-positive={item.severity === "positive"} class:severity-warning={item.severity === "warning"} class:severity-critical={item.severity === "critical"}>
              <span class="insight-icon">{item.icon}</span>
              <div class="insight-body">
                <div class="insight-title">{item.title}</div>
                <div class="insight-detail">{item.detail}</div>
              </div>
            </div>
          {/each}
        </div>
      {/if}

      <div class="insights-refresh-note">
        <span>Last generated: {insights.generated_at}</span>
        <button class="btn btn-sm" onclick={refreshInsights}>Refresh</button>
      </div>
    {:else if !insightsGenerated}
      <div class="insights-empty">
        <p>Generate your first AI insights report to get spending analysis and recommendations.</p>
        <button class="btn btn-primary" onclick={generateInsights}>Generate Insights</button>
      </div>
    {/if}
  </div>
</div>

<style>
  .page { max-width: 1320px; margin: 0 auto; }
  h1 { font-size: 1.75rem; font-weight: 700; color: var(--text-primary); margin-bottom: 1.25rem; }

  .filter-bar {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin-bottom: 1.5rem;
    flex-wrap: wrap;
  }
  .preset-btn {
    padding: 0.4rem 0.85rem;
    border: 1px solid var(--border-color);
    border-radius: 6px;
    background: var(--bg-card);
    color: var(--text-primary);
    font-size: 0.8rem;
    cursor: pointer;
    transition: background 0.15s, border-color 0.15s;
  }
  .preset-btn:hover { background: var(--bg-secondary); }
  .preset-btn.active { background: #2563eb; color: var(--bg-card); border-color: #2563eb; }
  .custom-dates { display: flex; align-items: center; gap: 0.5rem; }
  .custom-dates label { font-size: 0.8rem; color: var(--text-secondary); display: flex; align-items: center; gap: 0.3rem; }
  .custom-dates input[type="date"] { padding: 0.3rem 0.5rem; border: 1px solid var(--border-color); border-radius: 4px; font-size: 0.8rem; }

  .btn {
    padding: 0.5rem 1rem;
    border: 1px solid var(--border-color);
    border-radius: 6px;
    background: var(--bg-card);
    color: var(--text-primary);
    font-size: 0.875rem;
    cursor: pointer;
  }
  .btn:hover { background: var(--bg-secondary); }
  .btn-sm { padding: 0.3rem 0.65rem; font-size: 0.8rem; }
  .btn-primary { background: #2563eb; color: var(--bg-card); border-color: #2563eb; }
  .btn-primary:hover { background: #1d4ed8; }

  .loading-grid { display: flex; flex-direction: column; gap: 1.5rem; }
  .skeleton-card { background: var(--bg-card); border: 1px solid #e5e7eb; border-radius: 8px; padding: 1.25rem; }
  .skeleton-line { background: #e5e7eb; border-radius: 4px; animation: pulse 1.5s infinite; }
  .skeleton-line-sm { width: 60%; height: 0.75rem; margin-bottom: 0.5rem; }
  .skeleton-line-md { width: 40%; height: 1rem; }
  .skeleton-line-lg { width: 80%; height: 1.5rem; }
  .skeleton-block { width: 100%; height: 200px; background: #f3f4f6; border-radius: 4px; animation: pulse 1.5s infinite; }
  @keyframes pulse { 0%, 100% { opacity: 1; } 50% { opacity: 0.4; } }

  .error-state { text-align: center; padding: 3rem 2rem; color: #991b1b; background: #fee2e2; border: 1px solid #fecaca; border-radius: 8px; }
  .error-detail { font-size: 0.8rem; color: var(--text-secondary); margin: 0.5rem 0 1rem; word-break: break-all; }
  .empty-state { border: 2px dashed var(--border-color); border-radius: 8px; padding: 3rem 2rem; text-align: center; color: var(--text-secondary); font-size: 1rem; }
  .empty-state .btn { margin-top: 1rem; }

  .summary-cards { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 1.25rem; margin-bottom: 2rem; }
  .card { background: var(--bg-card); border: 1px solid var(--border-color); border-radius: 12px; padding: 1.25rem 1.35rem; display: flex; flex-direction: column; gap: 0.6rem; box-shadow: 0 1px 2px rgba(0,0,0,0.04); transition: box-shadow 0.15s, transform 0.15s; }
  .card:hover { box-shadow: 0 4px 14px rgba(0,0,0,0.07); transform: translateY(-1px); }
  .card-top { display: flex; align-items: center; justify-content: space-between; gap: 0.5rem; }
  .card-icon { font-size: 1.1rem; width: 2.1rem; height: 2.1rem; display: flex; align-items: center; justify-content: center; border-radius: 8px; flex-shrink: 0; }
  .card-icon-income { background: #d1fae5; }
  .card-icon-expenses { background: #fee2e2; }
  .card-icon-top { background: #fef3c7; }
  .card-label { font-size: 0.72rem; color: var(--text-secondary); text-transform: uppercase; letter-spacing: 0.06em; font-weight: 600; }
  .card-value { font-size: 1.6rem; font-weight: 700; font-variant-numeric: tabular-nums; color: var(--text-primary); line-height: 1.1; }
  .card-value-sm { font-size: 1.05rem; line-height: 1.25; }
  .card-sub-value { font-size: 0.9rem; color: var(--text-secondary); font-variant-numeric: tabular-nums; }
  .card-income { color: #16a34a; }
  .card-expenses { color: #dc2626; }

  .charts-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(320px, 1fr)); gap: 1.25rem; }
  .chart-card { background: var(--bg-card); border: 1px solid var(--border-color); border-radius: 12px; padding: 1.5rem; box-shadow: 0 1px 2px rgba(0,0,0,0.04); }
  .chart-card-wide { grid-column: 1 / -1; }
  .chart-card h3 { font-size: 1rem; font-weight: 600; color: var(--text-primary); margin-bottom: 1rem; }
  .chart-header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 1rem; }
  .chart-header h3 { margin-bottom: 0; }
  .trend-select { padding: 0.35rem 0.5rem; border: 1px solid var(--border-color); border-radius: 4px; font-size: 0.8rem; background: var(--bg-card); color: var(--text-primary); }
  .chart-wrap { position: relative; width: 100%; max-height: 350px; display: flex; justify-content: center; }
  .chart-wrap canvas { max-width: 100%; max-height: 350px; }

  .recurring-section { margin-top: 2.5rem; padding-top: 1.5rem; border-top: 1px solid var(--border-color); }
  .recurring-header { display: flex; justify-content: space-between; align-items: baseline; margin-bottom: 1rem; gap: 0.5rem; flex-wrap: wrap; }
  .recurring-header h2 { font-size: 1.25rem; font-weight: 700; color: var(--text-primary); margin: 0; }
  .recurring-total { font-size: 1.35rem; font-weight: 700; color: var(--text-primary); font-variant-numeric: tabular-nums; }
  .recurring-total-label { font-size: 0.85rem; font-weight: 500; color: var(--text-secondary); }
  .recurring-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); gap: 1rem; }
  .recurring-card { background: var(--bg-card); border: 1px solid var(--border-color); border-radius: 12px; padding: 1rem 1.1rem; box-shadow: 0 1px 2px rgba(0,0,0,0.04); }
  .recurring-card-top { display: flex; justify-content: space-between; align-items: baseline; gap: 0.5rem; margin-bottom: 0.5rem; }
  .recurring-desc { font-size: 0.88rem; font-weight: 600; color: var(--text-primary); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .recurring-freq { font-size: 0.68rem; text-transform: uppercase; letter-spacing: 0.04em; font-weight: 600; color: var(--text-secondary); background: var(--bg-secondary); border: 1px solid var(--border-color); border-radius: 4px; padding: 0.1rem 0.4rem; white-space: nowrap; }
  .recurring-card-bottom { display: flex; justify-content: space-between; align-items: baseline; gap: 0.5rem; }
  .recurring-amount { font-size: 1.1rem; font-weight: 700; color: var(--text-primary); font-variant-numeric: tabular-nums; }
  .recurring-per { font-size: 0.75rem; font-weight: 500; color: var(--text-secondary); }
  .recurring-meta { font-size: 0.72rem; color: var(--text-secondary); text-align: right; }

  .insights-section { margin-top: 2.5rem; padding-top: 1.5rem; border-top: 1px solid var(--border-color); }
  .insights-header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 1rem; flex-wrap: wrap; gap: 0.5rem; }
  .insights-header h2 { font-size: 1.25rem; font-weight: 700; color: var(--text-primary); margin: 0; }
  .insights-actions { display: flex; align-items: center; gap: 0.75rem; }
  .cached-note { font-size: 0.75rem; color: var(--text-muted); }

  .insights-loading { display: flex; flex-direction: column; gap: 1rem; }
  .insights-skeleton { background: #e5e7eb; border-radius: 8px; animation: pulse 1.5s infinite; }
  .insight-summary-skeleton { width: 100%; height: 4rem; }
  .insight-card-skeleton { width: 100%; height: 6rem; }
  .insights-grid-skeleton { display: grid; grid-template-columns: repeat(auto-fill, minmax(300px, 1fr)); gap: 1rem; }

  .insights-error { text-align: center; padding: 2rem; color: #991b1b; background: #fee2e2; border: 1px solid #fecaca; border-radius: 8px; }

  .insights-summary-card { background: #eff6ff; border: 1px solid #bfdbfe; border-radius: 10px; padding: 1.25rem; margin-bottom: 1.5rem; }
  .insights-summary-card p { margin: 0; font-size: 1rem; color: #1e40af; line-height: 1.5; }

  .insights-subheading { font-size: 1rem; font-weight: 600; color: var(--text-primary); margin: 1.5rem 0 0.75rem; }

  .insights-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(300px, 1fr)); gap: 1rem; margin-bottom: 0.5rem; }

  .insight-card { background: var(--bg-card); border: 1px solid var(--border-color); border-radius: 10px; padding: 1rem; display: flex; gap: 0.75rem; box-shadow: 0 1px 3px rgba(0,0,0,0.04); border-left: 4px solid var(--border-color); }
  .insight-card.severity-positive { border-left-color: #10b981; }
  .insight-card.severity-warning { border-left-color: #f59e0b; }
  .insight-card.severity-critical { border-left-color: #ef4444; }
  .insight-card-recommendation { border-left-color: #8b5cf6; }
  .insight-card-recommendation.severity-positive { border-left-color: #10b981; }
  .insight-card-recommendation.severity-warning { border-left-color: #f59e0b; }
  .insight-card-recommendation.severity-critical { border-left-color: #ef4444; }

  .insight-icon { font-size: 1.5rem; flex-shrink: 0; width: 2rem; text-align: center; padding-top: 0.1rem; }
  .insight-body { min-width: 0; }
  .insight-title { font-weight: 600; font-size: 0.9rem; color: var(--text-primary); margin-bottom: 0.25rem; }
  .insight-detail { font-size: 0.8rem; color: var(--text-secondary); line-height: 1.4; }

  .insights-refresh-note { margin-top: 1rem; text-align: center; font-size: 0.8rem; color: var(--text-muted); display: flex; align-items: center; justify-content: center; gap: 0.5rem; }

  .insights-empty { border: 2px dashed var(--border-color); border-radius: 8px; padding: 2.5rem 2rem; text-align: center; color: var(--text-secondary); }
  .insights-empty .btn { margin-top: 0.75rem; }
</style>