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
) -> Result<ForecastResult, String> {
    let baselines = compute_baselines(pool, &scenario.base_start_date, &scenario.base_end_date).await?;

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

    let adj_map: HashMap<i64, &ScenarioAdjustment> =
        adjustments.iter().map(|a| (a.category_id, a)).collect();
    let income_growth_monthly = defaults.income_growth_pct / 100.0 / 12.0;

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

            let amount = if let Some(adj) = adj_map.get(&bl.category_id) {
                if let Some(fixed) = adj.fixed_amount {
                    fixed
                } else {
                    bl.monthly_avg * (1.0 + adj.adjustment_pct / 100.0)
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
            });
        }

        let projected_net = projected_income - projected_expenses;

        totals.total_projected_income += projected_income;
        totals.total_projected_expenses += projected_expenses;
        totals.total_projected_net += projected_net;

        months.push(ForecastMonth {
            label,
            month_key,
            projected_income,
            projected_expenses,
            projected_net,
            categories,
        });
    }

    Ok(ForecastResult {
        scenario_name: scenario.name.clone(),
        scenario_id: scenario.id,
        months,
        totals,
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

    let base_excluded = HashSet::new();
    let base = calculate_forecast(pool, &base_scenario, &base_adjustments, &base_defaults, &base_excluded, months_ahead).await?;

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

        scenarios.push(calculate_forecast(pool, &scenario, &adjustments, &defaults, &excluded, months_ahead).await?);
    }

    Ok(ForecastComparison {
        base,
        scenarios,
        months_ahead,
    })
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

        if *credit > *debit || *credit > 0.0 {
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

fn add_months(date: &NaiveDate, n: i64) -> NaiveDate {
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

    #[tokio::test]
    async fn no_overlap_with_data_still_gives_a_positive_divisor() {
        let pool = pool_with_dates(&["2026-06-01"]).await;
        let (m, _, _) = base_period_months(&pool, "2020-01-01", "2020-12-31").await.unwrap();
        assert!(m > 0.0);
    }
}
