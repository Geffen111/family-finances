use crate::models::{
    CategorySpending, CategorySpendingChild, CategorySpendingGroup, CategoryTrend, DashboardSummary,
    MonthlyTrend,
};
use chrono::{Datelike, NaiveDate};
use sqlx::SqlitePool;
use std::collections::{BTreeMap, BTreeSet, HashMap};
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
    let mut base = String::from("SELECT CAST(COALESCE(SUM(t.credit), 0) AS REAL) FROM transactions t WHERE 1=1");
    base.push_str(EXCLUDE_BUDGET);
    let params = apply_date_filters(&mut base, &start_date, &end_date, "t");

    let mut q = sqlx::query_scalar::<_, f64>(&base);
    for p in &params {
        q = q.bind(p);
    }
    let total_income: f64 = q.fetch_one(&*pool).await.unwrap_or(0.0);

    let mut exp_base = String::from("SELECT CAST(COALESCE(SUM(t.debit), 0) AS REAL) FROM transactions t WHERE 1=1");
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
        "SELECT COALESCE(c.name, 'Uncategorised'), CAST(COALESCE(SUM(t.debit), 0) AS REAL) as total
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
            CAST(COALESCE(SUM(t.debit), 0) AS REAL),
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

/// Spending for the period grouped into a parent → child tree. Each parent's
/// `total` rolls up its own direct spend plus every child's. Assumes the app's
/// two-level category hierarchy (a category has at most one parent level).
#[tauri::command]
pub async fn get_category_spending_tree(
    pool: State<'_, SqlitePool>,
    start_date: Option<String>,
    end_date: Option<String>,
) -> Result<Vec<CategorySpendingGroup>, String> {
    // All categories, so we can resolve names/parents even for parents that have
    // no direct spend of their own in the period.
    let cats = sqlx::query_as::<_, (i64, String, Option<i64>)>(
        "SELECT id, name, parent_id FROM categories",
    )
    .fetch_all(&*pool)
    .await
    .map_err(|e| format!("DB query error: {}", e))?;
    let cat_map: HashMap<i64, (String, Option<i64>)> =
        cats.into_iter().map(|(id, name, pid)| (id, (name, pid))).collect();

    // Direct spend per category over the period (one row per category id; the
    // NULL id row is uncategorised spend).
    let mut query = String::from(
        "SELECT t.category_id, CAST(COALESCE(SUM(t.debit), 0) AS REAL), COUNT(*)
         FROM transactions t
         WHERE t.debit > 0",
    );
    query.push_str(EXCLUDE_BUDGET);
    let params = apply_date_filters(&mut query, &start_date, &end_date, "t");
    query.push_str(" GROUP BY t.category_id");

    let mut q = sqlx::query_as::<_, (Option<i64>, f64, i64)>(&query);
    for p in &params {
        q = q.bind(p);
    }
    let rows = q
        .fetch_all(&*pool)
        .await
        .map_err(|e| format!("DB query error: {}", e))?;

    // Accumulate into top-level groups keyed by the parent category id
    // (None = uncategorised).
    let mut groups: HashMap<Option<i64>, CategorySpendingGroup> = HashMap::new();

    for (cid, total, count) in rows {
        match cid {
            None => {
                let g = groups.entry(None).or_insert_with(|| CategorySpendingGroup {
                    category_id: None,
                    name: "Uncategorised".to_string(),
                    direct_total: 0.0,
                    total: 0.0,
                    transaction_count: 0,
                    children: Vec::new(),
                });
                g.direct_total += total;
                g.total += total;
                g.transaction_count += count;
            }
            Some(id) => {
                let (name, parent_id) = cat_map
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(|| (format!("Category {}", id), None));
                match parent_id {
                    // Child category: roll into its parent group.
                    Some(pid) => {
                        let pname = cat_map
                            .get(&pid)
                            .map(|(n, _)| n.clone())
                            .unwrap_or_else(|| format!("Category {}", pid));
                        let g = groups.entry(Some(pid)).or_insert_with(|| CategorySpendingGroup {
                            category_id: Some(pid),
                            name: pname,
                            direct_total: 0.0,
                            total: 0.0,
                            transaction_count: 0,
                            children: Vec::new(),
                        });
                        g.total += total;
                        g.transaction_count += count;
                        g.children.push(CategorySpendingChild {
                            category_id: id,
                            name,
                            total,
                            transaction_count: count,
                        });
                    }
                    // Top-level category with direct spend.
                    None => {
                        let g = groups.entry(Some(id)).or_insert_with(|| CategorySpendingGroup {
                            category_id: Some(id),
                            name: name.clone(),
                            direct_total: 0.0,
                            total: 0.0,
                            transaction_count: 0,
                            children: Vec::new(),
                        });
                        // Name may already be set from a child; keep the real name.
                        g.name = name;
                        g.direct_total += total;
                        g.total += total;
                        g.transaction_count += count;
                    }
                }
            }
        }
    }

    let mut result: Vec<CategorySpendingGroup> = groups.into_values().collect();
    for g in &mut result {
        g.children.sort_by(|a, b| b.total.partial_cmp(&a.total).unwrap_or(std::cmp::Ordering::Equal));
    }
    result.sort_by(|a, b| b.total.partial_cmp(&a.total).unwrap_or(std::cmp::Ordering::Equal));

    Ok(result)
}

#[tauri::command]
pub async fn get_monthly_trends(
    pool: State<'_, SqlitePool>,
    start_date: Option<String>,
    end_date: Option<String>,
) -> Result<Vec<MonthlyTrend>, String> {
    let mut query = String::from(
        "SELECT strftime('%Y-%m', t.date), CAST(COALESCE(SUM(t.credit), 0) AS REAL), CAST(COALESCE(SUM(t.debit), 0) AS REAL)
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
        "SELECT c.id, COALESCE(c.name, 'Uncategorised'), strftime('%Y-%m', t.date), CAST(COALESCE(SUM(t.debit), 0) AS REAL)
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
    /// Whether this category rolls unspent budget forward.
    pub rollover: bool,
    /// Carried-over balance entering the period (rollover categories only):
    /// positive = budget banked from prior months, negative = overspent.
    pub carryover: f64,
    /// Effective budget for the period including carryover (monthly_budget +
    /// carryover, never below zero). For non-rollover categories this equals
    /// monthly_budget.
    pub available: f64,
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
        "CAST(COALESCE((SELECT SUM(t.debit) FROM transactions t WHERE t.category_id = c.id",
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
    actual.push_str("), 0) AS REAL)");

    let query = format!(
        "SELECT c.id, c.name,
                CASE WHEN cp.name IS NOT NULL THEN cp.name || ' > ' || c.name ELSE c.name END,
                c.monthly_budget,
                c.rollover,
                {actual}
         FROM categories c
         LEFT JOIN categories cp ON c.parent_id = cp.id
         WHERE c.monthly_budget IS NOT NULL AND c.monthly_budget > 0
         ORDER BY 6 DESC"
    );

    let mut q = sqlx::query_as::<_, (i64, String, String, f64, bool, f64)>(&query);
    for p in &params {
        q = q.bind(p);
    }
    let rows = q
        .fetch_all(&*pool)
        .await
        .map_err(|e| format!("DB query error: {}", e))?;

    let mut result = Vec::with_capacity(rows.len());
    for (category_id, name, path, monthly_budget, rollover, actual) in rows {
        // Carryover is only meaningful for rollover categories when we know the
        // period start: bank = monthly_budget × (months elapsed before the
        // period) − spend before the period.
        let carryover = if rollover {
            if let Some(sd) = &start_date {
                compute_carryover(&pool, category_id, monthly_budget, sd).await?
            } else {
                0.0
            }
        } else {
            0.0
        };
        let available = if rollover {
            (monthly_budget + carryover).max(0.0)
        } else {
            monthly_budget
        };
        let denom = if available > 0.0 { available } else { monthly_budget };
        result.push(BudgetStatus {
            category_id,
            name,
            path,
            monthly_budget,
            actual,
            percentage: if denom > 0.0 { (actual / denom) * 100.0 } else { 0.0 },
            rollover,
            carryover,
            available,
        });
    }
    Ok(result)
}

/// Banked (or overspent) budget a rollover category carries into `start_date`.
/// Assumes the current `monthly_budget` applied to every prior month from the
/// category's first transaction up to (but excluding) the period start.
async fn compute_carryover(
    pool: &SqlitePool,
    category_id: i64,
    monthly_budget: f64,
    start_date: &str,
) -> Result<f64, String> {
    let row = sqlx::query_as::<_, (Option<String>, f64)>(
        "SELECT MIN(date), CAST(COALESCE(SUM(debit), 0) AS REAL)
         FROM transactions WHERE category_id = ? AND date < ?",
    )
    .bind(category_id)
    .bind(start_date)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("DB query error: {}", e))?;

    let (min_date, spend_before) = row;
    let Some(min_date) = min_date else {
        return Ok(0.0);
    };
    let prior_months = months_between(&min_date[..7], &start_date[..7]);
    Ok(monthly_budget * prior_months as f64 - spend_before)
}

/// Whole calendar months from `from` to `to`, both "YYYY-MM". Negative or zero
/// clamps to 0.
fn months_between(from: &str, to: &str) -> i64 {
    let parse = |s: &str| -> Option<(i64, i64)> {
        let mut it = s.split('-');
        Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?))
    };
    match (parse(from), parse(to)) {
        (Some((fy, fm)), Some((ty, tm))) => ((ty - fy) * 12 + (tm - fm)).max(0),
        _ => 0,
    }
}

#[derive(Debug, serde::Serialize)]
pub struct BudgetSuggestion {
    pub category_id: i64,
    pub path: String,
    /// Mean monthly spend over the look-back window, rounded to whole dollars.
    pub suggested: f64,
    pub current_budget: Option<f64>,
}

/// Suggests a monthly budget per category from recent spending: total debits
/// over the last `months` calendar months divided by `months`. Excludes
/// budget-excluded categories (e.g. internal transfers) and only returns
/// categories that actually had spending in the window.
#[tauri::command]
pub async fn get_budget_suggestions(
    pool: State<'_, SqlitePool>,
    months: Option<i64>,
) -> Result<Vec<BudgetSuggestion>, String> {
    let months = months.unwrap_or(3).clamp(1, 24);
    // Window: first day of the month `months` ago, up to today. Anchoring to a
    // month boundary keeps the divisor honest (whole months of data).
    let today = chrono::Local::now().naive_local().date();
    let start = (today - chrono::Months::new(months as u32))
        .with_day(1)
        .unwrap_or(today);
    let start_s = start.format("%Y-%m-%d").to_string();
    let end_s = today.format("%Y-%m-%d").to_string();

    let rows = sqlx::query_as::<_, (i64, String, Option<f64>, f64)>(
        "SELECT c.id,
                CASE WHEN cp.name IS NOT NULL THEN cp.name || ' > ' || c.name ELSE c.name END,
                c.monthly_budget,
                CAST(COALESCE(SUM(t.debit), 0) AS REAL) AS spend
         FROM categories c
         LEFT JOIN categories cp ON c.parent_id = cp.id
         JOIN transactions t ON t.category_id = c.id
             AND t.date >= ? AND t.date <= ?
         WHERE c.exclude_from_budget = 0
         GROUP BY c.id
         HAVING spend > 0
         ORDER BY spend DESC",
    )
    .bind(&start_s)
    .bind(&end_s)
    .fetch_all(&*pool)
    .await
    .map_err(|e| format!("DB query error: {}", e))?;

    Ok(rows
        .into_iter()
        .map(|(category_id, path, current_budget, spend)| BudgetSuggestion {
            category_id,
            path,
            suggested: (spend / months as f64).round(),
            current_budget,
        })
        .collect())
}

#[derive(Debug, serde::Serialize)]
pub struct NetWorthPoint {
    pub month: String,
    pub label: String,
    pub net_worth: f64,
    // Split so the dashboard can shade assets vs. liabilities separately.
    // `assets` = sum of asset-account balances; `liabilities` = amount owed on
    // liability accounts as a positive number; net_worth = assets - liabilities.
    pub assets: f64,
    pub liabilities: f64,
}

fn next_month(ym: &str) -> String {
    let mut parts = ym.split('-');
    let y: i32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(2024);
    let m: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(1);
    if m == 12 {
        format!("{:04}-01", y + 1)
    } else {
        format!("{:04}-{:02}", y, m + 1)
    }
}

#[tauri::command]
pub async fn get_net_worth_trend(pool: State<'_, SqlitePool>) -> Result<Vec<NetWorthPoint>, String> {
    // Liabilities subtract, assets add.
    let accounts = sqlx::query_as::<_, (i64, String)>("SELECT id, type FROM accounts")
        .fetch_all(&*pool)
        .await
        .map_err(|e| format!("DB query error: {}", e))?;
    let sign = |acct: i64| -> f64 {
        accounts
            .iter()
            .find(|(id, _)| *id == acct)
            .map(|(_, t)| if t == "liability" { -1.0 } else { 1.0 })
            .unwrap_or(1.0)
    };

    // Last known balance per account per month (rows are date-ordered, so a
    // later row in the same month overwrites the earlier one).
    let rows = sqlx::query_as::<_, (i64, String, f64)>(
        "SELECT account_id, date, balance FROM transactions \
         WHERE balance IS NOT NULL ORDER BY date ASC",
    )
    .fetch_all(&*pool)
    .await
    .map_err(|e| format!("DB query error: {}", e))?;

    if rows.is_empty() {
        return Ok(Vec::new());
    }

    let mut per_account: BTreeMap<i64, BTreeMap<String, f64>> = BTreeMap::new();
    let mut months: BTreeSet<String> = BTreeSet::new();
    for (acct, date, balance) in rows {
        if date.len() < 7 {
            continue;
        }
        let month = date[..7].to_string();
        per_account.entry(acct).or_default().insert(month.clone(), balance);
        months.insert(month);
    }

    let (Some(first), Some(last)) = (months.iter().next().cloned(), months.iter().next_back().cloned())
    else {
        return Ok(Vec::new());
    };

    let mut result = Vec::new();
    let mut cur = first;
    loop {
        let mut assets = 0.0;
        let mut liabilities = 0.0;
        for (acct, balances) in &per_account {
            // Most recent balance for this account at or before `cur`.
            if let Some((_, bal)) = balances.range(..=cur.clone()).next_back() {
                if sign(*acct) < 0.0 {
                    // Liability balances are stored as a positive amount owed.
                    liabilities += bal;
                } else {
                    assets += bal;
                }
            }
        }
        result.push(NetWorthPoint {
            month: cur.clone(),
            label: month_label(&cur),
            net_worth: assets - liabilities,
            assets,
            liabilities,
        });
        if cur == last {
            break;
        }
        cur = next_month(&cur);
    }

    Ok(result)
}