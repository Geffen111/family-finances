CREATE TABLE IF NOT EXISTS scenarios (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    description TEXT,
    horizon TEXT NOT NULL DEFAULT 'monthly',
    base_start_date TEXT NOT NULL,
    base_end_date TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS scenario_adjustments (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    scenario_id INTEGER NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
    category_id INTEGER NOT NULL REFERENCES categories(id),
    adjustment_pct REAL DEFAULT 0,
    fixed_amount REAL,
    UNIQUE(scenario_id, category_id)
);

CREATE TABLE IF NOT EXISTS scenario_defaults (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    scenario_id INTEGER NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
    default_adjustment_pct REAL DEFAULT 0,
    income_growth_pct REAL DEFAULT 0,
    UNIQUE(scenario_id)
);