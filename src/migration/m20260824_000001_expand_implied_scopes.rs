use sea_orm_migration::prelude::*;

/// Scopes were a hierarchy before they became a flat set: a write implied its
/// read, so consent normalized the read away and stored the write alone. Under
/// flat scopes such a grant reads nothing, and a client registration without
/// `workouts:read` can no longer authorize at all. This restores the read that
/// the grant already carried.
const SCOPE_TABLES: [&str; 5] = [
    "oauth_clients",
    "oauth_authorization_codes",
    "oauth_device_authorizations",
    "oauth_refresh_token_families",
    "oauth_access_tokens",
];

const IMPLIED_READS: [(&str, &str); 2] = [
    ("workouts:write", "workouts:read"),
    ("catalogue:write", "catalogue:read"),
];

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in SCOPE_TABLES {
            for (write, read) in IMPLIED_READS {
                // A client registration is stored without normalize_scope, so a
                // row may already hold both names. Adding a second copy of the
                // read would make validate_scope reject the whole scope.
                manager
                    .get_connection()
                    .execute_unprepared(&format!(
                        "UPDATE {table} SET scope = replace(scope, '{write}', '{write} {read}') \
                         WHERE scope LIKE '%{write}%' AND scope NOT LIKE '%{read}%'"
                    ))
                    .await?;
            }
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in SCOPE_TABLES {
            for (write, read) in IMPLIED_READS {
                manager
                    .get_connection()
                    .execute_unprepared(&format!(
                        "UPDATE {table} SET scope = replace(scope, '{write} {read}', '{write}') \
                         WHERE scope LIKE '%{write} {read}%'"
                    ))
                    .await?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DatabaseConnection, Statement};

    use super::*;
    use crate::migration::Migrator;

    async fn client_scope(db: &DatabaseConnection, id: &str) -> String {
        db.query_one_raw(Statement::from_string(
            db.get_database_backend(),
            format!("SELECT scope FROM oauth_clients WHERE id='{id}'"),
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get_by_index::<String>(0)
        .unwrap()
    }

    async fn insert_client(db: &DatabaseConnection, id: &str, scope: &str) {
        db.execute_unprepared(&format!(
            "INSERT INTO oauth_clients(id,issuer,application_type,grant_types,response_types,scope,token_endpoint_auth_method,created_at) \
             VALUES('{id}','https://frater.example','native','authorization_code','code','{scope}','none','2026-01-01T00:00:00Z')"
        ))
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn restores_the_reads_a_write_used_to_imply() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        Migrator::up(&db, None).await.unwrap();
        let manager = SchemaManager::new(&db);

        insert_client(
            &db,
            "collapsed",
            "workouts:write catalogue:write offline_access",
        )
        .await;
        insert_client(&db, "read-only", "workouts:read").await;
        insert_client(&db, "already-flat", "workouts:read workouts:write").await;

        Migration.up(&manager).await.unwrap();

        assert_eq!(
            client_scope(&db, "collapsed").await,
            "workouts:write workouts:read catalogue:write catalogue:read offline_access"
        );
        assert_eq!(client_scope(&db, "read-only").await, "workouts:read");
        assert_eq!(
            client_scope(&db, "already-flat").await,
            "workouts:read workouts:write"
        );

        Migration.up(&manager).await.unwrap();

        assert_eq!(
            client_scope(&db, "collapsed").await,
            "workouts:write workouts:read catalogue:write catalogue:read offline_access"
        );

        Migration.down(&manager).await.unwrap();

        assert_eq!(
            client_scope(&db, "collapsed").await,
            "workouts:write catalogue:write offline_access"
        );
        assert_eq!(client_scope(&db, "read-only").await, "workouts:read");
    }
}
