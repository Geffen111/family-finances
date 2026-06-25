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
            commands::import::move_transactions,
            commands::import::get_last_import,
            commands::historical::import_historical_csv,
            commands::categories::upload_categories_csv,
            commands::categories::get_categories,
            commands::categories::create_category,
            commands::categories::update_category,
            commands::categories::delete_category,
            commands::categories::assign_category,
            commands::categories::assign_categories_bulk,
            commands::categories::set_category_exclusion,
            commands::categories::set_category_rollover,
            commands::categories::get_uncategorised_transactions,
            commands::dashboard::get_dashboard_summary,
            commands::dashboard::get_budget_status,
            commands::dashboard::get_net_worth_trend,
            commands::dashboard::get_spending_by_category,
            commands::dashboard::get_category_spending_tree,
            commands::dashboard::get_monthly_trends,
            commands::dashboard::get_spending_trend_by_category,
            commands::dashboard::get_budget_suggestions,
            commands::dashboard::get_income_by_category,
            commands::dashboard::get_category_movers,
            commands::settings::save_api_key,
            commands::settings::get_api_key,
            commands::settings::save_household_name,
            commands::settings::get_household_name,
            commands::categorise::categorise_transactions,
            commands::categorise::accept_categorisations,
            commands::recurring::get_recurring_transactions,
            commands::recurring::list_recurring_costs,
            commands::recurring::create_recurring_cost,
            commands::recurring::update_recurring_cost,
            commands::recurring::delete_recurring_cost,
            commands::goals::list_savings_goals,
            commands::goals::create_savings_goal,
            commands::goals::update_savings_goal,
            commands::goals::delete_savings_goal,
            commands::assets::list_assets,
            commands::assets::create_asset,
            commands::assets::update_asset,
            commands::assets::delete_asset,
            commands::insights::get_insights,
            commands::insights::refresh_insights,
            commands::ask::ask_question,
            commands::forecasting::create_scenario,
            commands::forecasting::list_scenarios,
            commands::forecasting::get_scenario,
            commands::forecasting::update_scenario,
            commands::forecasting::delete_scenario,
            commands::forecasting::save_scenario_adjustment,
            commands::forecasting::get_scenario_adjustments,
            commands::forecasting::get_scenario_excluded_categories,
            commands::forecasting::set_scenario_category_exclusion,
            commands::forecasting::save_scenario_defaults,
            commands::forecasting::get_scenario_defaults,
            commands::forecasting::run_forecast,
            commands::export::export_transactions_csv,
            commands::export::export_summary_csv,
            commands::cashflow::get_upcoming_bills,
            commands::cashflow::get_safe_to_spend,
            commands::debt::list_liabilities,
            commands::debt::update_account_debt_terms,
            commands::debt::simulate_debt_payoff,
            commands::rules::list_category_rules,
            commands::rules::create_category_rule,
            commands::rules::update_category_rule,
            commands::rules::delete_category_rule,
            commands::rules::apply_category_rules,
            commands::tags::list_tags,
            commands::tags::add_tag_to_transaction,
            commands::tags::remove_tag_from_transaction,
            commands::tags::get_tags_for_transactions,
            commands::splits::get_transaction_splits,
            commands::splits::set_transaction_splits,
            commands::splits::get_split_transaction_ids,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}