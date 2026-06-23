use sqlx::Row;
use sqlx::SqlitePool;
use tauri::State;

#[tauri::command]
pub async fn export_transactions_csv(pool: State<'_, SqlitePool>) -> Result<String, String> {
    let rows = sqlx::query(
        "SELECT t.date, t.description, t.debit, t.credit, t.balance, \
         COALESCE(a.name, '') as account_name, \
         COALESCE(c.name, '') as category_name, \
         COALESCE(t.notes, '') as notes \
         FROM transactions t \
         LEFT JOIN accounts a ON t.account_id = a.id \
         LEFT JOIN categories c ON t.category_id = c.id \
         ORDER BY t.date DESC"
    )
    .fetch_all(&*pool)
    .await
    .map_err(|e| format!("DB query error: {}", e))?;

    let mut csv = String::from("Date,Description,Debit,Credit,Balance,Account,Category,Notes\n");
    for row in &rows {
        let date: String = row.get(0);
        let description: String = row.get(1);
        let debit: f64 = row.get(2);
        let credit: f64 = row.get(3);
        let balance: Option<f64> = row.get(4);
        let account_name: String = row.get(5);
        let category_name: String = row.get(6);
        let notes: String = row.get(7);

        csv.push_str(&format!(
            "{},{},{},{},{},{},{},{}\n",
            date,
            escape_csv(&description),
            debit,
            credit,
            balance.map_or(String::new(), |b| format!("{:.2}", b)),
            escape_csv(&account_name),
            escape_csv(&category_name),
            escape_csv(&notes),
        ));
    }

    Ok(csv)
}

#[tauri::command]
pub async fn export_summary_csv(
    pool: State<'_, SqlitePool>,
    start_date: Option<String>,
    end_date: Option<String>,
) -> Result<String, String> {
    let mut query = String::from(
        "SELECT COALESCE(c.name, 'Uncategorised') as category_name, \
         CAST(SUM(t.debit) AS REAL) as total, COUNT(*) as transaction_count \
         FROM transactions t \
         LEFT JOIN categories c ON t.category_id = c.id \
         WHERE t.debit > 0"
    );

    if start_date.is_some() {
        query.push_str(" AND t.date >= ?");
    }
    if end_date.is_some() {
        query.push_str(" AND t.date <= ?");
    }
    query.push_str(" GROUP BY t.category_id ORDER BY total DESC");

    let mut q = sqlx::query_as::<_, (String, f64, i64)>(&query);

    if let Some(ref sd) = start_date {
        q = q.bind(sd);
    }
    if let Some(ref ed) = end_date {
        q = q.bind(ed);
    }

    let rows = q
        .fetch_all(&*pool)
        .await
        .map_err(|e| format!("DB query error: {}", e))?;

    let grand_total: f64 = rows.iter().map(|r| r.1).sum();

    let mut csv = String::from("Category,Total,Percentage,Transaction Count\n");
    for (name, total, count) in &rows {
        let pct = if grand_total > 0.0 {
            (total / grand_total) * 100.0
        } else {
            0.0
        };
        csv.push_str(&format!(
            "{},{},{:.1}%,{}\n",
            escape_csv(name),
            total,
            pct,
            count
        ));
    }

    Ok(csv)
}

fn escape_csv(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}