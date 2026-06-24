use crate::models::Asset;
use sqlx::SqlitePool;
use tauri::State;

#[tauri::command]
pub async fn list_assets(pool: State<'_, SqlitePool>) -> Result<Vec<Asset>, String> {
    sqlx::query_as::<_, Asset>(
        "SELECT id, name, asset_type, value, notes, created_at \
         FROM assets ORDER BY value DESC, created_at",
    )
    .fetch_all(&*pool)
    .await
    .map_err(|e| format!("DB query error: {}", e))
}

#[tauri::command]
pub async fn create_asset(
    pool: State<'_, SqlitePool>,
    name: String,
    asset_type: Option<String>,
    value: f64,
    notes: Option<String>,
) -> Result<Asset, String> {
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let kind = asset_type.unwrap_or_else(|| "other".to_string());

    sqlx::query(
        "INSERT INTO assets (name, asset_type, value, notes, created_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&name)
    .bind(&kind)
    .bind(value)
    .bind(&notes)
    .bind(&now)
    .execute(&*pool)
    .await
    .map_err(|e| format!("DB insert error: {}", e))?;

    let id = sqlx::query_scalar::<_, i64>("SELECT last_insert_rowid()")
        .fetch_one(&*pool)
        .await
        .map_err(|e| format!("DB error: {}", e))?;

    Ok(Asset {
        id,
        name,
        asset_type: kind,
        value,
        notes,
        created_at: now,
    })
}

#[tauri::command]
pub async fn update_asset(
    pool: State<'_, SqlitePool>,
    id: i64,
    name: Option<String>,
    asset_type: Option<String>,
    value: Option<f64>,
    notes: Option<String>,
) -> Result<(), String> {
    let original = sqlx::query_as::<_, Asset>(
        "SELECT id, name, asset_type, value, notes, created_at FROM assets WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&*pool)
    .await
    .map_err(|e| format!("DB query error: {}", e))?
    .ok_or_else(|| format!("Asset {} not found", id))?;

    let new_name = name.unwrap_or(original.name);
    let new_type = asset_type.unwrap_or(original.asset_type);
    let new_value = value.unwrap_or(original.value);
    let new_notes = notes.or(original.notes);

    sqlx::query("UPDATE assets SET name = ?, asset_type = ?, value = ?, notes = ? WHERE id = ?")
        .bind(&new_name)
        .bind(&new_type)
        .bind(new_value)
        .bind(&new_notes)
        .bind(id)
        .execute(&*pool)
        .await
        .map_err(|e| format!("DB update error: {}", e))?;

    Ok(())
}

#[tauri::command]
pub async fn delete_asset(pool: State<'_, SqlitePool>, id: i64) -> Result<(), String> {
    sqlx::query("DELETE FROM assets WHERE id = ?")
        .bind(id)
        .execute(&*pool)
        .await
        .map_err(|e| format!("DB delete error: {}", e))?;
    Ok(())
}
