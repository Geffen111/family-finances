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
