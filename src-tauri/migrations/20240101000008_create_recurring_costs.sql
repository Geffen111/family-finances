CREATE TABLE IF NOT EXISTS recurring_costs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    amount REAL NOT NULL,
    frequency TEXT NOT NULL DEFAULT 'Monthly',
    category_id INTEGER REFERENCES categories(id) ON DELETE SET NULL,
    next_due_date TEXT,
    active INTEGER NOT NULL DEFAULT 1,
    notes TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
