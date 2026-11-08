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

/// Where a category's monthly figure comes from in a scenario.
///
/// The distinction that matters is double counting: a bill paid inside the base
/// period is *already* in that category's average, smeared over every month. So
/// `Scheduled` **replaces** the average for a bill you already pay, and the
/// `…PlusAverage` / `…PlusFixed` modes are for when the dated expense is new or
/// the category holds other spending too.
pub enum AdjustmentSource {
    /// The base-period average (or `fixed_amount`, if one is set).
    Average,
    /// Only the dated upcoming expenses linked to this category.
    Scheduled,
    /// Dated expenses *and* the average.
    ScheduledPlusAverage,
    /// Dated expenses plus `fixed_amount` as a monthly extra.
    ScheduledPlusFixed,
}

impl AdjustmentSource {
    /// Unknown values are an error rather than a silent fall back to the
    /// average: a typo'd source would otherwise look like it saved and then
    /// forecast something else.
    fn parse(raw: Option<&str>) -> Result<Self, String> {
        match raw.unwrap_or("average") {
            "average" | "" => Ok(Self::Average),
            "scheduled" => Ok(Self::Scheduled),
            "scheduled_plus_average" => Ok(Self::ScheduledPlusAverage),
            "scheduled_plus_fixed" => Ok(Self::ScheduledPlusFixed),
            other => Err(format!(
                "Unknown adjustment source \"{}\" — expected average, scheduled, \
                 scheduled_plus_average or scheduled_plus_fixed.",
                other
            )),
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::Average => "average",
            Self::Scheduled => "scheduled",
            Self::ScheduledPlusAverage => "scheduled_plus_average",
            Self::ScheduledPlusFixed => "scheduled_plus_fixed",
        }
    }

    fn is_average(&self) -> bool {
        matches!(self, Self::Average)
    }
}

#[tauri::command]
pub async fn save_scenario_adjustment(
    pool: State<'_, SqlitePool>,
    scenario_id: i64,
    category_id: i64,
    adjustment_pct: f64,
    fixed_amount: Option<f64>,
    source: Option<String>,
) -> Result<Option<ScenarioAdjustment>, String> {
    let source = AdjustmentSource::parse(source.as_deref())?;

    // 0% with no fixed amount is what the UI shows as "Default", so make it
    // mean that: drop the row and let the scenario's default % apply. Storing
    // it pinned the category at 0% — ignoring the default — while the badge
    // still claimed it was using the default.
    //
    // Only on the average source, though: a scheduled category at 0% with no
    // fixed amount is a real choice, and dropping its row would silently put it
    // back on the average.
    if adjustment_pct == 0.0 && fixed_amount.is_none() && source.is_average() {
        sqlx::query("DELETE FROM scenario_adjustments WHERE scenario_id = ? AND category_id = ?")
            .bind(scenario_id)
            .bind(category_id)
            .execute(&*pool)
            .await
            .map_err(|e| format!("DB error clearing adjustment: {}", e))?;
        return Ok(None);
    }

    sqlx::query(
        "INSERT INTO scenario_adjustments (scenario_id, category_id, adjustment_pct, fixed_amount, source)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT(scenario_id, category_id) DO UPDATE SET
           adjustment_pct = excluded.adjustment_pct,
           fixed_amount = excluded.fixed_amount,
           source = excluded.source",
    )
    .bind(scenario_id)
    .bind(category_id)
    .bind(adjustment_pct)
    .bind(fixed_amount)
    .bind(source.as_str())
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

/// What the dated upcoming expenses linked to one category add up to over a
/// forecast window.
#[derive(Debug, serde::Serialize)]
pub struct ScheduledCategory {
    pub category_id: i64,
    /// Which side of the category these fall on — a scheduled pay is reported
    /// separately from a scheduled bill in the same category.
    pub is_income: bool,
    /// Everything falling due in the window.
    pub total: f64,
    /// `total` spread over the window — the figure to compare against the
    /// category's monthly average when judging a double count.
    pub monthly_equivalent: f64,
    /// Months in the window this category has anything falling due in, so the
    /// UI can say "3 of 12 months" rather than implying it is every month.
    pub months_hit: i64,
}

#[derive(Debug, serde::Serialize)]
pub struct ScheduledSummary {
    pub months_ahead: i64,
    pub from: String,
    pub to: String,
    pub categories: Vec<ScheduledCategory>,
    /// Upcoming income and expenses with no category: a scheduled source can't see them,
    /// since there is no category row to attach them to.
    pub unlinked_total: f64,
    pub unlinked_items: i64,
}

/// The scheduled side of a scenario, over the same window `run_forecast` uses
/// (the month after the base period, for `months_ahead` months). Drives the
/// forecasting page's "Sched /mo" column and its double-count warning.
#[tauri::command]
pub async fn get_scheduled_amounts(
    pool: State<'_, SqlitePool>,
    scenario_id: i64,
    months_ahead: i64,
) -> Result<ScheduledSummary, String> {
    let months_ahead = months_ahead.clamp(1, 120);
    // Scenario 0 is the implicit baseline, which has no row of its own: fall
    // back to the imported data's last date, as compare_scenarios does.
    let base_end = if scenario_id == 0 {
        sqlx::query_scalar::<_, Option<String>>("SELECT MAX(date) FROM transactions")
            .fetch_one(&*pool)
            .await
            .map_err(|e| format!("DB error: {}", e))?
            .unwrap_or_default()
    } else {
        get_scenario(pool.clone(), scenario_id).await?.base_end_date
    };

    let start = chrono::NaiveDate::parse_from_str(&base_end, "%Y-%m-%d")
        .map_err(|_| "The scenario's base period needs a valid end date.".to_string())?
        .succ_opt()
        .ok_or_else(|| "The base period ends at the end of time.".to_string())?;

    let (map, unlinked_total, unlinked_items) =
        services::forecast::scheduled_by_month(&pool, start, months_ahead).await?;

    // The map is keyed by (category, month); the page wants it per category.
    let mut by_cat: std::collections::HashMap<(i64, bool), ScheduledCategory> = Default::default();
    for ((category_id, is_income, _month), amount) in &map {
        let e = by_cat
            .entry((*category_id, *is_income))
            .or_insert(ScheduledCategory {
                category_id: *category_id,
                is_income: *is_income,
                total: 0.0,
                monthly_equivalent: 0.0,
                months_hit: 0,
            });
        e.total += amount;
        e.months_hit += 1;
    }
    for e in by_cat.values_mut() {
        e.monthly_equivalent = e.total / months_ahead as f64;
    }

    Ok(ScheduledSummary {
        months_ahead,
        from: start.format("%Y-%m-%d").to_string(),
        to: services::forecast::add_months(&start, months_ahead)
            .format("%Y-%m-%d")
            .to_string(),
        categories: by_cat.into_values().collect(),
        unlinked_total,
        unlinked_items,
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
    let rows = sqlx::query_as::<_, (i64, i64, i64, String, f64, Option<f64>, String)>(
        "SELECT sa.id, sa.scenario_id, sa.category_id,
                COALESCE(c.name, 'Uncategorised') as category_path,
                sa.adjustment_pct, sa.fixed_amount, sa.source
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
        .map(
            |(id, scenario_id, category_id, category_path, adjustment_pct, fixed_amount, source)| {
                ScenarioAdjustmentWithPath {
                    id,
                    scenario_id,
                    category_id,
                    category_path,
                    adjustment_pct,
                    fixed_amount,
                    source,
                }
            },
        )
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
