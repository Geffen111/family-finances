use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Account {
    pub id: i64,
    pub name: String,
    #[sqlx(rename = "type")]
    #[serde(rename = "type")]
    pub account_type: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Category {
    pub id: i64,
    pub name: String,
    pub parent_id: Option<i64>,
    pub monthly_budget: Option<f64>,
    pub created_at: String,
    pub exclude_from_budget: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct CategoryWithPath {
    pub id: i64,
    pub name: String,
    pub parent_id: Option<i64>,
    pub monthly_budget: Option<f64>,
    pub created_at: String,
    pub exclude_from_budget: bool,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Transaction {
    pub id: i64,
    pub account_id: i64,
    pub category_id: Option<i64>,
    pub date: String,
    pub description: String,
    pub debit: f64,
    pub credit: f64,
    pub balance: Option<f64>,
    pub ai_category: Option<String>,
    pub ai_category_conf: Option<f64>,
    pub ai_categorised_at: Option<String>,
    pub notes: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SavingsGoal {
    pub id: i64,
    pub name: String,
    pub target_amount: f64,
    pub current_amount: f64,
    pub target_date: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Scenario {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub horizon: String,
    pub base_start_date: String,
    pub base_end_date: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ScenarioAdjustment {
    pub id: i64,
    pub scenario_id: i64,
    pub category_id: i64,
    pub adjustment_pct: f64,
    pub fixed_amount: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ScenarioDefault {
    pub id: i64,
    pub scenario_id: i64,
    pub default_adjustment_pct: f64,
    pub income_growth_pct: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardSummary {
    pub total_income: f64,
    pub total_expenses: f64,
    pub net: f64,
    pub top_category: String,
    pub top_category_amount: f64,
    pub transaction_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategorySpending {
    pub category_name: String,
    pub category_path: String,
    pub total: f64,
    pub percentage: f64,
    pub transaction_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonthlyTrend {
    pub month: String,
    pub label: String,
    pub income: f64,
    pub expenses: f64,
    pub net: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryTrend {
    pub category_id: Option<i64>,
    pub category_name: String,
    pub month: String,
    pub label: String,
    pub amount: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpendingInsights {
    pub summary: String,
    pub spending_patterns: Vec<InsightItem>,
    pub anomalies: Vec<InsightItem>,
    pub recommendations: Vec<InsightItem>,
    pub period_label: String,
    pub generated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsightItem {
    pub title: String,
    pub detail: String,
    pub severity: String,
    pub icon: String,
}

// -- Forecast models --

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForecastResult {
    pub scenario_name: String,
    pub scenario_id: i64,
    pub months: Vec<ForecastMonth>,
    pub totals: ForecastTotals,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForecastMonth {
    pub label: String,
    pub month_key: String,
    pub projected_income: f64,
    pub projected_expenses: f64,
    pub projected_net: f64,
    pub categories: Vec<ForecastCategoryAmount>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForecastCategoryAmount {
    pub category_id: i64,
    pub category_path: String,
    pub amount: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForecastTotals {
    pub total_projected_income: f64,
    pub total_projected_expenses: f64,
    pub total_projected_net: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForecastComparison {
    pub base: ForecastResult,
    pub scenarios: Vec<ForecastResult>,
    pub months_ahead: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioAdjustmentWithPath {
    pub id: i64,
    pub scenario_id: i64,
    pub category_id: i64,
    pub category_path: String,
    pub adjustment_pct: f64,
    pub fixed_amount: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryBaseline {
    pub category_id: i64,
    pub category_path: String,
    pub monthly_avg: f64,
    pub is_income: bool,
}