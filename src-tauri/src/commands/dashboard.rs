use crate::models::{CategorySpending, CategoryTrend, DashboardSummary, MonthlyTrend};
use chrono::NaiveDate;
use sqlx::SqlitePool;
use tauri::State;

// Excludes transactions whose category is flagged out of budgets/totals
// (e.g. internal transfers). NULL-safe: uncategorised rows are kept.
const EXCLUDE_BUDGET: &str =
    " AND NOT EXISTS (SELECT 1 FROM categories xc WHERE xc.id = t.category_id AND xc.exclude_from_budget = 1)";

fn apply_date_filters(
    sql: &mut String,
    start_date: &Option<String>,
    end_date: &Option<String>,
    table_alias: &str,
) -> Vec<String> {
    let mut params = Vec::new();
    if let Some(sd) = start_date {
        sql.push_str(&format!(" AND {}.date >= ?", table_alias));
        params.push(sd.clone());
    }
    if let Some(ed) = end_date {
        sql.push_str(&format!(" AND {}.date <= ?", table_alias));
        params.push(ed.clone());
    }
    params
}

fn month_label(ym: &str) -> String {
    if let Ok(d) = NaiveDate::parse_from_str(&format!("{}-01", ym), "%Y-%m-%d") {
        d.format("%b %Y").to_string()
    } else {
        ym.to_string()
    }
}

#[tauri::command]
pub async fn get_dashboard_summary(
    pool: State<'_, SqlitePool>,
    start_date: Option<String>,
    end_date: Option<String>,
) -> Result<DashboardSummary, String> {
    let mut base = String::from("SELECT COALESCE(SUM(t.credit), 0) FROM transactions t WHERE 1=1");
    base.push_str(EXCLUDE_BUDGET);
    let params = apply_date_filters(&mut base, &start_date, &end_date, "t");

    let mut q = sqlx::query_scalar::<_, f64>(&base);
    for p in &params {
        q = q.bind(p);
    }
    let total_income: f64 = q.fetch_one(&*pool).await.unwrap_or(0.0);

    let mut exp_base = String::from("SELECT COALESCE(SUM(t.debit), 0) FROM transactions t WHERE 1=1");
    exp_base.push_str(EXCLUDE_BUDGET);
    let params2 = apply_date_filters(&mut exp_base, &start_date, &end_date, "t");
    let mut q = sqlx::query_scalar::<_, f64>(&exp_base);
    for p in &params2 {
        q = q.bind(p);
    }
    let total_expenses: f64 = q.fetch_one(&*pool).await.unwrap_or(0.0);

    let mut cnt_base = String::from("SELECT COUNT(*) FROM transactions t WHERE 1=1");
    let params3 = apply_date_filters(&mut cnt_base, &start_date, &end_date, "t");
    let mut q = sqlx::query_scalar::<_, i64>(&cnt_base);
    for p in &params3 {
        q = q.bind(p);
    }
    let transaction_count: i64 = q.fetch_one(&*pool).await.unwrap_or(0);

    let mut top_base = String::from(
        "SELECT COALESCE(c.name, 'Uncategorised'), COALESCE(SUM(t.debit), 0) as total
         FROM transactions t
         LEFT JOIN categories c ON t.category_id = c.id
         WHERE 1=1"
    );
    top_base.push_str(EXCLUDE_BUDGET);
    let params4 = apply_date_filters(&mut top_base, &start_date, &end_date, "t");
    top_base.push_str(" GROUP BY c.name ORDER BY total DESC LIMIT 1");

    let mut q = sqlx::query_as::<_, (String, f64)>(&top_base);
    for p in &params4 {
        q = q.bind(p);
    }
    let (top_category, top_category_amount) = q.fetch_optional(&*pool).await
        .map_err(|e| format!("DB query error: {}", e))?
        .unwrap_or(("N/A".to_string(), 0.0));

    Ok(DashboardSummary {
        total_income,
        total_expenses,
        net: total_income - total_expenses,
        top_category,
        top_category_amount,
        transaction_count,
    })
}

#[tauri::command]
pub async fn get_spending_by_category(
    pool: State<'_, SqlitePool>,
    start_date: Option<String>,
    end_date: Option<String>,
) -> Result<Vec<CategorySpending>, String> {
    let mut query = String::from(
        "SELECT
            COALESCE(c.name, 'Uncategorised'),
            CASE WHEN cp.name IS NOT NULL THEN cp.name || ' > ' || c.name ELSE COALESCE(c.name, 'Uncategorised') END,
            COALESCE(SUM(t.debit), 0),
            COUNT(*)
         FROM transactions t
         LEFT JOIN categories c ON t.category_id = c.id
         LEFT JOIN categories cp ON c.parent_id = cp.id
         WHERE t.debit > 0"
    );
    query.push_str(EXCLUDE_BUDGET);
    let params = apply_date_filters(&mut query, &start_date, &end_date, "t");
    query.push_str(" GROUP BY c.name ORDER BY SUM(t.debit) DESC");

    let mut q = sqlx::query_as::<_, (String, String, f64, i64)>(&query);
    for p in &params {
        q = q.bind(p);
    }
    let rows = q.fetch_all(&*pool)
        .await
        .map_err(|e| format!("DB query error: {}", e))?;

    let total_spending: f64 = rows.iter().map(|r| r.2).sum();

    let result: Vec<CategorySpending> = rows
        .into_iter()
        .map(|(name, path, total, count)| CategorySpending {
            category_name: name,
            category_path: path,
            total,
            percentage: if total_spending > 0.0 {
                (total / total_spending) * 100.0
            } else {
                0.0
            },
            transaction_count: count,
        })
        .collect();

    Ok(result)
}

#[tauri::command]
pub async fn get_monthly_trends(
    pool: State<'_, SqlitePool>,
    start_date: Option<String>,
    end_date: Option<String>,
) -> Result<Vec<MonthlyTrend>, String> {
    let mut query = String::from(
        "SELECT strftime('%Y-%m', t.date), COALESCE(SUM(t.credit), 0), COALESCE(SUM(t.debit), 0)
         FROM transactions t WHERE 1=1"
    );
    query.push_str(EXCLUDE_BUDGET);
    let params = apply_date_filters(&mut query, &start_date, &end_date, "t");
    query.push_str(" GROUP BY strftime('%Y-%m', t.date) ORDER BY 1 ASC");

    let mut q = sqlx::query_as::<_, (String, f64, f64)>(&query);
    for p in &params {
        q = q.bind(p);
    }
    let rows = q.fetch_all(&*pool)
        .await
        .map_err(|e| format!("DB query error: {}", e))?;

    let result: Vec<MonthlyTrend> = rows
        .into_iter()
        .map(|(month, income, expenses)| {
            MonthlyTrend {
                month: month.clone(),
                label: month_label(&month),
                income,
                expenses,
                net: income - expenses,
            }
        })
        .collect();

    Ok(result)
}

#[tauri::command]
pub async fn get_spending_trend_by_category(
    pool: State<'_, SqlitePool>,
    start_date: Option<String>,
    end_date: Option<String>,
    category_ids: Option<Vec<i64>>,
) -> Result<Vec<CategoryTrend>, String> {
    let mut query = String::from(
        "SELECT c.id, COALESCE(c.name, 'Uncategorised'), strftime('%Y-%m', t.date), COALESCE(SUM(t.debit), 0)
         FROM transactions t
         LEFT JOIN categories c ON t.category_id = c.id
         WHERE 1=1"
    );
    query.push_str(EXCLUDE_BUDGET);
    let params = apply_date_filters(&mut query, &start_date, &end_date, "t");

    if let Some(ref ids) = category_ids {
        if !ids.is_empty() {
            query.push_str(" AND c.id IN (");
            for (i, _) in ids.iter().enumerate() {
                if i > 0 {
                    query.push(',');
                }
                query.push('?');
            }
            query.push(')');
        }
    }

    query.push_str(" GROUP BY c.id, strftime('%Y-%m', t.date) ORDER BY 3 ASC");

    let mut q = sqlx::query_as::<_, (Option<i64>, String, String, f64)>(&query);
    for p in &params {
        q = q.bind(p);
    }
    if let Some(ref ids) = category_ids {
        for id in ids {
            q = q.bind(id);
        }
    }

    let rows = q.fetch_all(&*pool)
        .await
        .map_err(|e| format!("DB query error: {}", e))?;

    let result: Vec<CategoryTrend> = rows
        .into_iter()
        .map(|(cat_id, cat_name, month, amount)| {
            CategoryTrend {
                category_id: cat_id,
                category_name: cat_name,
                month: month.clone(),
                label: month_label(&month),
                amount,
            }
        })
        .collect();

    Ok(result)
}

#[derive(Debug, serde::Serialize)]
pub struct BudgetStatus {
    pub category_id: i64,
    pub name: String,
    pub path: String,
    pub monthly_budget: f64,
    pub actual: f64,
    pub percentage: f64,
}

#[tauri::command]
pub async fn get_budget_status(
    pool: State<'_, SqlitePool>,
    start_date: Option<String>,
    end_date: Option<String>,
) -> Result<Vec<BudgetStatus>, String> {
    // Actual spend per budgeted category, over the given period. The date
    // filter lives inside a correlated subquery so categories with no
    // spending in the period still appear (actual = 0).
    let mut actual = String::from(
        "COALESCE((SELECT SUM(t.debit) FROM transactions t WHERE t.category_id = c.id",
    );
    let mut params: Vec<String> = Vec::new();
    if let Some(sd) = &start_date {
        actual.push_str(" AND t.date >= ?");
        params.push(sd.clone());
    }
    if let Some(ed) = &end_date {
        actual.push_str(" AND t.date <= ?");
        params.push(ed.clone());
    }
    actual.push_str("), 0)");

    let query = format!(
        "SELECT c.id, c.name,
                CASE WHEN cp.name IS NOT NULL THEN cp.name || ' > ' || c.name ELSE c.name END,
                c.monthly_budget,
                {actual}
         FROM categories c
         LEFT JOIN categories cp ON c.parent_id = cp.id
         WHERE c.monthly_budget IS NOT NULL AND c.monthly_budget > 0
         ORDER BY 5 DESC"
    );

    let mut q = sqlx::query_as::<_, (i64, String, String, f64, f64)>(&query);
    for p in &params {
        q = q.bind(p);
    }
    let rows = q
        .fetch_all(&*pool)
        .await
        .map_err(|e| format!("DB query error: {}", e))?;

    Ok(rows
        .into_iter()
        .map(|(category_id, name, path, monthly_budget, actual)| BudgetStatus {
            category_id,
            name,
            path,
            monthly_budget,
            actual,
            percentage: if monthly_budget > 0.0 {
                (actual / monthly_budget) * 100.0
            } else {
                0.0
            },
        })
        .collect())
}