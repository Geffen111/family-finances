<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { formatDate } from "$lib/format";
  import { showToast } from "$lib/stores/toast.svelte";

  interface RecurringCost {
    id: number;
    name: string;
    amount: number;
    frequency: string;
    kind: string;
    category_id: number | null;
    category_name: string | null;
    next_due_date: string | null;
    // The stored date rolled forward past any cycles that have already
    // happened — what to show, since nothing rewrites next_due_date on import.
    next_occurrence: string | null;
    end_date: string | null;
    active: boolean;
    notes: string | null;
    created_at: string;
    monthly_cost: number;
  }

  // A candidate the detector found in the transaction history, ready to be
  // added to the list above.
  interface DetectedItem {
    description: string;
    category: string;
    category_id: number | null;
    is_income: boolean;
    frequency: string;
    occurrences: number;
    avg_amount: number;
    monthly_cost: number;
    last_date: string;
    next_due_date: string;
    already_tracked: boolean;
    // Found by amount rather than by merchant name alone.
    fixed_amount: boolean;
  }

  // Items that look like one thing tracked more than once. The backend
  // decides what "look like" means (same merchant key, side, cadence and
  // amount); this page only shows them and lets you delete the spare.
  interface DuplicateGroup {
    name: string;
    kind: string;
    frequency: string;
    amount: number;
    items: RecurringCost[];
  }

  interface Category {
    id: number;
    path: string;
  }

  // Keep in sync with `payments_per_month` in src-tauri/src/commands/recurring.rs.
  // "One-off" is a cadence of one: a school camp or a one-time bill belongs in
  // this list too, and the forecast reads them all from the same place.
  const ONE_OFF = "One-off";
  // Keep in sync with KIND_EXPENSE / KIND_INCOME in
  // src-tauri/src/commands/recurring.rs.
  const EXPENSE = "expense";
  const INCOME = "income";
  const FREQUENCIES = [
    "Weekly",
    "Fortnightly",
    "Monthly",
    "Every 2 months",
    "Quarterly",
    "Half-yearly",
    "Yearly",
    ONE_OFF,
  ];

  let costs = $state<RecurringCost[]>([]);
  let categories = $state<Category[]>([]);
  let loading = $state(true);
  let duplicates = $state<DuplicateGroup[]>([]);
  let showDuplicates = $state(false);

  let showModal = $state(false);
  let editId = $state<number | null>(null);
  let formName = $state("");
  let formAmount = $state("");
  let formFrequency = $state("Monthly");
  let formKind = $state(EXPENSE);
  let formCategory = $state<string>("");
  let formDate = $state("");
  let formEndDate = $state("");
  let formActive = $state(true);
  let formNotes = $state("");

  let showDeleteConfirm = $state(false);
  let deleteId = $state(0);
  let deleteName = $state("");

  // Detection is a way to *populate* the list, not a second list: it opens on
  // demand, and what you keep from it becomes an ordinary tracked expense.
  let showScan = $state(false);
  let scanning = $state(false);
  let detected = $state<DetectedItem[]>([]);
  // Everyday spending is left out of the scan by default — a weekly shop is
  // regular without being a commitment — but it is a filter, so it can be
  // turned off rather than silently losing things.
  let includeEveryday = $state(false);
  let addedKeys = $state<Set<string>>(new Set());

  const currencyFormat = new Intl.NumberFormat("en-AU", { style: "currency", currency: "AUD" });
  function fmt(v: number): string { return currencyFormat.format(v); }

  // Local date, not toISOString(), which would shift to UTC and read as
  // yesterday for most of an Australian evening.
  function todayIso(): string {
    return new Date().toLocaleDateString("en-CA");
  }
  function addYearIso(): string {
    const d = new Date();
    d.setFullYear(d.getFullYear() + 1);
    return d.toLocaleDateString("en-CA");
  }

  // How many rows you could remove, not how many groups there are: "2 items
  // look like duplicates" is the number you act on.
  let duplicateExtras = $derived(
    duplicates.reduce((n, g) => n + g.items.length - 1, 0),
  );

  let isOneOff = $derived(formFrequency === ONE_OFF);
  let isIncomeForm = $derived(formKind === INCOME);
  // The only hard requirement a one-off adds: a date is the whole of what
  // places it, so saving without one is refused by the backend as well.
  let canSave = $derived(
    formName.trim().length > 0 && formAmount !== "" && (!isOneOff || formDate !== ""),
  );

  function hasEnded(c: RecurringCost): boolean {
    return c.end_date != null && c.end_date < todayIso();
  }
  function isSpent(c: RecurringCost): boolean {
    // A one-off whose date has passed has happened; it is history, not a plan.
    return c.frequency === ONE_OFF && c.next_due_date != null && c.next_due_date < todayIso();
  }
  // Still expected to cost something from here on.
  function isLive(c: RecurringCost): boolean {
    return c.active && !hasEnded(c) && !isSpent(c);
  }

  // Each side on its own: netting them into one number would hide both.
  let monthlyTotal = $derived(
    costs
      .filter((c) => isLive(c) && c.frequency !== ONE_OFF && c.kind !== INCOME)
      .reduce((s, c) => s + c.monthly_cost, 0),
  );
  let monthlyIncomeTotal = $derived(
    costs
      .filter((c) => isLive(c) && c.frequency !== ONE_OFF && c.kind === INCOME)
      .reduce((s, c) => s + c.monthly_cost, 0),
  );
  let annualTotal = $derived(monthlyTotal * 12);
  // One-offs have no monthly figure, so they are summed over the year ahead
  // rather than folded into a per-month average they would distort.
  let oneOffTotal = $derived(
    costs
      .filter(
        (c) =>
          c.active &&
          c.frequency === ONE_OFF &&
          c.kind !== INCOME &&
          c.next_due_date != null &&
          c.next_due_date >= todayIso() &&
          c.next_due_date <= addYearIso(),
      )
      .reduce((s, c) => s + c.amount, 0),
  );

  $effect(() => { load(); });

  async function load() {
    // Only the first load blanks the page. Replacing the grid with one line
    // of "Loading..." collapses the scroll container, so the browser clamps
    // the scroll to the top -- which is why saving an edit used to throw you
    // back up there. A refresh keeps what is on screen until the new data
    // replaces it.
    loading = costs.length === 0;
    try {
      const [c, cats, dupes] = await Promise.all([
        invoke<RecurringCost[]>("list_recurring_costs"),
        invoke<Category[]>("get_categories"),
        invoke<DuplicateGroup[]>("find_duplicate_costs"),
      ]);
      costs = c;
      categories = cats;
      duplicates = dupes;
    } catch (e) {
      showToast(String(e), "error");
    } finally {
      loading = false;
    }
  }

  function openAdd() {
    editId = null;
    formName = "";
    formAmount = "";
    formFrequency = "Monthly";
    formKind = EXPENSE;
    formCategory = "";
    formDate = "";
    formEndDate = "";
    formActive = true;
    formNotes = "";
    showModal = true;
  }

  function openEdit(c: RecurringCost) {
    editId = c.id;
    formName = c.name;
    formAmount = String(c.amount);
    formFrequency = c.frequency;
    formKind = c.kind;
    formCategory = c.category_id == null ? "" : String(c.category_id);
    formDate = c.next_due_date ?? "";
    formEndDate = c.end_date ?? "";
    formActive = c.active;
    formNotes = c.notes ?? "";
    showModal = true;
  }

  async function handleSave() {
    if (!canSave) return;
    const args = {
      name: formName.trim(),
      amount: parseFloat(formAmount),
      frequency: formFrequency,
      kind: formKind,
      categoryId: formCategory ? parseInt(formCategory, 10) : null,
      nextDueDate: formDate || null,
      // A one-off ends when it happens; an end date there would only confuse.
      endDate: isOneOff ? null : formEndDate || null,
      active: formActive,
      notes: formNotes.trim() || null,
    };
    try {
      if (editId == null) {
        await invoke("create_recurring_cost", args);
      } else {
        await invoke("update_recurring_cost", { id: editId, ...args });
      }
      showModal = false;
      await load();
      showToast("Saved.", "success");
    } catch (e) {
      showToast(String(e), "error");
    }
  }

  function openDelete(c: RecurringCost) {
    deleteId = c.id;
    deleteName = c.name;
    showDeleteConfirm = true;
  }

  async function handleDelete() {
    try {
      await invoke("delete_recurring_cost", { id: deleteId });
      showDeleteConfirm = false;
      await load();
      // Deleting the last spare empties the review, so close it rather than
      // leaving an empty dialog in the way.
      if (duplicates.length === 0) showDuplicates = false;
      showToast(`Deleted "${deleteName}".`, "success");
    } catch (e) {
      showToast(String(e), "error");
    }
  }

  async function openScan() {
    showScan = true;
    addedKeys = new Set();
    await runScan();
  }

  async function runScan() {
    scanning = true;
    try {
      detected = await invoke<DetectedItem[]>("get_recurring_transactions", {
        includeEveryday,
      });
    } catch (e) {
      showToast(String(e), "error");
      detected = [];
    } finally {
      scanning = false;
    }
  }

  // Take a detected candidate at face value and track it. Everything about it
  // is editable afterwards on its card — the point here is one click per item.
  async function addDetected(d: DetectedItem) {
    try {
      await invoke("create_recurring_cost", {
        name: d.description,
        amount: Math.round(d.avg_amount * 100) / 100,
        frequency: d.frequency,
        kind: d.is_income ? INCOME : EXPENSE,
        categoryId: d.category_id,
        nextDueDate: d.next_due_date,
        endDate: null,
        active: true,
        notes: `Detected from ${d.occurrences} transactions`,
      });
      markAdded(d);
      await load();
    } catch (e) {
      const msg = String(e);
      // The backend refuses a row identical to one already tracked. In here
      // that isn't a failure — what you wanted tracked is tracked — so mark
      // the row and say so, rather than flashing a red error at you.
      if (msg.includes("already in the list")) {
        markAdded(d);
        showToast(`"${d.description}" is already in your list.`);
        await load();
        return;
      }
      showToast(msg, "error");
    }
  }

  function markAdded(d: DetectedItem) {
    const next = new Set(addedKeys);
    next.add(detectedKey(d));
    addedKeys = next;
  }

  // A merchant can appear on both sides (paid and refunded), so the key that
  // marks a row "Added" has to carry the side too.
  function detectedKey(d: DetectedItem): string {
    return `${d.is_income ? "in" : "out"}:${d.description}:${d.avg_amount.toFixed(2)}`;
  }

  let scanCandidates = $derived(detected.filter((d) => !d.already_tracked));
</script>

<svelte:window onkeydown={(e) => { if (e.key !== "Escape") return; showModal = false; showDeleteConfirm = false; showScan = false; showDuplicates = false; }} />

<div class="page">
  <div class="header">
    <div>
      <h1>Upcoming Income &amp; Expenses</h1>
      {#if !loading && costs.length > 0}
        <p class="subtitle">
          {fmt(monthlyTotal)} out / month · {fmt(annualTotal)} / year
          {#if monthlyIncomeTotal > 0}· {fmt(monthlyIncomeTotal)} in / month{/if}
          {#if oneOffTotal > 0}· plus {fmt(oneOffTotal)} of one-offs in the year ahead{/if}
        </p>
      {/if}
    </div>
    <div class="header-actions">
      <button class="btn" onclick={openScan}>Scan transactions</button>
      <button class="btn btn-primary" onclick={openAdd}>Add</button>
    </div>
  </div>

  {#if !loading && duplicateExtras > 0}
    <div class="dup-note">
      <span>
        {duplicateExtras}
        {duplicateExtras === 1 ? "item looks like a duplicate" : "items look like duplicates"}
        of something already in the list.
      </span>
      <button class="btn btn-sm" onclick={() => { showDuplicates = true; }}>Review</button>
    </div>
  {/if}

  {#if loading}
    <p class="loading">Loading…</p>
  {:else if costs.length === 0}
    <div class="empty-state">
      <p>
        Nothing tracked yet. Add the money you know is coming and going — a quarterly rates
        notice, car insurance in March, next year's school camp, a fortnightly pay — or scan
        your transactions to find the regular ones automatically.
      </p>
      <div class="empty-actions">
        <button class="btn" onclick={openScan}>Scan transactions</button>
        <button class="btn btn-primary" onclick={openAdd}>Add</button>
      </div>
    </div>
  {:else}
    <div class="grid">
      {#each costs as c (c.id)}
        <div class="card" class:inactive={!isLive(c)} class:card-income={c.kind === INCOME}>
          <div class="card-top">
            <span class="card-name">{c.name}</span>
            <div class="card-actions">
              <button class="btn btn-sm" onclick={() => openEdit(c)}>Edit</button>
              <button class="btn btn-sm btn-danger" onclick={() => openDelete(c)}>Delete</button>
            </div>
          </div>
          <div class="card-figures">
            <span class="card-amount">{c.kind === INCOME ? "+" : ""}{fmt(c.amount)}</span>
            <span class="card-freq">{c.frequency}</span>
            {#if c.kind === INCOME}<span class="card-freq freq-income">Income</span>{/if}
          </div>
          <div class="card-foot">
            {#if c.frequency !== ONE_OFF}
              <span class="card-monthly">
                {c.kind === INCOME ? "+" : ""}{fmt(c.monthly_cost)}<span class="per">/mo</span>
              </span>
            {/if}
            {#if c.category_name}<span class="chip">{c.category_name}</span>{/if}
            {#if !c.active}<span class="chip chip-muted">Paused</span>{/if}
            {#if hasEnded(c)}<span class="chip chip-muted">Ended {formatDate(c.end_date)}</span>{/if}
            {#if isSpent(c)}<span class="chip chip-muted">{c.kind === INCOME ? "Received" : "Paid"}</span>{/if}
            {#if c.next_occurrence ?? c.next_due_date}
              <span class="card-due">
                {c.frequency === ONE_OFF ? "due" : "next"}
                {formatDate(c.next_occurrence ?? c.next_due_date)}
              </span>
            {/if}
            {#if c.end_date && !hasEnded(c)}
              <span class="card-due">until {formatDate(c.end_date)}</span>
            {/if}
          </div>
          {#if c.notes}<p class="card-notes">{c.notes}</p>{/if}
        </div>
      {/each}
    </div>
  {/if}
</div>

{#if showModal}
  <div class="modal-overlay" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) showModal = false; }}>
    <div class="modal" role="dialog" aria-modal="true" tabindex="-1">
      <h2>{editId == null ? "Add" : "Edit"} Upcoming {isIncomeForm ? "Income" : "Expense"}</h2>
      <label>Type
        <select bind:value={formKind}>
          <option value={EXPENSE}>Expense — money out</option>
          <option value={INCOME}>Income — money in</option>
        </select>
      </label>
      <label>Name<input type="text" bind:value={formName} placeholder={isIncomeForm ? "e.g. Pay" : "e.g. Car insurance"} /></label>
      <label>Amount<input type="number" bind:value={formAmount} placeholder="0.00" step="0.01" min="0" /></label>
      <label>Frequency
        <select bind:value={formFrequency}>
          {#each FREQUENCIES as f}<option value={f}>{f}</option>{/each}
        </select>
      </label>
      <label>Category (optional)
        <select bind:value={formCategory}>
          <option value="">— None —</option>
          {#each categories as cat}<option value={String(cat.id)}>{cat.path}</option>{/each}
        </select>
      </label>
      <label>
        {isOneOff ? "Date" : "Next due date (optional)"}
        <input type="date" bind:value={formDate} />
      </label>
      {#if isOneOff}
        <p class="hint">A one-off needs a date — that is what places it in the year.</p>
      {:else}
        <label>Ends after (optional)<input type="date" bind:value={formEndDate} /></label>
        <p class="hint">
          The last date this {isIncomeForm ? "is paid to you" : "falls due"}, if you know it —
          {isIncomeForm ? "the end of a contract" : "a car loan's final instalment"}.
        </p>
      {/if}
      <label>Notes (optional)<input type="text" bind:value={formNotes} placeholder="e.g. shared plan" /></label>
      <label class="checkbox"><input type="checkbox" bind:checked={formActive} /> Active</label>
      <div class="modal-actions">
        <button class="btn" onclick={() => { showModal = false; }}>Cancel</button>
        <button class="btn btn-primary" onclick={handleSave} disabled={!canSave}>Save</button>
      </div>
    </div>
  </div>
{/if}

{#if showScan}
  <div class="modal-overlay" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) showScan = false; }}>
    <div class="modal modal-lg" role="dialog" aria-modal="true" tabindex="-1">
      <h2>Found in your transactions</h2>
      <p class="hint">
        Money in or out at least three times on a regular cadence, seen within the last
        year. Everyday spending is skipped, and where one merchant line covers several
        subscriptions, each amount is offered separately. Adding one tracks it at its
        average amount — edit it afterwards if that isn't right.
      </p>
      {#if scanning}
        <p class="loading">Scanning…</p>
      {:else if detected.length === 0}
        <p class="loading">Nothing regular enough to spot yet. Three or more payments at a steady interval are needed.</p>
      {:else if scanCandidates.length === 0}
        <p class="loading">Every regular payment found is already tracked.</p>
      {:else}
        <label class="checkbox scan-filter">
          <input type="checkbox" bind:checked={includeEveryday} onchange={runScan} />
          Include everyday spending (food, retail, fuel, cash)
        </label>
        <div class="scan-list">
          {#each scanCandidates as d (detectedKey(d))}
            <div class="scan-row">
              <div class="scan-main">
                <span class="scan-name">
                  {d.description}
                  {#if d.is_income}<span class="scan-in">income</span>{/if}
                  {#if d.fixed_amount}<span class="scan-in" title="Found by amount: the same figure on the same rhythm, under a reference that changes every time">same amount</span>{/if}
                </span>
                <span class="scan-meta">
                  {d.frequency} · {d.is_income ? "+" : ""}{fmt(d.avg_amount)} avg ·
                  {d.occurrences} {d.is_income ? "payments in" : "payments"} ·
                  last {formatDate(d.last_date)} · {d.category}
                </span>
              </div>
              {#if addedKeys.has(detectedKey(d))}
                <span class="scan-added">Added</span>
              {:else}
                <button class="btn btn-sm btn-primary" onclick={() => addDetected(d)}>Add</button>
              {/if}
            </div>
          {/each}
        </div>
      {/if}
      <div class="modal-actions">
        <button class="btn" onclick={() => { showScan = false; }}>Done</button>
      </div>
    </div>
  </div>
{/if}

{#if showDuplicates}
  <div class="modal-overlay" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) showDuplicates = false; }}>
    <div class="modal modal-lg" role="dialog" aria-modal="true" tabindex="-1">
      <h2>Possible duplicates</h2>
      <p class="modal-intro">
        These agree on merchant, amount and cadence, so they are probably one thing
        tracked twice — every copy is counted, so a spare quietly inflates your totals
        and your forecast. Check the dates and notes before deleting: two real items
        can look alike.
      </p>
      <div class="scan-list">
        {#each duplicates as g (g.name + g.amount + g.frequency + g.kind)}
          <div class="dup-group">
            <div class="dup-head">
              {g.name}
              {#if g.kind === INCOME}<span class="scan-in">income</span>{/if}
              <span class="dup-head-meta">{fmt(g.amount)} · {g.frequency} · {g.items.length} copies</span>
            </div>
            {#each g.items as c (c.id)}
              <div class="scan-row">
                <div class="scan-main">
                  <span class="scan-name">{c.name}</span>
                  <span class="scan-meta">
                    {c.next_due_date ? `next ${formatDate(c.next_due_date)}` : "no date"} ·
                    {c.category_name ?? "No category"}
                    {#if c.notes}· {c.notes}{/if}
                  </span>
                </div>
                <button class="btn btn-sm btn-danger" onclick={() => openDelete(c)}>Delete</button>
              </div>
            {/each}
          </div>
        {/each}
      </div>
      <div class="modal-actions">
        <button class="btn" onclick={() => { showDuplicates = false; }}>Done</button>
      </div>
    </div>
  </div>
{/if}

{#if showDeleteConfirm}
  <div class="modal-overlay" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) showDeleteConfirm = false; }}>
    <div class="modal modal-sm" role="dialog" aria-modal="true" tabindex="-1">
      <h2>Delete Upcoming Item</h2>
      <p>Delete <strong>{deleteName}</strong>?</p>
      <div class="modal-actions">
        <button class="btn" onclick={() => { showDeleteConfirm = false; }}>Cancel</button>
        <button class="btn btn-danger" onclick={handleDelete}>Delete</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .page { max-width: 1320px; margin: 0 auto; }
  .header { display: flex; justify-content: space-between; align-items: flex-start; margin-bottom: 1.25rem; gap: 1rem; }
  .header-actions { display: flex; gap: 0.5rem; flex-shrink: 0; }
  h1 { font-size: 1.75rem; font-weight: 700; color: var(--text-primary); }
  .subtitle { margin-top: 0.25rem; font-size: 0.9rem; color: var(--text-secondary); font-variant-numeric: tabular-nums; }
  h2 { font-size: 1.25rem; font-weight: 600; margin-bottom: 1rem; color: var(--text-primary); }
  .hint { font-size: 0.78rem; color: var(--text-secondary); margin: -0.4rem 0 0.9rem; }

  .loading { color: var(--text-secondary); padding: 2rem 0; }
  .empty-state { border: 2px dashed var(--border-color); border-radius: var(--radius-card); padding: 3rem 2rem; text-align: center; color: var(--text-secondary); }
  .empty-actions { display: flex; gap: 0.5rem; justify-content: center; margin-top: 1rem; }

  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(300px, 1fr)); gap: 1.25rem; }
  .card { background: var(--bg-card); border: 1px solid var(--border-color); border-radius: var(--radius-card); padding: 1.25rem; box-shadow: var(--app-shadow); }
  .card.inactive { opacity: 0.6; }
  .card-top { display: flex; justify-content: space-between; align-items: center; gap: 0.5rem; margin-bottom: 0.75rem; }
  .card-name { font-size: 1.05rem; font-weight: 700; color: var(--text-primary); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .card-actions { display: flex; gap: 0.4rem; flex-shrink: 0; }
  .card-figures { display: flex; align-items: baseline; gap: 0.5rem; margin-bottom: 0.6rem; }
  .card-amount { font-size: 1.4rem; font-weight: 700; color: var(--text-primary); font-variant-numeric: tabular-nums; }
  .card-freq { font-size: 0.7rem; text-transform: uppercase; letter-spacing: 0.04em; font-weight: 600; color: var(--text-secondary); background: var(--bg-secondary); border: 1px solid var(--border-color); border-radius: 4px; padding: 0.1rem 0.4rem; }
  .card-foot { display: flex; align-items: center; gap: 0.6rem; font-size: 0.78rem; color: var(--text-secondary); flex-wrap: wrap; }
  .card-monthly { font-weight: 700; color: var(--text-primary); font-variant-numeric: tabular-nums; }
  .per { font-size: 0.7rem; font-weight: 500; color: var(--text-secondary); }
  .chip { font-size: 0.72rem; background: var(--bg-secondary); border: 1px solid var(--border-color); border-radius: 999px; padding: 0.1rem 0.5rem; }
  .chip-muted { color: var(--text-muted); }
  /* Income reads as a different kind of row at a glance, without relying on
     the sign alone. */
  .card-income .card-amount, .card-income .card-monthly { color: var(--pos); }
  .freq-income { background: var(--accent-soft); }
  .scan-in { font-size: 0.68rem; text-transform: uppercase; letter-spacing: 0.04em; color: var(--text-secondary); background: var(--accent-soft); border-radius: 999px; padding: 0.05rem 0.4rem; margin-left: 0.3rem; }
  .card-due { font-size: 0.72rem; color: var(--text-secondary); }
  .card-notes { margin-top: 0.6rem; font-size: 0.78rem; color: var(--text-secondary); }

  .scan-filter { font-size: 0.8rem; color: var(--text-secondary); margin: 0 0 0.6rem; }
  .scan-list { display: flex; flex-direction: column; gap: 0.5rem; max-height: 50vh; overflow-y: auto; }
  .scan-row { display: flex; align-items: center; justify-content: space-between; gap: 0.75rem; padding: 0.6rem 0.75rem; border: 1px solid var(--border-color); border-radius: 10px; background: var(--bg-secondary); }
  .scan-main { min-width: 0; }
  .scan-name { display: block; font-weight: 600; color: var(--text-primary); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .scan-meta { display: block; font-size: 0.74rem; color: var(--text-secondary); margin-top: 0.15rem; font-variant-numeric: tabular-nums; }
  .scan-added { font-size: 0.76rem; color: var(--text-muted); flex-shrink: 0; }

  .dup-note { display: flex; align-items: center; justify-content: space-between; gap: 0.75rem; flex-wrap: wrap;
    margin-bottom: 1rem; padding: 0.6rem 0.85rem; border: 1px solid var(--border-color);
    border-radius: 10px; background: var(--bg-secondary); font-size: 0.82rem; color: var(--text-secondary); }
  .dup-group { display: flex; flex-direction: column; gap: 0.4rem; }
  .dup-group + .dup-group { margin-top: 0.75rem; padding-top: 0.75rem; border-top: 1px solid var(--border-color); }
  .dup-head { font-size: 0.82rem; font-weight: 600; color: var(--text-primary); }
  .dup-head-meta { font-weight: 400; color: var(--text-secondary); margin-left: 0.4rem; font-variant-numeric: tabular-nums; }
  .modal-intro { font-size: 0.8rem; color: var(--text-secondary); line-height: 1.45; margin: 0 0 0.9rem; }

  .modal-overlay { position: fixed; inset: 0; background: rgba(0,0,0,0.4); display: flex; align-items: center; justify-content: center; z-index: 100; }
  .modal { background: var(--bg-card); border-radius: var(--radius-card); padding: 1.5rem; width: 420px; max-width: 90vw; max-height: 90vh; overflow-y: auto; box-shadow: 0 20px 60px rgba(0,0,0,0.15); }
  .modal-sm { width: 360px; }
  .modal-lg { width: 640px; }
  .modal label { display: flex; flex-direction: column; gap: 0.3rem; font-size: 0.85rem; font-weight: 500; color: var(--text-primary); margin-bottom: 0.75rem; }
  .modal label.checkbox { flex-direction: row; align-items: center; gap: 0.5rem; }
  .modal input, .modal select { padding: 0.5rem 0.65rem; border: 1px solid var(--border-color); border-radius: 10px; font-size: 0.9rem; background: var(--bg-card); color: var(--text-primary); }
  .modal label.checkbox input { width: auto; }
  .modal-actions { display: flex; justify-content: flex-end; gap: 0.5rem; margin-top: 1.25rem; }
</style>
