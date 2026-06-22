use crate::models::SpendingInsights;
use crate::services::insights;
use chrono::{Datelike, Months};
use sqlx::SqlitePool;
use tauri::State;

fn default_start_date() -> String {
    let today = chrono::Local::now().naive_local().date();
    let three_months_ago = today - Months::new(3);
    format!("{:04}-{:02}-01", three_months_ago.year(), three_months_ago.month())
}

fn default_end_date() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

fn resolve_dates(start: Option<String>, end: Option<String>) -> (String, String) {
    let s = start.unwrap_or_else(default_start_date);
    let e = end.unwrap_or_else(default_end_date);
    (s, e)
}

#[tauri::command]
pub async fn get_insights(
    pool: State<'_, SqlitePool>,
    start_date: Option<String>,
    end_date: Option<String>,
) -> Result<SpendingInsights, String> {
    let api_key = crate::commands::settings::get_api_key()
        .await?
        .ok_or_else(|| "OpenRouter API key not configured. Please add your API key in Settings.".to_string())?;

    let (sd, ed) = resolve_dates(start_date, end_date);
    insights::fetch_spending_insights(&*pool, &api_key, &sd, &ed, false).await
}

#[tauri::command]
pub async fn refresh_insights(
    pool: State<'_, SqlitePool>,
    start_date: Option<String>,
    end_date: Option<String>,
) -> Result<SpendingInsights, String> {
    let api_key = crate::commands::settings::get_api_key()
        .await?
        .ok_or_else(|| "OpenRouter API key not configured. Please add your API key in Settings.".to_string())?;

    let (sd, ed) = resolve_dates(start_date, end_date);
    insights::fetch_spending_insights(&*pool, &api_key, &sd, &ed, true).await
}
