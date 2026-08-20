use sea_orm_migration::prelude::*;

mod m20260815_000001_initial_schema;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(m20260815_000001_initial_schema::Migration)]
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
