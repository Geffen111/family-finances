-- When a one-off actually happened. Repeating items move their next_due_date
-- on instead; a one-off has no next date, so being paid is the only thing
-- left to record. NULL means "not yet", which is how everything that predates
-- this migration reads -- and a one-off whose date has passed with no payment
-- against it is now visibly overdue rather than silently assumed done.
ALTER TABLE recurring_costs ADD COLUMN paid_on TEXT;
