use crate::models::SavingsGoal;
use sqlx::SqlitePool;
use tauri::State;

#[tauri::command]
pub async fn list_savings_goals(pool: State<'_, SqlitePool>) -> Result<Vec<SavingsGoal>, String> {
    sqlx::query_as::<_, SavingsGoal>(
        "SELECT id, name, target_amount, current_amount, target_date, created_at \
         FROM savings_goals ORDER BY created_at",
    )
    .fetch_all(&*pool)
    .await
    .map_err(|e| format!("DB query error: {}", e))
}

#[tauri::command]
pub async fn create_savings_goal(
    pool: State<'_, SqlitePool>,
    name: String,
    target_amount: f64,
    current_amount: Option<f64>,
    target_date: Option<String>,
) -> Result<SavingsGoal, String> {
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let current = current_amount.unwrap_or(0.0);

    sqlx::query(
        "INSERT INTO savings_goals (name, target_amount, current_amount, target_date, created_at) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&name)
    .bind(target_amount)
    .bind(current)
    .bind(&target_date)
    .bind(&now)
    .execute(&*pool)
    .await
    .map_err(|e| format!("DB insert error: {}", e))?;

    let id = sqlx::query_scalar::<_, i64>("SELECT last_insert_rowid()")
        .fetch_one(&*pool)
        .await
        .map_err(|e| format!("DB error: {}", e))?;

    Ok(SavingsGoal {
        id,
        name,
        target_amount,
        current_amount: current,
        target_date,
        created_at: now,
    })
}

#[tauri::command]
pub async fn update_savings_goal(
    pool: State<'_, SqlitePool>,
    id: i64,
    name: Option<String>,
    target_amount: Option<f64>,
    current_amount: Option<f64>,
    target_date: Option<String>,
) -> Result<(), String> {
    let original = sqlx::query_as::<_, SavingsGoal>(
        "SELECT id, name, target_amount, current_amount, target_date, created_at \
         FROM savings_goals WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&*pool)
    .await
    .map_err(|e| format!("DB query error: {}", e))?
    .ok_or_else(|| format!("Goal {} not found", id))?;

    let new_name = name.unwrap_or(original.name);
    let new_target = target_amount.unwrap_or(original.target_amount);
    let new_current = current_amount.unwrap_or(original.current_amount);
    // target_date: passing None leaves it unchanged; clearing is uncommon here.
    let new_date = target_date.or(original.target_date);

    sqlx::query(
        "UPDATE savings_goals SET name = ?, target_amount = ?, current_amount = ?, target_date = ? WHERE id = ?",
    )
    .bind(&new_name)
    .bind(new_target)
    .bind(new_current)
    .bind(&new_date)
    .bind(id)
    .execute(&*pool)
    .await
    .map_err(|e| format!("DB update error: {}", e))?;

    Ok(())
}

#[tauri::command]
pub async fn delete_savings_goal(pool: State<'_, SqlitePool>, id: i64) -> Result<(), String> {
    sqlx::query("DELETE FROM savings_goals WHERE id = ?")
        .bind(id)
        .execute(&*pool)
        .await
        .map_err(|e| format!("DB delete error: {}", e))?;
    Ok(())
}
