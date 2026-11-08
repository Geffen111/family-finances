-- An upcoming expense can stop: a car loan has a final instalment, a
-- subscription is cancelled from a known date. Without this, every recurring
-- cost projected forever, which is wrong the moment the forecast reads them.
ALTER TABLE recurring_costs ADD COLUMN end_date TEXT;
