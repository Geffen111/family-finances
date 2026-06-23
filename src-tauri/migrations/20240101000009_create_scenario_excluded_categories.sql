-- Per-scenario forecast exclusions. A row here means the category is left out
-- of THIS scenario's projection only, independent of the global
-- categories.exclude_from_budget flag (which still applies everywhere).
CREATE TABLE IF NOT EXISTS scenario_excluded_categories (
    scenario_id INTEGER NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
    category_id INTEGER NOT NULL REFERENCES categories(id) ON DELETE CASCADE,
    PRIMARY KEY (scenario_id, category_id)
);
