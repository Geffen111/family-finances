# Family Finance

A local-first desktop app for tracking a household's money — accounts, spending, budgets, net worth and forecasts — built with Tauri, SvelteKit and Rust. Your data lives in a SQLite file on your own machine, not in the cloud.

> Personal project, built for one household. Shared publicly as a working example of a Tauri + SvelteKit + sqlx app; not a polished product or financial advice.

## Features

- **CSV import** per account, with duplicate-row detection so re-importing an overlapping statement won't double up.
- **Multiple accounts** typed as assets or liabilities (savings, everyday, credit card, home loan…), with the ability to move transactions between accounts.
- **Transactions** view with search, sort, bulk re-categorise, a searchable category picker, **tags** and **transaction splitting** (one payment across several categories). A windowed table plus concurrent SQLite reads keep it responsive on accounts with thousands of rows, including instant switching between accounts.
- **AI categorisation** (optional) via [OpenRouter](https://openrouter.ai/) — suggests categories for unclear transactions and **learns from your own past choices** (by merchant, and by account number + amount for contextless transfers). High-confidence matches auto-apply; the rest you review. Deterministic rules run first on import, before the AI.
- **Dashboard** — spending breakdown, monthly income vs expenses, category trends, and a **net worth over time** chart that shades assets vs. liabilities (manually-tracked assets like the family home included, debts subtracted).
- **Assets & investments** card feeding into net worth.
- **Budgets** per category (with optional rollover), **savings goals**, and **recurring / subscription detection**.
- **Cashflow** — upcoming-bill projection and a "safe to spend" figure.
- **Debt payoff planner** — snowball / avalanche simulation from each account's APR and minimum payment.
- **Forecasting** with adjustable scenarios.
- **Ask** — a natural-language query page (hybrid text-to-SQL) that answers questions about your data and shows its working.
- **Hearth** light/dark theme, configurable household name, and an in-app "update available" banner.

## Data & privacy

- Everything is stored in a local SQLite database (`finances.db`). On Windows it's kept under `%OneDrive%\Apps\FamilyFinance\` so it syncs across your own machines; otherwise it falls back to the platform data directory.
- Nothing is sent anywhere unless you opt into the AI features, which call OpenRouter using an API key you provide in Settings. No analytics, no accounts, no server.

## Install

Grab the latest Windows installer from the [releases page](https://github.com/Geffen111/family-finances/releases/latest):

- `*_x64-setup.exe` / `*_x64_*.msi` — Intel/AMD PCs
- `*_arm64-setup.exe` / `*_arm64_*.msi` — Windows on ARM

## Tech stack

- **Frontend:** SvelteKit + Svelte 5 (runes), TypeScript, Chart.js, `adapter-static`
- **Shell / backend:** Tauri 2, Rust
- **Storage:** SQLite via `sqlx` with embedded migrations
- **AI:** OpenRouter (model configurable)

## Development

Prerequisites: [Node.js](https://nodejs.org/) (≥ 22), [Rust](https://www.rust-lang.org/tools/install), [pnpm](https://pnpm.io/), and the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS (on Windows: the MSVC C++ build tools + WebView2).

```bash
pnpm install
pnpm tauri dev      # run the app with hot reload
pnpm tauri build    # produce installers for the current platform
```

Useful checks:

```bash
pnpm exec svelte-check          # frontend type/lint check
cargo check --manifest-path src-tauri/Cargo.toml
```

### Releases

Pushing to `main` triggers the GitHub Actions workflow ([`.github/workflows/build.yml`](.github/workflows/build.yml)), which builds x64 and ARM64 installers and publishes them to a rolling `latest` release, along with a `build-info.json` the app uses to detect updates.

> **Note on migrations:** `.sql` migration files are pinned to CRLF (`.gitattributes`) because sqlx checksums the file bytes; mixing line endings across machines causes a `VersionMismatch` at startup. Keep new migrations CRLF.

## License

[MIT](LICENSE).
