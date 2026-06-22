use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::path::PathBuf;

pub async fn init_db() -> SqlitePool {
    let db_path = get_db_path();
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).expect("Failed to create db directory");
    }

    let connect_opts = SqliteConnectOptions::new()
        .filename(&db_path)
        .create_if_missing(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(connect_opts)
        .await
        .expect("Failed to connect to database");

    run_migrations(&pool).await;
    seed_accounts(&pool).await;

    pool
}

fn get_db_path() -> PathBuf {
    // On Windows, store in OneDrive so the DB syncs across devices.
    // On Linux/macOS, fall back to the standard data directory.
    if let Ok(onedrive) = std::env::var("OneDrive") {
        let base = PathBuf::from(onedrive);
        base.join("Apps").join("FamilyFinance").join("finances.db")
    } else {
        let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
        base.join("family-finances").join("finances.db")
    }
}

async fn run_migrations(pool: &SqlitePool) {
    // Migrations are embedded into the binary at compile time. Reading them
    // from disk at runtime would look for the build machine's source path,
    // which doesn't exist on an installed copy, and would also only run the
    // first statement of multi-statement files. The embedded migrator runs
    // each full file in order and is idempotent.
    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .expect("Migration failed");
}

async fn seed_accounts(pool: &SqlitePool) {
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM accounts")
        .fetch_one(pool)
        .await
        .unwrap_or((0,));

    if count.0 == 0 {
        let accounts = [
            ("Savings", "asset"),
            ("Everyday Spending", "asset"),
            ("Home Loan", "liability"),
            ("Credit Card", "liability"),
        ];

        for (name, typ) in accounts {
            sqlx::query("INSERT INTO accounts (name, type) VALUES (?, ?)")
                .bind(name)
                .bind(typ)
                .execute(pool)
                .await
                .expect("Failed to seed account");
        }
    }
}