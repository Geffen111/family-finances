use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategorisationResult {
    pub category_path: String,
    pub confidence: f64,
    pub reasoning: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenRouterResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Choice {
    message: Message,
}

#[derive(Debug, Serialize, Deserialize)]
struct Message {
    content: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ApiResponseItem {
    transaction_index: usize,
    category_path: String,
    confidence: f64,
    reasoning: String,
}

pub struct AiCategorizer {
    client: reqwest::Client,
    api_key: String,
}

impl AiCategorizer {
    pub fn new(api_key: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
        }
    }

    pub async fn categorise_batch(
        &self,
        transactions: &[TransactionInfo],
        category_paths: &[String],
    ) -> Result<Vec<CategorisationResult>, String> {
        let categories_list = category_paths
            .iter()
            .map(|p| format!("- {}", p))
            .collect::<Vec<_>>()
            .join("\n");

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

Respond with a JSON array only:
[
  {{"transaction_index": 0, "category_path": "Food & Beverage > Food And Groceries", "confidence": 0.95, "reasoning": "Coles is a supermarket"}},
  {{"transaction_index": 1, ...}}
]

Transactions to categorise:
{}"#,
            categories_list, transactions_json
        );

        let body = serde_json::json!({
            "model": "deepseek/deepseek-v4-flash",
            "messages": [
                {"role": "user", "content": prompt}
            ],
            "temperature": 0.1,
            "max_tokens": 4096
        });

        let response = self
            .client
            .post("https://openrouter.ai/api/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
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

        let items: Vec<ApiResponseItem> = serde_json::from_str(cleaned)
            .map_err(|e| format!("Failed to parse AI response items: {} - Content: {}", e, cleaned))?;

        let mut results: Vec<CategorisationResult> = transactions
            .iter()
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
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionInfo {
    pub description: String,
    pub debit: f64,
    pub credit: f64,
}
