// Forward-looking cash flow: projects upcoming bills from recurring costs and
// derives a "safe to spend" figure from current liquid balances. All offline —
// works on data already in the DB.

use chrono::{Duration, Months, NaiveDate};
use serde::Serialize;
use sqlx::SqlitePool;
use tauri::State;

#[derive(Debug, Serialize)]
pub struct UpcomingBill {
    pub name: String,
    pub amount: f64,
    pub due_date: String,
    pub category_name: Option<String>,
    pub frequency: String,
}

#[derive(Debug, Serialize)]
pub struct SafeToSpend {
    /// Sum of the latest known balance across asset accounts.
    pub liquid: f64,
    /// Total of bills due within the horizon.
    pub upcoming_total: f64,
    /// liquid − upcoming_total.
    pub safe_to_spend: f64,
    pub horizon_days: i64,
    pub bills: Vec<UpcomingBill>,
}

/// Advance a date by one cycle of `frequency`. Unknown cadence falls back to a
/// month so projection never stalls.
fn advance(date: NaiveDate, frequency: &str) -> NaiveDate {
    match frequency {
        "Weekly" => date + Duration::days(7),
        "Fortnightly" => date + Duration::days(14),
        "Every 2 months" => date.checked_add_months(Months::new(2)).unwrap_or(date),
        "Quarterly" => date.checked_add_months(Months::new(3)).unwrap_or(date),
        "Half-yearly" => date.checked_add_months(Months::new(6)).unwrap_or(date),
        "Yearly" => date.checked_add_months(Months::new(12)).unwrap_or(date),
        // "Monthly" and anything unrecognised.
        _ => date.checked_add_months(Months::new(1)).unwrap_or(date),
    }
}

/// Bills due between today and today+`days`, expanding each active recurring
/// cost across its cadence. A due date in the past is rolled forward to its
/// next future occurrence first.
#[tauri::command]
pub async fn get_upcoming_bills(
    pool: State<'_, SqlitePool>,
    days: Option<i64>,
) -> Result<Vec<UpcomingBill>, String> {
    let days = days.unwrap_or(30).clamp(1, 365);
    let today = chrono::Local::now().naive_local().date();
    let horizon = today + Duration::days(days);

    let rows = sqlx::query_as::<_, (String, f64, String, Option<String>, Option<String>)>(
        "SELECT r.name, r.amount, r.frequency, r.next_due_date, c.name \
         FROM recurring_costs r \
         LEFT JOIN categories c ON r.category_id = c.id \
         WHERE r.active = 1 AND r.next_due_date IS NOT NULL",
    )
    .fetch_all(&*pool)
    .await
    .map_err(|e| format!("DB query error: {}", e))?;

    let mut bills = Vec::new();
    for (name, amount, frequency, next_due, category_name) in rows {
        let Some(due_str) = next_due else { continue };
        let Ok(mut due) = NaiveDate::parse_from_str(&due_str, "%Y-%m-%d") else {
            continue;
        };
        // Roll a stale due date forward to the next future occurrence.
        let mut guard = 0;
        while due < today && guard < 1000 {
            due = advance(due, &frequency);
            guard += 1;
        }
        // Emit every occurrence inside the window.
        while due <= horizon && guard < 1000 {
            bills.push(UpcomingBill {
                name: name.clone(),
                amount,
                due_date: due.format("%Y-%m-%d").to_string(),
                category_name: category_name.clone(),
                frequency: frequency.clone(),
            });
            due = advance(due, &frequency);
            guard += 1;
        }
    }

    bills.sort_by(|a, b| a.due_date.cmp(&b.due_date));
    Ok(bills)
}

/// Liquid balance (latest balance of each asset account) minus bills due within
/// the horizon.
#[tauri::command]
pub async fn get_safe_to_spend(
    pool: State<'_, SqlitePool>,
    days: Option<i64>,
) -> Result<SafeToSpend, String> {
    let horizon_days = days.unwrap_or(30).clamp(1, 365);

    // Latest known balance per asset account. Liability accounts and
    // balance-less accounts (e.g. the credit card) are skipped.
    let liquid = sqlx::query_scalar::<_, Option<f64>>(
        "SELECT CAST(SUM(latest) AS REAL) FROM (
            SELECT (
                SELECT t.balance FROM transactions t
                WHERE t.account_id = a.id AND t.balance IS NOT NULL
                ORDER BY t.date DESC, t.id DESC LIMIT 1
            ) AS latest
            FROM accounts a
            WHERE a.type = 'asset'
         ) WHERE latest IS NOT NULL",
    )
    .fetch_one(&*pool)
    .await
    .map_err(|e| format!("DB query error: {}", e))?
    .unwrap_or(0.0);

    let bills = get_upcoming_bills(pool, Some(horizon_days)).await?;
    let upcoming_total: f64 = bills.iter().map(|b| b.amount).sum();

    Ok(SafeToSpend {
        liquid,
        upcoming_total,
        safe_to_spend: liquid - upcoming_total,
        horizon_days,
        bills,
    })
}
