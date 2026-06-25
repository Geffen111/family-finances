// Transaction splitting: allocate a single transaction's amount across several
// categories (e.g. a supermarket shop that's part groceries, part household).
// Splits feed the `tx_effective` view, so split amounts show up per category in
// all category-grouped reporting. Overall income/expense totals are unchanged.

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tauri::State;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct TransactionSplit {
    pub id: i64,
    pub transaction_id: i64,
    pub category_id: Option<i64>,
    pub amount: f64,
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SplitInput {
    pub category_id: Option<i64>,
    pub amount: f64,
    pub notes: Option<String>,
}

#[tauri::command]
pub async fn get_transaction_splits(
    pool: State<'_, SqlitePool>,
    transaction_id: i64,
) -> Result<Vec<TransactionSplit>, String> {
    sqlx::query_as::<_, TransactionSplit>(
        "SELECT id, transaction_id, category_id, amount, notes \
         FROM transaction_splits WHERE transaction_id = ? ORDER BY id",
    )
    .bind(transaction_id)
    .fetch_all(&*pool)
    .await
    .map_err(|e| format!("DB query error: {}", e))
}

/// Ids of transactions that currently have splits, for a given account — lets
/// the windowed transactions table show a "split" marker without a per-row call.
#[tauri::command]
pub async fn get_split_transaction_ids(
    pool: State<'_, SqlitePool>,
    account_id: i64,
) -> Result<Vec<i64>, String> {
    sqlx::query_scalar::<_, i64>(
        "SELECT DISTINCT s.transaction_id FROM transaction_splits s \
         JOIN transactions t ON t.id = s.transaction_id WHERE t.account_id = ?",
    )
    .bind(account_id)
    .fetch_all(&*pool)
    .await
    .map_err(|e| format!("DB query error: {}", e))
}

/// Replace a transaction's splits. The amounts must sum to the transaction's
/// debit (splitting is for expenses). Passing an empty list clears the splits,
/// so the transaction reverts to its own single category.
#[tauri::command]
pub async fn set_transaction_splits(
    pool: State<'_, SqlitePool>,
    transaction_id: i64,
    splits: Vec<SplitInput>,
) -> Result<(), String> {
    let debit = sqlx::query_scalar::<_, f64>("SELECT debit FROM transactions WHERE id = ?")
        .bind(transaction_id)
        .fetch_optional(&*pool)
        .await
        .map_err(|e| format!("DB query error: {}", e))?
        .ok_or_else(|| format!("Transaction {} not found", transaction_id))?;

    if !splits.is_empty() {
        if debit <= 0.0 {
            return Err("Only expense (debit) transactions can be split.".to_string());
        }
        let sum: f64 = splits.iter().map(|s| s.amount).sum();
        if (sum - debit).abs() > 0.01 {
            return Err(format!(
                "Splits must add up to the transaction amount ({:.2}); they total {:.2}.",
                debit, sum
            ));
        }
        if splits.iter().any(|s| s.amount <= 0.0) {
            return Err("Each split amount must be greater than zero.".to_string());
        }
    }

    let mut tx = pool.begin().await.map_err(|e| format!("DB error: {}", e))?;
    sqlx::query("DELETE FROM transaction_splits WHERE transaction_id = ?")
        .bind(transaction_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("DB delete error: {}", e))?;
    for s in &splits {
        sqlx::query(
            "INSERT INTO transaction_splits (transaction_id, category_id, amount, notes) \
             VALUES (?, ?, ?, ?)",
        )
        .bind(transaction_id)
        .bind(s.category_id)
        .bind(s.amount)
        .bind(&s.notes)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("DB insert error: {}", e))?;
    }
    tx.commit().await.map_err(|e| format!("DB error: {}", e))?;
    Ok(())
}
