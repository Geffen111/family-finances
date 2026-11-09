// Matching imported transactions to the upcoming items that were expecting
// them. Nothing here changes a date on its own: an automatic match that is
// wrong moves a bill's whole schedule, and the one thing worse than a stale
// date is a confident wrong one. Every match is a suggestion the user
// confirms.

use crate::commands::cashflow::advance;
use crate::commands::categorise::normalize_desc;
use crate::commands::recurring::{payments_per_month, KIND_INCOME, ONE_OFF};
use crate::models::RecurringCost;
use chrono::NaiveDate;
use serde::Serialize;
use sqlx::SqlitePool;
use tauri::State;

/// How far either side of the expected date a payment can land and still be
/// that payment: half a cycle, so a fortnightly item can't claim the next
/// one's transaction, capped at ten days for the long cadences.
fn window_days(frequency: &str) -> i64 {
    let per_month = payments_per_month(frequency);
    if per_month <= 0.0 {
        return 0;
    }
    let cycle = 30.44 / per_month;
    ((cycle / 2.0).floor() as i64).clamp(2, 10)
}

/// An amount that matches on its own has to match closely — it is the only
/// evidence there is. Where the merchant line agrees as well, the amount is
/// allowed to move: an electricity bill is the same bill at $180 or $320.
const AMOUNT_ONLY_TOLERANCE: f64 = 0.05;
const NAMED_TOLERANCE: f64 = 0.60;

#[derive(Debug, Serialize)]
pub struct PaymentMatch {
    pub cost_id: i64,
    pub cost_name: String,
    pub kind: String,
    pub frequency: String,
    /// The occurrence this transaction appears to pay.
    pub expected_date: String,
    pub expected_amount: f64,
    pub transaction_id: i64,
    pub tx_date: String,
    pub tx_description: String,
    pub tx_amount: f64,
    /// True when the merchant line agreed, not just the amount and the date.
    pub matched_on_name: bool,
    /// What confirming would set the next due date to.
    pub new_due_date: String,
}

type TxRow = (i64, String, String, f64, f64);

/// Transactions that look like they pay something on the Upcoming list.
///
/// Only items whose stored date has been overtaken are considered — that is
/// the whole point, since nothing rewrites `next_due_date` when a payment
/// lands. One-offs are left alone: there is no next date to move, and the
/// page already reads a past one as done.
pub(crate) async fn find_matches(pool: &SqlitePool) -> Result<Vec<PaymentMatch>, String> {
    type CostRow = (i64, String, f64, String, String, Option<String>, Option<String>);
    let costs = sqlx::query_as::<_, CostRow>(
        "SELECT id, name, amount, frequency, kind, next_due_date, end_date \
         FROM recurring_costs \
         WHERE active = 1 AND next_due_date IS NOT NULL AND frequency <> ? \
         ORDER BY name COLLATE NOCASE",
    )
    .bind(ONE_OFF)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("DB query error: {}", e))?;

    if costs.is_empty() {
        return Ok(Vec::new());
    }

    // One query for the transactions that could possibly be in range, rather
    // than one per cost.
    let earliest = costs
        .iter()
        .filter_map(|(_, _, _, _, _, due, _)| due.as_deref())
        .filter_map(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        .min()
        .map(|d| d - chrono::Duration::days(10))
        .unwrap_or_else(|| chrono::Local::now().naive_local().date());

    let txs = sqlx::query_as::<_, TxRow>(
        "SELECT id, date, description, debit, credit FROM transactions \
         WHERE date >= ? AND (debit > 0 OR credit > 0) ORDER BY date",
    )
    .bind(earliest.format("%Y-%m-%d").to_string())
    .fetch_all(pool)
    .await
    .map_err(|e| format!("DB query error: {}", e))?;

    let today = chrono::Local::now().naive_local().date();
    let mut matches = Vec::new();

    for (cost_id, name, amount, frequency, kind, due, end_date) in costs {
        let is_income = kind == KIND_INCOME;
        let Some(stored) = due
            .as_deref()
            .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        else {
            continue;
        };
        let ends = end_date
            .as_deref()
            .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok());
        let window = window_days(&frequency);
        if window == 0 {
            continue;
        }
        let key = normalize_desc(&name);

        // Every occurrence from the stored date up to today — those are the
        // ones that should already have been paid. The *latest* match wins, so
        // a date left alone for months catches up in one click rather than
        // asking for a confirmation per cycle.
        let mut best: Option<(NaiveDate, &TxRow, bool)> = None;
        let mut occurrence = stored;
        let mut guard = 0;
        while occurrence <= today && guard < 400 {
            if ends.is_some_and(|e| occurrence > e) {
                break;
            }
            for tx in &txs {
                let (_, date_str, desc, debit, credit) = tx;
                let Ok(tx_date) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d") else {
                    continue;
                };
                if (tx_date - occurrence).num_days().abs() > window {
                    continue;
                }
                let tx_amount = if is_income { *credit } else { *debit };
                if tx_amount <= 0.0 {
                    continue;
                }
                let named = !key.is_empty() && {
                    let tx_key = normalize_desc(desc);
                    !tx_key.is_empty() && (tx_key.contains(&key) || key.contains(&tx_key))
                };
                let tolerance = if named {
                    NAMED_TOLERANCE
                } else {
                    AMOUNT_ONLY_TOLERANCE
                };
                if amount <= 0.0 || (tx_amount - amount).abs() / amount > tolerance {
                    continue;
                }
                // Prefer a later occurrence, and a named match over an
                // amount-only one within the same occurrence.
                let better = match &best {
                    None => true,
                    Some((best_occ, _, best_named)) => {
                        occurrence > *best_occ || (occurrence == *best_occ && named && !best_named)
                    }
                };
                if better {
                    best = Some((occurrence, tx, named));
                }
            }
            occurrence = advance(occurrence, &frequency);
            guard += 1;
        }

        let Some((occurrence, tx, named)) = best else {
            continue;
        };
        let (tx_id, tx_date_str, tx_desc, debit, credit) = tx;
        let tx_date = NaiveDate::parse_from_str(tx_date_str, "%Y-%m-%d")
            .map_err(|e| format!("Bad transaction date: {}", e))?;

        matches.push(PaymentMatch {
            cost_id,
            cost_name: name,
            kind,
            frequency: frequency.clone(),
            expected_date: occurrence.format("%Y-%m-%d").to_string(),
            expected_amount: amount,
            transaction_id: *tx_id,
            tx_date: tx_date_str.clone(),
            tx_description: tx_desc.split_whitespace().collect::<Vec<_>>().join(" "),
            tx_amount: if is_income { *credit } else { *debit },
            matched_on_name: named,
            new_due_date: roll_past(stored, &frequency, tx_date)
                .format("%Y-%m-%d")
                .to_string(),
        });
    }

    Ok(matches)
}

/// The first occurrence after a payment landed. Rolled from the stored date
/// rather than computed from the payment, so a bill that is always a few days
/// late keeps its own schedule instead of drifting later every month.
fn roll_past(stored: NaiveDate, frequency: &str, paid: NaiveDate) -> NaiveDate {
    let mut next = stored;
    let mut guard = 0;
    while next <= paid && guard < 400 {
        next = advance(next, frequency);
        guard += 1;
    }
    next
}

#[tauri::command]
pub async fn find_payment_matches(
    pool: State<'_, SqlitePool>,
) -> Result<Vec<PaymentMatch>, String> {
    find_matches(&pool).await
}

/// Record that a payment landed: move the item's next due date past it.
/// Takes the transaction rather than a date, so the date that moves the
/// schedule is the one the user actually saw on the row they confirmed.
pub(crate) async fn confirm(
    pool: &SqlitePool,
    cost_id: i64,
    transaction_id: i64,
) -> Result<RecurringCost, String> {
    let (frequency, due): (String, Option<String>) =
        sqlx::query_as("SELECT frequency, next_due_date FROM recurring_costs WHERE id = ?")
            .bind(cost_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("DB query error: {}", e))?
            .ok_or_else(|| format!("Upcoming item {} not found", cost_id))?;

    let stored = due
        .as_deref()
        .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        .ok_or_else(|| "That item has no due date to move.".to_string())?;

    let paid_str: String = sqlx::query_scalar("SELECT date FROM transactions WHERE id = ?")
        .bind(transaction_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("DB query error: {}", e))?
        .ok_or_else(|| format!("Transaction {} not found", transaction_id))?;
    let paid = NaiveDate::parse_from_str(&paid_str, "%Y-%m-%d")
        .map_err(|e| format!("Bad transaction date: {}", e))?;

    let next = roll_past(stored, &frequency, paid);
    sqlx::query("UPDATE recurring_costs SET next_due_date = ? WHERE id = ?")
        .bind(next.format("%Y-%m-%d").to_string())
        .bind(cost_id)
        .execute(pool)
        .await
        .map_err(|e| format!("DB update error: {}", e))?;

    crate::commands::recurring::fetch_cost(pool, cost_id).await
}

#[tauri::command]
pub async fn confirm_payment(
    pool: State<'_, SqlitePool>,
    cost_id: i64,
    transaction_id: i64,
) -> Result<RecurringCost, String> {
    confirm(&pool, cost_id, transaction_id).await
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
        sqlx::query("INSERT INTO accounts (id, name, type) VALUES (1, 'Test', 'asset')")
            .execute(&pool)
            .await
            .unwrap();
        pool
    }

    fn days_ago(n: i64) -> String {
        (chrono::Local::now().naive_local().date() - chrono::Duration::days(n))
            .format("%Y-%m-%d")
            .to_string()
    }

    async fn bill(pool: &SqlitePool, name: &str, amount: f64, frequency: &str, due: &str) -> i64 {
        sqlx::query_scalar::<_, i64>(
            "INSERT INTO recurring_costs (name, amount, frequency, next_due_date) \
             VALUES (?, ?, ?, ?) RETURNING id",
        )
        .bind(name)
        .bind(amount)
        .bind(frequency)
        .bind(due)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    async fn spend(pool: &SqlitePool, desc: &str, amount: f64, date: &str) -> i64 {
        sqlx::query_scalar::<_, i64>(
            "INSERT INTO transactions (account_id, date, description, debit) \
             VALUES (1, ?, ?, ?) RETURNING id",
        )
        .bind(date)
        .bind(desc)
        .bind(amount)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    /// The everyday case: a bill came out a few days off its date, for a few
    /// dollars more than expected, under the same merchant line.
    #[tokio::test]
    async fn a_payment_near_the_due_date_is_matched_and_confirming_moves_the_date() {
        let pool = pool().await;
        let id = bill(&pool, "Acme Insurance", 92.40, "Monthly", &days_ago(20)).await;
        spend(&pool, "Acme Insurance Premium", 97.10, &days_ago(18)).await;

        let matches = find_matches(&pool).await.unwrap();
        assert_eq!(matches.len(), 1, "expected one match: {matches:#?}");
        assert!(matches[0].matched_on_name);
        assert_eq!(matches[0].tx_amount, 97.10);

        let updated = confirm(&pool, id, matches[0].transaction_id).await.unwrap();
        // A cycle on from the *stored* date, not from the payment: a bill that
        // is always late must not drift later every month.
        assert_eq!(updated.next_due_date.as_deref(), Some(matches[0].new_due_date.as_str()));
        assert!(updated.next_due_date.unwrap().as_str() > days_ago(0).as_str());

        // And it stops being offered.
        assert!(find_matches(&pool).await.unwrap().is_empty());
    }

    /// A date left alone for months catches up in one click: the latest
    /// payment wins rather than one confirmation per missed cycle.
    #[tokio::test]
    async fn a_stale_date_catches_up_to_the_most_recent_payment() {
        let pool = pool().await;
        let id = bill(&pool, "Acme Gym", 60.00, "Monthly", &days_ago(95)).await;
        spend(&pool, "Acme Gym", 60.00, &days_ago(94)).await;
        spend(&pool, "Acme Gym", 60.00, &days_ago(64)).await;
        spend(&pool, "Acme Gym", 60.00, &days_ago(33)).await;

        let matches = find_matches(&pool).await.unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].tx_date, days_ago(33), "the latest payment should win");

        let updated = confirm(&pool, id, matches[0].transaction_id).await.unwrap();
        assert!(updated.next_due_date.unwrap().as_str() > days_ago(33).as_str());
    }

    /// Without the merchant line to go on, the amount is the only evidence
    /// there is, so it has to be close. A coincidental spend of a different
    /// size in the same week is not the bill.
    #[tokio::test]
    async fn an_unrelated_spend_of_the_wrong_size_is_not_matched() {
        let pool = pool().await;
        bill(&pool, "Acme Water", 180.00, "Quarterly", &days_ago(5)).await;
        spend(&pool, "Totally Different Shop", 240.00, &days_ago(4)).await;

        assert!(find_matches(&pool).await.unwrap().is_empty());
    }

    /// Same amount, same week, different merchant: accepted, because that is
    /// all a card payment under a changing reference ever gives you.
    #[tokio::test]
    async fn the_same_amount_in_the_window_is_matched_without_the_name() {
        let pool = pool().await;
        bill(&pool, "Acme Streaming", 18.99, "Monthly", &days_ago(6)).await;
        spend(&pool, "Sp 7731A Melbourne", 18.99, &days_ago(4)).await;

        let matches = find_matches(&pool).await.unwrap();
        assert_eq!(matches.len(), 1);
        assert!(!matches[0].matched_on_name);
    }

    /// A payment that hasn't happened yet isn't a payment. Only occurrences
    /// that are already due are looked at, so next week's bill can't be
    /// "confirmed" by an unrelated transaction today.
    #[tokio::test]
    async fn a_bill_still_in_the_future_is_left_alone() {
        let pool = pool().await;
        let future = (chrono::Local::now().naive_local().date() + chrono::Duration::days(12))
            .format("%Y-%m-%d")
            .to_string();
        bill(&pool, "Acme Rates", 310.00, "Quarterly", &future).await;
        spend(&pool, "Acme Rates", 310.00, &days_ago(1)).await;

        assert!(find_matches(&pool).await.unwrap().is_empty());
    }

    /// Income is matched on the credit side: a pay that landed moves the next
    /// pay date, and a debit of the same size must not stand in for it.
    #[tokio::test]
    async fn income_is_matched_on_the_credit_side() {
        let pool = pool().await;
        sqlx::query(
            "INSERT INTO recurring_costs (name, amount, frequency, kind, next_due_date) \
             VALUES ('Acme Payroll', 1450.00, 'Fortnightly', 'income', ?)",
        )
        .bind(days_ago(3))
        .execute(&pool)
        .await
        .unwrap();
        spend(&pool, "Acme Payroll", 1450.00, &days_ago(2)).await;
        assert!(
            find_matches(&pool).await.unwrap().is_empty(),
            "a debit must not pass for income"
        );

        sqlx::query(
            "INSERT INTO transactions (account_id, date, description, credit) \
             VALUES (1, ?, 'Acme Payroll', 1450.00)",
        )
        .bind(days_ago(2))
        .execute(&pool)
        .await
        .unwrap();
        let matches = find_matches(&pool).await.unwrap();
        assert_eq!(matches.len(), 1);
        assert!(matches[0].matched_on_name);
    }
}
