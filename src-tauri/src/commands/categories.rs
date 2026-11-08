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
        "SELECT id, name, parent_id, monthly_budget, created_at, exclude_from_budget, rollover FROM categories ORDER BY \
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
            rollover: parent.rollover,
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
                    rollover: child.rollover,
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

    // The id comes back from the INSERT itself. A follow-up
    // `SELECT last_insert_rowid()` is not safe here: the pool holds four
    // connections and hands out whichever is free, so that second statement
    // can run on a connection that has inserted nothing and answer 0.
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO categories (name, parent_id, monthly_budget, created_at) \
         VALUES (?, ?, ?, ?) RETURNING id",
    )
    .bind(&name)
    .bind(parent_id)
    .bind(monthly_budget)
    .bind(&now)
    .fetch_one(&*pool)
    .await
    .map_err(|e| format!("DB insert error: {}", e))?;

    Ok(Category {
        id,
        name,
        parent_id,
        monthly_budget,
        created_at: now,
        exclude_from_budget: false,
        rollover: false,
    })
}

/// How a name change should treat the transactions already filed under the
/// category. Transactions reference a category by id, so renaming in place
/// carries all of history with it -- usually what you want, but not always
/// ("Childcare" becoming "School fees" shouldn't rewrite last year).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RenameMode {
    /// Rename in place: every past transaction now reads as the new name.
    UpdateHistory,
    /// Leave the old category and its transactions untouched, and put the new
    /// name on a brand-new category for future use.
    KeepHistory,
}

impl RenameMode {
    fn parse(raw: Option<&str>) -> Result<Self, String> {
        match raw.unwrap_or("update_history") {
            "update_history" => Ok(Self::UpdateHistory),
            "keep_history" => Ok(Self::KeepHistory),
            other => Err(format!(
                "Unknown rename mode {:?} (expected update_history or keep_history)",
                other
            )),
        }
    }
}

/// The display path of a category: "Parent > Child", or just "Name".
async fn category_path(pool: &SqlitePool, id: i64) -> Result<String, String> {
    let row = sqlx::query_as::<_, (String, Option<String>)>(
        "SELECT c.name, p.name FROM categories c
         LEFT JOIN categories p ON p.id = c.parent_id
         WHERE c.id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("DB query error: {}", e))?
    .ok_or_else(|| format!("Category with id {} not found", id))?;

    Ok(match row.1 {
        Some(parent) => format!("{} > {}", parent, row.0),
        None => row.0,
    })
}

/// How many transactions are filed under this category -- including, for a
/// parent, everything in its children. Shown when renaming, so the choice
/// between carrying history over and branching off is a concrete one.
#[tauri::command]
pub async fn count_category_transactions(
    pool: State<'_, SqlitePool>,
    id: i64,
) -> Result<i64, String> {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM transactions
         WHERE category_id = ?
            OR category_id IN (SELECT id FROM categories WHERE parent_id = ?)",
    )
    .bind(id)
    .bind(id)
    .fetch_one(&*pool)
    .await
    .map_err(|e| format!("DB query error: {}", e))
}

/// `transactions.ai_category` keeps the path the AI suggested as *text*, so a
/// rename leaves it quoting a category name that no longer exists. Rewrite it
/// for rows pointing at this category (and, for a parent, its children), but
/// only where it still matches the old path -- a row recategorised by hand
/// since keeps its own history.
async fn refresh_ai_category_text(
    pool: &SqlitePool,
    id: i64,
    old_path: &str,
    new_path: &str,
) -> Result<(), String> {
    if old_path == new_path {
        return Ok(());
    }
    sqlx::query(
        "UPDATE transactions SET ai_category = ?
         WHERE ai_category = ?
           AND (category_id = ? OR category_id IN (SELECT id FROM categories WHERE parent_id = ?))",
    )
    .bind(new_path)
    .bind(old_path)
    .bind(id)
    .bind(id)
    .execute(pool)
    .await
    .map_err(|e| format!("DB update error: {}", e))?;

    // A child's path changes when its *parent* is renamed, so the children of
    // a renamed parent need the same treatment.
    let children =
        sqlx::query_as::<_, (i64, String)>("SELECT id, name FROM categories WHERE parent_id = ?")
            .bind(id)
            .fetch_all(pool)
            .await
            .map_err(|e| format!("DB query error: {}", e))?;

    for (child_id, child_name) in children {
        sqlx::query(
            "UPDATE transactions SET ai_category = ? WHERE ai_category = ? AND category_id = ?",
        )
        .bind(format!("{} > {}", new_path, child_name))
        .bind(format!("{} > {}", old_path, child_name))
        .bind(child_id)
        .execute(pool)
        .await
        .map_err(|e| format!("DB update error: {}", e))?;
    }

    Ok(())
}

/// Edit a category. `rename_mode` only matters when `name` actually changes:
/// "update_history" (the default, and the behaviour before it existed) renames
/// in place, so every transaction already in the category reads as the new
/// name; "keep_history" leaves the category and its transactions exactly as
/// they are and creates a new category under the new name, returning that one.
#[tauri::command]
pub async fn update_category(
    pool: State<'_, SqlitePool>,
    id: i64,
    name: Option<String>,
    parent_id: Option<i64>,
    monthly_budget: Option<f64>,
    rename_mode: Option<String>,
) -> Result<Category, String> {
    apply_category_edit(&pool, id, name, parent_id, monthly_budget, rename_mode).await
}

/// The body of [`update_category`], over a plain pool so it can be tested.
pub async fn apply_category_edit(
    pool: &SqlitePool,
    id: i64,
    name: Option<String>,
    parent_id: Option<i64>,
    monthly_budget: Option<f64>,
    rename_mode: Option<String>,
) -> Result<Category, String> {
    let mode = RenameMode::parse(rename_mode.as_deref())?;

    let original = sqlx::query_as::<_, Category>(
        "SELECT id, name, parent_id, monthly_budget, created_at, exclude_from_budget, rollover FROM categories WHERE id = ?"
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("DB query error: {}", e))?
    .ok_or_else(|| format!("Category with id {} not found", id))?;

    let new_name = name.unwrap_or_else(|| original.name.clone());
    let new_parent_id = parent_id.or(original.parent_id);
    let new_budget = monthly_budget.or(original.monthly_budget);
    let renamed = new_name != original.name;

    // Branch off instead of renaming: the old category is left completely
    // alone -- name, budget, flags and every transaction in it -- and the new
    // name starts a category of its own for future transactions.
    if renamed && mode == RenameMode::KeepHistory {
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let clash = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM categories
             WHERE name = ? AND IFNULL(parent_id, -1) = IFNULL(?, -1)",
        )
        .bind(&new_name)
        .bind(new_parent_id)
        .fetch_one(pool)
        .await
        .map_err(|e| format!("DB query error: {}", e))?;
        if clash > 0 {
            return Err(format!(
                "A category called {} already exists here. Pick another name, or move the transactions into the existing one.",
                new_name
            ));
        }

        let new_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO categories (name, parent_id, monthly_budget, created_at) \
             VALUES (?, ?, ?, ?) RETURNING id",
        )
        .bind(&new_name)
        .bind(new_parent_id)
        .bind(new_budget)
        .bind(&now)
        .fetch_one(pool)
        .await
        .map_err(|e| format!("DB insert error: {}", e))?;

        return Ok(Category {
            id: new_id,
            name: new_name,
            parent_id: new_parent_id,
            monthly_budget: new_budget,
            created_at: now,
            exclude_from_budget: false,
            rollover: false,
        });
    }

    let old_path = if renamed {
        Some(category_path(pool, id).await?)
    } else {
        None
    };

    sqlx::query("UPDATE categories SET name = ?, parent_id = ?, monthly_budget = ? WHERE id = ?")
        .bind(&new_name)
        .bind(new_parent_id)
        .bind(new_budget)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| format!("DB update error: {}", e))?;

    // History follows the id automatically; the one thing that does not is the
    // AI's stored text path.
    if let Some(old_path) = old_path {
        let new_path = category_path(pool, id).await?;
        refresh_ai_category_text(pool, id, &old_path, &new_path).await?;
    }

    Ok(Category {
        id,
        name: new_name,
        parent_id: new_parent_id,
        monthly_budget: new_budget,
        created_at: original.created_at,
        exclude_from_budget: original.exclude_from_budget,
        rollover: original.rollover,
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
pub async fn set_category_rollover(
    pool: State<'_, SqlitePool>,
    id: i64,
    rollover: bool,
) -> Result<(), String> {
    sqlx::query("UPDATE categories SET rollover = ? WHERE id = ?")
        .bind(rollover)
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
pub async fn assign_categories_bulk(
    pool: State<'_, SqlitePool>,
    transaction_ids: Vec<i64>,
    category_id: Option<i64>,
) -> Result<i64, String> {
    for id in &transaction_ids {
        sqlx::query("UPDATE transactions SET category_id = ? WHERE id = ?")
            .bind(category_id)
            .bind(id)
            .execute(&*pool)
            .await
            .map_err(|e| format!("DB update error: {}", e))?;
    }
    Ok(transaction_ids.len() as i64)
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


#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    /// A DB on the real schema with one parent/child pair, a transaction in
    /// the child, and the AI's text path stored against it.
    async fn seeded_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        for sql in [
            "INSERT INTO accounts (id, name) VALUES (1, 'Everyday')",
            "INSERT INTO categories (id, name) VALUES (10, 'Kids')",
            "INSERT INTO categories (id, name, parent_id) VALUES (11, 'Childcare', 10)",
            "INSERT INTO transactions (id, account_id, category_id, date, description, debit, ai_category)
             VALUES (1, 1, 11, '2026-02-01', 'Daycare', 100, 'Kids > Childcare')",
        ] {
            sqlx::query(sql).execute(&pool).await.unwrap();
        }
        pool
    }

    async fn category_count(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM categories")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    async fn tx_category(pool: &SqlitePool) -> (i64, Option<String>) {
        sqlx::query_as::<_, (i64, Option<String>)>(
            "SELECT category_id, ai_category FROM transactions WHERE id = 1",
        )
        .fetch_one(pool)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn update_history_renames_in_place_and_refreshes_the_ai_path() {
        let pool = seeded_pool().await;
        let saved = apply_category_edit(
            &pool,
            11,
            Some("School fees".to_string()),
            None,
            None,
            Some("update_history".to_string()),
        )
        .await
        .unwrap();

        assert_eq!(saved.id, 11, "renaming in place keeps the same category");
        assert_eq!(category_count(&pool).await, 2, "no category was added");
        let (cat, ai) = tx_category(&pool).await;
        assert_eq!(cat, 11, "the transaction still points at the same category");
        assert_eq!(
            ai.as_deref(),
            Some("Kids > School fees"),
            "the stored AI path should follow the rename"
        );
    }

    #[tokio::test]
    async fn keep_history_adds_a_category_and_leaves_the_old_one_alone() {
        let pool = seeded_pool().await;
        let saved = apply_category_edit(
            &pool,
            11,
            Some("School fees".to_string()),
            Some(10),
            None,
            Some("keep_history".to_string()),
        )
        .await
        .unwrap();

        assert_ne!(saved.id, 11, "branching off must not reuse the old id");
        assert_eq!(saved.name, "School fees");
        assert_eq!(category_count(&pool).await, 3, "one category was added");

        let old_name = sqlx::query_scalar::<_, String>("SELECT name FROM categories WHERE id = 11")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(old_name, "Childcare", "the old category keeps its name");

        let (cat, ai) = tx_category(&pool).await;
        assert_eq!(cat, 11, "history stays where it was");
        assert_eq!(ai.as_deref(), Some("Kids > Childcare"), "and reads the same");
    }

    #[tokio::test]
    async fn branching_onto_an_existing_name_is_refused() {
        let pool = seeded_pool().await;
        sqlx::query("INSERT INTO categories (id, name, parent_id) VALUES (12, 'School fees', 10)")
            .execute(&pool)
            .await
            .unwrap();

        let err = apply_category_edit(
            &pool,
            11,
            Some("School fees".to_string()),
            Some(10),
            None,
            Some("keep_history".to_string()),
        )
        .await
        .expect_err("a duplicate name under the same parent should be refused");
        assert!(err.contains("already exists"), "unhelpful error: {err}");
        assert_eq!(category_count(&pool).await, 3, "nothing was created");
    }

    /// Renaming a *parent* changes its children's paths too, so their stored
    /// AI text has to move with it.
    #[tokio::test]
    async fn renaming_a_parent_refreshes_its_children_paths() {
        let pool = seeded_pool().await;
        apply_category_edit(&pool, 10, Some("Family".to_string()), None, None, None)
            .await
            .unwrap();

        let (_, ai) = tx_category(&pool).await;
        assert_eq!(ai.as_deref(), Some("Family > Childcare"));
    }

    /// An edit that doesn't touch the name must behave exactly as before,
    /// whatever mode is passed.
    #[tokio::test]
    async fn a_budget_only_edit_is_unaffected_by_the_mode() {
        let pool = seeded_pool().await;
        let saved = apply_category_edit(
            &pool,
            11,
            Some("Childcare".to_string()),
            None,
            Some(250.0),
            Some("keep_history".to_string()),
        )
        .await
        .unwrap();

        assert_eq!(saved.id, 11);
        assert_eq!(saved.monthly_budget, Some(250.0));
        assert_eq!(category_count(&pool).await, 2, "no category was added");
    }

    #[tokio::test]
    async fn an_unknown_mode_is_rejected() {
        let pool = seeded_pool().await;
        let err = apply_category_edit(&pool, 11, Some("X".to_string()), None, None, Some("nope".to_string()))
            .await
            .expect_err("an unknown mode should not silently fall back");
        assert!(err.contains("Unknown rename mode"), "unhelpful error: {err}");
    }
}
