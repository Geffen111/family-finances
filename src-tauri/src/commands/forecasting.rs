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
    horizon: String,
    base_start_date: String,
    base_end_date: String,
) -> Result<Scenario, String> {
    let result = sqlx::query(
        "INSERT INTO scenarios (name, description, horizon, base_start_date, base_end_date)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&name)
    .bind(&description)
    .bind(&horizon)
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

    sqlx::query(
        "UPDATE scenarios SET name = ?, description = ?, horizon = ?, base_start_date = ?, base_end_date = ?
         WHERE id = ?",
    )
    .bind(name.unwrap_or(existing.name))
    .bind(description.or(existing.description))
    .bind(horizon.unwrap_or(existing.horizon))
    .bind(base_start_date.unwrap_or(existing.base_start_date))
    .bind(base_end_date.unwrap_or(existing.base_end_date))
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
) -> Result<ScenarioAdjustment, String> {
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

    Ok(adj)
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
