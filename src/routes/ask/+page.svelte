<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  interface AskResponse {
    answer: string;
    sql: string;
    explanation: string;
    columns: string[];
    rows: string[][];
    truncated: boolean;
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

  // --- Ask a question ---
  let question = $state("");
  let asking = $state(false);
  let answer = $state<AskResponse | null>(null);
  let askError = $state("");
  let showWorking = $state(false);

  const EXAMPLES = [
    "How much did I spend on Amazon in the last 12 months?",
    "How much would I save if I stopped going to cafes?",
    "What were my 5 biggest expenses last month?",
    "How much did I spend on groceries this year?",
  ];

  async function ask() {
    const q = question.trim();
    if (!q || asking) return;
    asking = true;
    askError = "";
    answer = null;
    showWorking = false;
    try {
      answer = await invoke<AskResponse>("ask_question", { question: q });
    } catch (e) {
      askError = String(e);
    } finally {
      asking = false;
    }
  }

  function useExample(ex: string) {
    question = ex;
    ask();
  }

  // --- AI Insights ---
  type InsightsRange = "last1" | "last3" | "last6" | "last12" | "ytd";
  const RANGE_LABELS: Record<InsightsRange, string> = {
    last1: "Last month",
    last3: "Last 3 months",
    last6: "Last 6 months",
    last12: "Last 12 months",
    ytd: "Year to date",
  };

  let insightsRange = $state<InsightsRange>("last3");
  let insights = $state<SpendingInsights | null>(null);
  let insightsLoading = $state(false);
  let insightsError = $state("");
  let insightsGenerated = $state(false);

  function ymd(d: Date): string {
    const m = String(d.getMonth() + 1).padStart(2, "0");
    const day = String(d.getDate()).padStart(2, "0");
    return `${d.getFullYear()}-${m}-${day}`;
  }

  // Returns the start/end dates for the selected range. Omitting endDate lets
  // the backend default it to today.
  function insightsParams(): { startDate?: string; endDate?: string } {
    const t = new Date();
    switch (insightsRange) {
      case "last1":
        return {
          startDate: ymd(new Date(t.getFullYear(), t.getMonth() - 1, 1)),
          endDate: ymd(new Date(t.getFullYear(), t.getMonth(), 0)),
        };
      case "last3":
        return { startDate: ymd(new Date(t.getFullYear(), t.getMonth() - 2, 1)) };
      case "last6":
        return { startDate: ymd(new Date(t.getFullYear(), t.getMonth() - 5, 1)) };
      case "last12":
        return { startDate: ymd(new Date(t.getFullYear(), t.getMonth() - 11, 1)) };
      case "ytd":
        return { startDate: ymd(new Date(t.getFullYear(), 0, 1)) };
    }
  }

  // Changing the range invalidates the shown report; require a fresh generate.
  function onRangeChange() {
    insights = null;
    insightsGenerated = false;
    insightsError = "";
  }

  async function generateInsights() {
    insightsLoading = true;
    insightsError = "";
    try {
      insights = await invoke<SpendingInsights>("get_insights", insightsParams());
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
    try {
      insights = await invoke<SpendingInsights>("refresh_insights", insightsParams());
      insightsGenerated = true;
    } catch (e) {
      insightsError = String(e);
    } finally {
      insightsLoading = false;
    }
  }
</script>

<div class="page">
  <h1>Ask</h1>

  <section class="ask-section">
    <div class="ask-box">
      <textarea
        class="ask-input"
        bind:value={question}
        placeholder="Ask anything about your finances, e.g. “How much did I spend on Amazon in the last 12 months?”"
        rows="2"
        onkeydown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); ask(); } }}
      ></textarea>
      <button class="btn btn-primary ask-btn" onclick={ask} disabled={asking || !question.trim()}>
        {asking ? "Thinking…" : "Ask"}
      </button>
    </div>

    <div class="examples">
      {#each EXAMPLES as ex}
        <button class="example-chip" onclick={() => useExample(ex)} disabled={asking}>{ex}</button>
      {/each}
    </div>

    {#if asking}
      <div class="ask-loading">
        <div class="skeleton-line skeleton-line-lg"></div>
        <div class="skeleton-line skeleton-line-md"></div>
      </div>
    {:else if askError}
      <div class="error-state">
        <p>Couldn’t answer that.</p>
        <p class="error-detail">{askError}</p>
      </div>
    {:else if answer}
      <div class="answer-card">
        <p class="answer-text">{answer.answer}</p>
        <button class="working-toggle" onclick={() => (showWorking = !showWorking)}>
          {showWorking ? "▾" : "▸"} Show the working
        </button>
        {#if showWorking}
          <div class="working">
            {#if answer.explanation}<p class="working-explain">{answer.explanation}</p>{/if}
            <pre class="working-sql">{answer.sql}</pre>
            {#if answer.columns.length > 0}
              <div class="working-table-wrap">
                <table class="working-table">
                  <thead>
                    <tr>{#each answer.columns as col}<th>{col}</th>{/each}</tr>
                  </thead>
                  <tbody>
                    {#each answer.rows as row}
                      <tr>{#each row as cell}<td>{cell}</td>{/each}</tr>
                    {/each}
                  </tbody>
                </table>
                {#if answer.truncated}<p class="working-note">Showing the first 200 rows.</p>{/if}
                {#if answer.rows.length === 0}<p class="working-note">No rows returned.</p>{/if}
              </div>
            {/if}
          </div>
        {/if}
      </div>
    {/if}
  </section>

  <section class="insights-section">
    <div class="insights-header">
      <div>
        <h2>AI Insights</h2>
        <p class="insights-sub">Spending analysis for {RANGE_LABELS[insightsRange].toLowerCase()}.</p>
      </div>
      <div class="insights-actions">
        <select class="range-select" bind:value={insightsRange} onchange={onRangeChange} disabled={insightsLoading}>
          {#each Object.entries(RANGE_LABELS) as [value, label]}
            <option {value}>{label}</option>
          {/each}
        </select>
        {#if !insightsGenerated && !insightsLoading}
          <button class="btn btn-primary" onclick={generateInsights}>Generate Insights</button>
        {:else if insightsGenerated && !insightsLoading}
          <button class="btn" onclick={refreshInsights}>Refresh</button>
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
  </section>
</div>

<style>
  .page { max-width: 1320px; margin: 0 auto; }
  h1 { font-size: 1.75rem; font-weight: 700; color: var(--text-primary); margin-bottom: 1.25rem; }
  h2 { font-size: 1.25rem; font-weight: 700; color: var(--text-primary); margin: 0; }

  .btn { padding: 0.5rem 1rem; border: 1px solid var(--border-color); border-radius: 10px; background: var(--bg-card); color: var(--text-primary); font-size: 0.875rem; cursor: pointer; }
  .btn:hover { background: var(--bg-secondary); }
  .btn:disabled { opacity: 0.5; cursor: not-allowed; }
  .btn-sm { padding: 0.3rem 0.65rem; font-size: 0.8rem; }
  .btn-primary { background: var(--accent); color: #fff; border-color: var(--accent); }
  .btn-primary:hover { background: var(--accent); }

  /* Ask */
  .ask-section { margin-bottom: 2.5rem; }
  .ask-box { display: flex; gap: 0.75rem; align-items: stretch; }
  .ask-input { flex: 1; padding: 0.75rem 0.9rem; border: 1px solid var(--border-color); border-radius: 14px; font-size: 0.95rem; font-family: inherit; background: var(--bg-card); color: var(--text-primary); resize: vertical; }
  .ask-btn { flex-shrink: 0; align-self: stretch; padding-inline: 1.5rem; }
  .examples { display: flex; flex-wrap: wrap; gap: 0.5rem; margin-top: 0.75rem; }
  .example-chip { font-size: 0.78rem; color: var(--text-secondary); background: var(--bg-secondary); border: 1px solid var(--border-color); border-radius: 999px; padding: 0.35rem 0.75rem; cursor: pointer; }
  .example-chip:hover { background: var(--bg-card); color: var(--text-primary); }
  .example-chip:disabled { opacity: 0.5; cursor: not-allowed; }

  .ask-loading { display: flex; flex-direction: column; gap: 0.6rem; margin-top: 1.5rem; }
  .skeleton-line { background: var(--track); border-radius: 4px; height: 1rem; animation: pulse 1.5s infinite; }
  .skeleton-line-md { width: 50%; }
  .skeleton-line-lg { width: 85%; }
  @keyframes pulse { 0%, 100% { opacity: 1; } 50% { opacity: 0.4; } }

  .answer-card { margin-top: 1.5rem; background: var(--accent-soft); border: 1px solid var(--border-color); border-radius: 14px; padding: 1.25rem; }
  .answer-text { margin: 0; font-size: 1.05rem; color: var(--text-primary); line-height: 1.55; white-space: pre-wrap; }
  .working-toggle { margin-top: 0.85rem; background: none; border: none; color: var(--accent); font-size: 0.82rem; cursor: pointer; padding: 0; }
  .working { margin-top: 0.85rem; border-top: 1px solid var(--border-color); padding-top: 0.85rem; }
  .working-explain { margin: 0 0 0.5rem; font-size: 0.82rem; color: var(--text-secondary); }
  .working-sql { background: var(--bg-secondary); border: 1px solid var(--border-color); border-radius: 10px; padding: 0.6rem 0.75rem; font-size: 0.78rem; color: var(--text-primary); overflow-x: auto; white-space: pre-wrap; word-break: break-word; }
  .working-table-wrap { margin-top: 0.75rem; max-height: 320px; overflow: auto; border: 1px solid var(--border-color); border-radius: 10px; }
  .working-table { width: 100%; border-collapse: collapse; font-size: 0.78rem; }
  .working-table th { text-align: left; padding: 0.4rem 0.6rem; background: var(--bg-secondary); border-bottom: 1px solid var(--border-color); font-weight: 600; color: var(--text-primary); position: sticky; top: 0; }
  .working-table td { padding: 0.35rem 0.6rem; border-bottom: 1px solid var(--border-color); color: var(--text-primary); font-variant-numeric: tabular-nums; }
  .working-note { margin: 0.5rem 0 0; font-size: 0.75rem; color: var(--text-muted); }

  .error-state { text-align: center; padding: 2rem; color: #991b1b; background: #fee2e2; border: 1px solid #fecaca; border-radius: 14px; margin-top: 1.5rem; }
  .error-detail { font-size: 0.8rem; color: var(--text-secondary); margin: 0.5rem 0 1rem; word-break: break-all; }

  /* Insights */
  .insights-section { margin-top: 2.5rem; padding-top: 1.5rem; border-top: 1px solid var(--border-color); }
  .insights-header { display: flex; justify-content: space-between; align-items: flex-start; margin-bottom: 1rem; flex-wrap: wrap; gap: 0.5rem; }
  .insights-sub { font-size: 0.85rem; color: var(--text-secondary); margin-top: 0.25rem; }
  .insights-actions { display: flex; align-items: center; gap: 0.75rem; flex-wrap: wrap; }
  .range-select { padding: 0.45rem 0.6rem; border: 1px solid var(--border-color); border-radius: 10px; font-size: 0.85rem; background: var(--bg-card); color: var(--text-primary); cursor: pointer; }
  .range-select:disabled { opacity: 0.5; cursor: not-allowed; }

  .insights-loading { display: flex; flex-direction: column; gap: 1rem; }
  .insights-skeleton { background: var(--track); border-radius: 14px; animation: pulse 1.5s infinite; }
  .insight-summary-skeleton { width: 100%; height: 4rem; }
  .insight-card-skeleton { width: 100%; height: 6rem; }
  .insights-grid-skeleton { display: grid; grid-template-columns: repeat(auto-fill, minmax(300px, 1fr)); gap: 1rem; }

  .insights-error { text-align: center; padding: 2rem; color: #991b1b; background: #fee2e2; border: 1px solid #fecaca; border-radius: 14px; }

  .insights-summary-card { background: var(--accent-soft); border: 1px solid var(--border-color); border-radius: 14px; padding: 1.25rem; margin-bottom: 1.5rem; }
  .insights-summary-card p { margin: 0; font-size: 1rem; color: var(--text-primary); line-height: 1.5; }

  .insights-subheading { font-size: 1rem; font-weight: 600; color: var(--text-primary); margin: 1.5rem 0 0.75rem; }

  .insights-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(300px, 1fr)); gap: 1rem; margin-bottom: 0.5rem; }

  .insight-card { background: var(--bg-card); border: 1px solid var(--border-color); border-radius: 14px; padding: 1rem; display: flex; gap: 0.75rem; box-shadow: var(--app-shadow); border-left: 4px solid var(--border-color); }
  .insight-card.severity-positive { border-left-color: var(--pos); }
  .insight-card.severity-warning { border-left-color: var(--amber); }
  .insight-card.severity-critical { border-left-color: var(--neg); }
  .insight-card-recommendation { border-left-color: #8b5cf6; }
  .insight-card-recommendation.severity-positive { border-left-color: var(--pos); }
  .insight-card-recommendation.severity-warning { border-left-color: var(--amber); }
  .insight-card-recommendation.severity-critical { border-left-color: var(--neg); }

  .insight-icon { font-size: 1.5rem; flex-shrink: 0; width: 2rem; text-align: center; padding-top: 0.1rem; }
  .insight-body { min-width: 0; }
  .insight-title { font-weight: 600; font-size: 0.9rem; color: var(--text-primary); margin-bottom: 0.25rem; }
  .insight-detail { font-size: 0.8rem; color: var(--text-secondary); line-height: 1.4; }

  .insights-refresh-note { margin-top: 1rem; text-align: center; font-size: 0.8rem; color: var(--text-muted); display: flex; align-items: center; justify-content: center; gap: 0.5rem; }

  .insights-empty { border: 2px dashed var(--border-color); border-radius: 14px; padding: 2.5rem 2rem; text-align: center; color: var(--text-secondary); }
  .insights-empty .btn { margin-top: 0.75rem; }
</style>
