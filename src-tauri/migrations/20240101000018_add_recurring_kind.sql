-- Money coming in is the same shape of thing as money going out: a fortnightly
-- pay, a quarterly dividend, a one-off tax refund all have an amount, a
-- cadence and a date. One list, one expansion, one forecast path — with a side.
-- Everything already in the table is an expense, which the default preserves.
ALTER TABLE recurring_costs ADD COLUMN kind TEXT NOT NULL DEFAULT 'expense';
