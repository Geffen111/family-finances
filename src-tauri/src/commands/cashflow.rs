// Forward-looking cash flow: projects upcoming bills from recurring costs and
// derives a "safe to spend" figure from current liquid balances. All offline —
// works on data already in the DB.

use crate::commands::recurring::ONE_OFF;
use chrono::{Duration, Months, NaiveDate};
use serde::Serialize;
use sqlx::SqlitePool;
use tauri::State;

#[derive(Debug, Serialize)]
pub struct UpcomingBill {
    pub name: String,
    pub amount: f64,
    pub due_date: String,
    /// Carried as well as the name so a caller can group by category without
    /// matching on text — the forecast needs the id.
    pub category_id: Option<i64>,
    pub category_name: Option<String>,
    pub frequency: String,
    /// True for money coming in. The expansion carries both sides; what each
    /// caller does with them differs.
    pub is_income: bool,
}

#[derive(Debug, Serialize)]
pub struct SafeToSpend {
    /// Sum of the latest known balance across asset accounts.
    pub liquid: f64,
    /// Total of bills due within the horizon.
    pub upcoming_total: f64,
    /// Expected income within the horizon. Reported but deliberately NOT
    /// added to `safe_to_spend`: money you haven't been paid yet isn't safe
    /// to spend, and the figure's whole job is to be the conservative one.
    pub upcoming_income: f64,
    /// liquid − upcoming_total.
    pub safe_to_spend: f64,
    pub horizon_days: i64,
    pub bills: Vec<UpcomingBill>,
}

/// Advance a date by one cycle of `frequency`. Unknown cadence falls back to a
/// month so projection never stalls. "One-off" has no next cycle and is never
/// passed here — `expand_bills` handles it before this is called.
pub(crate) fn advance(date: NaiveDate, frequency: &str) -> NaiveDate {
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

/// Every dated occurrence of the active upcoming expenses that falls in
/// `from..=to`, expanding each one across its cadence.
///
/// A due date before `from` is rolled forward to the first occurrence inside
/// the window, so a cost whose date hasn't been touched for months still lands
/// where it belongs. A cost with an `end_date` stops there — a car loan has a
/// final instalment. A "One-off" has exactly one occurrence and is not rolled
/// forward: once its date has passed it is history, not a future bill.
///
/// Plain pool rather than `State` so the forecast can call it too (same reason
/// as `liquid_assets` below).
pub async fn expand_bills(
    pool: &SqlitePool,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Vec<UpcomingBill>, String> {
    type Row = (
        String,         // name
        f64,            // amount
        String,         // frequency
        Option<String>, // next_due_date
        Option<String>, // end_date
        Option<i64>,    // category_id
        Option<String>, // category_name
        String,         // kind
    );
    let rows = sqlx::query_as::<_, Row>(
        "SELECT r.name, r.amount, r.frequency, r.next_due_date, r.end_date, r.category_id, c.name, r.kind \
         FROM recurring_costs r \
         LEFT JOIN categories c ON r.category_id = c.id \
         WHERE r.active = 1 AND r.next_due_date IS NOT NULL",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("DB query error: {}", e))?;

    let mut bills = Vec::new();
    for (name, amount, frequency, next_due, end_date, category_id, category_name, kind) in rows {
        let is_income = kind == crate::commands::recurring::KIND_INCOME;
        let Some(due_str) = next_due else { continue };
        let Ok(mut due) = NaiveDate::parse_from_str(&due_str, "%Y-%m-%d") else {
            continue;
        };
        let ends = end_date
            .as_deref()
            .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok());

        let mut emit = |due: NaiveDate| {
            bills.push(UpcomingBill {
                name: name.clone(),
                amount,
                due_date: due.format("%Y-%m-%d").to_string(),
                category_id,
                category_name: category_name.clone(),
                frequency: frequency.clone(),
                is_income,
            });
        };

        if frequency == ONE_OFF {
            if due >= from && due <= to && ends.is_none_or(|e| due <= e) {
                emit(due);
            }
            continue;
        }

        // Roll a stale due date forward to the first occurrence in the window.
        let mut guard = 0;
        while due < from && guard < 1000 {
            due = advance(due, &frequency);
            guard += 1;
        }
        // Then emit every occurrence inside it, up to the end date if there is one.
        while due <= to && ends.is_none_or(|e| due <= e) && guard < 1000 {
            emit(due);
            due = advance(due, &frequency);
            guard += 1;
        }
    }

    bills.sort_by(|a, b| a.due_date.cmp(&b.due_date));
    Ok(bills)
}

/// Bills due between today and today+`days`.
#[tauri::command]
pub async fn get_upcoming_bills(
    pool: State<'_, SqlitePool>,
    days: Option<i64>,
) -> Result<Vec<UpcomingBill>, String> {
    let days = days.unwrap_or(30).clamp(1, 365);
    let today = chrono::Local::now().naive_local().date();
    expand_bills(&pool, today, today + Duration::days(days)).await
}

/// The "Cash & accounts" figure: the latest known balance of every asset
/// account, summed, with the date it is current as at. Liability accounts and
/// balance-less ones (the credit card's CSVs carry no balance) are skipped.
///
/// One definition, three users: the dashboard's net-worth band, "safe to
/// spend" below, and the forecast's cash projection all have to agree, or the
/// same number appears differently on three pages.
pub async fn liquid_assets(pool: &SqlitePool) -> Result<(f64, Option<String>), String> {
    let row = sqlx::query_as::<_, (Option<f64>, Option<String>)>(
        "SELECT CAST(SUM(latest) AS REAL), MAX(as_at) FROM (
            SELECT
              (SELECT t.balance FROM transactions t
                 WHERE t.account_id = a.id AND t.balance IS NOT NULL
                 ORDER BY t.date DESC, t.id DESC LIMIT 1) AS latest,
              (SELECT t.date FROM transactions t
                 WHERE t.account_id = a.id AND t.balance IS NOT NULL
                 ORDER BY t.date DESC, t.id DESC LIMIT 1) AS as_at
            FROM accounts a
            WHERE a.type = 'asset'
         ) WHERE latest IS NOT NULL",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| format!("DB query error: {}", e))?;

    Ok((row.0.unwrap_or(0.0), row.1))
}

/// Liquid balance (latest balance of each asset account) minus bills due within
/// the horizon.
#[tauri::command]
pub async fn get_safe_to_spend(
    pool: State<'_, SqlitePool>,
    days: Option<i64>,
) -> Result<SafeToSpend, String> {
    let horizon_days = days.unwrap_or(30).clamp(1, 365);

    let (liquid, _as_at) = liquid_assets(&pool).await?;

    let today = chrono::Local::now().naive_local().date();
    let occurrences = expand_bills(&pool, today, today + Duration::days(horizon_days)).await?;

    // Expected income is reported beside the figure, not inside it.
    let upcoming_income: f64 = occurrences.iter().filter(|b| b.is_income).map(|b| b.amount).sum();
    let bills: Vec<UpcomingBill> = occurrences.into_iter().filter(|b| !b.is_income).collect();
    let upcoming_total: f64 = bills.iter().map(|b| b.amount).sum();

    Ok(SafeToSpend {
        liquid,
        upcoming_total,
        upcoming_income,
        safe_to_spend: liquid - upcoming_total,
        horizon_days,
        bills,
    })
}


#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    async fn add(
        pool: &SqlitePool,
        name: &str,
        amount: f64,
        frequency: &str,
        next_due: &str,
        end: Option<&str>,
    ) {
        sqlx::query(
            "INSERT INTO recurring_costs (name, amount, frequency, next_due_date, end_date) \
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(name)
        .bind(amount)
        .bind(frequency)
        .bind(next_due)
        .bind(end)
        .execute(pool)
        .await
        .unwrap();
    }

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn dates(bills: &[UpcomingBill]) -> Vec<&str> {
        bills.iter().map(|b| b.due_date.as_str()).collect()
    }

    #[tokio::test]
    async fn monthly_cost_expands_across_the_window() {
        let pool = pool().await;
        add(&pool, "Rent", 500.0, "Monthly", "2026-01-15", None).await;

        let bills = expand_bills(&pool, d("2026-01-01"), d("2026-03-31")).await.unwrap();
        assert_eq!(dates(&bills), ["2026-01-15", "2026-02-15", "2026-03-15"]);
    }

    #[tokio::test]
    async fn a_stale_due_date_rolls_forward_into_the_window() {
        let pool = pool().await;
        // Entered a year ago and never touched since: the occurrences that
        // belong in the window are this year's, not a replay from 2025.
        add(&pool, "Rego", 900.0, "Yearly", "2025-03-10", None).await;

        let bills = expand_bills(&pool, d("2026-01-01"), d("2026-12-31")).await.unwrap();
        assert_eq!(dates(&bills), ["2026-03-10"]);
    }

    #[tokio::test]
    async fn an_end_date_stops_the_expansion() {
        let pool = pool().await;
        // A car loan with three instalments left.
        add(&pool, "Car loan", 420.0, "Monthly", "2026-01-20", Some("2026-03-31")).await;

        let bills = expand_bills(&pool, d("2026-01-01"), d("2026-12-31")).await.unwrap();
        assert_eq!(dates(&bills), ["2026-01-20", "2026-02-20", "2026-03-20"]);
    }

    #[tokio::test]
    async fn an_end_date_before_the_window_yields_nothing() {
        let pool = pool().await;
        add(&pool, "Old gym", 60.0, "Monthly", "2025-01-05", Some("2025-06-05")).await;

        let bills = expand_bills(&pool, d("2026-01-01"), d("2026-12-31")).await.unwrap();
        assert!(bills.is_empty(), "a finished cost shouldn't project, got {:?}", dates(&bills));
    }

    #[tokio::test]
    async fn a_one_off_falls_once_and_is_never_rolled_forward() {
        let pool = pool().await;
        add(&pool, "School camp", 800.0, ONE_OFF, "2026-03-12", None).await;
        // Last year's camp has been and gone; it must not reappear as a future
        // bill the way a repeating cost's stale date would.
        add(&pool, "Last year's camp", 760.0, ONE_OFF, "2025-03-12", None).await;

        let bills = expand_bills(&pool, d("2026-01-01"), d("2026-12-31")).await.unwrap();
        assert_eq!(dates(&bills), ["2026-03-12"]);
        assert_eq!(bills[0].amount, 800.0);
    }

    #[tokio::test]
    async fn a_paused_cost_is_left_out() {
        let pool = pool().await;
        add(&pool, "Streaming", 20.0, "Monthly", "2026-01-08", None).await;
        sqlx::query("UPDATE recurring_costs SET active = 0")
            .execute(&pool)
            .await
            .unwrap();

        let bills = expand_bills(&pool, d("2026-01-01"), d("2026-06-30")).await.unwrap();
        assert!(bills.is_empty());
    }

    #[tokio::test]
    async fn a_one_off_has_no_monthly_equivalent() {
        use crate::commands::recurring::payments_per_month;
        assert_eq!(payments_per_month(ONE_OFF), 0.0);
        // An unknown label still falls back to monthly rather than vanishing.
        assert_eq!(payments_per_month("Who knows"), 1.0);
    }
}
