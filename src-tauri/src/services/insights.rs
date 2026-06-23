use crate::models::SpendingInsights;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct OpenRouterResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct Choice {
    message: Message,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct Message {
    content: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct CachedInsight {
    insight_data: String,
    generated_at: String,
}

fn compute_hash(data: &serde_json::Value) -> String {
    let json_str = serde_json::to_string(data).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(json_str.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub async fn fetch_spending_insights(
    pool: &SqlitePool,
    api_key: &str,
    start_date: &str,
    end_date: &str,
    force_refresh: bool,
) -> Result<SpendingInsights, String> {
    let (category_totals, monthly_changes, top_transactions, income, expenses, _avg_daily) =
        query_aggregated_data(pool, start_date, end_date).await?;

    let aggregate = serde_json::json!({
        "category_totals": category_totals,
        "monthly_changes": monthly_changes,
        "top_transactions": top_transactions,
        "income": income,
        "expenses": expenses,
    });
    let data_hash = compute_hash(&aggregate);

    let period_label = if end_date.is_empty() {
        format!("{} to present", start_date)
    } else {
        format!("{} to {}", start_date, end_date)
    };

    if !force_refresh {
        if let Some(cached) = check_cache(pool, start_date, end_date, &data_hash).await? {
            let mut insights: SpendingInsights =
                serde_json::from_str(&cached.insight_data).map_err(|e| format!("Failed to parse cached insights: {}", e))?;
            insights.generated_at = cached.generated_at;
            insights.period_label = period_label;
            return Ok(insights);
        }
    }

    let insights = call_openrouter(
        api_key,
        start_date,
        end_date,
        &category_totals,
        &monthly_changes,
        &top_transactions,
        income,
        expenses,
        &period_label,
    )
    .await?;

    save_cache(pool, start_date, end_date, &data_hash, &insights).await?;

    Ok(insights)
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct CategoryTotal {
    category: String,
    total: f64,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct MonthlyChange {
    category: String,
    change_pct: f64,
    previous_total: f64,
    current_total: f64,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct TopTransaction {
    date: String,
    description: String,
    amount: f64,
    category: String,
}

async fn query_aggregated_data(
    pool: &SqlitePool,
    start_date: &str,
    end_date: &str,
) -> Result<
    (
        Vec<CategoryTotal>,
        Vec<MonthlyChange>,
        Vec<TopTransaction>,
        f64,
        f64,
        f64,
    ),
    String,
> {
    let use_end = if end_date.is_empty() { start_date } else { end_date };

    let cats = query_category_totals(pool, start_date, use_end).await?;
    let changes = query_monthly_changes(pool, start_date, use_end).await?;
    let top = query_top_transactions(pool, start_date, use_end).await?;

    let income = query_total_income(pool, start_date, use_end).await?;
    let expenses = query_total_expenses(pool, start_date, use_end).await?;
    let avg_daily = query_avg_daily_spending(pool, start_date, use_end).await?;

    Ok((cats, changes, top, income, expenses, avg_daily))
}

async fn query_category_totals(
    pool: &SqlitePool,
    start: &str,
    end: &str,
) -> Result<Vec<CategoryTotal>, String> {
    let rows = sqlx::query_as::<_, (String, f64)>(
        "SELECT COALESCE(c.name, 'Uncategorised') as category, CAST(COALESCE(SUM(t.debit), 0) AS REAL) as total
         FROM transactions t
         LEFT JOIN categories c ON t.category_id = c.id
         WHERE t.debit > 0 AND t.date >= ? AND t.date <= ?
         GROUP BY c.name
         ORDER BY total DESC",
    )
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("DB error category totals: {}", e))?;

    Ok(rows
        .into_iter()
        .map(|(category, total)| CategoryTotal { category, total })
        .collect())
}

async fn query_monthly_changes(
    pool: &SqlitePool,
    start: &str,
    end: &str,
) -> Result<Vec<MonthlyChange>, String> {
    let rows = sqlx::query_as::<_, (String, String, f64)>(
        "SELECT COALESCE(c.name, 'Uncategorised') as category,
                strftime('%Y-%m', t.date) as month,
                CAST(COALESCE(SUM(t.debit), 0) AS REAL) as total
         FROM transactions t
         LEFT JOIN categories c ON t.category_id = c.id
         WHERE t.debit > 0 AND t.date >= ? AND t.date <= ?
         GROUP BY c.name, strftime('%Y-%m', t.date)
         ORDER BY c.name, month",
    )
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("DB error monthly changes: {}", e))?;

    let mut grouped: std::collections::HashMap<String, Vec<(String, f64)>> =
        std::collections::HashMap::new();
    for (cat, month, total) in rows {
        grouped.entry(cat).or_default().push((month, total));
    }

    let mut changes = Vec::new();
    for (category, months) in grouped {
        if months.len() >= 2 {
            let last = &months[months.len() - 1];
            let prev = &months[months.len() - 2];
            let change_pct = if prev.1 > 0.0 {
                ((last.1 - prev.1) / prev.1) * 100.0
            } else {
                0.0
            };
            changes.push(MonthlyChange {
                category,
                change_pct,
                previous_total: prev.1,
                current_total: last.1,
            });
        }
    }
    changes.sort_by(|a, b| b.change_pct.partial_cmp(&a.change_pct).unwrap_or(std::cmp::Ordering::Equal));

    Ok(changes)
}

async fn query_top_transactions(
    pool: &SqlitePool,
    start: &str,
    end: &str,
) -> Result<Vec<TopTransaction>, String> {
    let rows = sqlx::query_as::<_, (String, String, f64, Option<String>)>(
        "SELECT t.date, t.description, CAST(t.debit AS REAL), c.name
         FROM transactions t
         LEFT JOIN categories c ON t.category_id = c.id
         WHERE t.debit > 0 AND t.date >= ? AND t.date <= ?
         ORDER BY t.debit DESC
         LIMIT 10",
    )
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("DB error top transactions: {}", e))?;

    Ok(rows
        .into_iter()
        .map(|(date, description, amount, category)| TopTransaction {
            date,
            description,
            amount,
            category: category.unwrap_or_else(|| "Uncategorised".to_string()),
        })
        .collect())
}

async fn query_total_income(pool: &SqlitePool, start: &str, end: &str) -> Result<f64, String> {
    let result: Option<(f64,)> = sqlx::query_as(
        "SELECT CAST(COALESCE(SUM(t.credit), 0) AS REAL) FROM transactions t WHERE t.date >= ? AND t.date <= ?",
    )
    .bind(start)
    .bind(end)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("DB error income: {}", e))?;

    Ok(result.map(|r| r.0).unwrap_or(0.0))
}

async fn query_total_expenses(pool: &SqlitePool, start: &str, end: &str) -> Result<f64, String> {
    let result: Option<(f64,)> = sqlx::query_as(
        "SELECT CAST(COALESCE(SUM(t.debit), 0) AS REAL) FROM transactions t WHERE t.date >= ? AND t.date <= ?",
    )
    .bind(start)
    .bind(end)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("DB error expenses: {}", e))?;

    Ok(result.map(|r| r.0).unwrap_or(0.0))
}

async fn query_avg_daily_spending(pool: &SqlitePool, start: &str, end: &str) -> Result<f64, String> {
    let row: Option<(f64, i64)> = sqlx::query_as(
        "SELECT CAST(COALESCE(SUM(t.debit), 0) AS REAL), COUNT(DISTINCT t.date)
         FROM transactions t WHERE t.debit > 0 AND t.date >= ? AND t.date <= ?",
    )
    .bind(start)
    .bind(end)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("DB error avg daily: {}", e))?;

    match row {
        Some((total, days)) if days > 0 => Ok(total / days as f64),
        _ => Ok(0.0),
    }
}

async fn check_cache(
    pool: &SqlitePool,
    start_date: &str,
    end_date: &str,
    data_hash: &str,
) -> Result<Option<CachedInsight>, String> {
    let result: Option<(String, String)> = sqlx::query_as(
        "SELECT insight_data, generated_at FROM ai_insights
         WHERE period_start = ? AND period_end = ? AND data_hash = ?
         ORDER BY generated_at DESC LIMIT 1",
    )
    .bind(start_date)
    .bind(end_date)
    .bind(data_hash)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("DB error check cache: {}", e))?;

    Ok(result.map(|(insight_data, generated_at)| CachedInsight {
        insight_data,
        generated_at,
    }))
}

async fn save_cache(
    pool: &SqlitePool,
    start_date: &str,
    end_date: &str,
    data_hash: &str,
    insights: &SpendingInsights,
) -> Result<(), String> {
    let insight_data = serde_json::to_string(insights)
        .map_err(|e| format!("Failed to serialize insights: {}", e))?;

    sqlx::query(
        "INSERT INTO ai_insights (insight_type, insight_data, period_start, period_end, data_hash)
         VALUES ('spending_analysis', ?, ?, ?, ?)",
    )
    .bind(&insight_data)
    .bind(start_date)
    .bind(end_date)
    .bind(data_hash)
    .execute(pool)
    .await
    .map_err(|e| format!("DB error save cache: {}", e))?;

    Ok(())
}

async fn call_openrouter(
    api_key: &str,
    start_date: &str,
    end_date: &str,
    category_totals: &[CategoryTotal],
    monthly_changes: &[MonthlyChange],
    top_transactions: &[TopTransaction],
    income: f64,
    expenses: f64,
    period_label: &str,
) -> Result<SpendingInsights, String> {
    let csv_categories = category_totals
        .iter()
        .map(|c| format!("{},${:.2}", c.category, c.total))
        .collect::<Vec<_>>()
        .join("\n");

    let csv_changes = monthly_changes
        .iter()
        .map(|c| {
            format!(
                "{},{:.1}%,${:.2},${:.2}",
                c.category, c.change_pct, c.previous_total, c.current_total
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let top_list = top_transactions
        .iter()
        .map(|t| format!("{} | {} | ${:.2} | {}", t.date, t.description, t.amount, t.category))
        .collect::<Vec<_>>()
        .join("\n");

    let net = income - expenses;

    let prompt = format!(
        r#"You are a personal finance analyst for an Australian family. Analyse their spending data and provide insights.

Period: {start_date} to {end_date}

Monthly spending by category:
{cat_table}

Month-over-month changes:
{change_table}

Top transactions:
{top_table}

Summary: Total Income: ${income:.2}, Total Expenses: ${expenses:.2}, Net: ${net:.2}

Respond with JSON only:
{{
  "summary": "One-sentence overview of this period",
  "spending_patterns": [
    {{"title": "Grocery spending up 15%", "detail": "You spent $X more on groceries this month...", "severity": "warning", "icon": "📈"}}
  ],
  "anomalies": [
    {{"title": "Large one-off purchase", "detail": "A $X transaction at ...", "severity": "warning", "icon": "⚠️"}}
  ],
  "recommendations": [
    {{"title": "Review meal planning", "detail": "Setting a weekly meal plan could reduce...", "severity": "positive", "icon": "💡"}}
  ]
}}"#,
        start_date = start_date,
        end_date = end_date,
        cat_table = csv_categories,
        change_table = csv_changes,
        top_table = top_list,
        income = income,
        expenses = expenses,
        net = net,
    );

    let client = reqwest::Client::new();

    let body = serde_json::json!({
        "model": "deepseek/deepseek-v4-flash",
        "messages": [
            {"role": "user", "content": prompt}
        ],
        "temperature": 0.3,
        "max_tokens": 4096
    });

    let response = client
        .post("https://openrouter.ai/api/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("API request failed: {}", e))?;

    let status = response.status();
    let response_text = response
        .text()
        .await
        .map_err(|e| format!("Failed to read response: {}", e))?;

    if !status.is_success() {
        return Err(format!("OpenRouter API error ({}): {}", status, response_text));
    }

    let openrouter_resp: OpenRouterResponse = serde_json::from_str(&response_text)
        .map_err(|e| format!("Failed to parse API response: {} - Response: {}", e, response_text))?;

    let content = openrouter_resp
        .choices
        .first()
        .map(|c| c.message.content.clone())
        .ok_or_else(|| "No choices in API response".to_string())?;

    let cleaned = content
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    let mut insights: SpendingInsights =
        serde_json::from_str(cleaned).map_err(|e| format!("Failed to parse AI response: {} - Content: {}", e, cleaned))?;

    insights.period_label = period_label.to_string();
    insights.generated_at = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

    Ok(insights)
}
