use std::time::Duration;

use anyhow::{Context, Result};
use sea_orm::{ConnectOptions, Database, DatabaseConnection, sqlx::sqlite::SqliteJournalMode};
use sea_orm_migration::MigratorTrait;

use crate::migration::Migrator;

pub async fn connect(database_url: &str) -> Result<DatabaseConnection> {
    let mut options = ConnectOptions::new(database_url);
    options.max_connections(4);
    // Foreign keys are connection-local in SQLite. The timeout allows short write
    // bursts to serialize instead of immediately returning SQLITE_BUSY.
    options.map_sqlx_sqlite_opts(|options| {
        options
            .journal_mode(SqliteJournalMode::Wal)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5))
    });

    let db = Database::connect(options)
        .await
        .context("failed to connect to the database")?;
    Migrator::up(&db, None)
        .await
        .context("failed to apply database migrations")?;

    Ok(db)
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf, process};

    use sea_orm::{ConnectionTrait, DbBackend, Statement};

    use super::*;

    struct TempDatabase {
        directory: PathBuf,
        path: PathBuf,
    }

    impl TempDatabase {
        fn new() -> Self {
            let directory = std::env::temp_dir().join(format!("frater-{}-wal-test", process::id()));
            let _ = fs::remove_dir_all(&directory);
            fs::create_dir(&directory).unwrap();
            let path = directory.join("frater.db");

            Self { directory, path }
        }
    }

    impl Drop for TempDatabase {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }

    #[tokio::test]
    async fn file_databases_use_required_pragmas() {
        let temp_database = TempDatabase::new();
        let database_url = format!("sqlite://{}?mode=rwc", temp_database.path.display());

        let db = connect(&database_url).await.unwrap();
        let row = db
            .query_one_raw(Statement::from_string(
                DbBackend::Sqlite,
                "PRAGMA journal_mode",
            ))
            .await
            .unwrap()
            .unwrap();
        let journal_mode: String = row.try_get("", "journal_mode").unwrap();

        assert_eq!(journal_mode, "wal");

        let foreign_keys: i64 = db
            .query_one_raw(Statement::from_string(
                DbBackend::Sqlite,
                "PRAGMA foreign_keys",
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "foreign_keys")
            .unwrap();
        let busy_timeout: i64 = db
            .query_one_raw(Statement::from_string(
                DbBackend::Sqlite,
                "PRAGMA busy_timeout",
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "timeout")
            .unwrap();
        assert_eq!(foreign_keys, 1);
        assert_eq!(busy_timeout, 5_000);

        db.close().await.unwrap();
    }
}
