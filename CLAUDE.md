# Family Finance — project guide

Local-first desktop app to replace a family-finances spreadsheet. Public repo (MIT):
`github.com/Geffen111/family-finances`. No secrets or financial data live in the repo —
the SQLite DB stays on the user's machine.

## Stack
- **Shell:** Tauri v2 (Rust backend, `src-tauri/`)
- **Frontend:** SvelteKit 5 + TypeScript + Chart.js (`src/routes/`), static adapter → `build/`
- **DB:** SQLite via `sqlx` (bundled), migrations in `src-tauri/migrations/`
- **AI:** OpenRouter (`deepseek/deepseek-v4-flash`) for categorisation + insights; key entered in Settings, stored in AppData
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

CI quirks that are load-bearing — don't "simplify" these away:
- The CI pnpm is security-wrapped: it hard-fails on esbuild's build script and re-runs
  `pnpm install` before every script. Mitigated by `pnpm rebuild esbuild` in the install
  step + `npm_config_verify_deps_before_run: "false"`. pnpm 11 no longer reads the
  package.json `pnpm` field — project settings (`onlyBuiltDependencies`, `overrides`, e.g.
  the `cookie` security pin) live in `pnpm-workspace.yaml`.
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
history merchant-key lookup + few-shot, then LLM), `recurring` (subscription detection),
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
- **Budget rollover:** `categories.rollover` flag; carryover is computed on the fly in
  `get_budget_status` (monthly_budget × months since the category's first txn − spend
  before the period), not stored.
- **Emoji in Svelte:** put emoji in markup (browser decodes `&#x...;`) OR use `"\u{1F4C8}"`
  inside `{}` expressions — a raw `"&#x...;"` string in an expression renders as literal text.
- **Modals:** overlay `role="presentation"` closes on `e.target === e.currentTarget`; inner
  panel `role="dialog" aria-modal tabindex="-1"`; Escape close via a top-level `<svelte:window>`.
- **Big transaction lists:** the transactions table renders a 200-row window
  (`displayedTransactions`, Load more / Show all) — rendering every row of a large account
  froze the UI (each row has an 89-option category `<select>`). Totals/selection still use the
  full filtered set. Account lists are also prefetched into a `txCache` for instant switching.
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
