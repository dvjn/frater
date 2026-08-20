use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Users::Table)
                    .col(ColumnDef::new(Users::Id).text().not_null().primary_key())
                    .col(ColumnDef::new(Users::EmailNormalized).text().not_null())
                    .col(ColumnDef::new(Users::EmailDisplay).text().not_null())
                    .col(ColumnDef::new(Users::Role).text().not_null().check((
                        "ck_users_role",
                        Expr::col(Users::Role).is_in(["user", "superuser"]),
                    )))
                    .col(ColumnDef::new(Users::Status).text().not_null().check((
                        "ck_users_status",
                        Expr::col(Users::Status).is_in([
                            "pending_verification",
                            "active",
                            "disabled",
                            "deleting",
                        ]),
                    )))
                    .col(
                        ColumnDef::new(Users::AuthVersion)
                            .integer()
                            .not_null()
                            .default(0)
                            .check((
                                "ck_users_auth_version",
                                Expr::col(Users::AuthVersion).gte(0),
                            )),
                    )
                    .col(ColumnDef::new(Users::EmailVerifiedAt).text())
                    .col(ColumnDef::new(Users::CreatedAt).text().not_null())
                    .col(ColumnDef::new(Users::UpdatedAt).text().not_null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("uq_users_email_normalized")
                    .table(Users::Table)
                    .col(Users::EmailNormalized)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(PasswordCredentials::Table)
                    .col(
                        ColumnDef::new(PasswordCredentials::UserId)
                            .text()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(PasswordCredentials::PasswordHash)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(PasswordCredentials::PepperKeyId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(PasswordCredentials::CreatedAt)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(PasswordCredentials::UpdatedAt)
                            .text()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_password_credentials_user")
                            .from_tbl(PasswordCredentials::Table)
                            .from_col(PasswordCredentials::UserId)
                            .to_tbl(Users::Table)
                            .to_col(Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(AuthSessions::Table)
                    .col(
                        ColumnDef::new(AuthSessions::Id)
                            .text()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(AuthSessions::UserId).text().not_null())
                    .col(
                        ColumnDef::new(AuthSessions::Transport)
                            .string_len(32)
                            .not_null()
                            .check((
                                "ck_auth_sessions_transport",
                                Expr::col(AuthSessions::Transport)
                                    .is_in(["browser_cookie", "bearer"]),
                            )),
                    )
                    .col(
                        ColumnDef::new(AuthSessions::SecretDigest)
                            .blob()
                            .not_null()
                            .check((
                                "ck_auth_sessions_secret_digest",
                                Expr::cust("length(secret_digest) = 32"),
                            )),
                    )
                    .col(ColumnDef::new(AuthSessions::CsrfDigest).blob().check((
                        "ck_auth_sessions_csrf_digest",
                        Expr::cust("csrf_digest IS NULL OR length(csrf_digest) = 32"),
                    )))
                    .col(
                        ColumnDef::new(AuthSessions::KeyId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(AuthSessions::AuthVersion)
                            .integer()
                            .not_null()
                            .check((
                                "ck_auth_sessions_auth_version",
                                Expr::col(AuthSessions::AuthVersion).gte(0),
                            )),
                    )
                    .col(
                        ColumnDef::new(AuthSessions::AuthenticatedAt)
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(AuthSessions::CreatedAt).text().not_null())
                    .col(ColumnDef::new(AuthSessions::LastSeenAt).text().not_null())
                    .col(ColumnDef::new(AuthSessions::IdleExpiresAt).text().not_null())
                    .col(
                        ColumnDef::new(AuthSessions::AbsoluteExpiresAt)
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(AuthSessions::RevokedAt).text())
                    .col(ColumnDef::new(AuthSessions::RevocationReason).string_len(128))
                    .col(ColumnDef::new(AuthSessions::UserAgent).string_len(256))
                    .check((
                        "ck_auth_sessions_transport_csrf",
                        Expr::cust(
                            "(transport = 'browser_cookie' AND csrf_digest IS NOT NULL) OR (transport = 'bearer' AND csrf_digest IS NULL)",
                        ),
                    ))
                    .check((
                        "ck_auth_sessions_expiry",
                        Expr::cust("idle_expires_at <= absolute_expires_at"),
                    ))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_auth_sessions_user")
                            .from_tbl(AuthSessions::Table)
                            .from_col(AuthSessions::UserId)
                            .to_tbl(Users::Table)
                            .to_col(Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        for index in [
            Index::create()
                .name("idx_auth_sessions_user_active")
                .table(AuthSessions::Table)
                .col(AuthSessions::UserId)
                .col(AuthSessions::RevokedAt)
                .to_owned(),
            Index::create()
                .name("idx_auth_sessions_idle_expires_at")
                .table(AuthSessions::Table)
                .col(AuthSessions::IdleExpiresAt)
                .to_owned(),
            Index::create()
                .name("idx_auth_sessions_absolute_expires_at")
                .table(AuthSessions::Table)
                .col(AuthSessions::AbsoluteExpiresAt)
                .to_owned(),
        ] {
            manager.create_index(index).await?;
        }

        manager
            .create_table(
                Table::create()
                    .table(Muscles::Table)
                    .col(ColumnDef::new(Muscles::Id).text().not_null().primary_key())
                    .col(ColumnDef::new(Muscles::Name).text().not_null().unique_key())
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Equipment::Table)
                    .col(
                        ColumnDef::new(Equipment::Id)
                            .text()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Equipment::Name)
                            .text()
                            .not_null()
                            .unique_key(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Exercises::Table)
                    .col(
                        ColumnDef::new(Exercises::Id)
                            .text()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Exercises::Name)
                            .text()
                            .not_null()
                            .unique_key(),
                    )
                    .col(
                        ColumnDef::new(Exercises::ContractionType)
                            .string_len(9)
                            .not_null()
                            .check((
                                "ck_exercises_contraction_type",
                                Expr::col(Exercises::ContractionType)
                                    .is_in(["isometric", "dynamic"]),
                            )),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("uq_exercises_id_contraction_type")
                    .table(Exercises::Table)
                    .col(Exercises::Id)
                    .col(Exercises::ContractionType)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(ExerciseMuscles::Table)
                    .col(
                        ColumnDef::new(ExerciseMuscles::ExerciseId)
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(ExerciseMuscles::MuscleId).text().not_null())
                    .col(
                        ColumnDef::new(ExerciseMuscles::Role)
                            .string_len(9)
                            .not_null()
                            .check((
                                "ck_exercise_muscles_role",
                                Expr::col(ExerciseMuscles::Role).is_in(["primary", "secondary"]),
                            )),
                    )
                    .primary_key(
                        Index::create()
                            .name("pk_exercise_muscles")
                            .col(ExerciseMuscles::ExerciseId)
                            .col(ExerciseMuscles::MuscleId),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_exercise_muscles_exercise")
                            .from_tbl(ExerciseMuscles::Table)
                            .from_col(ExerciseMuscles::ExerciseId)
                            .to_tbl(Exercises::Table)
                            .to_col(Exercises::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_exercise_muscles_muscle")
                            .from_tbl(ExerciseMuscles::Table)
                            .from_col(ExerciseMuscles::MuscleId)
                            .to_tbl(Muscles::Table)
                            .to_col(Muscles::Id)
                            .on_delete(ForeignKeyAction::Restrict),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(ExerciseEquipment::Table)
                    .col(
                        ColumnDef::new(ExerciseEquipment::ExerciseId)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ExerciseEquipment::EquipmentId)
                            .text()
                            .not_null(),
                    )
                    .primary_key(
                        Index::create()
                            .name("pk_exercise_equipment")
                            .col(ExerciseEquipment::ExerciseId)
                            .col(ExerciseEquipment::EquipmentId),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_exercise_equipment_exercise")
                            .from_tbl(ExerciseEquipment::Table)
                            .from_col(ExerciseEquipment::ExerciseId)
                            .to_tbl(Exercises::Table)
                            .to_col(Exercises::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_exercise_equipment_equipment")
                            .from_tbl(ExerciseEquipment::Table)
                            .from_col(ExerciseEquipment::EquipmentId)
                            .to_tbl(Equipment::Table)
                            .to_col(Equipment::Id)
                            .on_delete(ForeignKeyAction::Restrict),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Sessions::Table)
                    .col(ColumnDef::new(Sessions::Id).text().not_null().primary_key())
                    .col(ColumnDef::new(Sessions::UserId).text().not_null())
                    .col(ColumnDef::new(Sessions::StartedAt).text().not_null())
                    .col(
                        ColumnDef::new(Sessions::ActivityType)
                            .string_len(8)
                            .not_null()
                            .check((
                                "ck_sessions_activity_type",
                                Expr::col(Sessions::ActivityType).is_in(["strength", "run"]),
                            )),
                    )
                    .col(ColumnDef::new(Sessions::Label).text())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_sessions_user")
                            .from_tbl(Sessions::Table)
                            .from_col(Sessions::UserId)
                            .to_tbl(Users::Table)
                            .to_col(Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        for index in [
            Index::create()
                .name("uq_sessions_id_user_activity_type")
                .table(Sessions::Table)
                .col(Sessions::Id)
                .col(Sessions::UserId)
                .col(Sessions::ActivityType)
                .unique()
                .to_owned(),
            Index::create()
                .name("idx_sessions_user_timeline")
                .table(Sessions::Table)
                .col(Sessions::UserId)
                .col(Sessions::StartedAt)
                .col(Sessions::Id)
                .to_owned(),
        ] {
            manager.create_index(index).await?;
        }

        manager
            .create_table(
                Table::create()
                    .table(SessionExercises::Table)
                    .col(
                        ColumnDef::new(SessionExercises::Id)
                            .text()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(SessionExercises::SessionId)
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(SessionExercises::UserId).text().not_null())
                    .col(
                        ColumnDef::new(SessionExercises::ExerciseId)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(SessionExercises::ActivityType)
                            .string_len(8)
                            .not_null()
                            .default("strength")
                            .check((
                                "ck_session_exercises_activity_type",
                                Expr::col(SessionExercises::ActivityType).eq("strength"),
                            )),
                    )
                    .col(
                        ColumnDef::new(SessionExercises::ContractionType)
                            .string_len(9)
                            .not_null()
                            .check((
                                "ck_session_exercises_contraction_type",
                                Expr::col(SessionExercises::ContractionType)
                                    .is_in(["isometric", "dynamic"]),
                            )),
                    )
                    // A move rewrites positions, so it parks rows above the
                    // ceiling first and the unique index never collides.
                    .col(
                        ColumnDef::new(SessionExercises::Position)
                            .integer()
                            .not_null()
                            .check((
                                "ck_session_exercises_position",
                                Expr::cust(
                                    "(position >= 0 AND position < 100) OR position >= 1000000",
                                ),
                            )),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_session_exercises_session")
                            .from_tbl(SessionExercises::Table)
                            .from_col(SessionExercises::SessionId)
                            .from_col(SessionExercises::UserId)
                            .from_col(SessionExercises::ActivityType)
                            .to_tbl(Sessions::Table)
                            .to_col(Sessions::Id)
                            .to_col(Sessions::UserId)
                            .to_col(Sessions::ActivityType)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_session_exercises_exercise")
                            .from_tbl(SessionExercises::Table)
                            .from_col(SessionExercises::ExerciseId)
                            .from_col(SessionExercises::ContractionType)
                            .to_tbl(Exercises::Table)
                            .to_col(Exercises::Id)
                            .to_col(Exercises::ContractionType)
                            .on_delete(ForeignKeyAction::Restrict),
                    )
                    .to_owned(),
            )
            .await?;
        for index in [
            Index::create()
                .name("uq_session_exercises_position")
                .table(SessionExercises::Table)
                .col(SessionExercises::SessionId)
                .col(SessionExercises::Position)
                .unique()
                .to_owned(),
            Index::create()
                .name("uq_session_exercises_id_user_contraction_type")
                .table(SessionExercises::Table)
                .col(SessionExercises::Id)
                .col(SessionExercises::UserId)
                .col(SessionExercises::ContractionType)
                .unique()
                .to_owned(),
        ] {
            manager.create_index(index).await?;
        }

        manager
            .create_table(
                Table::create()
                    .table(ExerciseSets::Table)
                    .col(
                        ColumnDef::new(ExerciseSets::Id)
                            .text()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(ExerciseSets::SessionExerciseId)
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(ExerciseSets::UserId).text().not_null())
                    .col(
                        ColumnDef::new(ExerciseSets::ContractionType)
                            .string_len(9)
                            .not_null()
                            .check((
                                "ck_exercise_sets_contraction_type",
                                Expr::col(ExerciseSets::ContractionType)
                                    .is_in(["isometric", "dynamic"]),
                            )),
                    )
                    .col(
                        ColumnDef::new(ExerciseSets::Position)
                            .integer()
                            .not_null()
                            .check((
                                "ck_exercise_sets_position",
                                Expr::cust("(position >= 0 AND position < 100) OR position >= 1000000"),
                            )),
                    )
                    .col(
                        ColumnDef::new(ExerciseSets::SetType)
                            .string_len(7)
                            .not_null()
                            .check((
                                "ck_exercise_sets_set_type",
                                Expr::col(ExerciseSets::SetType).is_in([
                                    "warmup", "working", "amrap", "drop",
                                ]),
                            )),
                    )
                    .col(ColumnDef::new(ExerciseSets::Reps).integer())
                    .col(ColumnDef::new(ExerciseSets::HoldSec).integer())
                    .col(
                        ColumnDef::new(ExerciseSets::LoadG)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .check((
                        "ck_exercise_sets_measurement",
                        Expr::cust(
                            "(contraction_type = 'dynamic' AND reps IS NOT NULL AND reps > 0 AND hold_sec IS NULL) OR (contraction_type = 'isometric' AND reps IS NULL AND hold_sec IS NOT NULL AND hold_sec > 0)",
                        ),
                    ))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_exercise_sets_session_exercise")
                            .from_tbl(ExerciseSets::Table)
                            .from_col(ExerciseSets::SessionExerciseId)
                            .from_col(ExerciseSets::UserId)
                            .from_col(ExerciseSets::ContractionType)
                            .to_tbl(SessionExercises::Table)
                            .to_col(SessionExercises::Id)
                            .to_col(SessionExercises::UserId)
                            .to_col(SessionExercises::ContractionType)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("uq_exercise_sets_position")
                    .table(ExerciseSets::Table)
                    .col(ExerciseSets::SessionExerciseId)
                    .col(ExerciseSets::Position)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Runs::Table)
                    .col(
                        ColumnDef::new(Runs::SessionId)
                            .text()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Runs::UserId).text().not_null())
                    .col(
                        ColumnDef::new(Runs::ActivityType)
                            .string_len(8)
                            .not_null()
                            .default("run")
                            .check((
                                "ck_runs_activity_type",
                                Expr::col(Runs::ActivityType).eq("run"),
                            )),
                    )
                    .col(
                        ColumnDef::new(Runs::DistanceM)
                            .integer()
                            .not_null()
                            .check(("ck_runs_distance_m", Expr::col(Runs::DistanceM).gt(0))),
                    )
                    .col(
                        ColumnDef::new(Runs::DurationSec)
                            .integer()
                            .not_null()
                            .check(("ck_runs_duration_sec", Expr::col(Runs::DurationSec).gt(0))),
                    )
                    .col(
                        ColumnDef::new(Runs::ElevationGainM)
                            .integer()
                            .not_null()
                            .default(0)
                            .check((
                                "ck_runs_elevation_gain_m",
                                Expr::col(Runs::ElevationGainM).gte(0),
                            )),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_runs_session")
                            .from_tbl(Runs::Table)
                            .from_col(Runs::SessionId)
                            .from_col(Runs::UserId)
                            .from_col(Runs::ActivityType)
                            .to_tbl(Sessions::Table)
                            .to_col(Sessions::Id)
                            .to_col(Sessions::UserId)
                            .to_col(Sessions::ActivityType)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        for index in [
            Index::create()
                .name("idx_exercise_muscles_muscle_id")
                .table(ExerciseMuscles::Table)
                .col(ExerciseMuscles::MuscleId)
                .to_owned(),
            Index::create()
                .name("idx_exercise_equipment_equipment_id")
                .table(ExerciseEquipment::Table)
                .col(ExerciseEquipment::EquipmentId)
                .to_owned(),
            Index::create()
                .name("idx_session_exercises_exercise_id")
                .table(SessionExercises::Table)
                .col(SessionExercises::ExerciseId)
                .to_owned(),
        ] {
            manager.create_index(index).await?;
        }

        manager
            .create_table(
                Table::create()
                    .table(OauthClients::Table)
                    .col(
                        ColumnDef::new(OauthClients::Id)
                            .string_len(64)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(OauthClients::Issuer)
                            .string_len(2048)
                            .not_null(),
                    )
                    .col(ColumnDef::new(OauthClients::ClientName).string_len(128))
                    .col(
                        ColumnDef::new(OauthClients::ApplicationType)
                            .string_len(8)
                            .not_null()
                            .check((
                                "ck_oauth_clients_application_type",
                                Expr::col(OauthClients::ApplicationType).is_in(["native", "web"]),
                            )),
                    )
                    .col(
                        ColumnDef::new(OauthClients::GrantTypes)
                            .string_len(256)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(OauthClients::ResponseTypes)
                            .string_len(32)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(OauthClients::Scope)
                            .string_len(512)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(OauthClients::TokenEndpointAuthMethod)
                            .string_len(16)
                            .not_null()
                            .default("none")
                            .check((
                                "ck_oauth_clients_public",
                                Expr::col(OauthClients::TokenEndpointAuthMethod).eq("none"),
                            )),
                    )
                    .col(
                        ColumnDef::new(OauthClients::CreatedAt)
                            .string_len(64)
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("uq_oauth_clients_id_issuer")
                    .table(OauthClients::Table)
                    .col(OauthClients::Id)
                    .col(OauthClients::Issuer)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(OauthClientRedirectUris::Table)
                    .col(
                        ColumnDef::new(OauthClientRedirectUris::ClientId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(OauthClientRedirectUris::Issuer)
                            .string_len(2048)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(OauthClientRedirectUris::RedirectUri)
                            .string_len(2048)
                            .not_null(),
                    )
                    .primary_key(
                        Index::create()
                            .name("pk_oauth_client_redirect_uris")
                            .col(OauthClientRedirectUris::ClientId)
                            .col(OauthClientRedirectUris::Issuer)
                            .col(OauthClientRedirectUris::RedirectUri),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_oauth_redirect_client")
                            .from_tbl(OauthClientRedirectUris::Table)
                            .from_col(OauthClientRedirectUris::ClientId)
                            .from_col(OauthClientRedirectUris::Issuer)
                            .to_tbl(OauthClients::Table)
                            .to_col(OauthClients::Id)
                            .to_col(OauthClients::Issuer)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(OauthAuthorizationCodes::Table)
                    .col(
                        ColumnDef::new(OauthAuthorizationCodes::Id)
                            .string_len(64)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(OauthAuthorizationCodes::SecretDigest)
                            .binary_len(32)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(OauthAuthorizationCodes::KeyId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(OauthAuthorizationCodes::UserId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(OauthAuthorizationCodes::ClientId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(OauthAuthorizationCodes::Issuer)
                            .string_len(2048)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(OauthAuthorizationCodes::RedirectUri)
                            .string_len(2048)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(OauthAuthorizationCodes::RegisteredRedirectUri)
                            .string_len(2048)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(OauthAuthorizationCodes::Resource)
                            .string_len(2048)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(OauthAuthorizationCodes::Scope)
                            .string_len(512)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(OauthAuthorizationCodes::AuthVersion)
                            .big_integer()
                            .not_null()
                            .check((
                                "ck_oauth_codes_auth_version",
                                Expr::col(OauthAuthorizationCodes::AuthVersion).gte(0),
                            )),
                    )
                    .col(
                        ColumnDef::new(OauthAuthorizationCodes::CodeChallenge)
                            .string_len(43)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(OauthAuthorizationCodes::CodeChallengeMethod)
                            .string_len(8)
                            .not_null()
                            .default("S256")
                            .check((
                                "ck_oauth_codes_pkce_method",
                                Expr::col(OauthAuthorizationCodes::CodeChallengeMethod).eq("S256"),
                            )),
                    )
                    .col(
                        ColumnDef::new(OauthAuthorizationCodes::CreatedAt)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(OauthAuthorizationCodes::ExpiresAt)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(ColumnDef::new(OauthAuthorizationCodes::ConsumedAt).string_len(64))
                    .col(
                        ColumnDef::new(OauthAuthorizationCodes::IssuedAccessTokenId).string_len(64),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_oauth_codes_user")
                            .from_tbl(OauthAuthorizationCodes::Table)
                            .from_col(OauthAuthorizationCodes::UserId)
                            .to_tbl(Users::Table)
                            .to_col(Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_oauth_codes_redirect")
                            .from_tbl(OauthAuthorizationCodes::Table)
                            .from_col(OauthAuthorizationCodes::ClientId)
                            .from_col(OauthAuthorizationCodes::Issuer)
                            .from_col(OauthAuthorizationCodes::RegisteredRedirectUri)
                            .to_tbl(OauthClientRedirectUris::Table)
                            .to_col(OauthClientRedirectUris::ClientId)
                            .to_col(OauthClientRedirectUris::Issuer)
                            .to_col(OauthClientRedirectUris::RedirectUri)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(OauthRefreshTokenFamilies::Table)
                    .col(
                        ColumnDef::new(OauthRefreshTokenFamilies::Id)
                            .string_len(64)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(OauthRefreshTokenFamilies::UserId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(OauthRefreshTokenFamilies::ClientId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(OauthRefreshTokenFamilies::Issuer)
                            .string_len(2048)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(OauthRefreshTokenFamilies::Resource)
                            .string_len(2048)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(OauthRefreshTokenFamilies::Scope)
                            .string_len(512)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(OauthRefreshTokenFamilies::AuthVersion)
                            .big_integer()
                            .not_null()
                            .check((
                                "ck_oauth_refresh_families_auth_version",
                                Expr::col(OauthRefreshTokenFamilies::AuthVersion).gte(0),
                            )),
                    )
                    .col(
                        ColumnDef::new(OauthRefreshTokenFamilies::CreatedAt)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(OauthRefreshTokenFamilies::AbsoluteExpiresAt)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(ColumnDef::new(OauthRefreshTokenFamilies::RevokedAt).string_len(64))
                    .col(ColumnDef::new(OauthRefreshTokenFamilies::RevocationReason).string_len(32))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_oauth_refresh_family_user")
                            .from_tbl(OauthRefreshTokenFamilies::Table)
                            .from_col(OauthRefreshTokenFamilies::UserId)
                            .to_tbl(Users::Table)
                            .to_col(Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_oauth_refresh_family_client")
                            .from_tbl(OauthRefreshTokenFamilies::Table)
                            .from_col(OauthRefreshTokenFamilies::ClientId)
                            .from_col(OauthRefreshTokenFamilies::Issuer)
                            .to_tbl(OauthClients::Table)
                            .to_col(OauthClients::Id)
                            .to_col(OauthClients::Issuer)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(OauthRefreshTokens::Table)
                    .col(
                        ColumnDef::new(OauthRefreshTokens::Id)
                            .string_len(64)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(OauthRefreshTokens::FamilyId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(OauthRefreshTokens::SecretDigest)
                            .binary_len(32)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(OauthRefreshTokens::KeyId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(OauthRefreshTokens::Generation)
                            .big_integer()
                            .not_null()
                            .check((
                                "ck_oauth_refresh_generation",
                                Expr::col(OauthRefreshTokens::Generation).gte(0),
                            )),
                    )
                    .col(
                        ColumnDef::new(OauthRefreshTokens::CreatedAt)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(OauthRefreshTokens::IdleExpiresAt)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(ColumnDef::new(OauthRefreshTokens::RotatedAt).string_len(64))
                    .col(ColumnDef::new(OauthRefreshTokens::RevokedAt).string_len(64))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_oauth_refresh_token_family")
                            .from_tbl(OauthRefreshTokens::Table)
                            .from_col(OauthRefreshTokens::FamilyId)
                            .to_tbl(OauthRefreshTokenFamilies::Table)
                            .to_col(OauthRefreshTokenFamilies::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(OauthAccessTokens::Table)
                    .col(
                        ColumnDef::new(OauthAccessTokens::Id)
                            .string_len(64)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(OauthAccessTokens::SecretDigest)
                            .binary_len(32)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(OauthAccessTokens::KeyId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(OauthAccessTokens::UserId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(OauthAccessTokens::ClientId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(OauthAccessTokens::Issuer)
                            .string_len(2048)
                            .not_null(),
                    )
                    .col(ColumnDef::new(OauthAccessTokens::RedirectUri).string_len(2048))
                    .col(
                        ColumnDef::new(OauthAccessTokens::Resource)
                            .string_len(2048)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(OauthAccessTokens::Scope)
                            .string_len(512)
                            .not_null(),
                    )
                    .col(ColumnDef::new(OauthAccessTokens::FamilyId).string_len(64))
                    .col(
                        ColumnDef::new(OauthAccessTokens::AuthVersion)
                            .big_integer()
                            .not_null()
                            .check((
                                "ck_oauth_tokens_auth_version",
                                Expr::col(OauthAccessTokens::AuthVersion).gte(0),
                            )),
                    )
                    .col(
                        ColumnDef::new(OauthAccessTokens::CreatedAt)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(OauthAccessTokens::ExpiresAt)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(ColumnDef::new(OauthAccessTokens::RevokedAt).string_len(64))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_oauth_tokens_user")
                            .from_tbl(OauthAccessTokens::Table)
                            .from_col(OauthAccessTokens::UserId)
                            .to_tbl(Users::Table)
                            .to_col(Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_oauth_tokens_redirect")
                            .from_tbl(OauthAccessTokens::Table)
                            .from_col(OauthAccessTokens::ClientId)
                            .from_col(OauthAccessTokens::Issuer)
                            .from_col(OauthAccessTokens::RedirectUri)
                            .to_tbl(OauthClientRedirectUris::Table)
                            .to_col(OauthClientRedirectUris::ClientId)
                            .to_col(OauthClientRedirectUris::Issuer)
                            .to_col(OauthClientRedirectUris::RedirectUri)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_oauth_tokens_family")
                            .from_tbl(OauthAccessTokens::Table)
                            .from_col(OauthAccessTokens::FamilyId)
                            .to_tbl(OauthRefreshTokenFamilies::Table)
                            .to_col(OauthRefreshTokenFamilies::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        for index in [
            Index::create()
                .name("uq_oauth_refresh_family_generation")
                .table(OauthRefreshTokens::Table)
                .col(OauthRefreshTokens::FamilyId)
                .col(OauthRefreshTokens::Generation)
                .unique()
                .to_owned(),
            Index::create()
                .name("idx_oauth_codes_expires_at")
                .table(OauthAuthorizationCodes::Table)
                .col(OauthAuthorizationCodes::ExpiresAt)
                .to_owned(),
            Index::create()
                .name("idx_oauth_codes_user_active")
                .table(OauthAuthorizationCodes::Table)
                .col(OauthAuthorizationCodes::UserId)
                .col(OauthAuthorizationCodes::ConsumedAt)
                .to_owned(),
            Index::create()
                .name("idx_oauth_tokens_expires_at")
                .table(OauthAccessTokens::Table)
                .col(OauthAccessTokens::ExpiresAt)
                .to_owned(),
            Index::create()
                .name("idx_oauth_tokens_user_active")
                .table(OauthAccessTokens::Table)
                .col(OauthAccessTokens::UserId)
                .col(OauthAccessTokens::RevokedAt)
                .to_owned(),
            Index::create()
                .name("idx_oauth_refresh_families_user")
                .table(OauthRefreshTokenFamilies::Table)
                .col(OauthRefreshTokenFamilies::UserId)
                .col(OauthRefreshTokenFamilies::RevokedAt)
                .to_owned(),
            Index::create()
                .name("idx_oauth_refresh_families_expiry")
                .table(OauthRefreshTokenFamilies::Table)
                .col(OauthRefreshTokenFamilies::AbsoluteExpiresAt)
                .to_owned(),
            Index::create()
                .name("idx_oauth_refresh_tokens_idle")
                .table(OauthRefreshTokens::Table)
                .col(OauthRefreshTokens::IdleExpiresAt)
                .to_owned(),
        ] {
            manager.create_index(index).await?;
        }

        manager
            .create_table(
                Table::create()
                    .table(OauthDeviceAuthorizations::Table)
                    .col(ColumnDef::new(OauthDeviceAuthorizations::Id).string_len(64).not_null().primary_key())
                    .col(ColumnDef::new(OauthDeviceAuthorizations::DeviceCodeDigest).binary_len(32).not_null())
                    .col(ColumnDef::new(OauthDeviceAuthorizations::UserCodeDigest).binary_len(32).not_null())
                    .col(ColumnDef::new(OauthDeviceAuthorizations::KeyId).string_len(64).not_null())
                    .col(ColumnDef::new(OauthDeviceAuthorizations::ClientId).string_len(64).not_null())
                    .col(ColumnDef::new(OauthDeviceAuthorizations::Issuer).string_len(2048).not_null())
                    .col(ColumnDef::new(OauthDeviceAuthorizations::Resource).string_len(2048).not_null())
                    .col(ColumnDef::new(OauthDeviceAuthorizations::Scope).string_len(512).not_null())
                    .col(ColumnDef::new(OauthDeviceAuthorizations::Status).string_len(16).not_null().default("pending").check((
                        "ck_oauth_device_status",
                        Expr::col(OauthDeviceAuthorizations::Status).is_in(["pending", "approved", "denied", "consumed"]),
                    )))
                    .col(ColumnDef::new(OauthDeviceAuthorizations::UserId).string_len(64))
                    .col(ColumnDef::new(OauthDeviceAuthorizations::AuthVersion).big_integer().check((
                        "ck_oauth_device_auth_version",
                        Expr::col(OauthDeviceAuthorizations::AuthVersion).gte(0),
                    )))
                    .col(ColumnDef::new(OauthDeviceAuthorizations::CreatedAt).string_len(64).not_null())
                    .col(ColumnDef::new(OauthDeviceAuthorizations::ExpiresAt).string_len(64).not_null())
                    .col(ColumnDef::new(OauthDeviceAuthorizations::IntervalSeconds).big_integer().not_null().check((
                        "ck_oauth_device_interval",
                        Expr::col(OauthDeviceAuthorizations::IntervalSeconds).gte(5),
                    )))
                    .col(ColumnDef::new(OauthDeviceAuthorizations::LastPollAt).string_len(64))
                    .col(ColumnDef::new(OauthDeviceAuthorizations::DecisionAt).string_len(64))
                    .col(ColumnDef::new(OauthDeviceAuthorizations::ConsumedAt).string_len(64))
                    .check((
                        "ck_oauth_device_code_digest_length",
                        Expr::cust("length(device_code_digest) = 32"),
                    ))
                    .check((
                        "ck_oauth_device_user_digest_length",
                        Expr::cust("length(user_code_digest) = 32"),
                    ))
                    .check((
                        "ck_oauth_device_state",
                        Expr::cust("(status = 'pending' AND user_id IS NULL AND auth_version IS NULL AND decision_at IS NULL AND consumed_at IS NULL) OR (status = 'denied' AND user_id IS NULL AND auth_version IS NULL AND decision_at IS NOT NULL AND consumed_at IS NULL) OR (status = 'approved' AND user_id IS NOT NULL AND auth_version IS NOT NULL AND decision_at IS NOT NULL AND consumed_at IS NULL) OR (status = 'consumed' AND user_id IS NOT NULL AND auth_version IS NOT NULL AND decision_at IS NOT NULL AND consumed_at IS NOT NULL)"),
                    ))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_oauth_device_client")
                            .from_tbl(OauthDeviceAuthorizations::Table)
                            .from_col(OauthDeviceAuthorizations::ClientId)
                            .from_col(OauthDeviceAuthorizations::Issuer)
                            .to_tbl(OauthClients::Table)
                            .to_col(OauthClients::Id)
                            .to_col(OauthClients::Issuer)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_oauth_device_user")
                            .from_tbl(OauthDeviceAuthorizations::Table)
                            .from_col(OauthDeviceAuthorizations::UserId)
                            .to_tbl(Users::Table)
                            .to_col(Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        for index in [
            Index::create()
                .name("uq_oauth_device_user_code_digest")
                .table(OauthDeviceAuthorizations::Table)
                .col(OauthDeviceAuthorizations::UserCodeDigest)
                .unique()
                .to_owned(),
            Index::create()
                .name("idx_oauth_device_client_issuer")
                .table(OauthDeviceAuthorizations::Table)
                .col(OauthDeviceAuthorizations::ClientId)
                .col(OauthDeviceAuthorizations::Issuer)
                .to_owned(),
            Index::create()
                .name("idx_oauth_device_expires_at")
                .table(OauthDeviceAuthorizations::Table)
                .col(OauthDeviceAuthorizations::ExpiresAt)
                .to_owned(),
        ] {
            manager.create_index(index).await?;
        }

        manager
            .create_table(
                Table::create()
                    .table(AuthOneTimeTokens::Table)
                    .col(
                        ColumnDef::new(AuthOneTimeTokens::Id)
                            .string_len(64)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(AuthOneTimeTokens::UserId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(AuthOneTimeTokens::Purpose)
                            .string_len(32)
                            .not_null()
                            .check((
                                "ck_auth_one_time_tokens_purpose",
                                Expr::col(AuthOneTimeTokens::Purpose)
                                    .is_in(["verify_email", "reset_password"]),
                            )),
                    )
                    .col(
                        ColumnDef::new(AuthOneTimeTokens::CodeDigest)
                            .binary_len(32)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(AuthOneTimeTokens::KeyId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(AuthOneTimeTokens::CreatedAt)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(AuthOneTimeTokens::ExpiresAt)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(AuthOneTimeTokens::Attempts)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(ColumnDef::new(AuthOneTimeTokens::ConsumedAt).string_len(64))
                    .check((
                        "ck_auth_one_time_tokens_digest_length",
                        Expr::cust("length(code_digest) = 32"),
                    ))
                    .check((
                        "ck_auth_one_time_tokens_expiry",
                        Expr::cust("expires_at > created_at"),
                    ))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_auth_one_time_tokens_user")
                            .from_tbl(AuthOneTimeTokens::Table)
                            .from_col(AuthOneTimeTokens::UserId)
                            .to_tbl(Users::Table)
                            .to_col(Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        for index in [
            Index::create()
                .unique()
                .name("idx_auth_one_time_tokens_user_purpose")
                .table(AuthOneTimeTokens::Table)
                .col(AuthOneTimeTokens::UserId)
                .col(AuthOneTimeTokens::Purpose)
                .to_owned(),
            Index::create()
                .name("idx_auth_one_time_tokens_expires_at")
                .table(AuthOneTimeTokens::Table)
                .col(AuthOneTimeTokens::ExpiresAt)
                .to_owned(),
        ] {
            manager.create_index(index).await?;
        }

        for index in [
            Index::create()
                .name("idx_oauth_tokens_family")
                .table(OauthAccessTokens::Table)
                .col(OauthAccessTokens::FamilyId)
                .to_owned(),
            Index::create()
                .name("idx_oauth_codes_redirect")
                .table(OauthAuthorizationCodes::Table)
                .col(OauthAuthorizationCodes::ClientId)
                .col(OauthAuthorizationCodes::Issuer)
                .col(OauthAuthorizationCodes::RegisteredRedirectUri)
                .to_owned(),
            Index::create()
                .name("idx_oauth_tokens_redirect")
                .table(OauthAccessTokens::Table)
                .col(OauthAccessTokens::ClientId)
                .col(OauthAccessTokens::Issuer)
                .col(OauthAccessTokens::RedirectUri)
                .to_owned(),
            Index::create()
                .name("idx_oauth_clients_issuer")
                .table(OauthClients::Table)
                .col(OauthClients::Issuer)
                .to_owned(),
        ] {
            manager.create_index(index).await?;
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Children before parents, so a foreign key never blocks a drop.
        for table in [
            "auth_one_time_tokens",
            "oauth_device_authorizations",
            "oauth_access_tokens",
            "oauth_refresh_tokens",
            "oauth_refresh_token_families",
            "oauth_authorization_codes",
            "oauth_client_redirect_uris",
            "oauth_clients",
            "runs",
            "exercise_sets",
            "session_exercises",
            "sessions",
            "exercise_equipment",
            "exercise_muscles",
            "exercises",
            "equipment",
            "muscles",
            "auth_sessions",
            "password_credentials",
            "users",
        ] {
            manager
                .drop_table(Table::drop().table(Alias::new(table)).to_owned())
                .await?;
        }
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Users {
    Table,
    AuthVersion,
    CreatedAt,
    EmailDisplay,
    EmailNormalized,
    EmailVerifiedAt,
    Id,
    Role,
    Status,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum PasswordCredentials {
    Table,
    CreatedAt,
    PasswordHash,
    PepperKeyId,
    UpdatedAt,
    UserId,
}

#[derive(DeriveIden)]
enum AuthSessions {
    Table,
    AbsoluteExpiresAt,
    AuthVersion,
    AuthenticatedAt,
    CreatedAt,
    CsrfDigest,
    Id,
    IdleExpiresAt,
    KeyId,
    LastSeenAt,
    RevocationReason,
    RevokedAt,
    SecretDigest,
    Transport,
    UserAgent,
    UserId,
}

#[derive(DeriveIden)]
enum Muscles {
    Table,
    Id,
    Name,
}

#[derive(DeriveIden)]
enum Equipment {
    Table,
    Id,
    Name,
}

#[derive(DeriveIden)]
enum Exercises {
    Table,
    ContractionType,
    Id,
    Name,
}

#[derive(DeriveIden)]
enum ExerciseMuscles {
    Table,
    ExerciseId,
    MuscleId,
    Role,
}

#[derive(DeriveIden)]
enum ExerciseEquipment {
    Table,
    EquipmentId,
    ExerciseId,
}

#[derive(DeriveIden)]
enum Sessions {
    Table,
    ActivityType,
    Id,
    Label,
    StartedAt,
    UserId,
}

#[derive(DeriveIden)]
enum SessionExercises {
    Table,
    ActivityType,
    ContractionType,
    ExerciseId,
    Id,
    Position,
    SessionId,
    UserId,
}

#[derive(DeriveIden)]
enum ExerciseSets {
    Table,
    ContractionType,
    HoldSec,
    Id,
    LoadG,
    Position,
    Reps,
    SessionExerciseId,
    SetType,
    UserId,
}

#[derive(DeriveIden)]
enum Runs {
    Table,
    ActivityType,
    DistanceM,
    DurationSec,
    ElevationGainM,
    SessionId,
    UserId,
}

#[derive(DeriveIden)]
enum OauthClients {
    ApplicationType,
    ClientName,
    CreatedAt,
    GrantTypes,
    Id,
    Issuer,
    ResponseTypes,
    Scope,
    Table,
    TokenEndpointAuthMethod,
}

#[derive(DeriveIden)]
enum OauthClientRedirectUris {
    ClientId,
    Issuer,
    RedirectUri,
    Table,
}

#[derive(DeriveIden)]
enum OauthAuthorizationCodes {
    AuthVersion,
    ClientId,
    CodeChallenge,
    CodeChallengeMethod,
    ConsumedAt,
    CreatedAt,
    ExpiresAt,
    Id,
    IssuedAccessTokenId,
    Issuer,
    KeyId,
    RedirectUri,
    RegisteredRedirectUri,
    Resource,
    Scope,
    SecretDigest,
    Table,
    UserId,
}

#[derive(DeriveIden)]
enum OauthRefreshTokenFamilies {
    AbsoluteExpiresAt,
    AuthVersion,
    ClientId,
    CreatedAt,
    Id,
    Issuer,
    Resource,
    RevocationReason,
    RevokedAt,
    Scope,
    Table,
    UserId,
}

#[derive(DeriveIden)]
enum OauthRefreshTokens {
    CreatedAt,
    FamilyId,
    Generation,
    Id,
    IdleExpiresAt,
    KeyId,
    RevokedAt,
    RotatedAt,
    SecretDigest,
    Table,
}

#[derive(DeriveIden)]
enum OauthAccessTokens {
    AuthVersion,
    ClientId,
    CreatedAt,
    ExpiresAt,
    FamilyId,
    Id,
    Issuer,
    KeyId,
    RedirectUri,
    Resource,
    RevokedAt,
    Scope,
    SecretDigest,
    Table,
    UserId,
}

#[derive(DeriveIden)]
enum OauthDeviceAuthorizations {
    AuthVersion,
    ClientId,
    ConsumedAt,
    CreatedAt,
    DecisionAt,
    DeviceCodeDigest,
    ExpiresAt,
    Id,
    IntervalSeconds,
    Issuer,
    KeyId,
    LastPollAt,
    Resource,
    Scope,
    Status,
    Table,
    UserCodeDigest,
    UserId,
}

#[derive(DeriveIden)]
enum AuthOneTimeTokens {
    Attempts,
    CodeDigest,
    ConsumedAt,
    CreatedAt,
    ExpiresAt,
    Id,
    KeyId,
    Purpose,
    Table,
    UserId,
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};

    async fn database() -> sea_orm::DatabaseConnection {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        db.execute_unprepared("PRAGMA foreign_keys=ON")
            .await
            .unwrap();
        Migration.up(&SchemaManager::new(&db)).await.unwrap();
        db
    }
    async fn user(db: &sea_orm::DatabaseConnection, id: &str, email: &str) {
        db.execute_unprepared(&format!("INSERT INTO users(id,email_normalized,email_display,role,status,auth_version,created_at,updated_at) VALUES('{id}','{email}','{email}','user','active',0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')")).await.unwrap();
    }

    #[tokio::test]
    async fn auth_checks_and_named_indexes_are_enforced() {
        let db = database().await;
        user(&db, "u1", "a@example.com").await;
        assert!(db.execute_unprepared("INSERT INTO users(id,email_normalized,email_display,role,status,auth_version,created_at,updated_at) VALUES('u2','a@example.com','A@example.com','user','active',0,'x','x')").await.is_err());
        assert!(db.execute_unprepared("INSERT INTO users(id,email_normalized,email_display,role,status,auth_version,created_at,updated_at) VALUES('bad','b@example.com','b','admin','active',0,'x','x')").await.is_err());
        assert!(db.execute_unprepared("INSERT INTO users(id,email_normalized,email_display,role,status,auth_version,created_at,updated_at) VALUES('bad','b@example.com','b','user','unknown',0,'x','x')").await.is_err());
        assert!(db.execute_unprepared("INSERT INTO users(id,email_normalized,email_display,role,status,auth_version,created_at,updated_at) VALUES('bad','b@example.com','b','user','active',-1,'x','x')").await.is_err());
        assert!(
            db.execute_unprepared(
                "INSERT INTO password_credentials VALUES('missing','hash','pepper','x','x')"
            )
            .await
            .is_err()
        );
        assert!(db.execute_unprepared("INSERT INTO auth_sessions(id,user_id,transport,secret_digest,csrf_digest,key_id,auth_version,authenticated_at,created_at,last_seen_at,idle_expires_at,absolute_expires_at) VALUES('missing-user','missing','browser_cookie',zeroblob(32),zeroblob(32),'k',0,'1','1','1','2','3')").await.is_err());
        assert!(db.execute_unprepared("INSERT INTO auth_sessions(id,user_id,transport,secret_digest,key_id,auth_version,authenticated_at,created_at,last_seen_at,idle_expires_at,absolute_expires_at) VALUES('s','u1','browser_cookie',zeroblob(32),'k',0,'1','1','1','2','3')").await.is_err());
        assert!(db.execute_unprepared("INSERT INTO auth_sessions(id,user_id,transport,secret_digest,csrf_digest,key_id,auth_version,authenticated_at,created_at,last_seen_at,idle_expires_at,absolute_expires_at) VALUES('s','u1','bearer',zeroblob(32),zeroblob(32),'k',0,'1','1','1','2','3')").await.is_err());
        assert!(db.execute_unprepared("INSERT INTO auth_sessions(id,user_id,transport,secret_digest,csrf_digest,key_id,auth_version,authenticated_at,created_at,last_seen_at,idle_expires_at,absolute_expires_at) VALUES('s','u1','browser_cookie',zeroblob(32),zeroblob(32),'k',0,'1','1','1','4','3')").await.is_err());
        assert!(db.execute_unprepared("INSERT INTO auth_sessions(id,user_id,transport,secret_digest,csrf_digest,key_id,auth_version,authenticated_at,created_at,last_seen_at,idle_expires_at,absolute_expires_at) VALUES('s','u1','browser_cookie',zeroblob(32),zeroblob(32),'k',-1,'1','1','1','2','3')").await.is_err());
        db.execute_unprepared("INSERT INTO auth_sessions(id,user_id,transport,secret_digest,csrf_digest,key_id,auth_version,authenticated_at,created_at,last_seen_at,idle_expires_at,absolute_expires_at) VALUES('s','u1','browser_cookie',zeroblob(32),zeroblob(32),'k',0,'1','1','1','2','3')").await.unwrap();
        assert!(db.execute_unprepared("INSERT INTO auth_sessions(id,user_id,transport,secret_digest,csrf_digest,key_id,auth_version,authenticated_at,created_at,last_seen_at,idle_expires_at,absolute_expires_at) VALUES('s','u1','browser_cookie',zeroblob(32),zeroblob(32),'k',0,'1','1','1','2','3')").await.is_err());
        let row=db.query_one_raw(Statement::from_string(DbBackend::Sqlite,"SELECT count(*) n FROM sqlite_master WHERE type='index' AND name IN ('uq_users_email_normalized','idx_auth_sessions_user_active','idx_auth_sessions_idle_expires_at','idx_auth_sessions_absolute_expires_at','uq_exercises_id_contraction_type','uq_sessions_id_user_activity_type','idx_sessions_user_timeline','uq_session_exercises_position','uq_session_exercises_id_user_contraction_type','uq_exercise_sets_position','idx_exercise_muscles_muscle_id','idx_exercise_equipment_equipment_id','idx_session_exercises_exercise_id')")).await.unwrap().unwrap();
        let n: i64 = row.try_get("", "n").unwrap();
        assert_eq!(n, 13);
    }

    #[tokio::test]
    async fn composite_ownership_and_user_cascade_are_enforced() {
        let db = database().await;
        user(&db, "u1", "a@example.com").await;
        user(&db, "u2", "b@example.com").await;
        db.execute_unprepared("INSERT INTO exercises VALUES('e','Squat','dynamic')")
            .await
            .unwrap();
        db.execute_unprepared("INSERT INTO sessions(id,user_id,started_at,activity_type) VALUES('w','u1','2026-01-01T00:00:00Z','strength')").await.unwrap();
        assert!(db.execute_unprepared("INSERT INTO session_exercises(id,session_id,user_id,exercise_id,contraction_type,position) VALUES('se','w','u2','e','dynamic',0)").await.is_err());
        assert!(db.execute_unprepared("INSERT INTO session_exercises(id,session_id,user_id,exercise_id,contraction_type,position) VALUES('wrong-contraction','w','u1','e','isometric',0)").await.is_err());
        db.execute_unprepared("INSERT INTO session_exercises(id,session_id,user_id,exercise_id,contraction_type,position) VALUES('se','w','u1','e','dynamic',0)").await.unwrap();
        assert!(db.execute_unprepared("INSERT INTO exercise_sets(id,session_exercise_id,user_id,contraction_type,position,set_type,reps) VALUES('set','se','u2','dynamic',0,'working',5)").await.is_err());
        assert!(db.execute_unprepared("INSERT INTO exercise_sets(id,session_exercise_id,user_id,contraction_type,position,set_type,hold_sec) VALUES('set','se','u1','isometric',0,'working',5)").await.is_err());
        db.execute_unprepared("INSERT INTO exercise_sets(id,session_exercise_id,user_id,contraction_type,position,set_type,reps) VALUES('set','se','u1','dynamic',0,'working',5)").await.unwrap();
        assert!(db.execute_unprepared("INSERT INTO runs(session_id,user_id,distance_m,duration_sec) VALUES('w','u1',1000,300)").await.is_err());
        db.execute_unprepared("INSERT INTO sessions(id,user_id,started_at,activity_type) VALUES('run','u1','2026-01-01T00:00:00Z','run')").await.unwrap();
        assert!(db.execute_unprepared("INSERT INTO session_exercises(id,session_id,user_id,exercise_id,contraction_type,position) VALUES('on-run','run','u1','e','dynamic',0)").await.is_err());
        assert!(db.execute_unprepared("INSERT INTO runs(session_id,user_id,distance_m,duration_sec) VALUES('run','u2',1000,300)").await.is_err());
        db.execute_unprepared("INSERT INTO runs(session_id,user_id,distance_m,duration_sec) VALUES('run','u1',1000,300)").await.unwrap();
        db.execute_unprepared(
            "INSERT INTO password_credentials VALUES('u1','hash','pepper','x','x')",
        )
        .await
        .unwrap();
        db.execute_unprepared("INSERT INTO auth_sessions(id,user_id,transport,secret_digest,csrf_digest,key_id,auth_version,authenticated_at,created_at,last_seen_at,idle_expires_at,absolute_expires_at) VALUES('auth','u1','browser_cookie',zeroblob(32),zeroblob(32),'k',0,'1','1','1','2','3')").await.unwrap();
        assert!(
            db.execute_unprepared("DELETE FROM exercises WHERE id='e'")
                .await
                .is_err()
        );
        db.execute_unprepared("DELETE FROM users WHERE id='u1'")
            .await
            .unwrap();
        for table in [
            "password_credentials",
            "auth_sessions",
            "sessions",
            "session_exercises",
            "exercise_sets",
            "runs",
        ] {
            let row = db
                .query_one_raw(Statement::from_string(
                    DbBackend::Sqlite,
                    format!("SELECT count(*) n FROM {table}"),
                ))
                .await
                .unwrap()
                .unwrap();
            let n: i64 = row.try_get("", "n").unwrap();
            assert_eq!(n, 0, "{table}");
        }
    }

    async fn fixtures(db: &sea_orm::DatabaseConnection) {
        db.execute_unprepared(
            "INSERT INTO users(id,email_normalized,email_display,role,status,auth_version,created_at,updated_at) VALUES('u','u@example.com','u@example.com','user','active',0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');
             INSERT INTO oauth_clients(id,issuer,application_type,grant_types,response_types,scope,token_endpoint_auth_method,created_at) VALUES('c','https://frater.example','web','authorization_code','code','workouts:read','none','2026-01-01T00:00:00Z');
             INSERT INTO oauth_client_redirect_uris(client_id,issuer,redirect_uri) VALUES('c','https://frater.example','https://client.example/callback')",
        ).await.unwrap();
    }

    #[tokio::test]
    async fn public_clients_exact_redirects_bindings_and_cascades_are_enforced() {
        let db = database().await;
        fixtures(&db).await;
        assert!(db.execute_unprepared("INSERT INTO oauth_clients(id,issuer,application_type,grant_types,response_types,scope,token_endpoint_auth_method,created_at) VALUES('bad','https://frater.example','web','authorization_code','code','workouts:read','client_secret_basic','x')").await.is_err());
        assert!(db.execute_unprepared("INSERT INTO oauth_client_redirect_uris(client_id,issuer,redirect_uri) VALUES('c','https://other.example','https://client.example/callback')").await.is_err());
        assert!(db.execute_unprepared("INSERT INTO oauth_authorization_codes(id,secret_digest,key_id,user_id,client_id,issuer,redirect_uri,registered_redirect_uri,resource,scope,auth_version,code_challenge,code_challenge_method,created_at,expires_at) VALUES('code',zeroblob(32),'k','u','c','https://frater.example','https://client.example/other','https://client.example/other','https://frater.example/mcp','workouts:read',0,'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa','S256','1','2')").await.is_err());
        db.execute_unprepared("INSERT INTO oauth_refresh_token_families(id,user_id,client_id,issuer,resource,scope,auth_version,created_at,absolute_expires_at) VALUES('f','u','c','https://frater.example','https://frater.example/mcp','workouts:read offline_access',0,'1','2'); INSERT INTO oauth_refresh_tokens(id,family_id,secret_digest,key_id,generation,created_at,idle_expires_at) VALUES('r','f',zeroblob(32),'k',0,'1','2'); INSERT INTO oauth_access_tokens(id,secret_digest,key_id,user_id,client_id,issuer,redirect_uri,resource,scope,family_id,auth_version,created_at,expires_at) VALUES('token',zeroblob(32),'k','u','c','https://frater.example','https://client.example/callback','https://frater.example/mcp','workouts:read','f',0,'1','2')").await.unwrap();
        assert!(db.execute_unprepared("INSERT INTO oauth_refresh_tokens(id,family_id,secret_digest,key_id,generation,created_at,idle_expires_at) VALUES('r2','f',zeroblob(32),'k',0,'1','2')").await.is_err());
        db.execute_unprepared("DELETE FROM oauth_clients WHERE id='c'")
            .await
            .unwrap();
        for table in [
            "oauth_client_redirect_uris",
            "oauth_authorization_codes",
            "oauth_refresh_token_families",
            "oauth_refresh_tokens",
            "oauth_access_tokens",
        ] {
            let count: i64 = db
                .query_one_raw(Statement::from_string(
                    DbBackend::Sqlite,
                    format!("SELECT count(*) AS n FROM {table}"),
                ))
                .await
                .unwrap()
                .unwrap()
                .try_get("", "n")
                .unwrap();
            assert_eq!(count, 0, "{table}");
        }
    }

    #[tokio::test]
    async fn oauth_indexes_are_named_and_created() {
        let db = database().await;
        let count: i64 = db.query_one_raw(Statement::from_string(DbBackend::Sqlite, "SELECT count(*) AS n FROM sqlite_master WHERE type='index' AND name IN ('uq_oauth_clients_id_issuer','uq_oauth_refresh_family_generation','idx_oauth_codes_expires_at','idx_oauth_codes_user_active','idx_oauth_tokens_expires_at','idx_oauth_tokens_user_active','idx_oauth_refresh_families_user','idx_oauth_refresh_families_expiry','idx_oauth_refresh_tokens_idle')")).await.unwrap().unwrap().try_get("", "n").unwrap();
        assert_eq!(count, 9);
    }

    #[tokio::test]
    async fn oauth_replay_column_and_indexes_are_created() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        db.execute_unprepared("PRAGMA foreign_keys=ON")
            .await
            .unwrap();
        let manager = SchemaManager::new(&db);
        Migration.up(&manager).await.unwrap();
        assert!(
            manager
                .has_column("oauth_authorization_codes", "issued_access_token_id")
                .await
                .unwrap()
        );
        let count: i64 = db.query_one_raw(Statement::from_string(DbBackend::Sqlite, "SELECT count(*) AS n FROM sqlite_master WHERE type='index' AND name IN ('idx_oauth_tokens_family','idx_oauth_codes_redirect','idx_oauth_tokens_redirect','idx_oauth_clients_issuer')")).await.unwrap().unwrap().try_get("", "n").unwrap();
        assert_eq!(count, 4);
    }

    #[tokio::test]
    async fn device_constraints_indexes_and_cascades_are_enforced() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        db.execute_unprepared("PRAGMA foreign_keys=ON")
            .await
            .unwrap();
        Migration.up(&SchemaManager::new(&db)).await.unwrap();
        db.execute_unprepared("INSERT INTO users(id,email_normalized,email_display,role,status,auth_version,created_at,updated_at) VALUES('u','u@example.com','u@example.com','user','active',0,'1','1'); INSERT INTO oauth_clients(id,issuer,application_type,grant_types,response_types,scope,token_endpoint_auth_method,created_at) VALUES('c','https://frater.example','native','urn:ietf:params:oauth:grant-type:device_code','','workouts:read','none','1'); INSERT INTO oauth_device_authorizations(id,device_code_digest,user_code_digest,key_id,client_id,issuer,resource,scope,status,created_at,expires_at,interval_seconds) VALUES('d',zeroblob(32),randomblob(32),'k','c','https://frater.example','https://frater.example/mcp','workouts:read','pending','1','2',5)").await.unwrap();
        assert!(db.execute_unprepared("INSERT INTO oauth_device_authorizations(id,device_code_digest,user_code_digest,key_id,client_id,issuer,resource,scope,status,created_at,expires_at,interval_seconds) SELECT 'd2',randomblob(32),user_code_digest,'k','c','https://frater.example','x','workouts:read','pending','1','2',5 FROM oauth_device_authorizations").await.is_err());
        assert!(
            db.execute_unprepared("UPDATE oauth_device_authorizations SET status='approved'")
                .await
                .is_err()
        );
        assert!(db.execute_unprepared("INSERT INTO oauth_device_authorizations(id,device_code_digest,user_code_digest,key_id,client_id,issuer,resource,scope,status,created_at,expires_at,interval_seconds) VALUES('bad-digest',randomblob(31),randomblob(32),'k','c','https://frater.example','https://frater.example/mcp','workouts:read','pending','1','2',5)").await.is_err());
        db.execute_unprepared("DELETE FROM oauth_clients WHERE id='c'")
            .await
            .unwrap();
        let count: i64 = db
            .query_one_raw(sea_orm::Statement::from_string(
                sea_orm::DbBackend::Sqlite,
                "SELECT count(*) AS n FROM oauth_device_authorizations",
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "n")
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn one_time_token_constraints_and_cascade_are_enforced() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        db.execute_unprepared("PRAGMA foreign_keys=ON")
            .await
            .unwrap();
        Migration.up(&SchemaManager::new(&db)).await.unwrap();
        db.execute_unprepared("INSERT INTO users(id,email_normalized,email_display,role,status,auth_version,created_at,updated_at) VALUES('u','u@example.com','u@example.com','user','pending_verification',0,'1','1')").await.unwrap();
        db.execute_unprepared("INSERT INTO auth_one_time_tokens(id,user_id,purpose,code_digest,key_id,created_at,expires_at) VALUES('t','u','verify_email',zeroblob(32),'k','1','2')").await.unwrap();
        assert!(db.execute_unprepared("INSERT INTO auth_one_time_tokens(id,user_id,purpose,code_digest,key_id,created_at,expires_at) VALUES('t2','u','other',zeroblob(32),'k','1','2')").await.is_err());
        assert!(db.execute_unprepared("INSERT INTO auth_one_time_tokens(id,user_id,purpose,code_digest,key_id,created_at,expires_at) VALUES('t3','u','verify_email',zeroblob(31),'k','1','2')").await.is_err());
        assert!(db.execute_unprepared("INSERT INTO auth_one_time_tokens(id,user_id,purpose,code_digest,key_id,created_at,expires_at) VALUES('t4','missing','verify_email',zeroblob(32),'k','1','2')").await.is_err());
        db.execute_unprepared("DELETE FROM users WHERE id='u'")
            .await
            .unwrap();
        let count: i64 = db
            .query_one_raw(sea_orm::Statement::from_string(
                sea_orm::DbBackend::Sqlite,
                "SELECT count(*) AS n FROM auth_one_time_tokens",
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "n")
            .unwrap();
        assert_eq!(count, 0);
    }
}
