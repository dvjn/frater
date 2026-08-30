use sea_orm_migration::prelude::*;

mod m20260815_000001_initial_schema;
mod m20260824_000001_expand_implied_scopes;
mod m20260824_000002_add_notes;
mod m20260824_000003_add_bodyweight;
mod m20260831_000001_add_run_splits;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20260815_000001_initial_schema::Migration),
            Box::new(m20260824_000001_expand_implied_scopes::Migration),
            Box::new(m20260824_000002_add_notes::Migration),
            Box::new(m20260824_000003_add_bodyweight::Migration),
            Box::new(m20260831_000001_add_run_splits::Migration),
        ]
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::Database;

    use super::*;

    #[tokio::test]
    async fn migration_round_trips() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let manager = SchemaManager::new(&db);
        let tables = [
            "users",
            "password_credentials",
            "auth_sessions",
            "auth_one_time_tokens",
            "oauth_clients",
            "oauth_client_redirect_uris",
            "oauth_authorization_codes",
            "oauth_device_authorizations",
            "oauth_refresh_token_families",
            "oauth_refresh_tokens",
            "oauth_access_tokens",
            "sessions",
            "session_exercises",
            "exercise_sets",
            "runs",
            "bodyweight_readings",
            "run_splits",
        ];

        Migrator::up(&db, None).await.unwrap();

        for table in tables {
            assert!(manager.has_table(table).await.unwrap(), "missing {table}");
        }

        Migrator::down(&db, None).await.unwrap();

        for table in tables {
            assert!(!manager.has_table(table).await.unwrap(), "retained {table}");
        }
    }
}
