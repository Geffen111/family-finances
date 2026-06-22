// One-time importer for the legacy spreadsheet export.
//
// Unlike `import::csv_import` (which only takes date/description/debit/credit/
// balance for a single chosen account), this preserves the already-assigned
// Category > Subcategory and the per-row note, and maps the embedded account
// name to an account. It is idempotent: re-running skips rows already present.
//
// Expected columns (legacy layout, with a 3-row preamble before the header):
//   Date, Account, Description, Debit, Credit, Balance, Category, Subcategory, Month, Note
// The Month column is ignored (the app derives month/year from Date).

use chrono::NaiveDate;
use csv::ReaderBuilder;
use serde::Serialize;
use sqlx::SqlitePool;
use tauri::State;

#[derive(Debug, Serialize)]
pub struct HistoricalImportSummary {
    pub imported: i64,
    pub skipped_duplicate: i64,
    pub skipped_invalid: i64,
    pub uncategorised: i64,
    pub accounts_created: i64,
    pub categories_created: i64,
}

fn clean_field(s: Option<&str>) -> String {
    s.unwrap_or("")
        .trim()
        .trim_start_matches('\u{feff}') // strip UTF-8 BOM on the first cell
        .trim()
        .to_string()
}

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

fn parse_amount(s: &str) -> f64 {
    let cleaned = s.trim().replace(',', "").replace('$', "").replace('\u{2212}', "-");
    if cleaned.is_empty() {
        0.0
    } else {
        cleaned.parse().unwrap_or(0.0)
    }
}

// The legacy sheet used slightly different account names than the app's seeds.
fn map_account_name(raw: &str) -> &str {
    match raw {
        "Everyday Account" => "Everyday Spending",
        "Savings Account" => "Savings",
        other => other,
    }
}

fn is_header_row(record: &csv::StringRecord) -> bool {
    let mut has_date = false;
    let mut has_other = false;
    for field in record.iter() {
        match clean_field(Some(field)).to_lowercase().as_str() {
            "date" => has_date = true,
            "category" | "account" | "description" => has_other = true,
            _ => {}
        }
    }
    has_date && has_other
}

async fn get_or_create_account(
    pool: &SqlitePool,
    name: &str,
    created: &mut i64,
) -> Result<i64, String> {
    if let Some(id) = sqlx::query_scalar::<_, i64>("SELECT id FROM accounts WHERE name = ?")
        .bind(name)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("DB query error: {}", e))?
    {
        return Ok(id);
    }
    sqlx::query("INSERT INTO accounts (name, type) VALUES (?, 'asset')")
        .bind(name)
        .execute(pool)
        .await
        .map_err(|e| format!("DB insert error: {}", e))?;
    *created += 1;
    sqlx::query_scalar::<_, i64>("SELECT last_insert_rowid()")
        .fetch_one(pool)
        .await
        .map_err(|e| format!("DB error: {}", e))
}

async fn get_or_create_parent(
    pool: &SqlitePool,
    name: &str,
    created: &mut i64,
) -> Result<i64, String> {
    if let Some(id) =
        sqlx::query_scalar::<_, i64>("SELECT id FROM categories WHERE name = ? AND parent_id IS NULL")
            .bind(name)
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("DB query error: {}", e))?
    {
        return Ok(id);
    }
    sqlx::query("INSERT INTO categories (name, parent_id) VALUES (?, NULL)")
        .bind(name)
        .execute(pool)
        .await
        .map_err(|e| format!("DB insert error: {}", e))?;
    *created += 1;
    sqlx::query_scalar::<_, i64>("SELECT last_insert_rowid()")
        .fetch_one(pool)
        .await
        .map_err(|e| format!("DB error: {}", e))
}

async fn get_or_create_child(
    pool: &SqlitePool,
    name: &str,
    parent_id: i64,
    created: &mut i64,
) -> Result<i64, String> {
    if let Some(id) =
        sqlx::query_scalar::<_, i64>("SELECT id FROM categories WHERE name = ? AND parent_id = ?")
            .bind(name)
            .bind(parent_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("DB query error: {}", e))?
    {
        return Ok(id);
    }
    sqlx::query("INSERT INTO categories (name, parent_id) VALUES (?, ?)")
        .bind(name)
        .bind(parent_id)
        .execute(pool)
        .await
        .map_err(|e| format!("DB insert error: {}", e))?;
    *created += 1;
    sqlx::query_scalar::<_, i64>("SELECT last_insert_rowid()")
        .fetch_one(pool)
        .await
        .map_err(|e| format!("DB error: {}", e))
}

// Returns the category_id for a Category/Subcategory pair, creating rows as
// needed. None means "leave uncategorised" (blank or "Unknown" parent).
async fn resolve_category(
    pool: &SqlitePool,
    parent: &str,
    child: &str,
    created: &mut i64,
) -> Result<Option<i64>, String> {
    let parent = parent.trim();
    let child = child.trim();

    if parent.is_empty() || parent.eq_ignore_ascii_case("Unknown") {
        return Ok(None);
    }

    let parent_id = get_or_create_parent(pool, parent, created).await?;

    if child.is_empty() || child.eq_ignore_ascii_case("Unknown") {
        return Ok(Some(parent_id));
    }

    let child_id = get_or_create_child(pool, child, parent_id, created).await?;
    Ok(Some(child_id))
}

#[tauri::command]
pub async fn import_historical_csv(
    pool: State<'_, SqlitePool>,
    csv_content: String,
) -> Result<HistoricalImportSummary, String> {
    let pool = &*pool;

    let mut rdr = ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(csv_content.as_bytes());

    let mut started = false;
    let mut summary = HistoricalImportSummary {
        imported: 0,
        skipped_duplicate: 0,
        skipped_invalid: 0,
        uncategorised: 0,
        accounts_created: 0,
        categories_created: 0,
    };

    for result in rdr.records() {
        let record = result.map_err(|e| format!("CSV parse error: {}", e))?;

        // Skip the title/instruction preamble until we reach the header row.
        if !started {
            if is_header_row(&record) {
                started = true;
            }
            continue;
        }

        let date_raw = clean_field(record.get(0));
        let account_raw = clean_field(record.get(1));
        let description = clean_field(record.get(2));
        let debit = parse_amount(&clean_field(record.get(3)));
        let credit = parse_amount(&clean_field(record.get(4)));
        let balance_field = clean_field(record.get(5));
        let parent = clean_field(record.get(6));
        let child = clean_field(record.get(7));
        // index 8 is Month — intentionally ignored
        let note = clean_field(record.get(9));

        let date = match parse_date(&date_raw) {
            Some(d) => d,
            None => {
                summary.skipped_invalid += 1;
                continue;
            }
        };
        if description.is_empty() || account_raw.is_empty() {
            summary.skipped_invalid += 1;
            continue;
        }

        let account_name = map_account_name(&account_raw);
        let account_id =
            get_or_create_account(pool, account_name, &mut summary.accounts_created).await?;

        // Idempotency: skip a row that already exists for this account.
        let exists: bool = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM transactions \
             WHERE account_id = ? AND date = ? AND description = ? AND debit = ? AND credit = ?",
        )
        .bind(account_id)
        .bind(&date)
        .bind(&description)
        .bind(debit)
        .bind(credit)
        .fetch_one(pool)
        .await
        .map_err(|e| format!("DB query error: {}", e))?
            > 0;
        if exists {
            summary.skipped_duplicate += 1;
            continue;
        }

        let category_id =
            resolve_category(pool, &parent, &child, &mut summary.categories_created).await?;
        if category_id.is_none() {
            summary.uncategorised += 1;
        }

        let balance: Option<f64> = if balance_field.is_empty() {
            None
        } else {
            Some(parse_amount(&balance_field))
        };
        let notes: Option<String> = if note.is_empty() { None } else { Some(note) };

        sqlx::query(
            "INSERT INTO transactions (account_id, category_id, date, description, debit, credit, balance, notes) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(account_id)
        .bind(category_id)
        .bind(&date)
        .bind(&description)
        .bind(debit)
        .bind(credit)
        .bind(balance)
        .bind(notes)
        .execute(pool)
        .await
        .map_err(|e| format!("DB insert error: {}", e))?;

        summary.imported += 1;
    }

    if !started {
        return Err(
            "Could not find a header row (expected columns like Date, Account, Category).".into(),
        );
    }

    Ok(summary)
}
