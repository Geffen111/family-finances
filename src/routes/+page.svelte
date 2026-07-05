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
    assets: number;
    liabilities: number;
  }

  interface Asset {
    id: number;
    name: string;
    asset_type: string;
    value: number;
    notes: string | null;
    created_at: string;
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

  interface UpcomingBill {
    name: string;
    amount: number;
    due_date: string;
    category_name: string | null;
    frequency: string;
  }

  interface SafeToSpend {
    liquid: number;
    upcoming_total: number;
    safe_to_spend: number;
    horizon_days: number;
    bills: UpcomingBill[];
  }

  interface IncomeSource {
    category_name: string;
    total: number;
    percentage: number;
  }

  interface CategoryMover {
    category_name: string;
    current: number;
    previous: number;
    delta: number;
  }

  let summary = $state<DashboardSummary | null>(null);
  let incomeSources = $state<IncomeSource[]>([]);
  let categoryMovers = $state<CategoryMover[]>([]);
  let monthlyTrends = $state<MonthlyTrend[]>([]);
  let categoryTrends = $state<CategoryTrend[]>([]);
  let categoryTree = $state<CategorySpendingGroup[]>([]);
  // Spending Breakdown has its own timeframe, so it fetches the tree separately
  // from the page-controlled `categoryTree` used by the Category Spending table.
  let breakdownTree = $state<CategorySpendingGroup[]>([]);
  let expandedCategories = $state<Set<number>>(new Set());
  let recurring = $state<RecurringCost[]>([]);
  let cashflow = $state<SafeToSpend | null>(null);
  let netWorth = $state<NetWorthPoint[]>([]);
  let assets = $state<Asset[]>([]);
  let loading = $state(true);
  let error = $state("");

  // Asset types: label + emoji icon, used in the card and to colour chart bands.
  const ASSET_TYPES = [
    { value: "property", label: "Property", icon: "\u{1F3E0}" },
    { value: "investment", label: "Investment", icon: "\u{1F4C8}" },
    { value: "vehicle", label: "Vehicle", icon: "\u{1F697}" },
    { value: "cash", label: "Cash", icon: "\u{1F4B5}" },
    { value: "other", label: "Other", icon: "\u{1F4E6}" },
  ];
  function assetIcon(type: string): string {
    return ASSET_TYPES.find((t) => t.value === type)?.icon ?? "\u{1F4E6}";
  }

  // New-asset form state.
  let newAssetName = $state("");
  let newAssetType = $state("property");
  let newAssetValue = $state("");
  let assetSaving = $state(false);
  let assetError = $state("");

  type DatePreset = "thisMonth" | "lastMonth" | "last3Months" | "last6Months" | "last24Months" | "ytd" | "all";
  let activePreset = $state<DatePreset>("last6Months");
  let customStart = $state("");
  let customEnd = $state("");
  let showCustom = $state(false);

  // --- Independent chart timeframes (untied from the page date picker) --------
  // Category Spending Trend: its own 6/12/24-month window + a multi-select set of
  // category names (checkboxes) so several trend lines can show at once.
  type TrendPreset = "6m" | "12m" | "24m";
  let trendPreset = $state<TrendPreset>("12m");
  let selectedTrendCats = $state<Set<string>>(new Set());
  let trendCatsSeeded = false; // seed the top 3 once, then leave the user's picks

  // Spending Breakdown: its own timeframe, plus the parent/subcategory drill.
  type BreakdownPreset = "lastMonth" | "3m" | "6m" | "12m";
  let breakdownPreset = $state<BreakdownPreset>("6m");
  // "all" = one slice per parent category; otherwise the stringified parent
  // category id, drilling into that parent's subcategories.
  let selectedBreakdown = $state<string>("all");

  // Parent categories kept OUT of the Spending Breakdown pie. This is chart-only
  // (internal transfers, loan repayments and investment-property/pass-through
  // categories aren't "spending" for this view) — it does NOT touch income/
  // expense totals, budgets or forecasts, which still use exclude_from_budget.
  const BREAKDOWN_EXCLUDE = new Set<string>([
    "Withdrawals & Transfers",
    "Loan Repayments",
    "Investment Property Expenses",
    "Income",
  ]);

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

  // Start-date param for the last `n` calendar months (inclusive of this month).
  function lastNMonthsParams(n: number): Record<string, unknown> {
    return { startDate: format(startOfMonth(subMonths(new Date(), n - 1)), "yyyy-MM-dd") };
  }
  // Just the previous whole calendar month.
  function lastMonthParams(): Record<string, unknown> {
    const last = subMonths(new Date(), 1);
    return {
      startDate: format(startOfMonth(last), "yyyy-MM-dd"),
      endDate: format(endOfMonth(last), "yyyy-MM-dd"),
    };
  }
  function trendParams(): Record<string, unknown> {
    return lastNMonthsParams(trendPreset === "6m" ? 6 : trendPreset === "12m" ? 12 : 24);
  }
  function breakdownParams(): Record<string, unknown> {
    switch (breakdownPreset) {
      case "lastMonth": return lastMonthParams();
      case "3m": return lastNMonthsParams(3);
      case "6m": return lastNMonthsParams(6);
      case "12m": return lastNMonthsParams(12);
    }
  }

  // Page date picker drives the lower section: summary cards, the Category
  // Spending table, income sources and movers.
  async function fetchPageData() {
    loading = true;
    error = "";
    const params = getParams();
    try {
      const [s, tree] = await Promise.all([
        invoke<DashboardSummary>("get_dashboard_summary", params),
        invoke<CategorySpendingGroup[]>("get_category_spending_tree", params),
      ]);
      summary = s;
      categoryTree = tree;
      incomeSources = await invoke<IncomeSource[]>("get_income_by_category", params);
      // Movers need an explicit window start; skip for the all-time view.
      categoryMovers = params.startDate
        ? await invoke<CategoryMover[]>("get_category_movers", params)
        : [];
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  // Spending Breakdown pie — its own timeframe.
  async function fetchBreakdown() {
    try {
      breakdownTree = await invoke<CategorySpendingGroup[]>("get_category_spending_tree", breakdownParams());
    } catch {
      breakdownTree = [];
    }
  }

  // Category Spending Trend — its own timeframe; seed the top 3 lines on first load.
  async function fetchTrend() {
    try {
      categoryTrends = await invoke<CategoryTrend[]>("get_spending_trend_by_category", trendParams());
      if (!trendCatsSeeded && categoryTrends.length > 0) {
        const top3 = rankTrendCategories(categoryTrends).slice(0, 3);
        selectedTrendCats = new Set(top3);
        trendCatsSeeded = true;
      }
    } catch {
      categoryTrends = [];
    }
  }

  // Monthly Income vs Expenses — locked to the last 12 months.
  async function fetchMonthly() {
    try {
      monthlyTrends = await invoke<MonthlyTrend[]>("get_monthly_trends", lastNMonthsParams(12));
    } catch {
      monthlyTrends = [];
    }
  }

  // Category names present in trend data, ranked by total spend over the window.
  function rankTrendCategories(rows: CategoryTrend[]): string[] {
    const totals = new Map<string, number>();
    for (const ct of rows) totals.set(ct.category_name, (totals.get(ct.category_name) ?? 0) + ct.amount);
    return [...totals.entries()].sort((a, b) => b[1] - a[1]).map(([name]) => name);
  }
  let trendCategoryOptions = $derived(rankTrendCategories(categoryTrends));

  function setPreset(preset: DatePreset) {
    activePreset = preset;
    showCustom = false;
    fetchPageData();
  }

  function applyCustom() {
    showCustom = true;
    fetchPageData();
  }

  function setTrendPreset(p: TrendPreset) {
    trendPreset = p;
    fetchTrend();
  }
  function setBreakdownPreset(p: BreakdownPreset) {
    breakdownPreset = p;
    fetchBreakdown();
  }
  function toggleTrendCat(name: string) {
    const next = new Set(selectedTrendCats);
    if (next.has(name)) next.delete(name);
    else next.add(name);
    selectedTrendCats = next;
  }

  function getFilteredTrends(): CategoryTrend[] {
    return categoryTrends.filter((ct) => selectedTrendCats.has(ct.category_name));
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

  // Spending Breakdown is driven by the category tree so it can show parent
  // totals or drill into one parent's children, and so it reuses the tree's
  // budget-exclusion. On top of that we drop the transfer/loan/etc. parents.
  let breakdownGroups = $derived(
    breakdownTree.filter((g) => g.total > 0 && !BREAKDOWN_EXCLUDE.has(g.name)),
  );
  // Parents worth drilling into (have subcategories) populate the dropdown.
  let breakdownParents = $derived(
    breakdownGroups.filter((g) => g.category_id != null && g.children.length > 0),
  );

  // Collapse a name/total list to at most 8 slices plus an "Other" bucket.
  function toSlices(items: { name: string; total: number }[]): { labels: string[]; data: number[] } | null {
    const rows = items.filter((i) => i.total > 0);
    if (rows.length === 0) return null;
    const top = rows.slice(0, 8);
    const rest = rows.slice(8);
    const labels = top.map((i) => i.name);
    const data = top.map((i) => i.total);
    if (rest.length > 0) {
      labels.push("Other");
      data.push(rest.reduce((s, i) => s + i.total, 0));
    }
    return { labels, data };
  }

  let doughnutData = $derived.by(() => {
    // Drill into a specific parent's subcategories when one is selected and still
    // present; otherwise fall back to the all-parents view.
    if (selectedBreakdown !== "all") {
      const g = breakdownGroups.find((grp) => String(grp.category_id) === selectedBreakdown);
      if (g) {
        const items = g.children.map((c) => ({ name: c.name, total: c.total }));
        if (g.direct_total > 0) items.push({ name: `${g.name} (direct)`, total: g.direct_total });
        items.sort((a, b) => b.total - a.total);
        return toSlices(items);
      }
    }
    return toSlices(breakdownGroups.map((g) => ({ name: g.name, total: g.total })));
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
              barPercentage: 0.72,
              categoryPercentage: 0.72,
            },
            {
              label: "Expenses",
              data: monthlyTrends.map((m) => m.expenses),
              backgroundColor: neg,
              borderWidth: 0,
              borderRadius: 5,
              borderSkipped: false,
              barPercentage: 0.72,
              categoryPercentage: 0.72,
            },
          ],
        },
        options: {
          // Horizontal bars (months down the y-axis) — spaces the months out
          // better than vertical columns, and drops the removed Net line.
          // Fills its card (fixed-height wrapper) rather than a set aspect ratio.
          indexAxis: "y",
          responsive: true,
          maintainAspectRatio: false,
          interaction: { intersect: false, mode: "index" },
          scales: {
            x: {
              beginAtZero: true,
              border: { display: false },
              grid: { color: grid },
              ticks: { color: tick, callback: (v: any) => fmt(v) },
            },
            y: { border: { display: false }, grid: { display: false }, ticks: { color: tick } },
          },
          plugins: {
            legend: { position: "bottom", labels: { color: themeVar("--text-secondary", "#7b7468"), usePointStyle: true, pointStyle: "circle", boxWidth: 8 } },
            tooltip: {
              callbacks: {
                label: (ctx: any) => `${ctx.dataset.label}: ${fmt(ctx.parsed.x ?? ctx.parsed)}`,
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

      // Order the x-axis by the yyyy-MM month key — sorting the display labels
      // alphabetically scrambles them ("Apr 26" < "Feb 26" < "Jan 26"). Build the
      // month sequence from `ct.month`, then map each to its human label.
      const monthToLabel = new Map<string, string>();
      for (const ct of filtered) monthToLabel.set(ct.month, ct.label);
      const months = [...monthToLabel.keys()].sort();
      const labels = months.map((m) => monthToLabel.get(m)!);
      const grouped = new Map<string, Map<string, number>>();
      for (const ct of filtered) {
        if (!grouped.has(ct.category_name)) grouped.set(ct.category_name, new Map());
        grouped.get(ct.category_name)!.set(ct.month, ct.amount);
      }

      const datasets: any[] = [];
      let ci = 0;
      for (const [name, byMonth] of grouped) {
        datasets.push({
          label: name,
          data: months.map((m) => byMonth.get(m) ?? 0),
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
          maintainAspectRatio: false,
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

  async function loadCashflow() {
    try {
      cashflow = await invoke<SafeToSpend>("get_safe_to_spend", { days: 30 });
    } catch (e) {
      cashflow = null;
    }
  }

  function billDateLabel(iso: string): string {
    try {
      return format(new Date(iso + "T00:00:00"), "EEE d MMM");
    } catch {
      return iso;
    }
  }

  async function loadNetWorth() {
    try {
      netWorth = await invoke<NetWorthPoint[]>("get_net_worth_trend");
    } catch (e) {
      netWorth = [];
    }
  }

  async function loadAssets() {
    try {
      assets = await invoke<Asset[]>("list_assets");
    } catch (e) {
      assets = [];
    }
  }

  let assetsTotal = $derived(assets.reduce((s, a) => s + a.value, 0));
  // Latest account-derived net worth, plus assets = total household net worth.
  let accountsNetWorth = $derived(netWorth.length > 0 ? netWorth[netWorth.length - 1].net_worth : 0);
  let totalNetWorth = $derived(accountsNetWorth + assetsTotal);

  async function addAsset() {
    assetError = "";
    const name = newAssetName.trim();
    const value = parseFloat(newAssetValue);
    if (!name) { assetError = "Enter a name."; return; }
    if (!isFinite(value)) { assetError = "Enter a value."; return; }
    assetSaving = true;
    try {
      await invoke("create_asset", { name, assetType: newAssetType, value, notes: null });
      newAssetName = "";
      newAssetValue = "";
      newAssetType = "property";
      await loadAssets();
    } catch (e) {
      assetError = String(e);
    } finally {
      assetSaving = false;
    }
  }

  async function removeAsset(id: number) {
    try {
      await invoke("delete_asset", { id });
      await loadAssets();
    } catch (e) {
      assetError = String(e);
    }
  }

  let netWorthChart: ChartType | null = null;

  $effect(() => {
    if (loading || netWorth.length === 0) return;
    // Read reactive state synchronously so the chart redraws when assets change.
    const assetList = assets;
    const dark = $darkMode;
    (async () => {
      void dark;
      const { default: Chart } = await import("chart.js/auto");
      const canvas = document.getElementById("netWorthChart") as HTMLCanvasElement | null;
      if (!canvas) return;
      if (netWorthChart) netWorthChart.destroy();
      const accent = themeVar("--accent", "#7f9a6f");
      const neg = themeVar("--neg", "#c77a5a");
      const grid = themeVar("--border-color", "#ece0cc");
      const tick = themeVar("--text-muted", "#a89f90");
      const series = chartSeries();
      const labels = netWorth.map((p) => p.label);
      const manualTotal = assetList.reduce((s, a) => s + a.value, 0);
      const hasLiabilities = netWorth.some((p) => p.liabilities > 0);

      // Composition bands share the "comp" stack: assets stack upward from zero,
      // liabilities (negative) stack downward, so the chart literally shows what
      // you own above the line and what you owe below it. The bold "Net worth"
      // line sits in its own stack so it reads as the true total (assets +
      // manually-tracked assets − liabilities) rather than being stacked itself.
      const datasets: any[] = [
        {
          label: "Cash & accounts",
          data: netWorth.map((p) => p.assets),
          stack: "comp",
          borderColor: accent,
          backgroundColor: withAlpha(accent, 0.45),
          borderWidth: 1,
          pointRadius: 0,
          tension: 0.35,
          fill: true,
          order: 3,
        },
      ];
      // One stacked band per manually-tracked asset (held flat across the period,
      // since assets carry a single current value, not month-by-month history).
      assetList.forEach((a, i) => {
        const c = series[(i + 1) % series.length];
        datasets.push({
          label: a.name,
          data: labels.map(() => a.value),
          stack: "comp",
          borderColor: c,
          backgroundColor: withAlpha(c, 0.45),
          borderWidth: 1,
          pointRadius: 0,
          tension: 0,
          fill: true,
          order: 3,
        });
      });
      if (hasLiabilities) {
        datasets.push({
          label: "Liabilities (loans, cards)",
          data: netWorth.map((p) => -p.liabilities),
          stack: "comp",
          borderColor: neg,
          backgroundColor: withAlpha(neg, 0.4),
          borderWidth: 1,
          pointRadius: 0,
          tension: 0.35,
          fill: true,
          order: 3,
        });
      }
      // True net worth = account assets + manual assets − liabilities.
      datasets.push({
        label: "Net worth",
        data: netWorth.map((p) => p.net_worth + manualTotal),
        stack: "net",
        borderColor: accent,
        backgroundColor: "transparent",
        borderWidth: 2.5,
        pointRadius: 2,
        pointBackgroundColor: accent,
        tension: 0.35,
        fill: false,
        order: 0,
      });

      netWorthChart = new Chart(canvas, {
        type: "line",
        data: { labels, datasets },
        options: {
          responsive: true,
          maintainAspectRatio: false,
          interaction: { intersect: false, mode: "index" },
          scales: {
            y: { stacked: true, border: { display: false }, grid: { color: grid }, ticks: { color: tick, callback: (v: any) => fmt(v) } },
            x: { stacked: true, border: { display: false }, grid: { display: false }, ticks: { color: tick } },
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

  onMount(() => {
    fetchPageData();
    fetchBreakdown();
    fetchTrend();
    fetchMonthly();
    loadRecurring();
    loadCashflow();
    loadNetWorth();
    loadAssets();
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
  </div>

  {#if loading}
    <div class="loading-grid">
      <div class="charts-grid">
        {#each Array(2) as _}
          <div class="chart-card"><div class="skeleton-line skeleton-line-md" style="margin-bottom:1rem;"></div><div class="skeleton-block"></div></div>
        {/each}
      </div>
      <div class="summary-cards">
        {#each Array(4) as _}
          <div class="skeleton-card"><div class="skeleton-line skeleton-line-sm"></div><div class="skeleton-line skeleton-line-lg"></div></div>
        {/each}
      </div>
    </div>
  {:else if error}
    <div class="error-state">
      <p>Error loading dashboard data.</p>
      <p class="error-detail">{error}</p>
      <button class="btn" onclick={fetchPageData}>Retry</button>
    </div>
  {:else if summary && summary.transaction_count === 0}
    <div class="empty-state">
      <p>No transactions yet. Import a CSV to get started.</p>
      <a href="/transactions" class="btn btn-primary">Go to Transactions</a>
    </div>
  {:else if summary}
    <!-- Top row: spending breakdown + monthly income vs expenses.
         Each has its own timeframe, untied from the page date picker below. -->
    <div class="charts-grid">
      <div class="chart-card">
        <div class="chart-header">
          <h3>Spending Breakdown</h3>
          <div class="chart-controls">
            <div class="mini-switch">
              <button class="mini-btn" class:active={breakdownPreset === "lastMonth"} onclick={() => setBreakdownPreset("lastMonth")}>Last Mo.</button>
              <button class="mini-btn" class:active={breakdownPreset === "3m"} onclick={() => setBreakdownPreset("3m")}>3M</button>
              <button class="mini-btn" class:active={breakdownPreset === "6m"} onclick={() => setBreakdownPreset("6m")}>6M</button>
              <button class="mini-btn" class:active={breakdownPreset === "12m"} onclick={() => setBreakdownPreset("12m")}>12M</button>
            </div>
            <select class="trend-select" bind:value={selectedBreakdown}>
              <option value="all">All categories</option>
              {#each breakdownParents as g (g.category_id)}
                <option value={String(g.category_id)}>{g.name}</option>
              {/each}
            </select>
          </div>
        </div>
        <div class="chart-wrap"><canvas id="doughnutChart"></canvas></div>
      </div>
      <div class="chart-card">
        <h3>Monthly Income vs Expenses <span class="chart-note">last 12 months</span></h3>
        <div class="chart-fill chart-fill-tall"><canvas id="barChart"></canvas></div>
      </div>
    </div>

    <!-- Category spending trend: own timeframe + multi-select category lines. -->
    <div class="chart-card dash-block">
      <div class="chart-header">
        <h3>Category Spending Trend</h3>
        <div class="chart-controls">
          <div class="mini-switch">
            <button class="mini-btn" class:active={trendPreset === "6m"} onclick={() => setTrendPreset("6m")}>6M</button>
            <button class="mini-btn" class:active={trendPreset === "12m"} onclick={() => setTrendPreset("12m")}>12M</button>
            <button class="mini-btn" class:active={trendPreset === "24m"} onclick={() => setTrendPreset("24m")}>24M</button>
          </div>
          <details class="cat-multi">
            <summary>{selectedTrendCats.size} {selectedTrendCats.size === 1 ? "category" : "categories"}</summary>
            <div class="cat-multi-menu">
              {#if trendCategoryOptions.length === 0}
                <p class="cat-multi-empty">No spending in this period.</p>
              {/if}
              {#each trendCategoryOptions as name (name)}
                <label class="cat-multi-item">
                  <input type="checkbox" checked={selectedTrendCats.has(name)} onchange={() => toggleTrendCat(name)} />
                  <span>{name}</span>
                </label>
              {/each}
            </div>
          </details>
        </div>
      </div>
      <div class="chart-fill"><canvas id="lineChart"></canvas></div>
    </div>

    <!-- Page date picker — controls only the summary cards + tables below. -->
    <div class="section-divider">
      <span class="section-divider-label">Summary period</span>
      <div class="filter-bar">
        <div class="period-switch">
          <button class="preset-btn" class:active={!showCustom && activePreset === "thisMonth"} onclick={() => setPreset("thisMonth")}>This Month</button>
          <button class="preset-btn" class:active={!showCustom && activePreset === "lastMonth"} onclick={() => setPreset("lastMonth")}>Last Mo.</button>
          <button class="preset-btn" class:active={!showCustom && activePreset === "last3Months"} onclick={() => setPreset("last3Months")}>3M</button>
          <button class="preset-btn" class:active={!showCustom && activePreset === "last6Months"} onclick={() => setPreset("last6Months")}>6M</button>
          <button class="preset-btn" class:active={!showCustom && activePreset === "last24Months"} onclick={() => setPreset("last24Months")}>24M</button>
          <button class="preset-btn" class:active={!showCustom && activePreset === "ytd"} onclick={() => setPreset("ytd")}>YTD</button>
          <button class="preset-btn" class:active={!showCustom && activePreset === "all"} onclick={() => setPreset("all")}>All</button>
          <button class="preset-btn" class:active={showCustom} onclick={() => { showCustom = !showCustom; if (!showCustom) fetchPageData(); }}>Custom</button>
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

    {#if netWorth.length > 1}
      <div class="chart-card dash-block">
        <h3>Net Worth Over Time</h3>
        <div class="chart-fill"><canvas id="netWorthChart"></canvas></div>
      </div>
    {/if}

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

    <div class="assets-card">
      <div class="chart-header">
        <h3>Assets &amp; Investments</h3>
        {#if assets.length > 0}
          <span class="assets-total">{fmt(assetsTotal)}</span>
        {/if}
      </div>

      {#if assets.length > 0}
        <ul class="assets-list">
          {#each assets as a (a.id)}
            <li class="asset-row">
              <span class="asset-icon">{assetIcon(a.asset_type)}</span>
              <span class="asset-name">{a.name}</span>
              <span class="asset-type">{a.asset_type}</span>
              <span class="asset-value">{fmt(a.value)}</span>
              <button class="asset-del" title="Remove" aria-label="Remove asset" onclick={() => removeAsset(a.id)}>&times;</button>
            </li>
          {/each}
        </ul>
        {#if netWorth.length > 0}
          <div class="assets-networth">
            <span>Total net worth (assets + accounts − debts)</span>
            <strong>{fmt(totalNetWorth)}</strong>
          </div>
        {/if}
      {:else}
        <p class="assets-empty">Add an asset like your home or investments to see it in your net worth.</p>
      {/if}

      <form class="asset-form" onsubmit={(e) => { e.preventDefault(); addAsset(); }}>
        <input class="asset-input asset-input-name" type="text" placeholder="e.g. Family home" bind:value={newAssetName} />
        <select class="asset-input asset-input-type" bind:value={newAssetType}>
          {#each ASSET_TYPES as t}
            <option value={t.value}>{t.icon} {t.label}</option>
          {/each}
        </select>
        <input class="asset-input asset-input-value" type="number" step="0.01" placeholder="Value" bind:value={newAssetValue} />
        <button class="btn btn-primary btn-sm" type="submit" disabled={assetSaving}>{assetSaving ? "Adding…" : "Add"}</button>
      </form>
      {#if assetError}
        <p class="asset-error">{assetError}</p>
      {/if}
    </div>
  {/if}

  {#if cashflow && (cashflow.liquid !== 0 || cashflow.bills.length > 0)}
    <div class="cashflow-card">
      <div class="cashflow-head">
        <div>
          <span class="cashflow-label">Safe to spend</span>
          <span class="cashflow-sub">after the next {cashflow.horizon_days} days of bills</span>
        </div>
        <span class="cashflow-amount" class:cashflow-neg={cashflow.safe_to_spend < 0}>
          {fmt(cashflow.safe_to_spend)}
        </span>
      </div>
      <div class="cashflow-breakdown">
        <span>{fmt(cashflow.liquid)} liquid</span>
        <span>&minus; {fmt(cashflow.upcoming_total)} upcoming bills</span>
      </div>
      {#if cashflow.bills.length > 0}
        <div class="cashflow-bills">
          {#each cashflow.bills.slice(0, 6) as bill (bill.name + bill.due_date)}
            <div class="cashflow-bill">
              <span class="cashflow-bill-date">{billDateLabel(bill.due_date)}</span>
              <span class="cashflow-bill-name">{bill.name}</span>
              <span class="cashflow-bill-amount">{fmt(bill.amount)}</span>
            </div>
          {/each}
          {#if cashflow.bills.length > 6}
            <div class="cashflow-more">+{cashflow.bills.length - 6} more</div>
          {/if}
        </div>
      {/if}
    </div>
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

  {#if !loading && (incomeSources.length > 0 || categoryMovers.length > 0)}
    <div class="reports-grid">
      {#if incomeSources.length > 0}
        <div class="report-card">
          <h3>Income sources</h3>
          <div class="report-list">
            {#each incomeSources.slice(0, 8) as src (src.category_name)}
              <div class="report-row">
                <span class="report-name">{src.category_name}</span>
                <div class="report-bar-track">
                  <div class="report-bar" style="width: {src.percentage}%;"></div>
                </div>
                <span class="report-val">{fmt(src.total)}</span>
              </div>
            {/each}
          </div>
        </div>
      {/if}

      {#if categoryMovers.length > 0}
        <div class="report-card">
          <h3>Biggest changes <span class="report-sub">vs previous period</span></h3>
          <div class="report-list">
            {#each categoryMovers as m (m.category_name)}
              <div class="report-row mover-row">
                <span class="report-name">{m.category_name}</span>
                <span class="mover-prev">{fmt(m.previous)} → {fmt(m.current)}</span>
                <span class="mover-delta" class:up={m.delta > 0} class:down={m.delta < 0}>
                  {m.delta > 0 ? "▲" : m.delta < 0 ? "▼" : ""} {fmt(Math.abs(m.delta))}
                </span>
              </div>
            {/each}
          </div>
        </div>
      {/if}
    </div>
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

  .loading-grid { display: flex; flex-direction: column; gap: 1.5rem; }
  .skeleton-card { background: var(--bg-card); border: 1px solid var(--track); border-radius: 14px; padding: 1.25rem; }
  .skeleton-line { background: var(--track); border-radius: 4px; animation: pulse 1.5s infinite; }
  .skeleton-line-sm { width: 60%; height: 0.75rem; margin-bottom: 0.5rem; }
  .skeleton-line-md { width: 40%; height: 1rem; }
  .skeleton-line-lg { width: 80%; height: 1.5rem; }
  .skeleton-block { width: 100%; height: 200px; background: var(--bg-secondary); border-radius: 4px; animation: pulse 1.5s infinite; }
  @keyframes pulse { 0%, 100% { opacity: 1; } 50% { opacity: 0.4; } }

  .error-state { text-align: center; padding: 3rem 2rem; color: var(--neg); background: var(--bg-secondary); border: 1px solid var(--neg); border-radius: 14px; }
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
  .chart-card h3 { font-size: 1.05rem; font-weight: 600; color: var(--text-primary); margin-bottom: 1rem; }
  .chart-header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 1rem; }
  .chart-header h3 { margin-bottom: 0; }
  .trend-select { padding: 0.4rem 0.7rem; border: 1px solid var(--border-color); border-radius: var(--radius-pill); font-size: 0.8rem; background: var(--bg-card); color: var(--text-primary); }
  .chart-wrap { position: relative; width: 100%; max-height: 350px; display: flex; justify-content: center; }
  .chart-wrap canvas { max-width: 100%; max-height: 350px; }
  /* Fill charts stretch to the card width and a fixed height (maintainAspectRatio
     off), so they use the full card instead of sitting at a fixed aspect ratio. */
  .chart-fill { position: relative; width: 100%; height: 340px; }
  .chart-fill-tall { height: 440px; }
  .chart-note { font-size: 0.72rem; font-weight: 400; color: var(--text-secondary); margin-left: 0.4rem; }

  /* Spacing when chart cards stack vertically (they're plain blocks, not grid items). */
  .dash-block { margin-top: 1.25rem; }

  /* Per-chart controls: little timeframe switch + a select/multi-select. */
  .chart-controls { display: flex; align-items: center; gap: 0.5rem; flex-wrap: wrap; justify-content: flex-end; }
  .mini-switch { display: inline-flex; align-items: center; gap: 2px; padding: 3px; border: 1px solid var(--border-color); border-radius: var(--radius-pill); background: var(--bg-card); }
  .mini-btn { padding: 4px 10px; border: none; border-radius: var(--radius-pill); background: transparent; color: var(--text-secondary); font-size: 0.75rem; font-weight: 500; cursor: pointer; transition: background 0.15s, color 0.15s; }
  .mini-btn:hover { color: var(--text-primary); }
  .mini-btn.active { background: var(--accent); color: #fff; }

  /* Multi-select category dropdown (native <details> for free open/close). */
  .cat-multi { position: relative; }
  .cat-multi > summary { list-style: none; cursor: pointer; padding: 0.4rem 0.7rem; border: 1px solid var(--border-color); border-radius: var(--radius-pill); font-size: 0.8rem; background: var(--bg-card); color: var(--text-primary); user-select: none; white-space: nowrap; }
  .cat-multi > summary::-webkit-details-marker { display: none; }
  .cat-multi > summary::after { content: "▾"; margin-left: 0.4rem; font-size: 0.7rem; color: var(--text-secondary); }
  .cat-multi[open] > summary::after { content: "▴"; }
  .cat-multi-menu { position: absolute; right: 0; top: calc(100% + 4px); z-index: 30; width: 240px; max-height: 300px; overflow-y: auto; background: var(--bg-card); border: 1px solid var(--border-color); border-radius: 12px; box-shadow: var(--app-shadow); padding: 0.35rem; }
  .cat-multi-item { display: flex; align-items: center; gap: 0.55rem; padding: 0.4rem 0.5rem; border-radius: 8px; font-size: 0.82rem; color: var(--text-primary); cursor: pointer; }
  .cat-multi-item:hover { background: var(--bg-secondary); }
  .cat-multi-item input { accent-color: var(--accent); width: 15px; height: 15px; flex-shrink: 0; }
  .cat-multi-item span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .cat-multi-empty { font-size: 0.8rem; color: var(--text-secondary); padding: 0.4rem 0.5rem; }

  /* Moved page date picker — labelled break that controls the section below. */
  .section-divider { display: flex; align-items: center; gap: 0.85rem; margin: 1.9rem 0 1.1rem; flex-wrap: wrap; border-top: 1px solid var(--border-color); padding-top: 1.4rem; }
  .section-divider-label { font-family: "Bitter", Georgia, serif; font-size: 1rem; font-weight: 600; color: var(--text-primary); }

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

  .cashflow-card {
    background: var(--bg-card);
    border: 1px solid var(--border-color);
    border-radius: var(--radius-card, 12px);
    padding: 1rem 1.25rem;
    margin-top: 1.75rem;
    margin-bottom: 1.5rem;
    box-shadow: var(--app-shadow);
  }
  .cashflow-head { display: flex; justify-content: space-between; align-items: flex-start; gap: 1rem; }
  .cashflow-label { display: block; font-size: 0.9rem; font-weight: 700; color: var(--text-primary); }
  .cashflow-sub { display: block; font-size: 0.75rem; color: var(--text-secondary); margin-top: 0.1rem; }
  .cashflow-amount { font-family: "Bitter", Georgia, serif; font-size: 1.5rem; font-weight: 700; color: var(--pos); font-variant-numeric: tabular-nums; letter-spacing: -0.01em; white-space: nowrap; }
  .cashflow-amount.cashflow-neg { color: var(--neg); }
  .cashflow-breakdown { display: flex; gap: 1rem; font-size: 0.75rem; color: var(--text-secondary); margin-top: 0.35rem; font-variant-numeric: tabular-nums; }
  .cashflow-bills { margin-top: 0.75rem; padding-top: 0.6rem; border-top: 1px solid var(--border-color); display: flex; flex-direction: column; gap: 0.3rem; }
  .cashflow-bill { display: grid; grid-template-columns: 6rem 1fr auto; align-items: center; gap: 0.5rem; font-size: 0.8rem; }
  .cashflow-bill-date { color: var(--text-secondary); }
  .cashflow-bill-name { color: var(--text-primary); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .cashflow-bill-amount { color: var(--text-primary); font-variant-numeric: tabular-nums; }
  .cashflow-more { font-size: 0.75rem; color: var(--text-secondary); margin-top: 0.15rem; }

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

  .reports-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(320px, 1fr)); gap: 1.25rem; margin-top: 1.5rem; }
  .report-card { background: var(--bg-card); border: 1px solid var(--border-color); border-radius: var(--radius-card); padding: 1.25rem 1.5rem; box-shadow: var(--app-shadow); }
  .report-card h3 { font-size: 1rem; font-weight: 600; color: var(--text-primary); margin-bottom: 1rem; }
  .report-sub { font-size: 0.75rem; font-weight: 400; color: var(--text-secondary); }
  .report-list { display: flex; flex-direction: column; gap: 0.6rem; }
  .report-row { display: grid; grid-template-columns: 1fr 1.2fr auto; align-items: center; gap: 0.6rem; font-size: 0.82rem; }
  .report-name { color: var(--text-primary); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .report-bar-track { height: 7px; background: var(--bg-secondary); border-radius: 999px; overflow: hidden; }
  .report-bar { height: 100%; background: var(--c2, var(--accent)); border-radius: 999px; }
  .report-val { font-variant-numeric: tabular-nums; color: var(--text-primary); white-space: nowrap; }
  .mover-row { grid-template-columns: 1fr auto auto; }
  .mover-prev { font-size: 0.75rem; color: var(--text-secondary); font-variant-numeric: tabular-nums; white-space: nowrap; }
  .mover-delta { font-variant-numeric: tabular-nums; font-weight: 600; white-space: nowrap; }
  .mover-delta.up { color: var(--neg); }
  .mover-delta.down { color: var(--pos); }

  .assets-card { background: var(--bg-card); border: 1px solid var(--border-color); border-radius: var(--radius-card); padding: 1.5rem; box-shadow: var(--app-shadow); margin-top: 1.25rem; }
  .assets-card h3 { font-size: 1rem; font-weight: 600; color: var(--text-primary); margin-bottom: 0; }
  .assets-total { font-size: 1.1rem; font-weight: 700; color: var(--text-primary); font-variant-numeric: tabular-nums; }
  .assets-list { list-style: none; margin: 0 0 0.75rem; padding: 0; }
  .asset-row { display: flex; align-items: center; gap: 0.7rem; padding: 0.55rem 0; border-bottom: 1px solid var(--bg-secondary); }
  .asset-icon { font-size: 1.1rem; width: 1.6rem; text-align: center; }
  .asset-name { font-weight: 600; color: var(--text-primary); }
  .asset-type { font-size: 0.72rem; color: var(--text-secondary); text-transform: capitalize; background: var(--accent-soft); padding: 0.1rem 0.5rem; border-radius: var(--radius-pill); }
  .asset-value { margin-left: auto; font-weight: 600; font-variant-numeric: tabular-nums; color: var(--text-primary); }
  .asset-del { background: none; border: none; color: var(--text-muted); font-size: 1.2rem; line-height: 1; cursor: pointer; padding: 0 0.2rem; }
  .asset-del:hover { color: var(--neg); }
  .assets-networth { display: flex; justify-content: space-between; align-items: center; padding: 0.6rem 0.75rem; margin-bottom: 0.75rem; background: var(--bg-secondary); border-radius: 10px; font-size: 0.9rem; color: var(--text-secondary); }
  .assets-networth strong { font-size: 1.1rem; color: var(--text-primary); font-variant-numeric: tabular-nums; }
  .assets-empty { color: var(--text-secondary); font-size: 0.9rem; margin: 0 0 0.9rem; }
  .asset-form { display: flex; gap: 0.5rem; flex-wrap: wrap; align-items: center; }
  .asset-input { padding: 0.45rem 0.6rem; border: 1px solid var(--border-color); border-radius: 8px; font-size: 0.85rem; background: var(--bg-card); color: var(--text-primary); }
  .asset-input-name { flex: 1; min-width: 160px; }
  .asset-input-value { width: 130px; }
  .asset-error { color: var(--neg); font-size: 0.8rem; margin: 0.5rem 0 0; }

</style>