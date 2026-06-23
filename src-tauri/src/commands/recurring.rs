// Detects recurring charges (subscriptions, regular bills) by grouping
// expense transactions by merchant and looking for a regular cadence.

use crate::commands::categorise::normalize_desc;
use crate::models::RecurringCost;
use chrono::NaiveDate;
use serde::Serialize;
use sqlx::SqlitePool;
use std::collections::HashMap;
use tauri::State;

/// Canonical payment frequencies and how many payments they amount to per
/// calendar month. Used to normalise a recurring cost to a monthly figure.
/// Keep the labels in sync with the frontend `<select>` options.
pub fn payments_per_month(frequency: &str) -> f64 {
    match frequency {
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
    pub frequency: String,
    pub occurrences: i64,
    pub avg_amount: f64,
    pub monthly_cost: f64,
    pub last_date: String,
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
    let rows = sqlx::query_as::<_, (String, String, f64, Option<String>)>(
        "SELECT t.date, t.description, t.debit, c.name \
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
    }
    let mut groups: HashMap<String, Entry> = HashMap::new();

    for (date_str, desc, debit, category) in rows {
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
        });
        e.dates.push(date);
        e.amounts.push(debit);
    }

    let mut items: Vec<RecurringItem> = Vec::new();
    for entry in groups.into_values() {
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
        let last_date = dates.last().unwrap().format("%Y-%m-%d").to_string();

        // Collapse whitespace in the sample description for display.
        let description = entry.sample_desc.split_whitespace().collect::<Vec<_>>().join(" ");

        items.push(RecurringItem {
            description,
            category: entry.category.unwrap_or_else(|| "Uncategorised".to_string()),
            frequency: frequency.to_string(),
            occurrences,
            avg_amount,
            monthly_cost: avg_amount * per_month,
            last_date,
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
    bool,           // active
    Option<String>, // notes
    String,         // created_at
);

fn row_to_cost(r: RecurringRow) -> RecurringCost {
    let (id, name, amount, frequency, category_id, category_name, next_due_date, active, notes, created_at) = r;
    let monthly_cost = amount * payments_per_month(&frequency);
    RecurringCost {
        id,
        name,
        amount,
        frequency,
        category_id,
        category_name,
        next_due_date,
        active,
        notes,
        created_at,
        monthly_cost,
    }
}

const SELECT_RECURRING: &str = "SELECT r.id, r.name, r.amount, r.frequency, r.category_id, c.name, \
     r.next_due_date, r.active, r.notes, r.created_at \
     FROM recurring_costs r \
     LEFT JOIN categories c ON r.category_id = c.id";

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
    active: Option<bool>,
    notes: Option<String>,
) -> Result<RecurringCost, String> {
    let active = active.unwrap_or(true);
    sqlx::query(
        "INSERT INTO recurring_costs (name, amount, frequency, category_id, next_due_date, active, notes) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&name)
    .bind(amount)
    .bind(&frequency)
    .bind(category_id)
    .bind(&next_due_date)
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
    active: Option<bool>,
    notes: Option<String>,
) -> Result<RecurringCost, String> {
    let active = active.unwrap_or(true);
    let affected = sqlx::query(
        "UPDATE recurring_costs \
         SET name = ?, amount = ?, frequency = ?, category_id = ?, next_due_date = ?, active = ?, notes = ? \
         WHERE id = ?",
    )
    .bind(&name)
    .bind(amount)
    .bind(&frequency)
    .bind(category_id)
    .bind(&next_due_date)
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
