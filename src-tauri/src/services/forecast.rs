use crate::models::{
    CategoryBaseline, ForecastCategoryAmount, ForecastComparison, ForecastMonth, ForecastResult,
    ForecastTotals, Scenario, ScenarioAdjustment, ScenarioDefault,
};
use chrono::{Datelike, NaiveDate};
use sqlx::SqlitePool;
use std::collections::HashMap;

pub async fn calculate_forecast(
    pool: &SqlitePool,
    scenario: &Scenario,
    adjustments: &[ScenarioAdjustment],
    defaults: &ScenarioDefault,
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

    let base = calculate_forecast(pool, &base_scenario, &base_adjustments, &base_defaults, months_ahead).await?;

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

        scenarios.push(calculate_forecast(pool, &scenario, &adjustments, &defaults, months_ahead).await?);
    }

    Ok(ForecastComparison {
        base,
        scenarios,
        months_ahead,
    })
}

async fn compute_baselines(
    pool: &SqlitePool,
    start_date: &str,
    end_date: &str,
) -> Result<Vec<CategoryBaseline>, String> {
    let use_end = if end_date.is_empty() { start_date } else { end_date };

    let rows = sqlx::query_as::<_, (Option<i64>, String, f64, f64)>(
        "SELECT t.category_id,
                COALESCE(c.name, 'Uncategorised') as category_name,
                COALESCE(SUM(t.debit), 0) as total_debit,
                COALESCE(SUM(t.credit), 0) as total_credit
         FROM transactions t
         LEFT JOIN categories c ON t.category_id = c.id
         WHERE t.date >= ? AND t.date <= ?
         GROUP BY t.category_id, c.name",
    )
    .bind(start_date)
    .bind(use_end)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("DB error baselines: {}", e))?;

    let num_months = month_count(start_date, use_end).max(1);

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
                monthly_avg: credit / num_months as f64,
                is_income: true,
            });
        }
        if *debit > 0.0 {
            baselines.push(CategoryBaseline {
                category_id,
                category_path,
                monthly_avg: debit / num_months as f64,
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

fn month_count(start: &str, end: &str) -> i64 {
    let s = NaiveDate::parse_from_str(start, "%Y-%m-%d").ok();
    let e = NaiveDate::parse_from_str(end, "%Y-%m-%d").ok();
    match (s, e) {
        (Some(sd), Some(ed)) => {
            let months = (ed.year() - sd.year()) * 12 + (ed.month() as i32 - sd.month() as i32);
            months.max(1) as i64
        }
        _ => 1,
    }
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
