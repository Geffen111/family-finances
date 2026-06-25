// Deterministic categorisation rules. A rule maps a description pattern to a
// category; rules run before the AI categoriser (cheaper, offline, predictable)
// and can be re-applied to existing transactions on demand.

use serde::Serialize;
use sqlx::SqlitePool;
use tauri::State;

#[derive(Debug, Serialize)]
pub struct CategoryRule {
    pub id: i64,
    pub match_type: String,
    pub pattern: String,
    pub category_id: i64,
    pub category_name: Option<String>,
    pub priority: i64,
    pub active: bool,
}

type RuleRow = (i64, String, String, i64, Option<String>, i64, bool);

const SELECT_RULES: &str = "SELECT r.id, r.match_type, r.pattern, r.category_id, \
     CASE WHEN cp.name IS NOT NULL THEN cp.name || ' > ' || c.name ELSE c.name END, \
     r.priority, r.active \
     FROM category_rules r \
     JOIN categories c ON r.category_id = c.id \
     LEFT JOIN categories cp ON c.parent_id = cp.id";

fn row_to_rule(r: RuleRow) -> CategoryRule {
    let (id, match_type, pattern, category_id, category_name, priority, active) = r;
    CategoryRule { id, match_type, pattern, category_id, category_name, priority, active }
}

/// Does `description` satisfy a rule? Case-insensitive.
fn rule_matches(description: &str, match_type: &str, pattern: &str) -> bool {
    let d = description.to_lowercase();
    let p = pattern.trim().to_lowercase();
    if p.is_empty() {
        return false;
    }
    match match_type {
        "equals" => d == p,
        "starts_with" => d.starts_with(&p),
        // "contains" and anything unrecognised.
        _ => d.contains(&p),
    }
}

#[tauri::command]
pub async fn list_category_rules(pool: State<'_, SqlitePool>) -> Result<Vec<CategoryRule>, String> {
    let rows = sqlx::query_as::<_, RuleRow>(&format!("{SELECT_RULES} ORDER BY r.priority DESC, r.id ASC"))
        .fetch_all(&*pool)
        .await
        .map_err(|e| format!("DB query error: {}", e))?;
    Ok(rows.into_iter().map(row_to_rule).collect())
}

#[tauri::command]
pub async fn create_category_rule(
    pool: State<'_, SqlitePool>,
    match_type: String,
    pattern: String,
    category_id: i64,
    priority: Option<i64>,
    active: Option<bool>,
) -> Result<CategoryRule, String> {
    if pattern.trim().is_empty() {
        return Err("Pattern cannot be empty.".to_string());
    }
    sqlx::query(
        "INSERT INTO category_rules (match_type, pattern, category_id, priority, active) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&match_type)
    .bind(pattern.trim())
    .bind(category_id)
    .bind(priority.unwrap_or(0))
    .bind(active.unwrap_or(true))
    .execute(&*pool)
    .await
    .map_err(|e| format!("DB insert error: {}", e))?;

    let id = sqlx::query_scalar::<_, i64>("SELECT last_insert_rowid()")
        .fetch_one(&*pool)
        .await
        .map_err(|e| format!("DB error: {}", e))?;

    let row = sqlx::query_as::<_, RuleRow>(&format!("{SELECT_RULES} WHERE r.id = ?"))
        .bind(id)
        .fetch_one(&*pool)
        .await
        .map_err(|e| format!("DB query error: {}", e))?;
    Ok(row_to_rule(row))
}

#[tauri::command]
pub async fn update_category_rule(
    pool: State<'_, SqlitePool>,
    id: i64,
    match_type: String,
    pattern: String,
    category_id: i64,
    priority: Option<i64>,
    active: Option<bool>,
) -> Result<CategoryRule, String> {
    if pattern.trim().is_empty() {
        return Err("Pattern cannot be empty.".to_string());
    }
    let affected = sqlx::query(
        "UPDATE category_rules SET match_type = ?, pattern = ?, category_id = ?, priority = ?, active = ? \
         WHERE id = ?",
    )
    .bind(&match_type)
    .bind(pattern.trim())
    .bind(category_id)
    .bind(priority.unwrap_or(0))
    .bind(active.unwrap_or(true))
    .bind(id)
    .execute(&*pool)
    .await
    .map_err(|e| format!("DB update error: {}", e))?
    .rows_affected();
    if affected == 0 {
        return Err(format!("Rule {} not found", id));
    }
    let row = sqlx::query_as::<_, RuleRow>(&format!("{SELECT_RULES} WHERE r.id = ?"))
        .bind(id)
        .fetch_one(&*pool)
        .await
        .map_err(|e| format!("DB query error: {}", e))?;
    Ok(row_to_rule(row))
}

#[tauri::command]
pub async fn delete_category_rule(pool: State<'_, SqlitePool>, id: i64) -> Result<(), String> {
    sqlx::query("DELETE FROM category_rules WHERE id = ?")
        .bind(id)
        .execute(&*pool)
        .await
        .map_err(|e| format!("DB delete error: {}", e))?;
    Ok(())
}

/// Apply active rules to transactions. With `only_uncategorised`, leaves
/// already-categorised transactions alone (the safe default — used after
/// import). Returns the number of transactions updated. When `account_id` is
/// set, only that account's rows are touched. Highest-priority matching rule
/// wins; a rule that wins is not overwritten by a lower-priority one in the same
/// run.
pub async fn apply_rules_internal(
    pool: &SqlitePool,
    only_uncategorised: bool,
    account_id: Option<i64>,
) -> Result<i64, String> {
    let rules = sqlx::query_as::<_, (i64, String, String, i64)>(
        "SELECT id, match_type, pattern, category_id FROM category_rules \
         WHERE active = 1 ORDER BY priority DESC, id ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("DB query error: {}", e))?;
    if rules.is_empty() {
        return Ok(0);
    }

    let mut q = String::from("SELECT id, description FROM transactions WHERE 1=1");
    if only_uncategorised {
        q.push_str(" AND category_id IS NULL");
    }
    if account_id.is_some() {
        q.push_str(" AND account_id = ?");
    }
    let mut query = sqlx::query_as::<_, (i64, String)>(&q);
    if let Some(aid) = account_id {
        query = query.bind(aid);
    }
    let txs = query
        .fetch_all(pool)
        .await
        .map_err(|e| format!("DB query error: {}", e))?;

    let mut updated = 0i64;
    for (tx_id, desc) in txs {
        // Rules are pre-sorted by priority DESC, so the first match wins.
        if let Some((_, _, _, category_id)) = rules
            .iter()
            .find(|(_, mt, pat, _)| rule_matches(&desc, mt, pat))
        {
            sqlx::query("UPDATE transactions SET category_id = ? WHERE id = ?")
                .bind(category_id)
                .bind(tx_id)
                .execute(pool)
                .await
                .map_err(|e| format!("DB update error: {}", e))?;
            updated += 1;
        }
    }
    Ok(updated)
}

#[tauri::command]
pub async fn apply_category_rules(
    pool: State<'_, SqlitePool>,
    only_uncategorised: Option<bool>,
) -> Result<i64, String> {
    apply_rules_internal(&pool, only_uncategorised.unwrap_or(true), None).await
}
