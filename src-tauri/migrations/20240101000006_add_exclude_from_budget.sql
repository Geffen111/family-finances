-- Categories flagged here are excluded from income/expense/budget totals and
-- forecasting baselines (e.g. internal transfers between your own accounts,
-- which aren't real income or spending).
ALTER TABLE categories ADD COLUMN exclude_from_budget INTEGER NOT NULL DEFAULT 0;

UPDATE categories SET exclude_from_budget = 1 WHERE name = 'Transfer - internal';
