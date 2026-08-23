use sea_orm_migration::prelude::*;

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum BodyweightReadings {
    Table,
    Id,
    UserId,
    RecordedOn,
    MassG,
}

#[derive(DeriveIden)]
enum Exercises {
    Table,
    BodyweightShare,
}

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(BodyweightReadings::Table)
                    .col(
                        ColumnDef::new(BodyweightReadings::Id)
                            .text()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(BodyweightReadings::UserId).text().not_null())
                    .col(
                        ColumnDef::new(BodyweightReadings::RecordedOn)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(BodyweightReadings::MassG)
                            .integer()
                            .not_null()
                            .check((
                                "ck_bodyweight_readings_mass_g",
                                Expr::col(BodyweightReadings::MassG).gt(0),
                            )),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_bodyweight_readings_user")
                            .from_tbl(BodyweightReadings::Table)
                            .from_col(BodyweightReadings::UserId)
                            .to_tbl(Users::Table)
                            .to_col(Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        // Also the lookup path for "the latest reading on or before a date".
        manager
            .create_index(
                Index::create()
                    .name("uq_bodyweight_readings_user_date")
                    .table(BodyweightReadings::Table)
                    .col(BodyweightReadings::UserId)
                    .col(BodyweightReadings::RecordedOn)
                    .unique()
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Exercises::Table)
                    .add_column(
                        ColumnDef::new(Exercises::BodyweightShare)
                            .integer()
                            .not_null()
                            .default(0)
                            .check((
                                "ck_exercises_bodyweight_share",
                                Expr::cust("bodyweight_share BETWEEN 0 AND 100"),
                            )),
                    )
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Exercises::Table)
                    .drop_column(Exercises::BodyweightShare)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(BodyweightReadings::Table).to_owned())
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
    async fn bodyweight_readings_and_share_are_added_and_removed() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        Migrator::up(&db, None).await.unwrap();
        let manager = SchemaManager::new(&db);

        assert!(manager.has_table("bodyweight_readings").await.unwrap());
        assert!(
            manager
                .has_column("exercises", "bodyweight_share")
                .await
                .unwrap()
        );

        db.execute_unprepared("INSERT INTO users(id,email_normalized,email_display,role,status,auth_version,created_at,updated_at) VALUES('u1','a@example.com','a@example.com','user','active',0,'1','1')")
            .await
            .unwrap();
        db.execute_unprepared(
            "INSERT INTO bodyweight_readings(id,user_id,recorded_on,mass_g) VALUES('b1','u1','2026-03-01',70000)",
        )
        .await
        .unwrap();
        assert!(
            db.execute_unprepared(
                "INSERT INTO bodyweight_readings(id,user_id,recorded_on,mass_g) VALUES('b2','u1','2026-03-01',71000)"
            )
            .await
            .is_err()
        );
        assert!(
            db.execute_unprepared(
                "INSERT INTO bodyweight_readings(id,user_id,recorded_on,mass_g) VALUES('b3','u1','2026-03-02',0)"
            )
            .await
            .is_err()
        );

        db.execute_unprepared(
            "INSERT INTO exercises(id,name,contraction_type) VALUES('e1','Pull-up','dynamic')",
        )
        .await
        .unwrap();
        let share: i64 = db
            .query_one_raw(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT bodyweight_share FROM exercises WHERE id='e1'",
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "bodyweight_share")
            .unwrap();
        assert_eq!(share, 0);
        assert!(
            db.execute_unprepared("UPDATE exercises SET bodyweight_share=101 WHERE id='e1'")
                .await
                .is_err()
        );

        Migration.down(&manager).await.unwrap();

        assert!(!manager.has_table("bodyweight_readings").await.unwrap());
        assert!(
            !manager
                .has_column("exercises", "bodyweight_share")
                .await
                .unwrap()
        );
    }
}
