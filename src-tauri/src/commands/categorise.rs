use crate::models::CategoryWithPath;
use crate::services::categorizer::{AiCategorizer, TransactionInfo};
use sqlx::SqlitePool;
use std::collections::{HashMap, HashSet};
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

// Reduce a noisy bank description to a stable "merchant key" so the same payee
// across different months/amounts collapses to one key. Drops dates, numbers,
// and common bank-statement boilerplate, keeping the distinctive merchant words.
pub fn normalize_desc(desc: &str) -> String {
    const NOISE: &[&str] = &[
        "visa", "purchase", "tfr", "wdl", "bpay", "internet", "withdrawal",
        "eftpos", "debit", "card", "direct", "credit", "payment", "pos", "pty",
        "ltd", "aus", "australia", "ref", "from", "the", "batch", "phone",
        "value", "date", "transaction", "deposit", "transfer",
    ];
    const MONTHS: &[&str] = &[
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    desc.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|tok| {
            let t = *tok;
            t.len() >= 3
                && !t.chars().any(|c| c.is_ascii_digit())
                && !NOISE.contains(&t)
                && !MONTHS.contains(&t)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// Pull the account-number-like digit runs out of a bank description. These are
// exactly what `normalize_desc` throws away, but for contextless transfers
// (e.g. "Transfer to 12345678") the account number is the only signal we have.
// Runs of >= 5 digits are kept; shorter ones (days, months, 4-digit years,
// small refs) are dropped so we don't key off date/noise digits.
fn extract_account_digits(desc: &str) -> String {
    let mut runs: Vec<String> = Vec::new();
    let mut current = String::new();
    let flush = |cur: &mut String, runs: &mut Vec<String>| {
        if cur.len() >= 5 {
            runs.push(cur.clone());
        }
        cur.clear();
    };
    for c in desc.chars() {
        if c.is_ascii_digit() {
            current.push(c);
        } else {
            flush(&mut current, &mut runs);
        }
    }
    flush(&mut current, &mut runs);
    runs.join("-")
}

// Key a transaction by (account number in description + direction + amount), or
// None when the description carries no account number. Lets the history matcher
// recognise repeated transfers whose only distinguishing detail is the account
// and amount — something the merchant key (which strips all digits) cannot.
fn account_amount_key(desc: &str, debit: f64, credit: f64) -> Option<String> {
    let digits = extract_account_digits(desc);
    if digits.is_empty() {
        return None;
    }
    let (amount, dir) = if debit > 0.0 { (debit, 'd') } else { (credit, 'c') };
    Some(format!("{}|{}|{:.2}", digits, dir, amount))
}

// Collapse whitespace and truncate, for compact few-shot examples.
fn short_desc(desc: &str) -> String {
    let collapsed = desc.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() > 60 {
        let mut s: String = collapsed.chars().take(57).collect();
        s.push_str("...");
        s
    } else {
        collapsed
    }
}

#[tauri::command]
pub async fn categorise_transactions(
    pool: State<'_, SqlitePool>,
) -> Result<Vec<CategorisationSuggestion>, String> {
    let categories = sqlx::query_as::<_, crate::models::Category>(
        "SELECT id, name, parent_id, monthly_budget, created_at, exclude_from_budget FROM categories ORDER BY id"
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
            exclude_from_budget: parent.exclude_from_budget,
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
                    exclude_from_budget: child.exclude_from_budget,
                    path,
                });
                has_child = true;
            }
        }
        if !has_child {
            category_paths.push(parent.name.clone());
        }
    }

    // Path lookup by id, for turning a matched category back into a path.
    let path_by_id: HashMap<i64, String> = category_with_paths
        .iter()
        .map(|c| (c.id, c.path.clone()))
        .collect();

    // Pull the user's already-categorised transactions to learn from.
    let categorised = sqlx::query_as::<_, crate::models::Transaction>(
        "SELECT id, account_id, category_id, date, description, debit, credit, balance, \
         ai_category, ai_category_conf, ai_categorised_at, notes, created_at \
         FROM transactions WHERE category_id IS NOT NULL",
    )
    .fetch_all(&*pool)
    .await
    .map_err(|e| format!("DB query error: {}", e))?;

    // history_map: merchant key -> majority category_id seen for that key.
    // acct_map: (account number + amount) key -> majority category_id, for
    //   contextless transfers the merchant key can't distinguish.
    // examples: one representative (description -> path) per category, for few-shot.
    let mut key_counts: HashMap<String, HashMap<i64, u32>> = HashMap::new();
    let mut acct_counts: HashMap<String, HashMap<i64, u32>> = HashMap::new();
    let mut examples: Vec<(String, String)> = Vec::new();
    let mut example_cats: HashSet<i64> = HashSet::new();

    for tx in &categorised {
        let Some(cid) = tx.category_id else { continue };
        let key = normalize_desc(&tx.description);
        if !key.is_empty() {
            *key_counts.entry(key).or_default().entry(cid).or_insert(0) += 1;
        }
        if let Some(akey) = account_amount_key(&tx.description, tx.debit, tx.credit) {
            *acct_counts.entry(akey).or_default().entry(cid).or_insert(0) += 1;
        }
        if examples.len() < 25 && !example_cats.contains(&cid) {
            if let Some(path) = path_by_id.get(&cid) {
                examples.push((short_desc(&tx.description), path.clone()));
                example_cats.insert(cid);
            }
        }
    }

    // Reduce each key's per-category tally to the single majority category.
    let majority = |counts: HashMap<String, HashMap<i64, u32>>| -> HashMap<String, i64> {
        counts
            .into_iter()
            .filter_map(|(key, counts)| {
                counts
                    .into_iter()
                    .max_by_key(|(_, n)| *n)
                    .map(|(cid, _)| (key, cid))
            })
            .collect()
    };
    let history_map = majority(key_counts);
    let acct_map = majority(acct_counts);

    let uncategorised = sqlx::query_as::<_, crate::models::Transaction>(
        "SELECT id, account_id, category_id, date, description, debit, credit, balance, \
         ai_category, ai_category_conf, ai_categorised_at, notes, created_at \
         FROM transactions WHERE category_id IS NULL \
         ORDER BY date DESC LIMIT 500",
    )
    .fetch_all(&*pool)
    .await
    .map_err(|e| format!("DB query error: {}", e))?;

    if uncategorised.is_empty() {
        return Ok(Vec::new());
    }

    let mut all_results: Vec<CategorisationSuggestion> = Vec::new();
    // Whatever history can't resolve falls through to the LLM.
    let mut llm_txs: Vec<&crate::models::Transaction> = Vec::new();

    for tx in &uncategorised {
        // Try the merchant key first; for contextless transfers it'll be empty,
        // so fall back to the account-number + amount key.
        let key = normalize_desc(&tx.description);
        let merchant_match = if key.is_empty() {
            None
        } else {
            history_map
                .get(&key)
                .and_then(|cid| path_by_id.get(cid).map(|p| (*cid, p.clone())))
        };

        let matched = match merchant_match {
            Some((cid, path)) => Some((
                cid,
                path,
                "Matched your previous categorisation of this merchant",
            )),
            None => account_amount_key(&tx.description, tx.debit, tx.credit)
                .and_then(|akey| acct_map.get(&akey).copied())
                .and_then(|cid| {
                    path_by_id.get(&cid).map(|p| {
                        (
                            cid,
                            p.clone(),
                            "Matched a previous transaction with the same account number and amount",
                        )
                    })
                }),
        };

        match matched {
            Some((cid, path, reasoning)) => all_results.push(CategorisationSuggestion {
                transaction_id: tx.id,
                date: tx.date.clone(),
                description: tx.description.clone(),
                debit: tx.debit,
                credit: tx.credit,
                suggested_category: path,
                category_id: Some(cid),
                confidence: 0.99,
                reasoning: reasoning.to_string(),
            }),
            None => llm_txs.push(tx),
        }
    }

    if !llm_txs.is_empty() {
        let api_key = crate::commands::settings::get_api_key()
            .await?
            .ok_or_else(|| {
                "OpenRouter API key not configured (needed for transactions with no history match). \
                 Go to Settings first."
                    .to_string()
            })?;
        let categorizer = AiCategorizer::new(api_key);

        for chunk in llm_txs.chunks(50) {
            let infos: Vec<TransactionInfo> = chunk
                .iter()
                .map(|t| TransactionInfo {
                    description: t.description.clone(),
                    debit: t.debit,
                    credit: t.credit,
                })
                .collect();

            let results = categorizer
                .categorise_batch(&infos, &category_paths, &examples)
                .await?;

            for (j, result) in results.iter().enumerate() {
                let tx = chunk[j];
                let category_id = find_category_id(&category_with_paths, &result.category_path);
                let confidence = result.confidence.clamp(0.0, 1.0);

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
        }
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
