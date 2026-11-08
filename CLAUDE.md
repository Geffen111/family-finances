# Family Finance — project guide

Local-first desktop app to replace a family-finances spreadsheet. Public repo (MIT):
`github.com/Geffen111/family-finances`. No secrets or financial data live in the repo —
the SQLite DB stays on the user's machine.

## Stack
- **Shell:** Tauri v2 (Rust backend, `src-tauri/`)
- **Frontend:** SvelteKit 5 + TypeScript + Chart.js (`src/routes/`), static adapter → `build/`
- **DB:** SQLite via `sqlx` (bundled), migrations in `src-tauri/migrations/`
- **AI:** OpenRouter for categorisation + insights + Ask; model picked in Settings
  (live `/models` list, default `deepseek/deepseek-v4-flash`) with an optional
  fallback model; key entered in Settings, stored in AppData
- **Package manager:** pnpm

## Verifying changes (do this before committing)
The Windows toolchain isn't on PATH by default in non-interactive shells:
```
# Rust backend (catches type errors without a full link):
PATH="$USERPROFILE/.cargo/bin:$PATH" RUSTFLAGS="" cargo check   # run in src-tauri/
# Frontend type-check (gate to keep green) and prod build:
PATH="/c/Program Files/nodejs:$APPDATA/npm:$PATH" pnpm check    # want 0 errors / 0 warnings
pnpm build                                                       # vite build, what CI runs
```
Backend tests: `cargo test --lib` in `src-tauri/` (in-memory SQLite, real migrations):
the forecast base-period maths and cash projection, `liquid_assets`, the category
rename modes, the upcoming-expense expansion (`expand_bills`), and the four
adjustment sources (which are easy to get quietly wrong). CI doesn't run them, so run them when touching that code.
`pnpm check` (svelte-check) does NOT gate the build, but keep it at 0/0. `link.exe` is
present locally, so `cargo check` works.

## Build & release (GitHub Actions → `.github/workflows/build.yml`)
Pushing to `main` builds **x64 and ARM64** installers and publishes them to a rolling
`latest` (public) GitHub Release, plus a `build-info.json` (the commit, via `GITHUB_SHA`).
Download page: `github.com/Geffen111/family-finances/releases/tag/latest` — `*_x64_*` for
normal PCs, `*_arm64_*` for Windows-on-ARM. Note: the workflow runs on *any* push to `main`
(including docs), so a README-only change still cuts a release.

The app's in-app "update available" banner (sidebar) compares the commit Vite stamps in
(`__APP_COMMIT__`, see `vite.config.js`) against `build-info.json`; needs the repo public.

A second workflow, `.github/workflows/audit.yml`, audits npm + Cargo advisories weekly
(and on any manifest/lockfile change). It is deliberately separate: an advisory published
against an untouched dependency shouldn't stop an installer being released. npm gates on
`--audit-level high`; `cargo audit` reads `src-tauri/.cargo/audit.toml`, which ignores
RUSTSEC-2023-0071 (`rsa` is in Cargo.lock via sqlx's optional MySQL driver and never
compiled — `cargo tree -i rsa` is empty).

CI quirks that are load-bearing — don't "simplify" these away:
- **esbuild's build script must stay approved** via `allowBuilds: { esbuild: true }` in
  `pnpm-workspace.yaml`. An unapproved (or placeholder) entry makes every `pnpm install`
  exit 1 with `ERR_PNPM_IGNORED_BUILDS`; pnpm 12 also blocks `pnpm rebuild` from working
  around it. CI pins `pnpm@12` (an unpinned install floated to 12 and broke the build) and
  runs a strict `pnpm install --frozen-lockfile`. `npm_config_verify_deps_before_run:
  "false"` stops pnpm re-running install before every script. pnpm 11+ doesn't read the
  package.json `pnpm` field — project settings (`allowBuilds`, `overrides`, e.g. the
  `cookie` security pin) live in `pnpm-workspace.yaml`.
- `rustflags: ""` on the Rust setup step (the action defaults to `-D warnings`, which would
  fail the build on any warning).
- ARM64 is a second `pnpm tauri build --target aarch64-pc-windows-msvc`; its bundles live
  under `src-tauri/target/aarch64-pc-windows-msvc/release/bundle/`.
- Actions are pinned to Node-24 majors (`checkout@v5`, `setup-node@v5`); `action-gh-release@v2`
  still warns about Node 20 until softprops ships a Node-24 release.

## Database
- Path: `%OneDrive%\Apps\FamilyFinance\finances.db` on Windows (so it syncs across devices),
  else `dirs::data_dir()/family-finances/`. See `src-tauri/src/db/mod.rs`.
- **Migrations are embedded** via `sqlx::migrate!("./migrations")` — never read them from disk
  at runtime (`CARGO_MANIFEST_DIR` only exists on the build machine; doing so crashed the
  installed app). The embedded migrator also runs multi-statement files correctly.
- Seeded accounts: Savings, Everyday Spending (assets), Home Loan, Credit Card (liabilities).

## Backend layout (`src-tauri/src/commands/`)
`import` (bank CSV, dedups), `historical` (one-time legacy spreadsheet import, preserves
categories/notes), `categories` (CRUD, bulk assign, exclude toggle), `categorise` (AI:
history merchant-key lookup + few-shot, then LLM), `recurring` (upcoming expenses: CRUD + subscription detection),
`dashboard` (summaries, spending, trends, budgets, net worth), `goals` (savings goals),
`forecasting`, `insights`, `settings`, `export`, `cashflow` (upcoming-bill
projection + "safe to spend"), `debt` (snowball/avalanche payoff sim, uses
`accounts.apr`/`min_payment`), `rules` (deterministic categorisation rules, run
on import before the AI), `tags` (transaction tags), `splits` (transaction
splitting). Register new commands in `src-tauri/src/lib.rs`.

## Conventions / gotchas
- **Currency:** AUD, `Intl.NumberFormat("en-AU")`. Dates stored `YYYY-MM-DD`.
- **`exclude_from_budget`** flag on categories keeps a category (e.g. internal transfers) out
  of all income/expense/budget/forecast aggregations — apply it via the NULL-safe
  `NOT EXISTS (... exclude_from_budget = 1)` guard, not a join.
- **AI categorisation shape:** `categorise_transactions` returns a `CategorisationRun`
  (suggestions + `history_matched`/`llm_categorised`/`llm_batches`/`elapsed_ms`/
  `llm_elapsed_ms`), and the UI shows that timing after each run — most rows are resolved
  locally by the history matcher, so a slow run means the AI leg is waiting on the
  provider. LLM batches are 25 transactions (bigger risks truncating the reply at
  `max_tokens: 4096`, which fails the run on a JSON parse error) and up to 4 run
  concurrently.
- **OpenRouter calls** all go through `services/openrouter.rs`. Never use a bare
  `reqwest::Client::new()` — it has no timeout, so a stalled provider hung the UI; one
  process-wide client (120s timeout) is shared, and `send_error` names a timeout as
  provider queueing. The model is **not** a constant: `model()` reads the `ai_model`
  setting (falling back to `DEFAULT_MODEL`) and `fallback_model()` the `ai_fallback_model`
  one. Call `chat_parsed` (or `chat_text`), which does the parse *inside* the attempt, so
  a reply that fails to parse counts as a failed attempt and the fallback model gets a go;
  it returns an `Answer<T>` carrying which model answered. Don't send a model id directly
  except via `chat_once`.
- **Reading back a row you just inserted:** take the id from the INSERT itself
  (`... RETURNING id` + `fetch_one`), never from a follow-up
  `SELECT last_insert_rowid()`. The pool holds **4 connections** and hands out
  whichever is free, so that second statement can run on a connection that has
  inserted nothing and answer `0` — the read-back then fails with *"no rows
  returned by a query that expected to return at least one row"*. It only bites
  once the pool has opened a second connection, which is why it looks
  intermittent and why nothing caught it for so long. A test that wants to catch
  it needs a *shared* in-memory DB
  (`sqlite:file:<name>?mode=memory&cache=shared`) with `min_connections(2)`:
  plain `sqlite::memory:` gives every connection its own database, which hides
  exactly this. (`result.last_insert_rowid()` off the insert's own `QueryResult`
  is fine — same statement, same connection.)
- **Settings file:** `commands/settings.rs` reads/writes `settings.json` through
  `load`/`read_str`/`write_str` — add a setting with a pair of commands over those, not
  another read-modify-write copy. `read_str` treats an empty string as unset.
- **`normalize_desc`** (`commands/categorise.rs`, `pub`) reduces a bank line to a stable
  merchant key; reused by recurring detection. Reuse it, don't reinvent.
- **`tx_effective` view** (migration 15) explodes split transactions into one row per
  split (split category + amount as `debit`) and passes non-split transactions through
  unchanged. **Category-grouped reporting must read `tx_effective`, not `transactions`**
  (spending by category/tree/trend, budget status + carryover, budget suggestions, top
  category, forecast baseline, category movers). Overall income/expense totals and any
  non-category aggregation still read the base `transactions` table (splits sum to the
  debit, so totals are identical). Splits are debit-only and validated to sum to the
  transaction's debit.
- **Per-page view state** lives in `src/lib/viewState.ts` (`loadView`/`saveView`/
  `clearView`, localStorage, one key per page). A page declares its defaults once,
  initialises each `$state` from the result, and writes back in a single `$effect` that
  reads them all. Only *view* state belongs there — filters, presets, sort, chart
  selections, expanded rows — never loaded data (the DB is the truth) and never modal
  open flags or spinners, which would restore the app mid-dialog. It's localStorage and
  not the DB on purpose: the DB syncs via OneDrive, and a filter left on one machine
  shouldn't follow you to another. JSON can't carry a `Set`, so Sets are enveloped as
  `{__set: [...]}` and rebuilt on load. Pages wired up: dashboard, transactions,
  forecasting, ask. When restoring a reference to a row (account, scenario), check it
  still exists and fall back — and give a filter the user can't see a way back
  (transactions has "Clear filters").
- **Cash & accounts** is one figure with one definition:
  `commands::cashflow::liquid_assets` (latest known balance per *asset* account, plus
  the date it's current as at). The dashboard's net-worth band, "safe to spend" and the
  forecast's cash projection all read it — don't re-derive it inline.
- **Forecast cash projection:** `ForecastMonth.projected_cash` is the opening balance
  plus a running sum of each month's net; `ForecastResult` carries `opening_cash` /
  `cash_as_at`. `compare_scenarios` resolves the opening balance **once** so the
  baseline and every scenario start from the same point. It gets its own chart on the
  forecasting page (a balance in the tens of thousands on the same axis as a monthly net
  in the low thousands flattens the net lines), and that chart must not clamp its y-axis
  at zero when a line goes negative — the month the money runs out is the point of it.
- **Renaming a category** takes a `rename_mode`: `update_history` (default — rename in
  place, so every past transaction reads as the new name, since transactions reference
  the category by id) or `keep_history` (leave the category and its transactions
  untouched and create a new category under the new name, which is what the command
  returns — so any follow-up flag calls must target *that* id). The logic is in
  `apply_category_edit` over a plain pool; the command is a thin wrapper, which is what
  makes it testable. Renaming in place also rewrites `transactions.ai_category`, which
  stores the AI's path as **text** and would otherwise quote a name that no longer
  exists — only where it still matches the old path, so hand-recategorised rows keep
  their history.
- **Upcoming income & expenses** (the page at `/recurring`, labelled "Upcoming" in the
  sidebar — the route is unchanged on purpose) is one list of dated money moving, in
  **both directions**: `recurring_costs.kind` is `'expense'` (the default, so everything
  that predates migration 18 is unchanged) or `'income'`. A fortnightly pay and a
  fortnightly bill differ only in sign, so they share the table, the expansion and the
  forecast path; an unknown kind is an error, since a mistyped one would silently
  subtract a salary. The list also holds repeating cadences *and* `One-off`, which is a
  real `frequency` value, not a separate table.
  `payments_per_month("One-off")` is **0**, not the 1.0 fallback, so a school camp can't
  inflate a per-month total; the pages show its dated amount instead. `end_date` is the
  last date a cost can fall on (a car loan's final instalment). Subscription detection
  (`get_recurring_transactions`) is a *populate* action behind "Scan transactions", not a
  second list: it returns `category_id` and a computed `next_due_date` so one click adds a
  candidate as a tracked expense, and `already_tracked` (merchant key via `normalize_desc`)
  keeps what you've added out of the offer.
- **`cashflow::expand_bills(pool, from, to)`** is the one place a recurring cost becomes
  dated occurrences: it rolls a stale `next_due_date` forward into the window, stops at
  `end_date`, and treats a `One-off` as exactly one occurrence that is *never* rolled
  forward (a past one-off is history, not a future bill). Each occurrence carries
  `is_income`. Plain pool, like `liquid_assets`, so the forecast can call it;
  `get_upcoming_bills` and "safe to spend" are thin callers. **"Safe to spend" counts
  expenses only** — `upcoming_income` is reported beside it and deliberately not added,
  because money you haven't been paid yet isn't safe to spend.
  Don't re-expand cadences inline — `advance` is `pub(crate)` for the same reason.
- **Adjustment source** (`scenario_adjustments.source`, migration 17) decides where a
  category's monthly figure comes from: `average` (the default, and what every scenario
  did before), `scheduled` (only the dated upcoming expenses linked to the category),
  `scheduled_plus_average`, or `scheduled_plus_fixed` (dated expenses plus `fixed_amount`
  as a *monthly extra* — the same column means "instead of the average" under `average`).
  The thing to understand before touching this: **a bill paid inside the base period is
  already in that category's average**, smeared over every month, so `scheduled` on its
  own is the right pick for an existing bill and the `plus` modes are for genuinely new
  ones. The forecasting page warns on `scheduled_plus_average` whenever the category has
  both history and linked expenses — *not* on the two figures being similar, which would
  stay quiet on exactly the common case (a yearly $1,200 bill in a six-month base period
  averages ~$200/mo against a scheduled ~$100/mo). The row's `adjustment_pct` scales the
  scheduled part; a fixed amount is never scaled. Unknown sources are an error, not a
  silent fall back to the average. `save_scenario_adjustment` only deletes a 0%/no-fixed
  row when the source is `average` — otherwise a scheduled category would silently revert.
- **Scheduled amounts in the forecast:** `services::forecast::scheduled_by_month` buckets
  `expand_bills` into `(category_id, is_income, "YYYY-MM")` once per run, before the month
  loop. The side is part of the key because baselines are split the same way — a scheduled
  pay lands on the category's income row and a bill on its expense row, so one category
  can hold both without them cancelling out. A category with no base-period activity has
  **no baseline row**, so `calculate_forecast` adds a zero-average row for any category on
  a scheduled source — the expense side always (a fixed extra has to land somewhere) and
  the income side only where something income-y is actually scheduled. Occurrences with no
  category can't be attributed to anything; `get_scheduled_amounts` returns them as
  `unlinked_total`/`unlinked_items` so the page can say so instead of losing them.
- **Forecast base period:** category averages divide by `base_period_months`
  (`services/forecast.rs`) — the period clamped to the imported data's date range, in days
  ÷ 30.44. Don't go back to counting whole months: `end_month − start_month` made
  1 Jan–30 Jun "5 months" and inflated every average by 20%. In scenario adjustments, 0% with
  no fixed amount deletes the row, so the category uses the scenario's default %.
- **Budget rollover:** `categories.rollover` flag; carryover is computed on the fly in
  `get_budget_status` (monthly_budget × months since the category's first txn − spend
  before the period), not stored.
- **Emoji in Svelte:** put emoji in markup (browser decodes `&#x...;`) OR use `"\u{1F4C8}"`
  inside `{}` expressions — a raw `"&#x...;"` string in an expression renders as literal text.
- **Modals:** overlay `role="presentation"` closes on `e.target === e.currentTarget`; inner
  panel `role="dialog" aria-modal tabindex="-1"`; Escape close via a top-level `<svelte:window>`.
- **Big transaction lists:** the transactions table renders a 200-row window
  (`displayedTransactions`, Load more / Show all). The per-row category picker is a
  `CategoryCombobox` (`src/lib/components`) — a button that opens a searchable, fixed-position
  menu — not a native `<select>`; the old select rendered all ~89 options for every row and
  froze large accounts, so a closed combobox now renders no option nodes. Totals/selection
  still use the full filtered set. Account lists are prefetched into a `txCache` for instant
  switching. (The bulk-bar, split and AI-review pickers stay native `<select>`s — single
  instances, no perf concern.)
- **Balance column** auto-hides when the selected account has no running balance
  (`hasBalance`, e.g. the credit card).
- **Sidebar** collapses to an icons-only rail (chevron toggle, persisted in `localStorage`).

## Known caveats
- **Net worth** (`get_net_worth_trend`) returns `assets` and `liabilities` per month
  separately (liability = positive amount owed); net_worth = assets − liabilities, carrying
  forward each account's last monthly `balance`. The dashboard chart shades assets up /
  liabilities down with a true net-worth line. Data gaps to know: the **Credit Card has no
  `balance` data** in its CSVs, so its liability currently counts as $0; the **Home Loan only
  has balances from ~Jun 2026**, so the line steps down when the loan first appears.
- **Recurring detection** needs ≥3 occurrences and a regular cadence; thresholds are in
  `recurring.rs::classify`.

## Workflow
Commit + push are expected for finished changes (it's how the user gets a new installer).
Keep commits scoped per feature/fix; end commit messages with the Co-Authored-By trailer.
