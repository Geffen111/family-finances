mod commands;
mod db;
mod models;
mod services;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let pool = tauri::async_runtime::block_on(db::init_db());
            app.manage(pool);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::import::csv_import,
            commands::import::get_transactions,
            commands::import::get_accounts,
            commands::historical::import_historical_csv,
            commands::categories::upload_categories_csv,
            commands::categories::get_categories,
            commands::categories::create_category,
            commands::categories::update_category,
            commands::categories::delete_category,
            commands::categories::assign_category,
            commands::categories::assign_categories_bulk,
            commands::categories::set_category_exclusion,
            commands::categories::get_uncategorised_transactions,
            commands::dashboard::get_dashboard_summary,
            commands::dashboard::get_budget_status,
            commands::dashboard::get_net_worth_trend,
            commands::dashboard::get_spending_by_category,
            commands::dashboard::get_monthly_trends,
            commands::dashboard::get_spending_trend_by_category,
            commands::settings::save_api_key,
            commands::settings::get_api_key,
            commands::categorise::categorise_transactions,
            commands::categorise::accept_categorisations,
            commands::recurring::get_recurring_transactions,
            commands::insights::get_insights,
            commands::insights::refresh_insights,
            commands::forecasting::create_scenario,
            commands::forecasting::list_scenarios,
            commands::forecasting::get_scenario,
            commands::forecasting::update_scenario,
            commands::forecasting::delete_scenario,
            commands::forecasting::save_scenario_adjustment,
            commands::forecasting::get_scenario_adjustments,
            commands::forecasting::save_scenario_defaults,
            commands::forecasting::get_scenario_defaults,
            commands::forecasting::run_forecast,
            commands::export::export_transactions_csv,
            commands::export::export_summary_csv,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}