use sea_orm::{ConnectionTrait, Statement};
use sea_orm_migration::prelude::*;
use uuid::Uuid;

const CREATE_RUNS_WITH_TOTALS: &str = "CREATE TABLE runs_rebuilt (\
    session_id text NOT NULL PRIMARY KEY, \
    user_id text NOT NULL, \
    activity_type varchar(8) NOT NULL DEFAULT 'run' CONSTRAINT ck_runs_activity_type CHECK (activity_type = 'run'), \
    distance_m integer NOT NULL CONSTRAINT ck_runs_distance_m CHECK (distance_m > 0), \
    duration_sec integer NOT NULL CONSTRAINT ck_runs_duration_sec CHECK (duration_sec > 0), \
    elevation_gain_m integer NOT NULL DEFAULT 0 CONSTRAINT ck_runs_elevation_gain_m CHECK (elevation_gain_m >= 0), \
    CONSTRAINT fk_runs_session FOREIGN KEY (session_id, user_id, activity_type) REFERENCES sessions (id, user_id, activity_type) ON DELETE CASCADE)";

const CREATE_RUNS_WITHOUT_TOTALS: &str = "CREATE TABLE runs_rebuilt (\
    session_id text NOT NULL PRIMARY KEY, \
    user_id text NOT NULL, \
    activity_type varchar(8) NOT NULL DEFAULT 'run' CONSTRAINT ck_runs_activity_type CHECK (activity_type = 'run'), \
    elevation_gain_m integer NOT NULL DEFAULT 0 CONSTRAINT ck_runs_elevation_gain_m CHECK (elevation_gain_m >= 0), \
    CONSTRAINT fk_runs_session FOREIGN KEY (session_id, user_id, activity_type) REFERENCES sessions (id, user_id, activity_type) ON DELETE CASCADE)";

async fn rebuild_runs<C: ConnectionTrait>(
    connection: &C,
    create: &str,
    copy: &str,
) -> Result<(), DbErr> {
    connection
        .execute_unprepared(&format!(
            "{create}; {copy}; DROP TABLE runs; ALTER TABLE runs_rebuilt RENAME TO runs;"
        ))
        .await?;
    Ok(())
}

#[derive(DeriveIden)]
enum RunSplits {
    Table,
    ActivityType,
    DistanceM,
    DurationSec,
    Id,
    Position,
    SessionId,
    UserId,
}

#[derive(DeriveIden)]
enum Sessions {
    Table,
    ActivityType,
    Id,
    UserId,
}

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(RunSplits::Table)
                    .col(
                        ColumnDef::new(RunSplits::Id)
                            .text()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(RunSplits::SessionId).text().not_null())
                    .col(ColumnDef::new(RunSplits::UserId).text().not_null())
                    .col(
                        ColumnDef::new(RunSplits::ActivityType)
                            .string_len(8)
                            .not_null()
                            .default("run")
                            .check((
                                "ck_run_splits_activity_type",
                                Expr::col(RunSplits::ActivityType).eq("run"),
                            )),
                    )
                    .col(
                        ColumnDef::new(RunSplits::Position)
                            .integer()
                            .not_null()
                            .check((
                                "ck_run_splits_position",
                                Expr::cust(
                                    "(position >= 0 AND position < 100) OR position >= 1000000",
                                ),
                            )),
                    )
                    .col(
                        ColumnDef::new(RunSplits::DistanceM)
                            .integer()
                            .not_null()
                            .check((
                                "ck_run_splits_distance_m",
                                Expr::col(RunSplits::DistanceM).gt(0),
                            )),
                    )
                    .col(
                        ColumnDef::new(RunSplits::DurationSec)
                            .integer()
                            .not_null()
                            .check((
                                "ck_run_splits_duration_sec",
                                Expr::col(RunSplits::DurationSec).gt(0),
                            )),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_run_splits_session")
                            .from_tbl(RunSplits::Table)
                            .from_col(RunSplits::SessionId)
                            .from_col(RunSplits::UserId)
                            .from_col(RunSplits::ActivityType)
                            .to_tbl(Sessions::Table)
                            .to_col(Sessions::Id)
                            .to_col(Sessions::UserId)
                            .to_col(Sessions::ActivityType)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("uq_run_splits_position")
                    .table(RunSplits::Table)
                    .col(RunSplits::SessionId)
                    .col(RunSplits::Position)
                    .unique()
                    .to_owned(),
            )
            .await?;

        let connection = manager.get_connection();
        let existing = connection
            .query_all_raw(Statement::from_string(
                connection.get_database_backend(),
                "SELECT session_id, user_id, activity_type, distance_m, duration_sec FROM runs",
            ))
            .await?;
        for run in existing {
            let session_id: String = run.try_get("", "session_id")?;
            let user_id: String = run.try_get("", "user_id")?;
            let activity_type: String = run.try_get("", "activity_type")?;
            let distance_m: i64 = run.try_get("", "distance_m")?;
            let duration_sec: i64 = run.try_get("", "duration_sec")?;
            connection
                .execute_raw(Statement::from_sql_and_values(
                    connection.get_database_backend(),
                    "INSERT INTO run_splits(id,session_id,user_id,activity_type,position,distance_m,duration_sec) VALUES(?,?,?,?,0,?,?)",
                    [
                        Uuid::now_v7().to_string().into(),
                        session_id.into(),
                        user_id.into(),
                        activity_type.into(),
                        distance_m.into(),
                        duration_sec.into(),
                    ],
                ))
                .await?;
        }

        rebuild_runs(
            connection,
            CREATE_RUNS_WITHOUT_TOTALS,
            "INSERT INTO runs_rebuilt(session_id,user_id,activity_type,elevation_gain_m) \
             SELECT session_id,user_id,activity_type,elevation_gain_m FROM runs",
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        rebuild_runs(
            manager.get_connection(),
            CREATE_RUNS_WITH_TOTALS,
            "INSERT INTO runs_rebuilt(session_id,user_id,activity_type,distance_m,duration_sec,elevation_gain_m) \
             SELECT runs.session_id, runs.user_id, runs.activity_type, \
                    (SELECT COALESCE(SUM(distance_m),0) FROM run_splits WHERE session_id = runs.session_id), \
                    (SELECT COALESCE(SUM(duration_sec),0) FROM run_splits WHERE session_id = runs.session_id), \
                    runs.elevation_gain_m \
             FROM runs",
        )
        .await?;
        manager
            .drop_table(Table::drop().table(RunSplits::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database};

    use super::*;
    use crate::migration::Migrator;

    #[tokio::test]
    async fn run_splits_enforce_constraints_and_are_removed() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        db.execute_unprepared("PRAGMA foreign_keys = ON")
            .await
            .unwrap();
        Migrator::up(&db, None).await.unwrap();
        let manager = SchemaManager::new(&db);

        assert!(manager.has_table("run_splits").await.unwrap());

        db.execute_unprepared("INSERT INTO users(id,email_normalized,email_display,role,status,auth_version,created_at,updated_at) VALUES('u1','a@example.com','a@example.com','user','active',0,'1','1')")
            .await
            .unwrap();
        db.execute_unprepared(
            "INSERT INTO sessions(id,user_id,started_at,activity_type) VALUES('s1','u1','1','run')",
        )
        .await
        .unwrap();
        db.execute_unprepared(
            "INSERT INTO run_splits(id,session_id,user_id,activity_type,position,distance_m,duration_sec) VALUES('r1','s1','u1','run',0,1000,300)",
        )
        .await
        .unwrap();

        for bad in [
            "INSERT INTO run_splits(id,session_id,user_id,activity_type,position,distance_m,duration_sec) VALUES('r2','s1','u1','strength',1,1000,300)",
            "INSERT INTO run_splits(id,session_id,user_id,activity_type,position,distance_m,duration_sec) VALUES('r3','s1','u1','run',500,1000,300)",
            "INSERT INTO run_splits(id,session_id,user_id,activity_type,position,distance_m,duration_sec) VALUES('r4','s1','u1','run',1,0,300)",
            "INSERT INTO run_splits(id,session_id,user_id,activity_type,position,distance_m,duration_sec) VALUES('r5','s1','u1','run',1,1000,0)",
            "INSERT INTO run_splits(id,session_id,user_id,activity_type,position,distance_m,duration_sec) VALUES('r6','s1','u1','run',0,1000,300)",
        ] {
            assert!(db.execute_unprepared(bad).await.is_err(), "accepted {bad}");
        }

        db.execute_unprepared("DELETE FROM sessions WHERE id='s1'")
            .await
            .unwrap();
        let remaining = db
            .query_all_raw(sea_orm::Statement::from_string(
                sea_orm::DbBackend::Sqlite,
                "SELECT id FROM run_splits",
            ))
            .await
            .unwrap();
        assert!(remaining.is_empty());

        Migration.down(&manager).await.unwrap();

        assert!(!manager.has_table("run_splits").await.unwrap());
    }

    async fn column_names(db: &sea_orm::DatabaseConnection, table: &str) -> Vec<String> {
        db.query_all_raw(sea_orm::Statement::from_string(
            sea_orm::DbBackend::Sqlite,
            format!("SELECT name FROM pragma_table_info('{table}')"),
        ))
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.try_get::<String>("", "name").unwrap())
        .collect()
    }

    #[tokio::test]
    async fn the_totals_of_an_existing_run_become_its_only_split_and_come_back_on_down() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        db.execute_unprepared("PRAGMA foreign_keys = ON")
            .await
            .unwrap();
        let earlier = Migrator::migrations().len() - 1;
        Migrator::up(&db, Some(earlier as u32)).await.unwrap();
        let manager = SchemaManager::new(&db);

        db.execute_unprepared("INSERT INTO users(id,email_normalized,email_display,role,status,auth_version,created_at,updated_at) VALUES('u1','a@example.com','a@example.com','user','active',0,'1','1')")
            .await
            .unwrap();
        db.execute_unprepared(
            "INSERT INTO sessions(id,user_id,started_at,activity_type) VALUES('s1','u1','1','run')",
        )
        .await
        .unwrap();
        db.execute_unprepared("INSERT INTO runs(session_id,user_id,activity_type,distance_m,duration_sec,elevation_gain_m) VALUES('s1','u1','run',8000,2400,40)")
            .await
            .unwrap();

        Migration.up(&manager).await.unwrap();

        let backfilled = db
            .query_all_raw(sea_orm::Statement::from_string(
                sea_orm::DbBackend::Sqlite,
                "SELECT id,session_id,user_id,activity_type,position,distance_m,duration_sec FROM run_splits",
            ))
            .await
            .unwrap();
        assert_eq!(backfilled.len(), 1);
        let split = &backfilled[0];
        assert!(
            Uuid::parse_str(&split.try_get::<String>("", "id").unwrap())
                .unwrap()
                .get_version_num()
                == 7
        );
        assert_eq!(split.try_get::<String>("", "session_id").unwrap(), "s1");
        assert_eq!(split.try_get::<String>("", "user_id").unwrap(), "u1");
        assert_eq!(split.try_get::<String>("", "activity_type").unwrap(), "run");
        assert_eq!(split.try_get::<i64>("", "position").unwrap(), 0);
        assert_eq!(split.try_get::<i64>("", "distance_m").unwrap(), 8_000);
        assert_eq!(split.try_get::<i64>("", "duration_sec").unwrap(), 2_400);

        assert_eq!(
            column_names(&db, "runs").await,
            ["session_id", "user_id", "activity_type", "elevation_gain_m"]
        );

        db.execute_unprepared("INSERT INTO run_splits(id,session_id,user_id,activity_type,position,distance_m,duration_sec) VALUES('extra','s1','u1','run',1,2000,500)")
            .await
            .unwrap();

        Migration.down(&manager).await.unwrap();

        assert!(!manager.has_table("run_splits").await.unwrap());
        let restored = db
            .query_one_raw(sea_orm::Statement::from_string(
                sea_orm::DbBackend::Sqlite,
                "SELECT distance_m,duration_sec,elevation_gain_m FROM runs WHERE session_id='s1'",
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(restored.try_get::<i64>("", "distance_m").unwrap(), 10_000);
        assert_eq!(restored.try_get::<i64>("", "duration_sec").unwrap(), 2_900);
        assert_eq!(restored.try_get::<i64>("", "elevation_gain_m").unwrap(), 40);
    }
}
