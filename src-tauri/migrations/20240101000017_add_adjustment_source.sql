-- Where a category's monthly figure comes from in a scenario. 'average' is
-- what every existing scenario already does, so the default keeps them
-- behaving exactly as before.
--   average               the base-period average (or fixed_amount if set)
--   scheduled             only the dated upcoming expenses linked to it
--   scheduled_plus_average   both (double counts if the bill is in the history)
--   scheduled_plus_fixed     the dated expenses plus fixed_amount per month
ALTER TABLE scenario_adjustments ADD COLUMN source TEXT NOT NULL DEFAULT 'average';
