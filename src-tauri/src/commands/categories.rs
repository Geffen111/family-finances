use crate::models::{Category, CategoryWithPath};
use csv::ReaderBuilder;
use sqlx::SqlitePool;
use tauri::State;

#[tauri::command]
pub async fn upload_categories_csv(
    pool: State<'_, SqlitePool>,
    csv_content: String,
) -> Result<i64, String> {
    let mut rdr = ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(csv_content.as_bytes());

    let mut imported: i64 = 0;

    for result in rdr.records() {
        let record = result.map_err(|e| format!("CSV parse error: {}", e))?;

        let parent_name = record.get(0).map(|s| s.trim()).unwrap_or("").to_string();
        let child_name = record.get(1).map(|s| s.trim()).unwrap_or("").to_string();

        if parent_name.is_empty() || child_name.is_empty() {
            continue;
        }

        let parent_id: i64 = match sqlx::query_scalar::<_, i64>(
            "SELECT id FROM categories WHERE name = ? AND parent_id IS NULL"
        )
        .bind(&parent_name)
        .fetch_optional(&*pool)
        .await
        .map_err(|e| format!("DB query error: {}", e))?
        {
            Some(id) => id,
            None => {
                sqlx::query(
                    "INSERT INTO categories (name, parent_id) VALUES (?, NULL)"
                )
                .bind(&parent_name)
                .execute(&*pool)
                .await
                .map_err(|e| format!("DB insert error: {}", e))?;

                sqlx::query_scalar::<_, i64>(
                    "SELECT id FROM categories WHERE name = ? AND parent_id IS NULL"
                )
                .bind(&parent_name)
                .fetch_one(&*pool)
                .await
                .map_err(|e| format!("DB query error: {}", e))?
            }
        };

        let exists: bool = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM categories WHERE name = ? AND parent_id = ?"
        )
        .bind(&child_name)
        .bind(parent_id)
        .fetch_one(&*pool)
        .await
        .map_err(|e| format!("DB query error: {}", e))?
        > 0;

        if !exists {
            sqlx::query(
                "INSERT INTO categories (name, parent_id) VALUES (?, ?)"
            )
            .bind(&child_name)
            .bind(parent_id)
            .execute(&*pool)
            .await
            .map_err(|e| format!("DB insert error: {}", e))?;
        }

        imported += 1;
    }

    Ok(imported)
}

#[tauri::command]
pub async fn get_categories(pool: State<'_, SqlitePool>) -> Result<Vec<CategoryWithPath>, String> {
    let rows = sqlx::query_as::<_, Category>(
        "SELECT id, name, parent_id, monthly_budget, created_at, exclude_from_budget FROM categories ORDER BY \
         CASE WHEN parent_id IS NULL THEN 0 ELSE 1 END, parent_id, name"
    )
    .fetch_all(&*pool)
    .await
    .map_err(|e| format!("DB query error: {}", e))?;

    let parents: Vec<_> = rows.iter().filter(|c| c.parent_id.is_none()).collect();

    let mut result = Vec::new();

    for parent in &parents {
        result.push(CategoryWithPath {
            id: parent.id,
            name: parent.name.clone(),
            parent_id: parent.parent_id,
            monthly_budget: parent.monthly_budget,
            created_at: parent.created_at.clone(),
            exclude_from_budget: parent.exclude_from_budget,
            path: parent.name.clone(),
        });

        for child in &rows {
            if child.parent_id == Some(parent.id) {
                result.push(CategoryWithPath {
                    id: child.id,
                    name: child.name.clone(),
                    parent_id: child.parent_id,
                    monthly_budget: child.monthly_budget,
                    created_at: child.created_at.clone(),
                    exclude_from_budget: child.exclude_from_budget,
                    path: format!("{} > {}", parent.name, child.name),
                });
            }
        }
    }

    Ok(result)
}

#[tauri::command]
pub async fn create_category(
    pool: State<'_, SqlitePool>,
    name: String,
    parent_id: Option<i64>,
    monthly_budget: Option<f64>,
) -> Result<Category, String> {
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

    sqlx::query(
        "INSERT INTO categories (name, parent_id, monthly_budget, created_at) VALUES (?, ?, ?, ?)"
    )
    .bind(&name)
    .bind(parent_id)
    .bind(monthly_budget)
    .bind(&now)
    .execute(&*pool)
    .await
    .map_err(|e| format!("DB insert error: {}", e))?;

    let id = sqlx::query_scalar::<_, i64>("SELECT last_insert_rowid()")
        .fetch_one(&*pool)
        .await
        .map_err(|e| format!("DB error: {}", e))?;

    Ok(Category {
        id,
        name,
        parent_id,
        monthly_budget,
        created_at: now,
        exclude_from_budget: false,
    })
}

#[tauri::command]
pub async fn update_category(
    pool: State<'_, SqlitePool>,
    id: i64,
    name: Option<String>,
    parent_id: Option<i64>,
    monthly_budget: Option<f64>,
) -> Result<Category, String> {
    let original = sqlx::query_as::<_, Category>(
        "SELECT id, name, parent_id, monthly_budget, created_at, exclude_from_budget FROM categories WHERE id = ?"
    )
    .bind(id)
    .fetch_optional(&*pool)
    .await
    .map_err(|e| format!("DB query error: {}", e))?
    .ok_or_else(|| format!("Category with id {} not found", id))?;

    let new_name = name.unwrap_or(original.name);
    let new_parent_id = parent_id.or(original.parent_id);
    let new_budget = monthly_budget.or(original.monthly_budget);

    sqlx::query(
        "UPDATE categories SET name = ?, parent_id = ?, monthly_budget = ? WHERE id = ?"
    )
    .bind(&new_name)
    .bind(new_parent_id)
    .bind(new_budget)
    .bind(id)
    .execute(&*pool)
    .await
    .map_err(|e| format!("DB update error: {}", e))?;

    Ok(Category {
        id,
        name: new_name,
        parent_id: new_parent_id,
        monthly_budget: new_budget,
        created_at: original.created_at,
        exclude_from_budget: original.exclude_from_budget,
    })
}

#[tauri::command]
pub async fn delete_category(
    pool: State<'_, SqlitePool>,
    id: i64,
) -> Result<(), String> {
    sqlx::query("UPDATE transactions SET category_id = NULL WHERE category_id = ?")
        .bind(id)
        .execute(&*pool)
        .await
        .map_err(|e| format!("DB update error: {}", e))?;

    sqlx::query("DELETE FROM categories WHERE id = ?")
        .bind(id)
        .execute(&*pool)
        .await
        .map_err(|e| format!("DB delete error: {}", e))?;

    Ok(())
}

#[tauri::command]
pub async fn set_category_exclusion(
    pool: State<'_, SqlitePool>,
    id: i64,
    exclude: bool,
) -> Result<(), String> {
    sqlx::query("UPDATE categories SET exclude_from_budget = ? WHERE id = ?")
        .bind(exclude)
        .bind(id)
        .execute(&*pool)
        .await
        .map_err(|e| format!("DB update error: {}", e))?;
    Ok(())
}

#[tauri::command]
pub async fn assign_category(
    pool: State<'_, SqlitePool>,
    transaction_id: i64,
    category_id: Option<i64>,
) -> Result<(), String> {
    sqlx::query("UPDATE transactions SET category_id = ? WHERE id = ?")
        .bind(category_id)
        .bind(transaction_id)
        .execute(&*pool)
        .await
        .map_err(|e| format!("DB update error: {}", e))?;

    Ok(())
}

#[tauri::command]
pub async fn get_uncategorised_transactions(
    pool: State<'_, SqlitePool>,
    account_id: Option<i64>,
) -> Result<Vec<crate::models::Transaction>, String> {
    let mut query = String::from(
        "SELECT id, account_id, category_id, date, description, debit, credit, balance, \
         ai_category, ai_category_conf, ai_categorised_at, notes, created_at \
         FROM transactions WHERE category_id IS NULL"
    );

    if account_id.is_some() {
        query.push_str(" AND account_id = ?");
    }
    query.push_str(" ORDER BY date DESC");

    let mut q = sqlx::query_as::<_, crate::models::Transaction>(&query);

    if let Some(aid) = account_id {
        q = q.bind(aid);
    }

    q.fetch_all(&*pool)
        .await
        .map_err(|e| format!("DB query error: {}", e))
}