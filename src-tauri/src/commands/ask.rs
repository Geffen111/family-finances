// Natural-language questions about the user's finances. Hybrid approach:
//   1. LLM turns the question into a single read-only SQL query.
//   2. We validate it (SELECT-only, single statement) and run it for exact figures.
//   3. LLM turns the rows into a plain-English answer, reasoning about
//      hypotheticals ("what if I stopped...") over the real numbers.
// The generated SQL and rows are returned too, so the user can check the working.

use crate::models::AskResponse;
use sqlx::{Column, Row, SqlitePool};
use tauri::State;

const MAX_DISPLAY_ROWS: usize = 200;
const MAX_ANSWER_ROWS: usize = 50;

// Tokens that would mutate the database or schema. Matched whole-word against
// the tokenised query, so identifiers like `created_at` are not affected.
const FORBIDDEN_TOKENS: &[&str] = &[
    "insert", "update", "delete", "drop", "alter", "create", "attach", "detach",
    "pragma", "replace", "vacuum", "reindex", "truncate", "grant", "commit",
    "begin", "rollback", "savepoint",
];

fn schema_doc(today: &str) -> String {
    format!(
        r#"SQLite database for an Australian family's finances. Today is {today}. Tables:

transactions(id, account_id, category_id, date TEXT 'YYYY-MM-DD', description TEXT,
  debit REAL, credit REAL, balance REAL, notes TEXT, created_at TEXT)
  - debit  = money OUT (an expense/spend), stored as a positive number.
  - credit = money IN (income/deposit), stored as a positive number.
  - description is the raw bank line (e.g. 'AMAZON AU SYDNEY'); match merchants with
    'description LIKE ''%amazon%'' COLLATE NOCASE'.
  - category_id may be NULL (uncategorised).

categories(id, name, parent_id, monthly_budget REAL, exclude_from_budget INTEGER, created_at)
  - parent_id links a sub-category to its parent (NULL for top-level).
  - exclude_from_budget = 1 marks internal transfers etc. that are NOT real
    income/spending. Exclude these from spend/income totals unless asked otherwise:
    'AND NOT EXISTS (SELECT 1 FROM categories xc WHERE xc.id = transactions.category_id AND xc.exclude_from_budget = 1)'.

accounts(id, name, type TEXT 'asset'|'liability', created_at)

Date helpers: use date('now'), date('now','-12 months'), strftime('%Y-%m', date), etc."#,
        today = today
    )
}

#[tauri::command]
pub async fn ask_question(
    pool: State<'_, SqlitePool>,
    question: String,
) -> Result<AskResponse, String> {
    let api_key = crate::commands::settings::get_api_key()
        .await?
        .ok_or_else(|| "OpenRouter API key not configured. Please add your API key in Settings.".to_string())?;

    let q = question.trim();
    if q.is_empty() {
        return Err("Please enter a question.".to_string());
    }

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let schema = schema_doc(&today);

    // Step 1: generate SQL (with one retry if the query fails to execute).
    let mut plan = generate_sql(&api_key, &schema, q, None).await?;
    let (columns, rows, truncated) = match run_query(&pool, &plan.sql).await {
        Ok(r) => r,
        Err(first_err) => {
            plan = generate_sql(&api_key, &schema, q, Some(&first_err)).await?;
            run_query(&pool, &plan.sql)
                .await
                .map_err(|e| format!("Could not run the generated query: {}", e))?
        }
    };

    // Step 3: turn the rows into a plain-English answer.
    let answer = answer_from_rows(&api_key, q, &plan.explanation, &columns, &rows).await?;

    Ok(AskResponse {
        answer,
        sql: plan.sql,
        explanation: plan.explanation,
        columns,
        rows,
        truncated,
    })
}

struct QueryPlan {
    sql: String,
    explanation: String,
}

async fn generate_sql(
    api_key: &str,
    schema: &str,
    question: &str,
    retry_error: Option<&str>,
) -> Result<QueryPlan, String> {
    let retry_note = match retry_error {
        Some(e) => format!(
            "\n\nYour previous query failed with this error, fix it:\n{}",
            e
        ),
        None => String::new(),
    };

    let prompt = format!(
        r#"{schema}

Write ONE read-only SQL query (a single SELECT, optionally with a leading WITH/CTE) that
retrieves the data needed to answer this question:

"{question}"

Rules:
- Read-only: SELECT only. No INSERT/UPDATE/DELETE/PRAGMA/etc. One statement, no semicolons.
- Prefer aggregates (SUM, COUNT, AVG) so the result is small and directly answers the question.
- Cast money sums with CAST(... AS REAL) so they come back as decimals.
- Add a LIMIT (<= 200) if the query could return many rows.

Respond with JSON only, no markdown:
{{"sql": "SELECT ...", "explanation": "one short line on what this computes"}}{retry_note}"#,
        schema = schema,
        question = question,
        retry_note = retry_note,
    );

    let content = call_openrouter(api_key, &prompt).await?;
    let cleaned = strip_code_fences(&content);

    #[derive(serde::Deserialize)]
    struct Plan {
        sql: String,
        #[serde(default)]
        explanation: String,
    }
    let plan: Plan = serde_json::from_str(cleaned)
        .map_err(|e| format!("Could not parse the model's query: {} - {}", e, cleaned))?;

    let sql = sanitize_sql(&plan.sql)?;
    Ok(QueryPlan {
        sql,
        explanation: plan.explanation,
    })
}

/// Validates that `sql` is a single read-only statement and returns it trimmed.
fn sanitize_sql(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim().trim_end_matches(';').trim();
    if trimmed.is_empty() {
        return Err("The model returned an empty query.".to_string());
    }
    // Single statement only.
    if trimmed.contains(';') {
        return Err("Only a single statement is allowed.".to_string());
    }
    let lower = trimmed.to_lowercase();
    if !(lower.starts_with("select") || lower.starts_with("with")) {
        return Err("Only read-only SELECT queries are allowed.".to_string());
    }
    // Whole-word check against mutating keywords (so `created_at` is fine).
    for token in lower.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
        if FORBIDDEN_TOKENS.contains(&token) {
            return Err(format!("Query rejected: contains disallowed keyword '{}'.", token));
        }
    }
    Ok(trimmed.to_string())
}

#[allow(clippy::type_complexity)]
async fn run_query(
    pool: &SqlitePool,
    sql: &str,
) -> Result<(Vec<String>, Vec<Vec<String>>, bool), String> {
    let fetched = sqlx::query(sql)
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;

    let mut columns: Vec<String> = Vec::new();
    if let Some(first) = fetched.first() {
        columns = first.columns().iter().map(|c| c.name().to_string()).collect();
    }

    let truncated = fetched.len() > MAX_DISPLAY_ROWS;
    let rows: Vec<Vec<String>> = fetched
        .iter()
        .take(MAX_DISPLAY_ROWS)
        .map(|row| {
            (0..columns.len())
                .map(|i| value_to_string(row, i))
                .collect()
        })
        .collect();

    Ok((columns, rows, truncated))
}

/// Best-effort stringify of a dynamically-typed SQLite cell.
fn value_to_string(row: &sqlx::sqlite::SqliteRow, i: usize) -> String {
    if let Ok(v) = row.try_get::<Option<i64>, _>(i) {
        return v.map(|x| x.to_string()).unwrap_or_default();
    }
    if let Ok(v) = row.try_get::<Option<f64>, _>(i) {
        return v.map(|x| format!("{}", x)).unwrap_or_default();
    }
    if let Ok(v) = row.try_get::<Option<String>, _>(i) {
        return v.unwrap_or_default();
    }
    String::new()
}

async fn answer_from_rows(
    api_key: &str,
    question: &str,
    explanation: &str,
    columns: &[String],
    rows: &[Vec<String>],
) -> Result<String, String> {
    // Compact text table for the model.
    let mut table = columns.join(" | ");
    table.push('\n');
    for row in rows.iter().take(MAX_ANSWER_ROWS) {
        table.push_str(&row.join(" | "));
        table.push('\n');
    }
    if rows.is_empty() {
        table.push_str("(no rows)\n");
    }

    let prompt = format!(
        r#"You are a personal finance assistant for an Australian family (currency AUD).
The user asked:

"{question}"

To answer, this query was run ({explanation}). Its result:

{table}

Write a concise, direct answer in plain English using these figures. Format money as AUD
(e.g. $1,234.56). If the question is hypothetical (e.g. "how much would I save if I
stopped..."), use the figures to project the saving (state the period and any assumption,
e.g. annualising a monthly amount). If the result is empty, say no matching data was found.
Answer in 1-3 sentences. Do not mention SQL."#,
        question = question,
        explanation = explanation,
        table = table,
    );

    call_openrouter(api_key, &prompt).await
}

fn strip_code_fences(content: &str) -> &str {
    content
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim()
}

async fn call_openrouter(api_key: &str, prompt: &str) -> Result<String, String> {
    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "model": "deepseek/deepseek-v4-flash",
        "messages": [{"role": "user", "content": prompt}],
        "temperature": 0.1,
        "max_tokens": 1024
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
    let text = response
        .text()
        .await
        .map_err(|e| format!("Failed to read response: {}", e))?;

    if !status.is_success() {
        return Err(format!("OpenRouter API error ({}): {}", status, text));
    }

    #[derive(serde::Deserialize)]
    struct Resp {
        choices: Vec<Choice>,
    }
    #[derive(serde::Deserialize)]
    struct Choice {
        message: Msg,
    }
    #[derive(serde::Deserialize)]
    struct Msg {
        content: String,
    }

    let parsed: Resp = serde_json::from_str(&text)
        .map_err(|e| format!("Failed to parse API response: {} - {}", e, text))?;
    parsed
        .choices
        .first()
        .map(|c| c.message.content.clone())
        .ok_or_else(|| "No choices in API response".to_string())
}
