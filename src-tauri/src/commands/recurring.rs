// Detects recurring charges (subscriptions, regular bills) by grouping
// expense transactions by merchant and looking for a regular cadence.

use crate::commands::categorise::normalize_desc;
use crate::models::RecurringCost;
use chrono::NaiveDate;
use serde::Serialize;
use sqlx::SqlitePool;
use std::collections::{HashMap, HashSet};
use tauri::State;

/// The cadence for something that happens once — school camp, a rego bill you
/// are not sure will repeat. Stored in the same `frequency` column as the
/// repeating cadences, so there is one list to read and one list to forecast.
pub const ONE_OFF: &str = "One-off";

/// Canonical payment frequencies and how many payments they amount to per
/// calendar month. Used to normalise a recurring cost to a monthly figure.
/// Keep the labels in sync with the frontend `<select>` options.
pub fn payments_per_month(frequency: &str) -> f64 {
    match frequency {
        // A one-off has no monthly equivalent. Zero, not the 1.0 fallback: it
        // must not inflate the "per month" total, and the pages that care show
        // its dated amount instead.
        ONE_OFF => 0.0,
        "Weekly" => 52.0 / 12.0,
        "Fortnightly" => 26.0 / 12.0,
        "Monthly" => 1.0,
        "Every 2 months" => 0.5,
        "Quarterly" => 1.0 / 3.0,
        "Half-yearly" => 1.0 / 6.0,
        "Yearly" => 1.0 / 12.0,
        // Unknown cadence: treat as monthly rather than zeroing the cost.
        _ => 1.0,
    }
}

#[derive(Debug, Serialize)]
pub struct RecurringItem {
    pub description: String,
    pub category: String,
    /// The id as well as the name, so adding a detected item to the tracked
    /// list can carry its category over instead of matching on text.
    pub category_id: Option<i64>,
    pub frequency: String,
    pub occurrences: i64,
    pub avg_amount: f64,
    pub monthly_cost: f64,
    pub last_date: String,
    /// `last_date` advanced by one cycle, rolled forward if that is still in
    /// the past — what the tracked item's next due date should be.
    pub next_due_date: String,
    /// True when an upcoming expense with the same merchant key is already
    /// tracked, so the UI can offer the rest and not re-add this one.
    pub already_tracked: bool,
}

fn classify(median_gap: f64) -> Option<(&'static str, f64)> {
    // Returns (label, payments-per-month) for a plausible cadence, else None.
    let per_month = 30.44 / median_gap;
    let label = match median_gap {
        g if (5.0..=10.0).contains(&g) => "Weekly",
        g if (11.0..=18.0).contains(&g) => "Fortnightly",
        g if (19.0..=45.0).contains(&g) => "Monthly",
        g if (46.0..=75.0).contains(&g) => "Every 2 months",
        g if (76.0..=135.0).contains(&g) => "Quarterly",
        g if (136.0..=225.0).contains(&g) => "Half-yearly",
        g if (300.0..=420.0).contains(&g) => "Yearly",
        _ => return None,
    };
    Some((label, per_month))
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = values.len();
    if n == 0 {
        0.0
    } else if n % 2 == 1 {
        values[n / 2]
    } else {
        (values[n / 2 - 1] + values[n / 2]) / 2.0
    }
}

#[tauri::command]
pub async fn get_recurring_transactions(
    pool: State<'_, SqlitePool>,
) -> Result<Vec<RecurringItem>, String> {
    let rows = sqlx::query_as::<_, (String, String, f64, Option<String>, Option<i64>)>(
        "SELECT t.date, t.description, t.debit, c.name, c.id \
         FROM transactions t \
         LEFT JOIN categories c ON t.category_id = c.id \
         WHERE t.debit > 0 \
           AND NOT EXISTS (SELECT 1 FROM categories xc WHERE xc.id = t.category_id AND xc.exclude_from_budget = 1) \
         ORDER BY t.date ASC",
    )
    .fetch_all(&*pool)
    .await
    .map_err(|e| format!("DB query error: {}", e))?;

    struct Entry {
        dates: Vec<NaiveDate>,
        amounts: Vec<f64>,
        sample_desc: String,
        category: Option<String>,
        category_id: Option<i64>,
    }
    let mut groups: HashMap<String, Entry> = HashMap::new();

    for (date_str, desc, debit, category, category_id) in rows {
        let key = normalize_desc(&desc);
        if key.is_empty() {
            continue;
        }
        let Ok(date) = NaiveDate::parse_from_str(&date_str, "%Y-%m-%d") else {
            continue;
        };
        let e = groups.entry(key).or_insert_with(|| Entry {
            dates: Vec::new(),
            amounts: Vec::new(),
            sample_desc: desc.clone(),
            category,
            category_id,
        });
        e.dates.push(date);
        e.amounts.push(debit);
    }

    // The merchant keys already in the tracked list, so a detected item that is
    // already an upcoming expense can be marked rather than offered again.
    let tracked_names = sqlx::query_scalar::<_, String>("SELECT name FROM recurring_costs")
        .fetch_all(&*pool)
        .await
        .map_err(|e| format!("DB query error: {}", e))?;
    let tracked: HashSet<String> = tracked_names
        .iter()
        .map(|n| normalize_desc(n))
        .filter(|k| !k.is_empty())
        .collect();

    let today = chrono::Local::now().naive_local().date();
    let mut items: Vec<RecurringItem> = Vec::new();
    for (key, entry) in groups.into_iter() {
        if entry.dates.len() < 3 {
            continue;
        }
        let mut dates = entry.dates;
        dates.sort();
        let gaps: Vec<f64> = dates
            .windows(2)
            .map(|w| (w[1] - w[0]).num_days() as f64)
            .filter(|d| *d > 0.0)
            .collect();
        if gaps.is_empty() {
            continue;
        }
        let median_gap = median(gaps);
        let Some((frequency, per_month)) = classify(median_gap) else {
            continue;
        };

        let occurrences = entry.amounts.len() as i64;
        let avg_amount = entry.amounts.iter().sum::<f64>() / entry.amounts.len() as f64;
        let last = *dates.last().unwrap();
        let last_date = last.format("%Y-%m-%d").to_string();

        // Where the next one falls: one cycle on from the last sighting, rolled
        // forward if the series has been quiet for longer than that. Reuses the
        // cadence arithmetic the bill projection already does.
        let mut next_due = crate::commands::cashflow::advance(last, frequency);
        let mut guard = 0;
        while next_due < today && guard < 1000 {
            next_due = crate::commands::cashflow::advance(next_due, frequency);
            guard += 1;
        }

        // Collapse whitespace in the sample description for display.
        let description = entry.sample_desc.split_whitespace().collect::<Vec<_>>().join(" ");

        items.push(RecurringItem {
            description,
            category: entry.category.unwrap_or_else(|| "Uncategorised".to_string()),
            category_id: entry.category_id,
            frequency: frequency.to_string(),
            occurrences,
            avg_amount,
            monthly_cost: avg_amount * per_month,
            last_date,
            next_due_date: next_due.format("%Y-%m-%d").to_string(),
            already_tracked: tracked.contains(&key),
        });
    }

    items.sort_by(|a, b| b.monthly_cost.partial_cmp(&a.monthly_cost).unwrap());
    Ok(items)
}

// -- Manually managed recurring costs / subscriptions (CRUD) --

type RecurringRow = (
    i64,            // id
    String,         // name
    f64,            // amount
    String,         // frequency
    Option<i64>,    // category_id
    Option<String>, // category_name
    Option<String>, // next_due_date
    Option<String>, // end_date
    bool,           // active
    Option<String>, // notes
    String,         // created_at
);

fn row_to_cost(r: RecurringRow) -> RecurringCost {
    let (id, name, amount, frequency, category_id, category_name, next_due_date, end_date, active, notes, created_at) = r;
    let monthly_cost = amount * payments_per_month(&frequency);
    RecurringCost {
        id,
        name,
        amount,
        frequency,
        category_id,
        category_name,
        next_due_date,
        end_date,
        active,
        notes,
        created_at,
        monthly_cost,
    }
}

const SELECT_RECURRING: &str = "SELECT r.id, r.name, r.amount, r.frequency, r.category_id, c.name, \
     r.next_due_date, r.end_date, r.active, r.notes, r.created_at \
     FROM recurring_costs r \
     LEFT JOIN categories c ON r.category_id = c.id";

/// A date must parse, and an end date must not sit before the next due date —
/// a cost that ends before it starts would silently never appear anywhere.
/// A one-off without a date can't be placed at all, so it is refused here
/// rather than quietly dropped by the projection.
fn validate_dates(
    frequency: &str,
    next_due_date: &Option<String>,
    end_date: &Option<String>,
) -> Result<(), String> {
    let parse = |d: &str, what: &str| {
        NaiveDate::parse_from_str(d, "%Y-%m-%d")
            .map_err(|_| format!("{} must be a date in YYYY-MM-DD form, got \"{}\"", what, d))
    };
    let due = match next_due_date.as_deref().filter(|d| !d.is_empty()) {
        Some(d) => Some(parse(d, "The due date")?),
        None => {
            if frequency == ONE_OFF {
                return Err("A one-off needs a date — that is the only thing that places it.".into());
            }
            None
        }
    };
    if let Some(end) = end_date.as_deref().filter(|d| !d.is_empty()) {
        let end = parse(end, "The end date")?;
        if let Some(due) = due {
            if end < due {
                return Err(format!(
                    "The end date ({}) is before the next due date ({}), so this would never fall due.",
                    end, due
                ));
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn list_recurring_costs(pool: State<'_, SqlitePool>) -> Result<Vec<RecurringCost>, String> {
    let rows = sqlx::query_as::<_, RecurringRow>(
        &format!("{SELECT_RECURRING} ORDER BY r.active DESC, r.name COLLATE NOCASE"),
    )
    .fetch_all(&*pool)
    .await
    .map_err(|e| format!("DB query error: {}", e))?;

    Ok(rows.into_iter().map(row_to_cost).collect())
}

#[tauri::command]
pub async fn create_recurring_cost(
    pool: State<'_, SqlitePool>,
    name: String,
    amount: f64,
    frequency: String,
    category_id: Option<i64>,
    next_due_date: Option<String>,
    end_date: Option<String>,
    active: Option<bool>,
    notes: Option<String>,
) -> Result<RecurringCost, String> {
    let active = active.unwrap_or(true);
    validate_dates(&frequency, &next_due_date, &end_date)?;
    sqlx::query(
        "INSERT INTO recurring_costs (name, amount, frequency, category_id, next_due_date, end_date, active, notes) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&name)
    .bind(amount)
    .bind(&frequency)
    .bind(category_id)
    .bind(&next_due_date)
    .bind(&end_date)
    .bind(active)
    .bind(&notes)
    .execute(&*pool)
    .await
    .map_err(|e| format!("DB insert error: {}", e))?;

    let id = sqlx::query_scalar::<_, i64>("SELECT last_insert_rowid()")
        .fetch_one(&*pool)
        .await
        .map_err(|e| format!("DB error: {}", e))?;

    let row = sqlx::query_as::<_, RecurringRow>(&format!("{SELECT_RECURRING} WHERE r.id = ?"))
        .bind(id)
        .fetch_one(&*pool)
        .await
        .map_err(|e| format!("DB query error: {}", e))?;

    Ok(row_to_cost(row))
}

#[tauri::command]
pub async fn update_recurring_cost(
    pool: State<'_, SqlitePool>,
    id: i64,
    name: String,
    amount: f64,
    frequency: String,
    category_id: Option<i64>,
    next_due_date: Option<String>,
    end_date: Option<String>,
    active: Option<bool>,
    notes: Option<String>,
) -> Result<RecurringCost, String> {
    let active = active.unwrap_or(true);
    validate_dates(&frequency, &next_due_date, &end_date)?;
    let affected = sqlx::query(
        "UPDATE recurring_costs \
         SET name = ?, amount = ?, frequency = ?, category_id = ?, next_due_date = ?, end_date = ?, \
             active = ?, notes = ? \
         WHERE id = ?",
    )
    .bind(&name)
    .bind(amount)
    .bind(&frequency)
    .bind(category_id)
    .bind(&next_due_date)
    .bind(&end_date)
    .bind(active)
    .bind(&notes)
    .bind(id)
    .execute(&*pool)
    .await
    .map_err(|e| format!("DB update error: {}", e))?
    .rows_affected();

    if affected == 0 {
        return Err(format!("Recurring cost {} not found", id));
    }

    let row = sqlx::query_as::<_, RecurringRow>(&format!("{SELECT_RECURRING} WHERE r.id = ?"))
        .bind(id)
        .fetch_one(&*pool)
        .await
        .map_err(|e| format!("DB query error: {}", e))?;

    Ok(row_to_cost(row))
}

#[tauri::command]
pub async fn delete_recurring_cost(pool: State<'_, SqlitePool>, id: i64) -> Result<(), String> {
    sqlx::query("DELETE FROM recurring_costs WHERE id = ?")
        .bind(id)
        .execute(&*pool)
        .await
        .map_err(|e| format!("DB delete error: {}", e))?;
    Ok(())
}
