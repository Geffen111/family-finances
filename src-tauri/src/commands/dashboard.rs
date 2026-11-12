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

// A category's **net** spend over the grouped rows: what went out, less
// anything credited back to it. Rebates, refunds and reimbursements are
// categorised against the cost they offset, so the net is the only figure
// that means anything -- a $400 specialist visit with a $332 Medicare rebate
// cost $68, and reporting the $400 as spending while the $332 shows up under
// income reads a refund as money coming in. The forecast nets the same way
// (`services::forecast::compute_baselines`), which is what keeps a category's
// projection and its history telling the same story.
//
// Pair it with `HAVING <alias> > 0` in any spending view: a category that
// nets the other way round isn't spending, and belongs in the income one.
const NET_SPEND: &str = "CAST(COALESCE(SUM(t.debit), 0) - COALESCE(SUM(t.credit), 0) AS REAL)";

// The same netting the other way up: what came into the category less
// anything paid back out of it.
const NET_INCOME: &str = "CAST(COALESCE(SUM(t.credit), 0) - COALESCE(SUM(t.debit), 0) AS REAL)";

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
    dashboard_summary(&pool, start_date, end_date).await
}

/// Plain pool, so the netting has a test. See `NET_SPEND`.
pub async fn dashboard_summary(
    pool: &SqlitePool,
    start_date: Option<String>,
    end_date: Option<String>,
) -> Result<DashboardSummary, String> {
    // The headline figures are the netted categories added back up, not raw
    // credits and debits: a rebate inside a spending category comes off that
    // category instead of inflating the income tile. The grouping is what
    // makes the netting mean something -- netted over everything at once,
    // income and expenses would simply cancel each other out.
    //
    // `tx_effective` rather than `transactions` because the grouping is by
    // category: a split transaction's debit has to land on each split's own
    // category for its net to be right. The two agree on the grand totals.
    let mut inner = String::from(
        "SELECT t.category_id, COALESCE(SUM(t.debit), 0) - COALESCE(SUM(t.credit), 0) AS net
         FROM tx_effective t WHERE 1=1",
    );
    inner.push_str(EXCLUDE_BUDGET);
    let params = apply_date_filters(&mut inner, &start_date, &end_date, "t");
    inner.push_str(" GROUP BY t.category_id");

    let totals_sql = format!(
        "SELECT CAST(COALESCE(SUM(CASE WHEN net < 0 THEN -net END), 0) AS REAL),
                CAST(COALESCE(SUM(CASE WHEN net > 0 THEN net END), 0) AS REAL)
         FROM ({inner})"
    );
    let mut q = sqlx::query_as::<_, (f64, f64)>(&totals_sql);
    for p in &params {
        q = q.bind(p);
    }
    let (total_income, total_expenses) = q
        .fetch_one(pool)
        .await
        .map_err(|e| format!("DB query error: {}", e))?;

    let mut cnt_base = String::from("SELECT COUNT(*) FROM transactions t WHERE 1=1");
    let params3 = apply_date_filters(&mut cnt_base, &start_date, &end_date, "t");
    let mut q = sqlx::query_scalar::<_, i64>(&cnt_base);
    for p in &params3 {
        q = q.bind(p);
    }
    let transaction_count: i64 = q.fetch_one(pool).await.unwrap_or(0);

    let mut top_base = format!(
        "SELECT COALESCE(c.name, 'Uncategorised'), {NET_SPEND} as total
         FROM tx_effective t
         LEFT JOIN categories c ON t.category_id = c.id
         WHERE 1=1"
    );
    top_base.push_str(EXCLUDE_BUDGET);
    let params4 = apply_date_filters(&mut top_base, &start_date, &end_date, "t");
    top_base.push_str(" GROUP BY c.name HAVING total > 0 ORDER BY total DESC LIMIT 1");

    let mut q = sqlx::query_as::<_, (String, f64)>(&top_base);
    for p in &params4 {
        q = q.bind(p);
    }
    let (top_category, top_category_amount) = q.fetch_optional(pool).await
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
    spending_by_category(&pool, start_date, end_date).await
}

/// Plain pool, so the netting has a test. See `NET_SPEND`.
pub async fn spending_by_category(
    pool: &SqlitePool,
    start_date: Option<String>,
    end_date: Option<String>,
) -> Result<Vec<CategorySpending>, String> {
    // No `t.debit > 0` filter any more: the credits have to be in the group
    // for the netting to see them. The count is then every transaction behind
    // the figure, the rebates included, which is what it should be.
    let mut query = format!(
        "SELECT
            COALESCE(c.name, 'Uncategorised'),
            CASE WHEN cp.name IS NOT NULL THEN cp.name || ' > ' || c.name ELSE COALESCE(c.name, 'Uncategorised') END,
            {NET_SPEND} AS total,
            COUNT(*)
         FROM tx_effective t
         LEFT JOIN categories c ON t.category_id = c.id
         LEFT JOIN categories cp ON c.parent_id = cp.id
         WHERE 1=1"
    );
    query.push_str(EXCLUDE_BUDGET);
    let params = apply_date_filters(&mut query, &start_date, &end_date, "t");
    query.push_str(" GROUP BY c.name HAVING total > 0 ORDER BY total DESC");

    let mut q = sqlx::query_as::<_, (String, String, f64, i64)>(&query);
    for p in &params {
        q = q.bind(p);
    }
    let rows = q.fetch_all(pool)
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
    let mut query = format!(
        "SELECT t.category_id, {NET_SPEND} AS net, COUNT(*)
         FROM tx_effective t
         WHERE 1=1",
    );
    query.push_str(EXCLUDE_BUDGET);
    let params = apply_date_filters(&mut query, &start_date, &end_date, "t");
    // A category whose refunds outweigh its spending is dropped rather than
    // subtracted from its parent: a negative slice of a spending breakdown is
    // not a thing, and the surplus shows up as income instead.
    query.push_str(" GROUP BY t.category_id HAVING net > 0");

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
    // Netted per category *within each month*, then added up — the same rule
    // as the summary tiles, so the bars and the tiles can't disagree.
    let mut inner = String::from(
        "SELECT strftime('%Y-%m', t.date) AS ym, t.category_id,
                COALESCE(SUM(t.debit), 0) - COALESCE(SUM(t.credit), 0) AS net
         FROM tx_effective t WHERE 1=1"
    );
    inner.push_str(EXCLUDE_BUDGET);
    let params = apply_date_filters(&mut inner, &start_date, &end_date, "t");
    inner.push_str(" GROUP BY ym, t.category_id");

    let query = format!(
        "SELECT ym,
                CAST(COALESCE(SUM(CASE WHEN net < 0 THEN -net END), 0) AS REAL),
                CAST(COALESCE(SUM(CASE WHEN net > 0 THEN net END), 0) AS REAL)
         FROM ({inner}) GROUP BY ym ORDER BY ym ASC"
    );

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
    // Netted per category-month, and deliberately not clamped at zero: a
    // month whose rebates landed after the bills really did cost less than
    // nothing, and flattening that to $0 would hide where the money went.
    let mut query = format!(
        "SELECT c.id, COALESCE(c.name, 'Uncategorised'), strftime('%Y-%m', t.date), {NET_SPEND}
         FROM tx_effective t
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
    budget_status(&pool, start_date, end_date).await
}

/// Plain pool, so the netting has a test. See `NET_SPEND`.
pub async fn budget_status(
    pool: &SqlitePool,
    start_date: Option<String>,
    end_date: Option<String>,
) -> Result<Vec<BudgetStatus>, String> {
    // Actual spend per budgeted category, over the given period. The date
    // filter lives inside a correlated subquery so categories with no
    // spending in the period still appear (actual = 0).
    // Net spend, so a rebate counts against the budget it was spent out of.
    let mut actual = String::from(
        "CAST(COALESCE((SELECT COALESCE(SUM(t.debit), 0) - COALESCE(SUM(t.credit), 0)
                        FROM tx_effective t WHERE t.category_id = c.id",
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
        .fetch_all(pool)
        .await
        .map_err(|e| format!("DB query error: {}", e))?;

    let mut result = Vec::with_capacity(rows.len());
    for (category_id, name, path, monthly_budget, rollover, actual) in rows {
        // Refunds can outrun the spending in a short period. "Minus $40 of a
        // $300 budget" isn't a thing a progress bar can draw, and the honest
        // reading is simply that nothing was spent.
        let actual = actual.max(0.0);
        // Carryover is only meaningful for rollover categories when we know the
        // period start: bank = monthly_budget × (months elapsed before the
        // period) − spend before the period.
        let carryover = if rollover {
            if let Some(sd) = &start_date {
                compute_carryover(pool, category_id, monthly_budget, sd).await?
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
        // Net, like the period's own spend: a rebate banked before the period
        // started is budget that was never really spent.
        "SELECT MIN(date), CAST(COALESCE(SUM(debit), 0) - COALESCE(SUM(credit), 0) AS REAL)
         FROM tx_effective WHERE category_id = ? AND date < ?",
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
pub struct IncomeSource {
    pub category_name: String,
    pub total: f64,
    pub percentage: f64,
}

/// Net income grouped by category over the period: what came in, less anything
/// paid back out of the same category. A category that nets the other way is
/// spending, not income, and is left out — which is what keeps a Medicare
/// rebate off this chart and against the medical bill it belongs to.
///
/// Reads `tx_effective` because the grouping is by category: splits are
/// debit-only, so a split transaction's debits have to land on their own
/// categories before anything is subtracted.
#[tauri::command]
pub async fn get_income_by_category(
    pool: State<'_, SqlitePool>,
    start_date: Option<String>,
    end_date: Option<String>,
) -> Result<Vec<IncomeSource>, String> {
    income_by_category(&pool, start_date, end_date).await
}

/// Plain pool, so the netting has a test. See `NET_INCOME`.
pub async fn income_by_category(
    pool: &SqlitePool,
    start_date: Option<String>,
    end_date: Option<String>,
) -> Result<Vec<IncomeSource>, String> {
    let mut query = format!(
        "SELECT COALESCE(c.name, 'Uncategorised'), {NET_INCOME} AS total
         FROM tx_effective t
         LEFT JOIN categories c ON t.category_id = c.id
         WHERE 1=1",
    );
    query.push_str(EXCLUDE_BUDGET);
    let params = apply_date_filters(&mut query, &start_date, &end_date, "t");
    query.push_str(" GROUP BY c.name HAVING total > 0 ORDER BY total DESC");

    let mut q = sqlx::query_as::<_, (String, f64)>(&query);
    for p in &params {
        q = q.bind(p);
    }
    let rows = q.fetch_all(pool).await.map_err(|e| format!("DB query error: {}", e))?;
    let grand: f64 = rows.iter().map(|r| r.1).sum();
    Ok(rows
        .into_iter()
        .map(|(category_name, total)| IncomeSource {
            category_name,
            total,
            percentage: if grand > 0.0 { (total / grand) * 100.0 } else { 0.0 },
        })
        .collect())
}

async fn movers_spend(
    pool: &SqlitePool,
    s: NaiveDate,
    e: NaiveDate,
) -> Result<Vec<(String, f64)>, String> {
    sqlx::query_as::<_, (String, f64)>(&format!(
        "SELECT COALESCE(c.name, 'Uncategorised'), {NET_SPEND} AS total
         FROM tx_effective t
         LEFT JOIN categories c ON t.category_id = c.id
         WHERE t.date >= ? AND t.date <= ?
           AND NOT EXISTS (SELECT 1 FROM categories xc WHERE xc.id = t.category_id AND xc.exclude_from_budget = 1)
         GROUP BY c.name HAVING total > 0",
    ))
    .bind(s.format("%Y-%m-%d").to_string())
    .bind(e.format("%Y-%m-%d").to_string())
    .fetch_all(pool)
    .await
    .map_err(|err| format!("DB query error: {}", err))
}

#[derive(Debug, serde::Serialize)]
pub struct CategoryMover {
    pub category_name: String,
    pub current: f64,
    pub previous: f64,
    pub delta: f64,
}

/// Biggest changes in category spend versus the immediately-preceding period of
/// equal length. Needs an explicit window; `end_date` defaults to today.
/// Split-aware (reads tx_effective).
#[tauri::command]
pub async fn get_category_movers(
    pool: State<'_, SqlitePool>,
    start_date: String,
    end_date: Option<String>,
) -> Result<Vec<CategoryMover>, String> {
    let start = NaiveDate::parse_from_str(&start_date, "%Y-%m-%d")
        .map_err(|_| "Invalid start_date".to_string())?;
    let end = match &end_date {
        Some(e) => NaiveDate::parse_from_str(e, "%Y-%m-%d").map_err(|_| "Invalid end_date".to_string())?,
        None => chrono::Local::now().naive_local().date(),
    };
    if end < start {
        return Err("end_date is before start_date".to_string());
    }
    // Previous window: same length, ending the day before `start`.
    let span = end - start;
    let prev_end = start - chrono::Duration::days(1);
    let prev_start = prev_end - span;

    let cur_rows = movers_spend(&pool, start, end).await?;
    let prev_rows = movers_spend(&pool, prev_start, prev_end).await?;

    let mut map: HashMap<String, (f64, f64)> = HashMap::new();
    for (name, total) in cur_rows {
        map.entry(name).or_default().0 = total;
    }
    for (name, total) in prev_rows {
        map.entry(name).or_default().1 = total;
    }
    let mut movers: Vec<CategoryMover> = map
        .into_iter()
        .map(|(category_name, (current, previous))| CategoryMover {
            category_name,
            current,
            previous,
            delta: current - previous,
        })
        .collect();
    movers.sort_by(|a, b| b.delta.abs().partial_cmp(&a.delta.abs()).unwrap());
    movers.truncate(8);
    Ok(movers)
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

    let rows = sqlx::query_as::<_, (i64, String, Option<f64>, f64)>(&format!(
        "SELECT c.id,
                CASE WHEN cp.name IS NOT NULL THEN cp.name || ' > ' || c.name ELSE c.name END,
                c.monthly_budget,
                {NET_SPEND} AS spend
         FROM categories c
         LEFT JOIN categories cp ON c.parent_id = cp.id
         JOIN tx_effective t ON t.category_id = c.id
             AND t.date >= ? AND t.date <= ?
         WHERE c.exclude_from_budget = 0
         GROUP BY c.id
         HAVING spend > 0
         ORDER BY spend DESC",
    ))
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

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    /// A migrated database holding one month of a household that claims money
    /// back: $400 of specialist visits with a $300 Medicare rebate filed
    /// against the same category, $150 of groceries, and a $5,000 pay.
    async fn pool_with_a_rebate() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        for sql in [
            "INSERT INTO accounts (id, name) VALUES (1, 'Everyday')",
            "INSERT INTO categories (id, name, monthly_budget) VALUES (10, 'Medical', 300)",
            "INSERT INTO categories (id, name) VALUES (11, 'Groceries'), (12, 'Salary')",
            "INSERT INTO transactions (account_id, category_id, date, description, debit)
             VALUES (1, 10, '2026-03-04', 'Specialist', 400),
                    (1, 11, '2026-03-06', 'Coles', 150)",
            "INSERT INTO transactions (account_id, category_id, date, description, credit)
             VALUES (1, 10, '2026-03-20', 'Medicare rebate', 300),
                    (1, 12, '2026-03-15', 'Pay', 5000)",
        ] {
            sqlx::query(sql).execute(&pool).await.unwrap();
        }
        pool
    }

    fn march() -> (Option<String>, Option<String>) {
        (Some("2026-03-01".to_string()), Some("2026-03-31".to_string()))
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 0.01
    }

    /// The rebate comes off the medical bill instead of being added to the
    /// income tile. $5,300 of credits is $5,000 of income and a $100 cost.
    #[tokio::test]
    async fn the_summary_tiles_report_netted_categories() {
        let pool = pool_with_a_rebate().await;
        let (s, e) = march();
        let summary = dashboard_summary(&pool, s, e).await.unwrap();

        assert!(close(summary.total_income, 5000.0), "income {}", summary.total_income);
        assert!(close(summary.total_expenses, 250.0), "expenses {}", summary.total_expenses);
        assert!(close(summary.net, 4750.0), "net {}", summary.net);
        // Groceries at $150 now outspends the $100 net of the medical visit;
        // gross, the $400 would have taken the top slot.
        assert_eq!(summary.top_category, "Groceries");
        assert!(close(summary.top_category_amount, 150.0));
        // Every row still counts, the rebate included.
        assert_eq!(summary.transaction_count, 4);
    }

    /// The netting must not quietly change the net: whatever moves between
    /// the income and expense tiles, the difference is what it always was.
    #[tokio::test]
    async fn netting_leaves_the_net_where_it_was() {
        let pool = pool_with_a_rebate().await;
        let (s, e) = march();
        let summary = dashboard_summary(&pool, s.clone(), e.clone()).await.unwrap();

        let gross: (f64, f64) = sqlx::query_as(
            "SELECT CAST(COALESCE(SUM(credit), 0) AS REAL), CAST(COALESCE(SUM(debit), 0) AS REAL)
             FROM transactions WHERE date >= ? AND date <= ?",
        )
        .bind(s.unwrap())
        .bind(e.unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();

        assert!(close(gross.0 - gross.1, summary.net), "net moved");
        // ...and it is a different split of the same number.
        assert!(gross.0 > summary.total_income);
    }

    /// Spending by category reports the net cost, and a month whose rebates
    /// outran its bills drops out of a *spending* breakdown entirely.
    #[tokio::test]
    async fn spending_by_category_is_net_and_drops_categories_in_credit() {
        let pool = pool_with_a_rebate().await;
        sqlx::query(
            "INSERT INTO transactions (account_id, category_id, date, description, credit)
             VALUES (1, 11, '2026-03-28', 'Grocery refund', 500)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let (s, e) = march();
        let rows = spending_by_category(&pool, s, e).await.unwrap();
        let names: Vec<&str> = rows.iter().map(|r| r.category_name.as_str()).collect();
        assert_eq!(names, vec!["Medical"], "got {names:?}");
        assert!(close(rows[0].total, 100.0), "medical {}", rows[0].total);
        // Percentages are of the netted total, so they still add to 100.
        assert!(close(rows[0].percentage, 100.0));
        // Both the visit and the rebate sit behind that figure.
        assert_eq!(rows[0].transaction_count, 2);
    }

    /// A rebate is not an income source. It belongs against the bill, so the
    /// income chart shows the pay alone.
    #[tokio::test]
    async fn income_by_category_leaves_out_a_category_that_nets_to_a_cost() {
        let pool = pool_with_a_rebate().await;
        let (s, e) = march();
        let rows = income_by_category(&pool, s, e).await.unwrap();

        assert_eq!(rows.len(), 1, "got {rows:?}");
        assert_eq!(rows[0].category_name, "Salary");
        assert!(close(rows[0].total, 5000.0));
        assert!(close(rows[0].percentage, 100.0));
    }

    /// Money claimed back is budget that wasn't spent: $400 of visits less a
    /// $300 rebate is a third of a $300 budget, not 133% of it.
    #[tokio::test]
    async fn a_rebate_counts_against_the_budget_it_was_spent_from() {
        let pool = pool_with_a_rebate().await;
        let (s, e) = march();
        let rows = budget_status(&pool, s, e).await.unwrap();
        let medical = rows.iter().find(|r| r.name == "Medical").unwrap();

        assert!(close(medical.actual, 100.0), "actual {}", medical.actual);
        assert!(close(medical.percentage, 100.0 / 300.0 * 100.0), "pct {}", medical.percentage);
    }

    /// Refunds can outrun spending over a short window. A negative "actual"
    /// is not something a progress bar can draw, and the honest reading is
    /// that nothing was spent.
    #[tokio::test]
    async fn a_category_more_than_refunded_reads_as_nothing_spent() {
        let pool = pool_with_a_rebate().await;
        sqlx::query(
            "INSERT INTO transactions (account_id, category_id, date, description, credit)
             VALUES (1, 10, '2026-03-25', 'Insurance payout', 250)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let (s, e) = march();
        let rows = budget_status(&pool, s, e).await.unwrap();
        let medical = rows.iter().find(|r| r.name == "Medical").unwrap();

        assert!(close(medical.actual, 0.0), "actual {}", medical.actual);
        assert!(medical.percentage >= 0.0, "pct {}", medical.percentage);
    }
}
