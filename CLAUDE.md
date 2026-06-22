# Family Finance — project guide

Local-first desktop app to replace a family-finances spreadsheet. Private repo:
`github.com/Geffen111/family-finances`.

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
`latest` GitHub Release (private). Download page:
`github.com/Geffen111/family-finances/releases/tag/latest` — `*_x64_*` for normal PCs,
`*_arm64_*` for Windows-on-ARM.

CI quirks that are load-bearing — don't "simplify" these away:
- The CI pnpm is security-wrapped: it ignores `onlyBuiltDependencies` and hard-fails on
  esbuild's build script, and re-runs `pnpm install` before every script. Mitigated by
  `pnpm rebuild esbuild` in the install step + `npm_config_verify_deps_before_run: "false"`.
- `rustflags: ""` on the Rust setup step (the action defaults to `-D warnings`, which would
  fail the build on any warning).
- ARM64 is a second `pnpm tauri build --target aarch64-pc-windows-msvc`; its bundles live
  under `src-tauri/target/aarch64-pc-windows-msvc/release/bundle/`.

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
`forecasting`, `insights`, `settings`, `export`. Register new commands in `src-tauri/src/lib.rs`.

## Conventions / gotchas
- **Currency:** AUD, `Intl.NumberFormat("en-AU")`. Dates stored `YYYY-MM-DD`.
- **`exclude_from_budget`** flag on categories keeps a category (e.g. internal transfers) out
  of all income/expense/budget/forecast aggregations — apply it via the NULL-safe
  `NOT EXISTS (... exclude_from_budget = 1)` guard, not a join.
- **`normalize_desc`** (`commands/categorise.rs`, `pub`) reduces a bank line to a stable
  merchant key; reused by recurring detection. Reuse it, don't reinvent.
- **Emoji in Svelte:** put emoji in markup (browser decodes `&#x...;`) OR use `"\u{1F4C8}"`
  inside `{}` expressions — a raw `"&#x...;"` string in an expression renders as literal text.
- **Modals:** overlay `role="presentation"` closes on `e.target === e.currentTarget`; inner
  panel `role="dialog" aria-modal tabindex="-1"`; Escape close via a top-level `<svelte:window>`.

## Known caveats
- **Net worth** (`get_net_worth_trend`) signs liabilities negative and carries forward each
  account's last monthly `balance`. If a bank stored a credit-card balance as negative, that
  account may read oddly — verify against real data; the sign convention is easy to flip.
- **Recurring detection** needs ≥3 occurrences and a regular cadence; thresholds are in
  `recurring.rs::classify`.

## Workflow
Commit + push are expected for finished changes (it's how the user gets a new installer).
Keep commits scoped per feature/fix; end commit messages with the Co-Authored-By trailer.
