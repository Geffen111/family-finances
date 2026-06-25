use crate::models::{Account, Transaction};
use csv::ReaderBuilder;
use chrono::NaiveDate;
use sqlx::SqlitePool;
use tauri::State;

fn parse_date(date_str: &str) -> Option<String> {
    let date_str = date_str.trim();
    if date_str.is_empty() {
        return None;
    }
    if let Ok(d) = NaiveDate::parse_from_str(date_str, "%d/%m/%Y") {
        return Some(d.format("%Y-%m-%d").to_string());
    }
    if let Ok(d) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
        return Some(d.format("%Y-%m-%d").to_string());
    }
    None
}

fn looks_like_header(row: &csv::StringRecord) -> bool {
    let headers = ["date", "description", "debit", "credit", "balance"];
    for field in row.iter() {
        let lower = field.trim().to_lowercase();
        if headers.contains(&lower.as_str()) {
            return true;
        }
    }
    false
}

#[derive(Debug, serde::Serialize)]
pub struct CsvImportResult {
    pub imported: i64,
    pub skipped_duplicate: i64,
}

#[tauri::command]
pub async fn csv_import(
    pool: State<'_, SqlitePool>,
    csv_content: String,
    account_id: i64,
) -> Result<CsvImportResult, String> {
    let mut rdr = ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(csv_content.as_bytes());

    let mut first = true;
    let mut imported: i64 = 0;
    let mut skipped_duplicate: i64 = 0;

    for result in rdr.records() {
        let record = result.map_err(|e| format!("CSV parse error: {}", e))?;

        if first {
            first = false;
            if looks_like_header(&record) {
                continue;
            }
        }

        let date_raw = record.get(0).unwrap_or("").trim();
        let description = record.get(1).unwrap_or("").trim();
        let debit_raw = record.get(2).unwrap_or("0").trim();
        let credit_raw = record.get(3).unwrap_or("0").trim();
        let balance_raw = record.get(4).unwrap_or("").trim();

        if date_raw.is_empty() || description.is_empty() {
            continue;
        }

        let date = parse_date(date_raw)
            .ok_or_else(|| format!("Invalid date format: {}", date_raw))?;

        let debit: f64 = if debit_raw.is_empty() {
            0.0
        } else {
            debit_raw.replace(",", "").replace("$", "").replace("−", "-")
                .parse()
                .map_err(|_| format!("Invalid debit: {}", debit_raw))?
        };

        let credit: f64 = if credit_raw.is_empty() {
            0.0
        } else {
            credit_raw.replace(",", "").replace("$", "").replace("−", "-")
                .parse()
                .map_err(|_| format!("Invalid credit: {}", credit_raw))?
        };

        let balance: Option<f64> = if balance_raw.is_empty() {
            None
        } else {
            Some(
                balance_raw.replace(",", "").replace("$", "").replace("−", "-")
                    .parse()
                    .map_err(|_| format!("Invalid balance: {}", balance_raw))?
            )
        };

        // Skip a row that already exists for this account (same date,
        // description and amounts) so re-importing a statement is safe.
        let exists: bool = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM transactions \
             WHERE account_id = ? AND date = ? AND description = ? AND debit = ? AND credit = ?",
        )
        .bind(account_id)
        .bind(&date)
        .bind(description)
        .bind(debit)
        .bind(credit)
        .fetch_one(&*pool)
        .await
        .map_err(|e| format!("DB query error: {}", e))?
            > 0;
        if exists {
            skipped_duplicate += 1;
            continue;
        }

        sqlx::query(
            "INSERT INTO transactions (account_id, date, description, debit, credit, balance) VALUES (?, ?, ?, ?, ?, ?)"
        )
        .bind(account_id)
        .bind(&date)
        .bind(description)
        .bind(debit)
        .bind(credit)
        .bind(balance)
        .execute(&*pool)
        .await
        .map_err(|e| format!("DB insert error: {}", e))?;

        imported += 1;
    }

    // Auto-categorise freshly imported rows with any matching rules (offline,
    // deterministic) before the user reaches for the AI categoriser.
    if imported > 0 {
        let _ = crate::commands::rules::apply_rules_internal(&pool, true, Some(account_id)).await;
    }

    Ok(CsvImportResult {
        imported,
        skipped_duplicate,
    })
}

#[tauri::command]
pub async fn get_transactions(
    pool: State<'_, SqlitePool>,
    account_id: i64,
    start_date: Option<String>,
    end_date: Option<String>,
) -> Result<Vec<Transaction>, String> {
    let mut query = String::from(
        "SELECT id, account_id, category_id, date, description, debit, credit, balance, \
         ai_category, ai_category_conf, ai_categorised_at, notes, created_at \
         FROM transactions WHERE account_id = ?"
    );

    if start_date.is_some() {
        query.push_str(" AND date >= ?");
    }
    if end_date.is_some() {
        query.push_str(" AND date <= ?");
    }
    query.push_str(" ORDER BY date DESC");

    let mut q = sqlx::query_as::<_, Transaction>(&query).bind(account_id);

    if let Some(ref sd) = start_date {
        q = q.bind(sd);
    }
    if let Some(ref ed) = end_date {
        q = q.bind(ed);
    }

    q.fetch_all(&*pool)
        .await
        .map_err(|e| format!("DB query error: {}", e))
}

#[tauri::command]
pub async fn get_accounts(pool: State<'_, SqlitePool>) -> Result<Vec<Account>, String> {
    sqlx::query_as::<_, Account>("SELECT id, name, type, created_at FROM accounts ORDER BY id")
    .fetch_all(&*pool)
    .await
    .map_err(|e| format!("DB query error: {}", e))
}

#[derive(Debug, serde::Serialize)]
pub struct MoveResult {
    pub moved: i64,
    pub skipped_duplicate: i64,
}

// Move one or more transactions to a different account. Used to fix imports
// that landed against the wrong account. Mirrors the CSV import dedupe guard:
// if an identical row (same date, description and amounts) already exists in the
// target account, the moved copy is dropped instead of creating a duplicate.
#[tauri::command]
pub async fn move_transactions(
    pool: State<'_, SqlitePool>,
    transaction_ids: Vec<i64>,
    account_id: i64,
) -> Result<MoveResult, String> {
    if transaction_ids.is_empty() {
        return Ok(MoveResult { moved: 0, skipped_duplicate: 0 });
    }

    // Confirm the target account exists before reassigning.
    let target_exists: bool = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM accounts WHERE id = ?",
    )
    .bind(account_id)
    .fetch_one(&*pool)
    .await
    .map_err(|e| format!("DB query error: {}", e))?
        > 0;
    if !target_exists {
        return Err(format!("Account {} does not exist", account_id));
    }

    let mut moved: i64 = 0;
    let mut skipped_duplicate: i64 = 0;

    for id in &transaction_ids {
        let tx = sqlx::query_as::<_, Transaction>(
            "SELECT id, account_id, category_id, date, description, debit, credit, balance, \
             ai_category, ai_category_conf, ai_categorised_at, notes, created_at \
             FROM transactions WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&*pool)
        .await
        .map_err(|e| format!("DB query error: {}", e))?;

        let Some(tx) = tx else { continue };

        // Already in the target account — nothing to do.
        if tx.account_id == account_id {
            continue;
        }

        // Does an identical row already exist in the target account?
        let dup: bool = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM transactions \
             WHERE account_id = ? AND date = ? AND description = ? AND debit = ? AND credit = ? AND id != ?",
        )
        .bind(account_id)
        .bind(&tx.date)
        .bind(&tx.description)
        .bind(tx.debit)
        .bind(tx.credit)
        .bind(id)
        .fetch_one(&*pool)
        .await
        .map_err(|e| format!("DB query error: {}", e))?
            > 0;

        if dup {
            // The target already has this transaction; drop the duplicate copy
            // rather than moving it and creating two identical rows.
            sqlx::query("DELETE FROM transactions WHERE id = ?")
                .bind(id)
                .execute(&*pool)
                .await
                .map_err(|e| format!("DB delete error: {}", e))?;
            skipped_duplicate += 1;
        } else {
            sqlx::query("UPDATE transactions SET account_id = ? WHERE id = ?")
                .bind(account_id)
                .bind(id)
                .execute(&*pool)
                .await
                .map_err(|e| format!("DB update error: {}", e))?;
            moved += 1;
        }
    }

    Ok(MoveResult { moved, skipped_duplicate })
}

// When was the most recent transaction imported for this account. Derived from
// created_at (set at insert time), so it needs no extra tracking table.
#[tauri::command]
pub async fn get_last_import(
    pool: State<'_, SqlitePool>,
    account_id: i64,
) -> Result<Option<String>, String> {
    sqlx::query_scalar::<_, Option<String>>(
        "SELECT MAX(created_at) FROM transactions WHERE account_id = ?",
    )
    .bind(account_id)
    .fetch_one(&*pool)
    .await
    .map_err(|e| format!("DB query error: {}", e))
}
