// Detects recurring charges (subscriptions, regular bills) by grouping
// expense transactions by merchant and looking for a regular cadence.

use crate::commands::categorise::normalize_desc;
use chrono::NaiveDate;
use serde::Serialize;
use sqlx::SqlitePool;
use std::collections::HashMap;
use tauri::State;

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
