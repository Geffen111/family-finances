// Free-form tags on transactions (e.g. "holiday-2026", "tax-deductible").
// Cross-cut categories: a transaction has one category but any number of tags.

use serde::Serialize;
use sqlx::SqlitePool;
use std::collections::HashMap;
use tauri::State;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Tag {
    pub id: i64,
    pub name: String,
}

#[tauri::command]
pub async fn list_tags(pool: State<'_, SqlitePool>) -> Result<Vec<Tag>, String> {
    sqlx::query_as::<_, Tag>("SELECT id, name FROM tags ORDER BY name COLLATE NOCASE")
        .fetch_all(&*pool)
        .await
        .map_err(|e| format!("DB query error: {}", e))
}

/// Tag a transaction, creating the tag if it doesn't exist yet (case-insensitive).
#[tauri::command]
pub async fn add_tag_to_transaction(
    pool: State<'_, SqlitePool>,
    transaction_id: i64,
    tag_name: String,
) -> Result<Tag, String> {
    let name = tag_name.trim().to_string();
    if name.is_empty() {
        return Err("Tag name cannot be empty.".to_string());
    }

    sqlx::query("INSERT OR IGNORE INTO tags (name) VALUES (?)")
        .bind(&name)
        .execute(&*pool)
        .await
        .map_err(|e| format!("DB insert error: {}", e))?;

    let tag = sqlx::query_as::<_, Tag>("SELECT id, name FROM tags WHERE name = ? COLLATE NOCASE")
        .bind(&name)
        .fetch_one(&*pool)
        .await
        .map_err(|e| format!("DB query error: {}", e))?;

    sqlx::query("INSERT OR IGNORE INTO transaction_tags (transaction_id, tag_id) VALUES (?, ?)")
        .bind(transaction_id)
        .bind(tag.id)
        .execute(&*pool)
        .await
        .map_err(|e| format!("DB insert error: {}", e))?;

    Ok(tag)
}

#[tauri::command]
pub async fn remove_tag_from_transaction(
    pool: State<'_, SqlitePool>,
    transaction_id: i64,
    tag_id: i64,
) -> Result<(), String> {
    sqlx::query("DELETE FROM transaction_tags WHERE transaction_id = ? AND tag_id = ?")
        .bind(transaction_id)
        .bind(tag_id)
        .execute(&*pool)
        .await
        .map_err(|e| format!("DB delete error: {}", e))?;
    // Drop the tag entirely if nothing references it any more.
    sqlx::query(
        "DELETE FROM tags WHERE id = ? AND NOT EXISTS \
         (SELECT 1 FROM transaction_tags WHERE tag_id = ?)",
    )
    .bind(tag_id)
    .bind(tag_id)
    .execute(&*pool)
    .await
    .map_err(|e| format!("DB delete error: {}", e))?;
    Ok(())
}

/// Tags for a batch of transactions, keyed by transaction id. The frontend
/// renders only a window of rows, so it asks for just those ids.
#[tauri::command]
pub async fn get_tags_for_transactions(
    pool: State<'_, SqlitePool>,
    transaction_ids: Vec<i64>,
) -> Result<HashMap<i64, Vec<Tag>>, String> {
    let mut map: HashMap<i64, Vec<Tag>> = HashMap::new();
    if transaction_ids.is_empty() {
        return Ok(map);
    }

    let placeholders = vec!["?"; transaction_ids.len()].join(",");
    let sql = format!(
        "SELECT tt.transaction_id, t.id, t.name \
         FROM transaction_tags tt JOIN tags t ON tt.tag_id = t.id \
         WHERE tt.transaction_id IN ({placeholders}) \
         ORDER BY t.name COLLATE NOCASE"
    );
    let mut q = sqlx::query_as::<_, (i64, i64, String)>(&sql);
    for id in &transaction_ids {
        q = q.bind(id);
    }
    let rows = q
        .fetch_all(&*pool)
        .await
        .map_err(|e| format!("DB query error: {}", e))?;

    for (tx_id, id, name) in rows {
        map.entry(tx_id).or_default().push(Tag { id, name });
    }
    Ok(map)
}
