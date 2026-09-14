use crate::models::{
    ForecastComparison, Scenario, ScenarioAdjustment, ScenarioAdjustmentWithPath,
    ScenarioDefault,
};
use crate::services;
use sqlx::SqlitePool;
use tauri::State;

#[tauri::command]
pub async fn create_scenario(
    pool: State<'_, SqlitePool>,
    name: String,
    description: Option<String>,
    base_start_date: String,
    base_end_date: String,
) -> Result<Scenario, String> {
    validate_base_period(&base_start_date, &base_end_date)?;
    // No horizon: the projection is always monthly and its length comes from
    // the "Months Ahead" slider, so the old Monthly/Quarterly/Yearly picker
    // changed nothing. The column keeps its 'monthly' default.
    let result = sqlx::query(
        "INSERT INTO scenarios (name, description, base_start_date, base_end_date)
         VALUES (?, ?, ?, ?)",
    )
    .bind(&name)
    .bind(&description)
    .bind(&base_start_date)
    .bind(&base_end_date)
    .execute(&*pool)
    .await
    .map_err(|e| format!("DB error creating scenario: {}", e))?;

    let id = result.last_insert_rowid();
    let scenario = sqlx::query_as::<_, Scenario>("SELECT * FROM scenarios WHERE id = ?")
        .bind(id)
        .fetch_one(&*pool)
        .await
        .map_err(|e| format!("DB error fetching scenario: {}", e))?;

    Ok(scenario)
}

#[tauri::command]
pub async fn list_scenarios(pool: State<'_, SqlitePool>) -> Result<Vec<Scenario>, String> {
    let scenarios = sqlx::query_as::<_, Scenario>(
        "SELECT * FROM scenarios ORDER BY created_at DESC",
    )
    .fetch_all(&*pool)
    .await
    .map_err(|e| format!("DB error listing scenarios: {}", e))?;

    Ok(scenarios)
}

#[tauri::command]
pub async fn get_scenario(
    pool: State<'_, SqlitePool>,
    id: i64,
) -> Result<Scenario, String> {
    let scenario = sqlx::query_as::<_, Scenario>("SELECT * FROM scenarios WHERE id = ?")
        .bind(id)
        .fetch_optional(&*pool)
        .await
        .map_err(|e| format!("DB error fetching scenario: {}", e))?
        .ok_or_else(|| format!("Scenario {} not found", id))?;

    Ok(scenario)
}

#[tauri::command]
pub async fn update_scenario(
    pool: State<'_, SqlitePool>,
    id: i64,
    name: Option<String>,
    description: Option<String>,
    horizon: Option<String>,
    base_start_date: Option<String>,
    base_end_date: Option<String>,
) -> Result<Scenario, String> {
    let existing = get_scenario(pool.clone(), id).await?;
    let start = base_start_date.unwrap_or(existing.base_start_date);
    let end = base_end_date.unwrap_or(existing.base_end_date);
    validate_base_period(&start, &end)?;
    // None keeps the current description; an emptied one clears it.
    let description = match description {
        Some(d) if d.trim().is_empty() => None,
        Some(d) => Some(d.trim().to_string()),
        None => existing.description,
    };

    sqlx::query(
        "UPDATE scenarios SET name = ?, description = ?, horizon = ?, base_start_date = ?, base_end_date = ?
         WHERE id = ?",
    )
    .bind(name.unwrap_or(existing.name))
    .bind(description)
    .bind(horizon.unwrap_or(existing.horizon))
    .bind(start)
    .bind(end)
    .bind(id)
    .execute(&*pool)
    .await
    .map_err(|e| format!("DB error updating scenario: {}", e))?;

    get_scenario(pool.clone(), id).await
}

#[tauri::command]
pub async fn delete_scenario(
    pool: State<'_, SqlitePool>,
    id: i64,
) -> Result<(), String> {
    sqlx::query("DELETE FROM scenario_adjustments WHERE scenario_id = ?")
        .bind(id)
        .execute(&*pool)
        .await
        .map_err(|e| format!("DB error deleting adjustments: {}", e))?;

    sqlx::query("DELETE FROM scenario_defaults WHERE scenario_id = ?")
        .bind(id)
        .execute(&*pool)
        .await
        .map_err(|e| format!("DB error deleting defaults: {}", e))?;

    sqlx::query("DELETE FROM scenario_excluded_categories WHERE scenario_id = ?")
        .bind(id)
        .execute(&*pool)
        .await
        .map_err(|e| format!("DB error deleting exclusions: {}", e))?;

    sqlx::query("DELETE FROM scenarios WHERE id = ?")
        .bind(id)
        .execute(&*pool)
        .await
        .map_err(|e| format!("DB error deleting scenario: {}", e))?;

    Ok(())
}

#[tauri::command]
pub async fn save_scenario_adjustment(
    pool: State<'_, SqlitePool>,
    scenario_id: i64,
    category_id: i64,
    adjustment_pct: f64,
    fixed_amount: Option<f64>,
) -> Result<Option<ScenarioAdjustment>, String> {
    // 0% with no fixed amount is what the UI shows as "Default", so make it
    // mean that: drop the row and let the scenario's default % apply. Storing
    // it pinned the category at 0% — ignoring the default — while the badge
    // still claimed it was using the default.
    if adjustment_pct == 0.0 && fixed_amount.is_none() {
        sqlx::query("DELETE FROM scenario_adjustments WHERE scenario_id = ? AND category_id = ?")
            .bind(scenario_id)
            .bind(category_id)
            .execute(&*pool)
            .await
            .map_err(|e| format!("DB error clearing adjustment: {}", e))?;
        return Ok(None);
    }

    sqlx::query(
        "INSERT INTO scenario_adjustments (scenario_id, category_id, adjustment_pct, fixed_amount)
         VALUES (?, ?, ?, ?)
         ON CONFLICT(scenario_id, category_id) DO UPDATE SET
           adjustment_pct = excluded.adjustment_pct,
           fixed_amount = excluded.fixed_amount",
    )
    .bind(scenario_id)
    .bind(category_id)
    .bind(adjustment_pct)
    .bind(fixed_amount)
    .execute(&*pool)
    .await
    .map_err(|e| format!("DB error saving adjustment: {}", e))?;

    let adj = sqlx::query_as::<_, ScenarioAdjustment>(
        "SELECT * FROM scenario_adjustments WHERE scenario_id = ? AND category_id = ?",
    )
    .bind(scenario_id)
    .bind(category_id)
    .fetch_one(&*pool)
    .await
    .map_err(|e| format!("DB error fetching adjustment: {}", e))?;

    Ok(Some(adj))
}

/// One category's monthly averages over a scenario's base period — the
/// figures its % adjustments scale, shown next to each category so you can
/// see what it projects at before changing anything.
#[derive(Debug, serde::Serialize)]
pub struct CategoryAverage {
    pub category_id: i64,
    pub monthly_income: f64,
    pub monthly_expense: f64,
}

#[derive(Debug, serde::Serialize)]
pub struct ScenarioBaselines {
    /// Months of data the averages cover (the base period clamped to the
    /// transactions actually imported).
    pub months: f64,
    pub data_start: String,
    pub data_end: String,
    pub categories: Vec<CategoryAverage>,
}

#[tauri::command]
pub async fn get_scenario_baselines(
    pool: State<'_, SqlitePool>,
    scenario_id: i64,
) -> Result<ScenarioBaselines, String> {
    let scenario = get_scenario(pool.clone(), scenario_id).await?;
    let (months, data_start, data_end) = services::forecast::base_period_months(
        &pool,
        &scenario.base_start_date,
        &scenario.base_end_date,
    )
    .await?;
    let baselines = services::forecast::compute_baselines(
        &pool,
        &scenario.base_start_date,
        &scenario.base_end_date,
    )
    .await?;

    // compute_baselines yields separate income and expense rows per category.
    let mut by_cat: std::collections::HashMap<i64, CategoryAverage> = Default::default();
    for bl in baselines {
        let entry = by_cat.entry(bl.category_id).or_insert(CategoryAverage {
            category_id: bl.category_id,
            monthly_income: 0.0,
            monthly_expense: 0.0,
        });
        if bl.is_income {
            entry.monthly_income += bl.monthly_avg;
        } else {
            entry.monthly_expense += bl.monthly_avg;
        }
    }

    Ok(ScenarioBaselines {
        months,
        data_start,
        data_end,
        categories: by_cat.into_values().collect(),
    })
}

fn validate_base_period(start: &str, end: &str) -> Result<(), String> {
    let parse = |d: &str| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d");
    match (parse(start), parse(end)) {
        (Ok(s), Ok(e)) if s <= e => Ok(()),
        (Ok(_), Ok(_)) => Err("The base period must end on or after its start date.".to_string()),
        _ => Err("The base period needs a valid start and end date.".to_string()),
    }
}

#[tauri::command]
pub async fn get_scenario_adjustments(
    pool: State<'_, SqlitePool>,
    scenario_id: i64,
) -> Result<Vec<ScenarioAdjustmentWithPath>, String> {
    let rows = sqlx::query_as::<_, (i64, i64, i64, String, f64, Option<f64>)>(
        "SELECT sa.id, sa.scenario_id, sa.category_id,
                COALESCE(c.name, 'Uncategorised') as category_path,
                sa.adjustment_pct, sa.fixed_amount
         FROM scenario_adjustments sa
         LEFT JOIN categories c ON sa.category_id = c.id
         WHERE sa.scenario_id = ?
         ORDER BY category_path",
    )
    .bind(scenario_id)
    .fetch_all(&*pool)
    .await
    .map_err(|e| format!("DB error fetching adjustments: {}", e))?;

    Ok(rows
        .into_iter()
        .map(|(id, scenario_id, category_id, category_path, adjustment_pct, fixed_amount)| {
            ScenarioAdjustmentWithPath {
                id,
                scenario_id,
                category_id,
                category_path,
                adjustment_pct,
                fixed_amount,
            }
        })
        .collect())
}

#[tauri::command]
pub async fn get_scenario_excluded_categories(
    pool: State<'_, SqlitePool>,
    scenario_id: i64,
) -> Result<Vec<i64>, String> {
    sqlx::query_scalar::<_, i64>(
        "SELECT category_id FROM scenario_excluded_categories WHERE scenario_id = ?",
    )
    .bind(scenario_id)
    .fetch_all(&*pool)
    .await
    .map_err(|e| format!("DB error fetching exclusions: {}", e))
}

#[tauri::command]
pub async fn set_scenario_category_exclusion(
    pool: State<'_, SqlitePool>,
    scenario_id: i64,
    category_id: i64,
    excluded: bool,
) -> Result<(), String> {
    if excluded {
        sqlx::query(
            "INSERT INTO scenario_excluded_categories (scenario_id, category_id) \
             VALUES (?, ?) ON CONFLICT(scenario_id, category_id) DO NOTHING",
        )
        .bind(scenario_id)
        .bind(category_id)
        .execute(&*pool)
        .await
        .map_err(|e| format!("DB error excluding category: {}", e))?;
    } else {
        sqlx::query(
            "DELETE FROM scenario_excluded_categories WHERE scenario_id = ? AND category_id = ?",
        )
        .bind(scenario_id)
        .bind(category_id)
        .execute(&*pool)
        .await
        .map_err(|e| format!("DB error including category: {}", e))?;
    }
    Ok(())
}

#[tauri::command]
pub async fn save_scenario_defaults(
    pool: State<'_, SqlitePool>,
    scenario_id: i64,
    default_adjustment_pct: f64,
    income_growth_pct: f64,
) -> Result<ScenarioDefault, String> {
    sqlx::query(
        "INSERT INTO scenario_defaults (scenario_id, default_adjustment_pct, income_growth_pct)
         VALUES (?, ?, ?)
         ON CONFLICT(scenario_id) DO UPDATE SET
           default_adjustment_pct = excluded.default_adjustment_pct,
           income_growth_pct = excluded.income_growth_pct",
    )
    .bind(scenario_id)
    .bind(default_adjustment_pct)
    .bind(income_growth_pct)
    .execute(&*pool)
    .await
    .map_err(|e| format!("DB error saving defaults: {}", e))?;

    let defaults = sqlx::query_as::<_, ScenarioDefault>(
        "SELECT * FROM scenario_defaults WHERE scenario_id = ?",
    )
    .bind(scenario_id)
    .fetch_one(&*pool)
    .await
    .map_err(|e| format!("DB error fetching defaults: {}", e))?;

    Ok(defaults)
}

#[tauri::command]
pub async fn get_scenario_defaults(
    pool: State<'_, SqlitePool>,
    scenario_id: i64,
) -> Result<Option<ScenarioDefault>, String> {
    let defaults = sqlx::query_as::<_, ScenarioDefault>(
        "SELECT * FROM scenario_defaults WHERE scenario_id = ?",
    )
    .bind(scenario_id)
    .fetch_optional(&*pool)
    .await
    .map_err(|e| format!("DB error fetching defaults: {}", e))?;

    Ok(defaults)
}

#[tauri::command]
pub async fn run_forecast(
    pool: State<'_, SqlitePool>,
    scenario_ids: Vec<i64>,
    months_ahead: i64,
) -> Result<ForecastComparison, String> {
    services::forecast::compare_scenarios(&*pool, scenario_ids, months_ahead).await
}
