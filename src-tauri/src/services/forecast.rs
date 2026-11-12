use crate::models::{
    CategoryBaseline, ForecastCategoryAmount, ForecastComparison, ForecastMonth, ForecastResult,
    ForecastTotals, Scenario, ScenarioAdjustment, ScenarioDefault,
};
use chrono::{Datelike, NaiveDate};
use sqlx::SqlitePool;
use std::collections::{HashMap, HashSet};

pub async fn calculate_forecast(
    pool: &SqlitePool,
    scenario: &Scenario,
    adjustments: &[ScenarioAdjustment],
    defaults: &ScenarioDefault,
    excluded_category_ids: &HashSet<i64>,
    months_ahead: i64,
    opening_cash: f64,
    cash_as_at: Option<String>,
) -> Result<ForecastResult, String> {
    let mut baselines =
        compute_baselines(pool, &scenario.base_start_date, &scenario.base_end_date).await?;


    let start_date = NaiveDate::parse_from_str(&scenario.base_end_date, "%Y-%m-%d")
        .map_err(|e| format!("Invalid base_end_date: {}", e))?
        .succ_opt()
        .unwrap_or_else(|| {
            NaiveDate::parse_from_str(&scenario.base_end_date, "%Y-%m-%d").unwrap()
        });

    let mut months = Vec::new();
    let mut totals = ForecastTotals {
        total_projected_income: 0.0,
        total_projected_expenses: 0.0,
        total_projected_net: 0.0,
    };

    // Runs forward from today's cash position, one month at a time, so the
    // line answers "where does this scenario leave the bank balance" rather
    // than just "what is each month's net".
    let mut running_cash = opening_cash;

    let adj_map: HashMap<i64, &ScenarioAdjustment> =
        adjustments.iter().map(|a| (a.category_id, a)).collect();
    let income_growth_monthly = defaults.income_growth_pct / 100.0 / 12.0;

    // The dated upcoming income and expenses, resolved once for the whole
    // window rather than queried per month. Only the categories on a scheduled
    // source read it, but it costs one query either way.
    let (schedule, _unlinked, _unlinked_items) =
        scheduled_by_month(pool, start_date, months_ahead).await?;

    // A category with nothing in the base period has no baseline row at all,
    // so the month loop below would never visit it — and a scheduled source on
    // a brand-new expense (next year's school camp, in a category you have
    // never spent in) or a new income stream would project nothing whatever
    // you entered. Give those categories a zero-average row, on whichever side
    // their dated amounts actually fall.
    // Categories the budget flag keeps out of every aggregation. A synthesised
    // baseline row below would otherwise be a way back in for one of them.
    let budget_excluded: HashSet<i64> =
        sqlx::query_scalar::<_, i64>("SELECT id FROM categories WHERE exclude_from_budget = 1")
            .fetch_all(pool)
            .await
            .map_err(|e| format!("DB query error: {}", e))?
            .into_iter()
            .collect();

    for adj in adjustments {
        if adj.source == "average"
            || excluded_category_ids.contains(&adj.category_id)
            || budget_excluded.contains(&adj.category_id)
        {
            continue;
        }
        for is_income in [false, true] {
            let has_side = baselines
                .iter()
                .any(|b| b.category_id == adj.category_id && b.is_income == is_income);
            let has_scheduled = schedule
                .keys()
                .any(|(cid, inc, _)| *cid == adj.category_id && *inc == is_income);
            // The expense side is created even with nothing scheduled, so a
            // fixed extra still lands; the income side only when there is
            // scheduled income, or every adjusted category would sprout one.
            if has_side || (is_income && !has_scheduled) {
                continue;
            }
            baselines.push(CategoryBaseline {
                category_id: adj.category_id,
                category_path: get_category_path(pool, Some(adj.category_id))
                    .await
                    .unwrap_or_else(|| "Uncategorised".to_string()),
                monthly_avg: 0.0,
                is_income,
            });
        }
    }

    // A fixed amount is a single figure for the whole category and carries no
    // side of its own, so it has to land on exactly one of the category's
    // baseline rows. A spending category with one refund in the base period
    // has an income row as well as an expense row, and adding the fixed amount
    // to both put it into the projection twice, once each way: income and
    // expenses both moved by it and the net never budged, whatever you typed.
    // One $46 reimbursement in a dining category was enough to make a $5,000
    // figure look exactly like a $500 one. It lands on the side the category's
    // history is predominantly on, and on the expense side where there is none
    // (which is also the only side a synthesised row is guaranteed to have).
    let mut fixed_side: HashMap<i64, bool> = HashMap::new();
    for adj in adjustments {
        let side_total = |income: bool| {
            baselines
                .iter()
                .filter(|b| b.category_id == adj.category_id && b.is_income == income)
                .map(|b| b.monthly_avg)
                .sum::<f64>()
        };
        fixed_side.insert(adj.category_id, side_total(true) > side_total(false));
    }

    for m in 0..months_ahead {
        let current_date = add_months(&start_date, m);
        let month_key = current_date.format("%Y-%m").to_string();
        let label = current_date.format("%b %Y").to_string();

        let mut categories = Vec::new();
        let mut projected_income = 0.0;
        let mut projected_expenses = 0.0;

        for bl in &baselines {
            // Per-scenario exclusion: drop this category from this projection
            // only (the global exclude_from_budget filter already ran in
            // compute_baselines).
            if excluded_category_ids.contains(&bl.category_id) {
                continue;
            }

            // Dated amounts land on their own side of the category: a pay on
            // the income row, a bill on the expense row. A category can hold
            // both, and they must not cancel each other out.
            let scheduled = schedule
                .get(&(bl.category_id, bl.is_income, month_key.clone()))
                .copied()
                .unwrap_or(0.0);

            let amount = if let Some(adj) = adj_map.get(&bl.category_id) {
                // The row's own % scales the scheduled part too, so "everything
                // goes up 10%" still means something on a scheduled category.
                // A fixed amount is absolute and is never scaled.
                let pct = 1.0 + adj.adjustment_pct / 100.0;
                // None on the category's other side: see `fixed_side` above.
                // The other side keeps projecting its own average, so a refund
                // history isn't thrown away either.
                let fixed = match fixed_side.get(&bl.category_id) {
                    Some(side) if *side == bl.is_income => adj.fixed_amount,
                    _ => None,
                };
                match adj.source.as_str() {
                    // Dated expenses *instead of* the average: the right pick
                    // when the bill is already in the base period, since the
                    // average has it smeared across every month.
                    "scheduled" => scheduled * pct,
                    // Both — only correct when the scheduled item is new, or
                    // the category holds other ongoing spending as well.
                    "scheduled_plus_average" => scheduled * pct + bl.monthly_avg * pct,
                    // Dated expenses plus your own monthly figure for the rest,
                    // when the average isn't the number you'd use.
                    "scheduled_plus_fixed" => scheduled * pct + fixed.unwrap_or(0.0),
                    // "average" and anything unrecognised: unchanged behaviour.
                    _ => match fixed {
                        Some(f) => f,
                        None => bl.monthly_avg * pct,
                    },
                }
            } else {
                bl.monthly_avg * (1.0 + defaults.default_adjustment_pct / 100.0)
            };

            let compounded = if bl.is_income && income_growth_monthly > 0.0 {
                amount * (1.0 + income_growth_monthly).powi(m as i32)
            } else {
                amount
            };

            if bl.is_income {
                projected_income += compounded;
            } else {
                projected_expenses += compounded;
            }

            categories.push(ForecastCategoryAmount {
                category_id: bl.category_id,
                category_path: bl.category_path.clone(),
                amount: compounded,
                is_income: bl.is_income,
            });
        }

        let projected_net = projected_income - projected_expenses;
        running_cash += projected_net;

        totals.total_projected_income += projected_income;
        totals.total_projected_expenses += projected_expenses;
        totals.total_projected_net += projected_net;

        months.push(ForecastMonth {
            label,
            month_key,
            projected_income,
            projected_expenses,
            projected_net,
            projected_cash: running_cash,
            categories,
        });
    }

    Ok(ForecastResult {
        scenario_name: scenario.name.clone(),
        scenario_id: scenario.id,
        months,
        totals,
        opening_cash,
        cash_as_at,
    })
}

pub async fn compare_scenarios(
    pool: &SqlitePool,
    scenario_ids: Vec<i64>,
    months_ahead: i64,
) -> Result<ForecastComparison, String> {
    let base_scenario = build_base_scenario(pool).await?;
    let base_adjustments = Vec::new();
    let base_defaults = ScenarioDefault {
        id: 0,
        scenario_id: 0,
        default_adjustment_pct: 0.0,
        income_growth_pct: 0.0,
    };

    // Resolved once: the baseline and every scenario have to project from the
    // same opening balance, or the lines aren't comparable.
    let (opening_cash, cash_as_at) = crate::commands::cashflow::liquid_assets(pool).await?;

    let base_excluded = HashSet::new();
    let base = calculate_forecast(
        pool,
        &base_scenario,
        &base_adjustments,
        &base_defaults,
        &base_excluded,
        months_ahead,
        opening_cash,
        cash_as_at.clone(),
    )
    .await?;

    let mut scenarios = Vec::new();
    for sid in scenario_ids {
        let scenario = sqlx::query_as::<_, Scenario>("SELECT * FROM scenarios WHERE id = ?")
            .bind(sid)
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("DB error fetching scenario {}: {}", e, sid))?
            .ok_or_else(|| format!("Scenario {} not found", sid))?;

        let adjustments = sqlx::query_as::<_, ScenarioAdjustment>(
            "SELECT * FROM scenario_adjustments WHERE scenario_id = ?",
        )
        .bind(sid)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("DB error fetching adjustments: {}", e))?;

        let defaults = sqlx::query_as::<_, ScenarioDefault>(
            "SELECT * FROM scenario_defaults WHERE scenario_id = ?",
        )
        .bind(sid)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("DB error fetching defaults: {}", e))?
        .unwrap_or(ScenarioDefault {
            id: 0,
            scenario_id: sid,
            default_adjustment_pct: 0.0,
            income_growth_pct: 0.0,
        });

        let excluded: HashSet<i64> = sqlx::query_scalar::<_, i64>(
            "SELECT category_id FROM scenario_excluded_categories WHERE scenario_id = ?",
        )
        .bind(sid)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("DB error fetching exclusions: {}", e))?
        .into_iter()
        .collect();

        scenarios.push(
            calculate_forecast(
                pool,
                &scenario,
                &adjustments,
                &defaults,
                &excluded,
                months_ahead,
                opening_cash,
                cash_as_at.clone(),
            )
            .await?,
        );
    }

    Ok(ForecastComparison {
        base,
        scenarios,
        months_ahead,
    })
}

/// The dated upcoming income and expenses over a forecast window, bucketed by
/// `(category_id, is_income, "YYYY-MM")`, plus the total and count of
/// occurrences that aren't linked to a category (nothing can attribute those
/// to a category row, so the UI says so rather than quietly losing them).
///
/// The side is part of the key because a category's baseline is split the same
/// way: a scheduled pay has to land on the income row, not be subtracted.
///
/// `cashflow::expand_bills` does the cadence work — this only buckets.
pub async fn scheduled_by_month(
    pool: &SqlitePool,
    start: NaiveDate,
    months_ahead: i64,
) -> Result<(HashMap<(i64, bool, String), f64>, f64, i64), String> {
    let end = add_months(&start, months_ahead.max(0));
    let bills = crate::commands::cashflow::expand_bills(pool, start, end).await?;

    // `exclude_from_budget` keeps a category out of every forecast figure, and
    // a dated occurrence is no exception: a tracked transfer to savings is
    // real money moving, but counting it would double the household's
    // outgoings. `compute_baselines` filters the same flag, so dropping it
    // here is what keeps the two sides of the page agreeing.
    let excluded: HashSet<i64> =
        sqlx::query_scalar::<_, i64>("SELECT id FROM categories WHERE exclude_from_budget = 1")
            .fetch_all(pool)
            .await
            .map_err(|e| format!("DB query error: {}", e))?
            .into_iter()
            .collect();

    let mut map: HashMap<(i64, bool, String), f64> = HashMap::new();
    let mut unlinked = 0.0;
    let mut unlinked_items = 0;
    for b in bills {
        match b.category_id {
            // Linked, but to a category the forecast doesn't count. Not
            // "unlinked": there is nothing for the user to go and fix.
            Some(cid) if excluded.contains(&cid) => {}
            Some(cid) => {
                let month = b.due_date[..7].to_string();
                *map.entry((cid, b.is_income, month)).or_insert(0.0) += b.amount;
            }
            None => {
                unlinked += b.amount;
                unlinked_items += 1;
            }
        }
    }
    Ok((map, unlinked, unlinked_items))
}

pub async fn compute_baselines(
    pool: &SqlitePool,
    start_date: &str,
    end_date: &str,
) -> Result<Vec<CategoryBaseline>, String> {
    let use_end = if end_date.is_empty() { start_date } else { end_date };

    let rows = sqlx::query_as::<_, (Option<i64>, String, f64, f64)>(
        "SELECT t.category_id,
                COALESCE(c.name, 'Uncategorised') as category_name,
                CAST(COALESCE(SUM(t.debit), 0) AS REAL) as total_debit,
                CAST(COALESCE(SUM(t.credit), 0) AS REAL) as total_credit
         FROM tx_effective t
         LEFT JOIN categories c ON t.category_id = c.id
         WHERE t.date >= ? AND t.date <= ?
           AND NOT EXISTS (SELECT 1 FROM categories xc WHERE xc.id = t.category_id AND xc.exclude_from_budget = 1)
         GROUP BY t.category_id, c.name",
    )
    .bind(start_date)
    .bind(use_end)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("DB error baselines: {}", e))?;

    let (num_months, _, _) = base_period_months(pool, start_date, use_end).await?;

    let mut baselines = Vec::new();
    for (cid, cname, debit, credit) in &rows {
        let category_id = cid.unwrap_or(0);
        let category_path = if cname == "Uncategorised" {
            "Uncategorised".to_string()
        } else {
            get_category_path(pool, *cid).await.unwrap_or_else(|| cname.clone())
        };

        // Any credit at all, even a single refund, gives the category an
        // income row -- deliberately, since one category can hold both sides
        // (a pay and a bill, interest and fees) and they must not cancel out.
        // The first half of the old `credit > debit || credit > 0` condition
        // never decided anything; this is the same test, said once.
        if *credit > 0.0 {
            baselines.push(CategoryBaseline {
                category_id,
                category_path: category_path.clone(),
                monthly_avg: credit / num_months,
                is_income: true,
            });
        }
        if *debit > 0.0 {
            baselines.push(CategoryBaseline {
                category_id,
                category_path,
                monthly_avg: debit / num_months,
                is_income: false,
            });
        }
    }

    Ok(baselines)
}

async fn get_category_path(pool: &SqlitePool, category_id: Option<i64>) -> Option<String> {
    let id = category_id?;
    let result = sqlx::query_as::<_, (String,)>(
        "WITH RECURSIVE cat_path AS (
            SELECT id, name, parent_id, name AS path FROM categories WHERE id = ?
            UNION ALL
            SELECT c.id, c.name, c.parent_id, c.name || ' > ' || cp.path
            FROM categories c
            JOIN cat_path cp ON c.id = cp.parent_id
        )
        SELECT path FROM cat_path WHERE parent_id IS NULL",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .ok()?;
    result.map(|r| r.0)
}

async fn build_base_scenario(pool: &SqlitePool) -> Result<Scenario, String> {
    let row = sqlx::query_as::<_, (String, String)>(
        "SELECT COALESCE(MIN(date), ''), COALESCE(MAX(date), '') FROM transactions",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| format!("DB error base scenario: {}", e))?;

    Ok(Scenario {
        id: 0,
        name: "Baseline".to_string(),
        description: Some("Simple linear extension of current averages".to_string()),
        horizon: "monthly".to_string(),
        base_start_date: row.0,
        base_end_date: row.1,
        created_at: String::new(),
    })
}

/// Average Gregorian month length in days (365.2425 / 12).
const DAYS_PER_MONTH: f64 = 30.436875;

/// How many months of data a base period's averages are spread over, plus the
/// dates actually covered: `(months, data_start, data_end)`.
///
/// This used to be `end_month - start_month`, which isn't inclusive: 1 Jan to
/// 30 Jun counted as 5 months, inflating every category average by 20%. The
/// period is now clamped to the dates we hold transactions for — imports are
/// monthly, so a period ending "today" usually runs past the last import, and
/// averaging over days with no data yet would understate everything — then
/// measured in days and converted at the average month length.
pub async fn base_period_months(
    pool: &SqlitePool,
    start: &str,
    end: &str,
) -> Result<(f64, String, String), String> {
    let (min_date, max_date) = sqlx::query_as::<_, (Option<String>, Option<String>)>(
        "SELECT MIN(date), MAX(date) FROM transactions",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| format!("DB error base period: {}", e))?;

    let parse = |d: &str| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok();
    let (Some(mut s), Some(mut e)) = (parse(start), parse(end)) else {
        return Ok((1.0, start.to_string(), end.to_string()));
    };
    if let Some(first) = min_date.as_deref().and_then(parse) {
        s = s.max(first);
    }
    if let Some(last) = max_date.as_deref().and_then(parse) {
        e = e.min(last);
    }
    // No overlap with the data at all: there are no rows to average, so any
    // positive divisor gives the same (zero) result.
    let days = ((e - s).num_days() + 1).max(1);
    Ok((
        days as f64 / DAYS_PER_MONTH,
        s.format("%Y-%m-%d").to_string(),
        e.format("%Y-%m-%d").to_string(),
    ))
}

pub fn add_months(date: &NaiveDate, n: i64) -> NaiveDate {
    let total_months = date.month() as i64 - 1 + n;
    let year = date.year() + (total_months / 12) as i32;
    let month = (total_months % 12 + 1) as u32;
    let day = date.day().min(days_in_month(year, month));
    NaiveDate::from_ymd_opt(year, month, day).unwrap_or(*date)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    /// An in-memory DB holding transactions on each of the given dates.
    async fn pool_with_dates(dates: &[&str]) -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query("CREATE TABLE transactions (date TEXT NOT NULL)")
            .execute(&pool)
            .await
            .unwrap();
        for d in dates {
            sqlx::query("INSERT INTO transactions (date) VALUES (?)")
                .bind(d)
                .execute(&pool)
                .await
                .unwrap();
        }
        pool
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 0.01
    }

    #[tokio::test]
    async fn full_half_year_counts_six_months_not_five() {
        // Regression: `end_month - start_month` made this 5 (+20% averages).
        let pool = pool_with_dates(&["2025-12-01", "2026-12-31"]).await;
        let (m, s, e) = base_period_months(&pool, "2026-01-01", "2026-06-30").await.unwrap();
        assert!(close(m, 181.0 / DAYS_PER_MONTH), "got {m}");
        assert!(m > 5.9 && m < 6.0);
        assert_eq!((s.as_str(), e.as_str()), ("2026-01-01", "2026-06-30"));
    }

    #[tokio::test]
    async fn period_past_last_import_is_clamped_to_the_data() {
        // Base period ends "today" but the last import only reaches 31 Aug.
        let pool = pool_with_dates(&["2024-05-01", "2026-08-31"]).await;
        let (m, s, e) = base_period_months(&pool, "2026-07-01", "2026-09-13").await.unwrap();
        assert_eq!((s.as_str(), e.as_str()), ("2026-07-01", "2026-08-31"));
        assert!(close(m, 62.0 / DAYS_PER_MONTH), "got {m}");
    }

    #[tokio::test]
    async fn period_before_first_transaction_is_clamped_at_the_start() {
        let pool = pool_with_dates(&["2026-03-01", "2026-12-31"]).await;
        let (m, s, _) = base_period_months(&pool, "2026-01-01", "2026-03-31").await.unwrap();
        assert_eq!(s, "2026-03-01");
        assert!(close(m, 31.0 / DAYS_PER_MONTH), "got {m}");
    }

    #[tokio::test]
    async fn single_calendar_month_is_about_one() {
        let pool = pool_with_dates(&["2026-01-01", "2026-12-31"]).await;
        let (m, _, _) = base_period_months(&pool, "2026-01-01", "2026-01-31").await.unwrap();
        assert!(close(m, 31.0 / DAYS_PER_MONTH), "got {m}");
    }

    /// End to end through the real schema (every migration, including the
    /// tx_effective view): six months at $600/month must average $600 — the
    /// old month count divided by 5 and reported $720.
    #[tokio::test]
    async fn baselines_average_the_real_monthly_amounts() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        for sql in [
            "INSERT INTO accounts (id, name) VALUES (1, 'Everyday')",
            "INSERT INTO categories (id, name) VALUES (1, 'Food'), (3, 'Income'), (5, 'Other')",
            "INSERT INTO categories (id, name, parent_id) VALUES (2, 'Groceries', 1), (4, 'Salary', 3)",
            // Bookends in another category: real data spans years, so the
            // base period sits well inside the imported range.
            "INSERT INTO transactions (account_id, category_id, date, description, debit)
             VALUES (1, 5, '2025-06-01', 'older', 10), (1, 5, '2026-12-31', 'newer', 10)",
        ] {
            sqlx::query(sql).execute(&pool).await.unwrap();
        }
        for m in 1..=6 {
            sqlx::query(
                "INSERT INTO transactions (account_id, category_id, date, description, debit, credit)
                 VALUES (1, 2, ?, 'Coles', 600, 0), (1, 4, ?, 'Pay', 0, 5000)",
            )
            .bind(format!("2026-{m:02}-15"))
            .bind(format!("2026-{m:02}-10"))
            .execute(&pool)
            .await
            .unwrap();
        }

        let bl = compute_baselines(&pool, "2026-01-01", "2026-06-30").await.unwrap();
        let find = |cid: i64, income: bool| {
            bl.iter().find(|b| b.category_id == cid && b.is_income == income).unwrap().monthly_avg
        };
        let groceries = find(2, false);
        let salary = find(4, true);
        // 181 days is 5.95 average-length months, so within ~1% of the flat figure.
        assert!((groceries - 600.0).abs() < 6.0, "groceries averaged {groceries}");
        assert!((salary - 5000.0).abs() < 50.0, "salary averaged {salary}");
    }

    /// The cash line is the opening balance plus a running sum of each
    /// month's net -- not the net repeated, and not reset each month.
    #[tokio::test]
    async fn cash_projection_accumulates_from_the_opening_balance() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        for sql in [
            "INSERT INTO accounts (id, name) VALUES (1, 'Everyday')",
            "INSERT INTO categories (id, name) VALUES (1, 'Food'), (3, 'Income'), (5, 'Other')",
            "INSERT INTO categories (id, name, parent_id) VALUES (2, 'Groceries', 1), (4, 'Salary', 3)",
            "INSERT INTO transactions (account_id, category_id, date, description, debit)
             VALUES (1, 5, '2025-06-01', 'older', 10), (1, 5, '2026-12-31', 'newer', 10)",
        ] {
            sqlx::query(sql).execute(&pool).await.unwrap();
        }
        // $5,000 in and $600 out every month: a net of $4,400.
        for m in 1..=6 {
            sqlx::query(
                "INSERT INTO transactions (account_id, category_id, date, description, debit, credit)
                 VALUES (1, 2, ?, 'Coles', 600, 0), (1, 4, ?, 'Pay', 0, 5000)",
            )
            .bind(format!("2026-{m:02}-15"))
            .bind(format!("2026-{m:02}-10"))
            .execute(&pool)
            .await
            .unwrap();
        }

        let scenario = Scenario {
            id: 1,
            name: "Test".to_string(),
            description: None,
            horizon: String::new(),
            base_start_date: "2026-01-01".to_string(),
            base_end_date: "2026-06-30".to_string(),
            created_at: String::new(),
        };
        let defaults = ScenarioDefault {
            id: 0,
            scenario_id: 1,
            default_adjustment_pct: 0.0,
            income_growth_pct: 0.0,
        };

        let result = calculate_forecast(
            &pool,
            &scenario,
            &[],
            &defaults,
            &HashSet::new(),
            3,
            10_000.0,
            Some("2026-06-30".to_string()),
        )
        .await
        .unwrap();

        assert_eq!(result.opening_cash, 10_000.0);
        assert_eq!(result.months.len(), 3);
        let net = result.months[0].projected_net;
        assert!((net - 4400.0).abs() < 60.0, "monthly net was {net}");
        // Each month must sit one net above the last, starting from the opening
        // balance -- that is the whole point of the line.
        let mut expected = 10_000.0;
        for (i, m) in result.months.iter().enumerate() {
            expected += m.projected_net;
            assert!(
                (m.projected_cash - expected).abs() < 0.01,
                "month {i} cash was {} not {expected}",
                m.projected_cash
            );
        }
        assert!(result.months[2].projected_cash > result.months[0].projected_cash);
    }

    /// "Cash & accounts" is the latest balance of each *asset* account:
    /// liabilities and accounts whose CSVs carry no balance are left out.
    #[tokio::test]
    async fn liquid_assets_sums_the_latest_balance_per_asset_account() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        for sql in [
            "INSERT INTO accounts (id, name, type) VALUES
               (1, 'Savings', 'asset'), (2, 'Everyday', 'asset'),
               (3, 'Credit Card', 'asset'), (4, 'Home Loan', 'liability')",
            // Savings: later row wins. Everyday: one row.
            "INSERT INTO transactions (account_id, date, description, debit, balance) VALUES
               (1, '2026-05-01', 'x', 1, 500),
               (1, '2026-06-01', 'x', 1, 900),
               (2, '2026-06-02', 'x', 1, 100),
               (3, '2026-06-03', 'x', 1, NULL),
               (4, '2026-06-04', 'x', 1, 250000)",
        ] {
            sqlx::query(sql).execute(&pool).await.unwrap();
        }

        let (total, as_at) = crate::commands::cashflow::liquid_assets(&pool).await.unwrap();
        assert_eq!(total, 1000.0, "900 + 100, ignoring the loan and the card");
        assert_eq!(as_at.as_deref(), Some("2026-06-02"));
    }

    #[tokio::test]
    async fn no_overlap_with_data_still_gives_a_positive_divisor() {
        let pool = pool_with_dates(&["2026-06-01"]).await;
        let (m, _, _) = base_period_months(&pool, "2020-01-01", "2020-12-31").await.unwrap();
        assert!(m > 0.0);
    }

    /// Six months of history plus a yearly bill in a category of its own, and
    /// a scenario whose base period is that history. Returns the pool and the
    /// scenario so each source can be forecast over the same data.
    async fn pool_with_a_dated_bill() -> (SqlitePool, Scenario, ScenarioDefault) {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        for sql in [
            "INSERT INTO accounts (id, name) VALUES (1, 'Everyday')",
            "INSERT INTO categories (id, name) VALUES (1, 'Groceries'), (6, 'Insurance'), (7, 'School')",
            "INSERT INTO transactions (account_id, category_id, date, description, debit)
             VALUES (1, 1, '2025-06-01', 'older', 10), (1, 1, '2026-12-31', 'newer', 10)",
            // The bill was paid once inside the base period: its average is
            // therefore already carrying it, spread over every month.
            "INSERT INTO transactions (account_id, category_id, date, description, debit)
             VALUES (1, 6, '2026-03-10', 'Car insurance', 1200)",
            // Due again in March, inside the forecast window.
            "INSERT INTO recurring_costs (name, amount, frequency, category_id, next_due_date)
             VALUES ('Car insurance', 1200, 'Yearly', 6, '2027-03-10')",
        ] {
            sqlx::query(sql).execute(&pool).await.unwrap();
        }

        let scenario = Scenario {
            id: 1,
            name: "Test".to_string(),
            description: None,
            horizon: String::new(),
            base_start_date: "2026-01-01".to_string(),
            base_end_date: "2026-06-30".to_string(),
            created_at: String::new(),
        };
        let defaults = ScenarioDefault {
            id: 0,
            scenario_id: 1,
            default_adjustment_pct: 0.0,
            income_growth_pct: 0.0,
        };
        (pool, scenario, defaults)
    }

    /// A category flagged `exclude_from_budget` is out of every forecast
    /// aggregation — internal transfers are the whole reason the flag exists.
    /// A scheduled basis must not be a way back in: `compute_baselines`
    /// filters the flag, but a scheduled source synthesises its own baseline
    /// row when the category has no history, which would walk straight past it.
    #[tokio::test]
    async fn an_excluded_category_stays_out_even_on_a_scheduled_basis() {
        let (pool, scenario, defaults) = pool_with_a_dated_bill().await;
        for sql in [
            "INSERT INTO categories (id, name, exclude_from_budget) VALUES (9, 'Transfers', 1)",
            "INSERT INTO recurring_costs (name, amount, frequency, category_id, next_due_date)
             VALUES ('To savings', 500, 'Monthly', 9, '2026-07-05')",
        ] {
            sqlx::query(sql).execute(&pool).await.unwrap();
        }

        let months = monthly_amounts(
            &pool,
            &scenario,
            &defaults,
            &[adjustment(9, "scheduled", 0.0, None)],
            9,
        )
        .await;
        assert!(
            months.iter().all(|(_, amount)| *amount == 0.0),
            "an excluded category should project nothing: {months:?}"
        );

        // And the figure the page shows has to agree with the projection.
        let (map, _, _) = scheduled_by_month(
            &pool,
            NaiveDate::from_ymd_opt(2026, 7, 1).unwrap(),
            12,
        )
        .await
        .unwrap();
        assert!(
            !map.keys().any(|(cid, _, _)| *cid == 9),
            "an excluded category should not appear in the schedule either"
        );
    }

    fn adjustment(category_id: i64, source: &str, pct: f64, fixed: Option<f64>) -> ScenarioAdjustment {
        ScenarioAdjustment {
            id: 1,
            scenario_id: 1,
            category_id,
            adjustment_pct: pct,
            fixed_amount: fixed,
            source: source.to_string(),
        }
    }

    /// What one category projects at in each month of the forecast.
    async fn monthly_amounts(
        pool: &SqlitePool,
        scenario: &Scenario,
        defaults: &ScenarioDefault,
        adjustments: &[ScenarioAdjustment],
        category_id: i64,
    ) -> Vec<(String, f64)> {
        let result = calculate_forecast(
            pool,
            scenario,
            adjustments,
            defaults,
            &HashSet::new(),
            12,
            0.0,
            None,
        )
        .await
        .unwrap();
        result
            .months
            .iter()
            .map(|m| {
                let amount = m
                    .categories
                    .iter()
                    .find(|c| c.category_id == category_id)
                    .map(|c| c.amount)
                    .unwrap_or(0.0);
                (m.month_key.clone(), amount)
            })
            .collect()
    }

    #[tokio::test]
    async fn the_average_source_is_unchanged_and_smears_the_bill() {
        let (pool, scenario, defaults) = pool_with_a_dated_bill().await;
        let months = monthly_amounts(&pool, &scenario, &defaults, &[], 6).await;

        // $1,200 over a ~5.95-month base period, every month alike: this is
        // exactly the behaviour the scheduled sources exist to replace.
        let smeared = 1200.0 / (181.0 / DAYS_PER_MONTH);
        for (key, amount) in &months {
            assert!(close(*amount, smeared), "{key} projected {amount}, not {smeared}");
        }
    }

    #[tokio::test]
    async fn the_scheduled_source_puts_the_whole_bill_in_its_month() {
        let (pool, scenario, defaults) = pool_with_a_dated_bill().await;
        let adj = [adjustment(6, "scheduled", 0.0, None)];
        let months = monthly_amounts(&pool, &scenario, &defaults, &adj, 6).await;

        for (key, amount) in &months {
            let expected = if key == "2027-03" { 1200.0 } else { 0.0 };
            assert!(close(*amount, expected), "{key} projected {amount}, not {expected}");
        }
    }

    #[tokio::test]
    async fn scheduled_plus_average_adds_both_which_is_the_double_count() {
        let (pool, scenario, defaults) = pool_with_a_dated_bill().await;
        let adj = [adjustment(6, "scheduled_plus_average", 0.0, None)];
        let months = monthly_amounts(&pool, &scenario, &defaults, &adj, 6).await;

        let smeared = 1200.0 / (181.0 / DAYS_PER_MONTH);
        let march = months.iter().find(|(k, _)| k == "2027-03").unwrap().1;
        let other = months.iter().find(|(k, _)| k == "2027-04").unwrap().1;
        assert!(close(march, 1200.0 + smeared), "March projected {march}");
        assert!(close(other, smeared), "April projected {other}");
    }

    #[tokio::test]
    async fn scheduled_plus_fixed_uses_your_own_monthly_figure_for_the_rest() {
        let (pool, scenario, defaults) = pool_with_a_dated_bill().await;
        // "The bill in March, and $50 a month of other insurance bits."
        let adj = [adjustment(6, "scheduled_plus_fixed", 0.0, Some(50.0))];
        let months = monthly_amounts(&pool, &scenario, &defaults, &adj, 6).await;

        for (key, amount) in &months {
            let expected = if key == "2027-03" { 1250.0 } else { 50.0 };
            assert!(close(*amount, expected), "{key} projected {amount}, not {expected}");
        }
    }

    #[tokio::test]
    async fn a_percentage_scales_the_scheduled_amount_but_not_the_fixed_extra() {
        let (pool, scenario, defaults) = pool_with_a_dated_bill().await;
        let adj = [adjustment(6, "scheduled_plus_fixed", 10.0, Some(50.0))];
        let months = monthly_amounts(&pool, &scenario, &defaults, &adj, 6).await;

        let march = months.iter().find(|(k, _)| k == "2027-03").unwrap().1;
        assert!(close(march, 1320.0 + 50.0), "March projected {march}");
    }

    /// The category has no history at all, so it has no baseline row — without
    /// the synthetic row in calculate_forecast a brand-new expense would
    /// silently project nothing.
    #[tokio::test]
    async fn a_scheduled_expense_in_an_unspent_category_still_projects() {
        let (pool, scenario, defaults) = pool_with_a_dated_bill().await;
        sqlx::query(
            "INSERT INTO recurring_costs (name, amount, frequency, category_id, next_due_date)
             VALUES ('School camp', 800, 'One-off', 7, '2026-11-20')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let adj = [adjustment(7, "scheduled", 0.0, None)];
        let months = monthly_amounts(&pool, &scenario, &defaults, &adj, 7).await;

        let nov = months.iter().find(|(k, _)| k == "2026-11").unwrap().1;
        assert!(close(nov, 800.0), "November projected {nov}");
        let total: f64 = months.iter().map(|(_, a)| a).sum();
        assert!(close(total, 800.0), "the one-off fell {total} in total, not 800");
    }

    #[tokio::test]
    async fn an_expense_with_no_category_is_reported_not_silently_dropped() {
        let (pool, scenario, _defaults) = pool_with_a_dated_bill().await;
        sqlx::query(
            "INSERT INTO recurring_costs (name, amount, frequency, next_due_date)
             VALUES ('Something', 300, 'Monthly', '2026-08-01')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let start = NaiveDate::parse_from_str(&scenario.base_end_date, "%Y-%m-%d")
            .unwrap()
            .succ_opt()
            .unwrap();
        let (map, unlinked, items) = scheduled_by_month(&pool, start, 12).await.unwrap();

        assert!(close(unlinked, 300.0 * 12.0), "unlinked total was {unlinked}");
        assert_eq!(items, 12);
        // The linked bill is still bucketed to its own month.
        assert_eq!(map.get(&(6, false, "2027-03".to_string())).copied(), Some(1200.0));
    }

    /// A scheduled pay has to land on the income side: added to income, not
    /// subtracted as if it were a bill.
    #[tokio::test]
    async fn scheduled_income_is_added_not_subtracted() {
        let (pool, scenario, defaults) = pool_with_a_dated_bill().await;
        sqlx::query(
            "INSERT INTO recurring_costs (name, amount, frequency, kind, category_id, next_due_date)
             VALUES ('Dividend', 2000, 'Quarterly', 'income', 7, '2026-09-15')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let adj = [adjustment(7, "scheduled", 0.0, None)];
        let result = calculate_forecast(
            &pool, &scenario, &adj, &defaults, &HashSet::new(), 12, 0.0, None,
        )
        .await
        .unwrap();

        let sept = result.months.iter().find(|m| m.month_key == "2026-09").unwrap();
        let oct = result.months.iter().find(|m| m.month_key == "2026-10").unwrap();
        assert!(close(sept.projected_income, 2000.0), "September income was {}", sept.projected_income);
        assert!(close(oct.projected_income, 0.0), "October income was {}", oct.projected_income);
        // The quarterly payment must not have been read as a bill.
        assert!(
            close(sept.projected_expenses, oct.projected_expenses),
            "the dividend changed expenses: {} vs {}",
            sept.projected_expenses,
            oct.projected_expenses
        );
        assert!(sept.projected_net > oct.projected_net);
    }

    /// Income and an expense in the SAME category must not cancel out.
    #[tokio::test]
    async fn both_sides_of_one_category_are_kept_apart() {
        let (pool, scenario, defaults) = pool_with_a_dated_bill().await;
        for sql in [
            "INSERT INTO recurring_costs (name, amount, frequency, kind, category_id, next_due_date)
             VALUES ('Rent received', 1500, 'Monthly', 'income', 7, '2026-08-01')",
            "INSERT INTO recurring_costs (name, amount, frequency, kind, category_id, next_due_date)
             VALUES ('Agent fee', 120, 'Monthly', 'expense', 7, '2026-08-05')",
        ] {
            sqlx::query(sql).execute(&pool).await.unwrap();
        }

        let adj = [adjustment(7, "scheduled", 0.0, None)];
        let result = calculate_forecast(
            &pool, &scenario, &adj, &defaults, &HashSet::new(), 6, 0.0, None,
        )
        .await
        .unwrap();

        let aug = result.months.iter().find(|m| m.month_key == "2026-08").unwrap();
        // The category has a row per side, so both amounts must be present --
        // netted to 1,380 would mean the two had been folded together.
        let mut amounts: Vec<f64> = aug
            .categories
            .iter()
            .filter(|c| c.category_id == 7)
            .map(|c| c.amount)
            .collect();
        amounts.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(amounts.len(), 2, "expected an income and an expense row, got {amounts:?}");
        assert!(close(amounts[0], 120.0) && close(amounts[1], 1500.0), "got {amounts:?}");
        // Income is this category's alone; expenses also carry the fixture's
        // insurance average, so only the delta is this category's.
        assert!(close(aug.projected_income, 1500.0), "income was {}", aug.projected_income);
        let insurance = 1200.0 / (181.0 / DAYS_PER_MONTH);
        assert!(
            close(aug.projected_expenses, insurance + 120.0),
            "expenses were {}, not {} + 120",
            aug.projected_expenses,
            insurance
        );
    }

    /// A fixed monthly figure has to change the projection. One refund in a
    /// spending category gives it an income baseline row as well as an expense
    /// one, and the fixed amount used to be applied to both -- income and
    /// expenses each moved by it and the net came out identical, so $5,000 and
    /// $500 drew the same line and the box looked like it did nothing.
    #[tokio::test]
    async fn a_fixed_amount_moves_the_net_even_with_a_refund_in_the_category() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        for sql in [
            "INSERT INTO accounts (id, name) VALUES (1, 'Everyday')",
            "INSERT INTO categories (id, name) VALUES (1, 'Dining'), (2, 'Salary'), (5, 'Other')",
            "INSERT INTO transactions (account_id, category_id, date, description, debit)
             VALUES (1, 5, '2025-06-01', 'older', 10), (1, 5, '2026-12-31', 'newer', 10)",
            // The one reimbursement that makes this category two-sided.
            "INSERT INTO transactions (account_id, category_id, date, description, credit)
             VALUES (1, 1, '2026-03-05', 'Dinner split', 46)",
        ] {
            sqlx::query(sql).execute(&pool).await.unwrap();
        }
        for m in 1..=6 {
            sqlx::query(
                "INSERT INTO transactions (account_id, category_id, date, description, debit, credit)
                 VALUES (1, 1, ?, 'Cafe', 800, 0), (1, 2, ?, 'Pay', 0, 5000)",
            )
            .bind(format!("2026-{m:02}-15"))
            .bind(format!("2026-{m:02}-10"))
            .execute(&pool)
            .await
            .unwrap();
        }

        let scenario = Scenario {
            id: 1,
            name: "Test".to_string(),
            description: None,
            horizon: String::new(),
            base_start_date: "2026-01-01".to_string(),
            base_end_date: "2026-06-30".to_string(),
            created_at: String::new(),
        };
        let defaults = ScenarioDefault {
            id: 0,
            scenario_id: 1,
            default_adjustment_pct: 0.0,
            income_growth_pct: 0.0,
        };
        let run = |fixed: f64| {
            let scenario = scenario.clone();
            let defaults = defaults.clone();
            let pool = pool.clone();
            async move {
                let adj = [ScenarioAdjustment {
                    id: 1,
                    scenario_id: 1,
                    category_id: 1,
                    adjustment_pct: 0.0,
                    fixed_amount: Some(fixed),
                    source: "average".to_string(),
                }];
                calculate_forecast(
                    &pool,
                    &scenario,
                    &adj,
                    &defaults,
                    &HashSet::new(),
                    1,
                    0.0,
                    None,
                )
                .await
                .unwrap()
            }
        };

        let cheap = run(500.0).await.months[0].clone();
        let dear = run(5000.0).await.months[0].clone();

        // The figure lands on the expense side, where the category's history is.
        assert!(close(dear.projected_expenses - cheap.projected_expenses, 4500.0),
            "expenses moved by {}", dear.projected_expenses - cheap.projected_expenses);
        // And nowhere else: the refund's own average is all the income side has.
        assert!(close(dear.projected_income, cheap.projected_income),
            "income moved from {} to {}", cheap.projected_income, dear.projected_income);
        assert!(close(cheap.projected_net - dear.projected_net, 4500.0),
            "the net moved by {}", cheap.projected_net - dear.projected_net);
        // The refund still projects -- the fix must not drop the other side.
        let refund = 46.0 / (181.0 / DAYS_PER_MONTH);
        assert!((cheap.projected_income - 5000.0 - refund).abs() < 60.0,
            "income was {}", cheap.projected_income);
    }
}
