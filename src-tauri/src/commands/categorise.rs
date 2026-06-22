use crate::models::CategoryWithPath;
use crate::services::categorizer::{AiCategorizer, CategorisationResult, TransactionInfo};
use sqlx::SqlitePool;
use tauri::State;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct CategorisationSuggestion {
    pub transaction_id: i64,
    pub date: String,
    pub description: String,
    pub debit: f64,
    pub credit: f64,
    pub suggested_category: String,
    pub category_id: Option<i64>,
    pub confidence: f64,
    pub reasoning: String,
}

fn find_category_id(categories: &[CategoryWithPath], path: &str) -> Option<i64> {
    if path == "Unknown > Unknown" {
        return None;
    }
    categories.iter().find(|c| c.path == path).map(|c| c.id)
}

#[tauri::command]
pub async fn categorise_transactions(
    pool: State<'_, SqlitePool>,
) -> Result<Vec<CategorisationSuggestion>, String> {
    let api_key = crate::commands::settings::get_api_key()
        .await?
        .ok_or_else(|| "OpenRouter API key not configured. Go to Settings first.".to_string())?;

    let categories = sqlx::query_as::<_, crate::models::Category>(
        "SELECT id, name, parent_id, monthly_budget, created_at FROM categories ORDER BY id"
    )
    .fetch_all(&*pool)
    .await
    .map_err(|e| format!("DB query error: {}", e))?;

    let parents: Vec<_> = categories.iter().filter(|c| c.parent_id.is_none()).collect();

    let mut category_paths = Vec::new();
    let mut category_with_paths = Vec::new();

    for parent in &parents {
        let parent_path = parent.name.clone();
        category_with_paths.push(CategoryWithPath {
            id: parent.id,
            name: parent.name.clone(),
            parent_id: parent.parent_id,
            monthly_budget: parent.monthly_budget,
            created_at: parent.created_at.clone(),
            path: parent_path,
        });

        let mut has_child = false;
        for child in &categories {
            if child.parent_id == Some(parent.id) {
                let path = format!("{} > {}", parent.name, child.name);
                category_paths.push(path.clone());
                category_with_paths.push(CategoryWithPath {
                    id: child.id,
                    name: child.name.clone(),
                    parent_id: child.parent_id,
                    monthly_budget: child.monthly_budget,
                    created_at: child.created_at.clone(),
                    path,
                });
                has_child = true;
            }
        }
        if !has_child {
            category_paths.push(parent.name.clone());
        }
    }

    let uncategorised: Vec<crate::models::Transaction> = sqlx::query_as::<_, crate::models::Transaction>(
        "SELECT id, account_id, category_id, date, description, debit, credit, balance, \
         ai_category, ai_category_conf, ai_categorised_at, notes, created_at \
         FROM transactions WHERE category_id IS NULL \
         ORDER BY date DESC LIMIT 50"
    )
    .fetch_all(&*pool)
    .await
    .map_err(|e| format!("DB query error: {}", e))?;

    if uncategorised.is_empty() {
        return Ok(Vec::new());
    }

    let tx_infos: Vec<TransactionInfo> = uncategorised
        .iter()
        .map(|t| TransactionInfo {
            description: t.description.clone(),
            debit: t.debit,
            credit: t.credit,
        })
        .collect();

    let categorizer = AiCategorizer::new(api_key);

    let mut all_results: Vec<CategorisationSuggestion> = Vec::new();
    let mut remaining = tx_infos.as_slice();

    while !remaining.is_empty() {
        let batch = if remaining.len() > 50 { &remaining[..50] } else { remaining };
        let results: Vec<CategorisationResult> = categorizer.categorise_batch(batch, &category_paths).await?;

        let batch_start = uncategorised.len() - remaining.len();
        for (j, result) in results.iter().enumerate() {
            let tx = &uncategorised[batch_start + j];
            let category_id = find_category_id(&category_with_paths, &result.category_path);
            let confidence = if result.confidence > 1.0 { 1.0 } else if result.confidence < 0.0 { 0.0 } else { result.confidence };

            all_results.push(CategorisationSuggestion {
                transaction_id: tx.id,
                date: tx.date.clone(),
                description: tx.description.clone(),
                debit: tx.debit,
                credit: tx.credit,
                suggested_category: result.category_path.clone(),
                category_id,
                confidence,
                reasoning: result.reasoning.clone(),
            });
        }

        remaining = if remaining.len() > 50 { &remaining[50..] } else { &[] };
    }

    Ok(all_results)
}

#[tauri::command]
pub async fn accept_categorisations(
    pool: State<'_, SqlitePool>,
    suggestions: Vec<CategorisationSuggestion>,
) -> Result<i64, String> {
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let mut accepted: i64 = 0;

    for s in &suggestions {
        if let Some(category_id) = s.category_id {
            sqlx::query(
                "UPDATE transactions SET category_id = ?, ai_category = ?, ai_category_conf = ?, ai_categorised_at = ? WHERE id = ?"
            )
            .bind(category_id)
            .bind(&s.suggested_category)
            .bind(s.confidence)
            .bind(&now)
            .bind(s.transaction_id)
            .execute(&*pool)
            .await
            .map_err(|e| format!("DB update error: {}", e))?;

            accepted += 1;
        }
    }

    Ok(accepted)
}
