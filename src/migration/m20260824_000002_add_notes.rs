use sea_orm_migration::prelude::*;

#[derive(DeriveIden)]
enum Sessions {
    Table,
    Notes,
}

#[derive(DeriveIden)]
enum SessionExercises {
    Table,
    Notes,
}

#[derive(DeriveIden)]
enum ExerciseSets {
    Table,
    Notes,
}

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Sessions::Table)
                    .add_column(ColumnDef::new(Sessions::Notes).text())
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(SessionExercises::Table)
                    .add_column(ColumnDef::new(SessionExercises::Notes).text())
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(ExerciseSets::Table)
                    .add_column(ColumnDef::new(ExerciseSets::Notes).text())
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(ExerciseSets::Table)
                    .drop_column(ExerciseSets::Notes)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(SessionExercises::Table)
                    .drop_column(SessionExercises::Notes)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Sessions::Table)
                    .drop_column(Sessions::Notes)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};

    use super::*;
    use crate::migration::Migrator;

    #[tokio::test]
    async fn notes_columns_are_added_and_removed() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        Migrator::up(&db, None).await.unwrap();
        let manager = SchemaManager::new(&db);

        for table in ["sessions", "session_exercises", "exercise_sets"] {
            assert!(manager.has_column(table, "notes").await.unwrap());
        }

        db.execute_unprepared("INSERT INTO users(id,email_normalized,email_display,role,status,auth_version,created_at,updated_at) VALUES('u1','a@example.com','a@example.com','user','active',0,'1','1')")
            .await
            .unwrap();
        db.execute_unprepared("INSERT INTO sessions(id,user_id,started_at,activity_type,notes) VALUES('s1','u1','2026-01-01T00:00:00Z','strength','felt strong')")
            .await
            .unwrap();

        let stored: String = db
            .query_one_raw(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT notes FROM sessions WHERE id='s1'",
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "notes")
            .unwrap();
        assert_eq!(stored, "felt strong");

        Migration.down(&manager).await.unwrap();

        for table in ["sessions", "session_exercises", "exercise_sets"] {
            assert!(!manager.has_column(table, "notes").await.unwrap());
        }
        // Dropping the column must not take the row with it.
        let surviving: String = db
            .query_one_raw(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT activity_type FROM sessions WHERE id='s1'",
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "activity_type")
            .unwrap();
        assert_eq!(surviving, "strength");
    }
}
