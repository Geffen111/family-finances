CREATE TABLE IF NOT EXISTS transaction_splits (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    transaction_id INTEGER NOT NULL REFERENCES transactions(id) ON DELETE CASCADE,
    category_id INTEGER REFERENCES categories(id) ON DELETE SET NULL,
    amount REAL NOT NULL,
    notes TEXT
);

-- Category-attributed view: a transaction with splits contributes its split
-- rows (each its own category + amount as debit); a transaction without splits
-- contributes itself. Category-grouped reporting reads this so splits show up
-- per category, while overall income/expense totals still read transactions.
CREATE VIEW IF NOT EXISTS tx_effective AS
SELECT t.id, t.account_id, t.category_id, t.date, t.description, t.debit, t.credit
FROM transactions t
WHERE NOT EXISTS (SELECT 1 FROM transaction_splits s WHERE s.transaction_id = t.id)
UNION ALL
SELECT t.id, t.account_id, s.category_id, t.date, t.description, s.amount AS debit, 0 AS credit
FROM transactions t
JOIN transaction_splits s ON s.transaction_id = t.id;
