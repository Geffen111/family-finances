use crate::services::openrouter::{self, Answer};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategorisationResult {
    pub category_path: String,
    pub confidence: f64,
    pub reasoning: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ApiResponseItem {
    transaction_index: usize,
    category_path: String,
    confidence: f64,
    reasoning: String,
}

pub struct AiCategorizer {
    api_key: String,
}

impl AiCategorizer {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }

    /// Categorise one batch. The reply has to parse as a JSON array of items,
    /// so a model that answers with prose (or truncates mid-array) counts as a
    /// failure and the configured fallback model gets the batch instead — see
    /// `openrouter::chat_parsed`.
    pub async fn categorise_batch(
        &self,
        transactions: &[TransactionInfo],
        category_paths: &[String],
        examples: &[(String, String)],
    ) -> Result<Answer<Vec<CategorisationResult>>, String> {
        let categories_list = category_paths
            .iter()
            .map(|p| format!("- {}", p))
            .collect::<Vec<_>>()
            .join("\n");

        // Few-shot examples drawn from the user's own past categorisations so
        // the model imitates their conventions and custom category names.
        let examples_block = if examples.is_empty() {
            String::new()
        } else {
            let lines = examples
                .iter()
                .map(|(desc, path)| format!("- \"{}\" -> {}", desc, path))
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                "\nExamples of how the user has categorised past transactions \
(follow these conventions closely):\n{}\n",
                lines
            )
        };

        let transactions_json = serde_json::to_string(
            &transactions
                .iter()
                .enumerate()
                .map(|(i, t)| serde_json::json!({
                    "transaction_index": i,
                    "description": t.description,
                    "debit": t.debit,
                    "credit": t.credit,
                }))
                .collect::<Vec<_>>(),
        )
        .map_err(|e| format!("Failed to serialize transactions: {}", e))?;

        let prompt = format!(
            r#"You are a personal finance categorisation assistant. Categorise each transaction into one of these categories.

Available categories (Parent > Child):
{}

For each transaction, return the best matching category path. If no category fits well, use "Unknown > Unknown".
{}
Respond with a JSON array only:
[
  {{"transaction_index": 0, "category_path": "Food & Beverage > Food And Groceries", "confidence": 0.95, "reasoning": "Coles is a supermarket"}},
  {{"transaction_index": 1, ...}}
]

Transactions to categorise:
{}"#,
            categories_list, examples_block, transactions_json
        );

        let count = transactions.len();
        openrouter::chat_parsed(&self.api_key, &prompt, 0.1, 4096, move |content| {
            let items: Vec<ApiResponseItem> = serde_json::from_str(content).map_err(|e| {
                format!("Failed to parse AI response items: {} - Content: {}", e, content)
            })?;

            let mut results: Vec<CategorisationResult> = (0..count)
                .map(|_| CategorisationResult {
                    category_path: "Unknown > Unknown".to_string(),
                    confidence: 0.0,
                    reasoning: "No suggestion".to_string(),
                })
                .collect();

            for item in items {
                if item.transaction_index < results.len() {
                    results[item.transaction_index] = CategorisationResult {
                        category_path: item.category_path,
                        confidence: item.confidence,
                        reasoning: item.reasoning,
                    };
                }
            }

            Ok(results)
        })
        .await
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionInfo {
    pub description: String,
    pub debit: f64,
    pub credit: f64,
}
