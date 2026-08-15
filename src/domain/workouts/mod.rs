mod exercises;
mod history;
mod log;
mod sessions;
mod sets;

use chrono::{DateTime, NaiveDate, TimeDelta, Utc};
use serde::{Deserialize, Deserializer, Serialize, de};
use uuid::Uuid;

use super::{catalogue::Page, error::DomainError};

pub use history::{StatsRange, VolumeGrouping};
pub use log::{LogWorkout, LogWorkoutExercise, RepeatLastWorkout};

pub const MAX_SESSION_EXERCISES: usize = 100;
pub const MAX_EXERCISE_SETS: usize = 100;
const MAX_POSITION: u64 = 99;
const TEMP_POSITION_BASE: i64 = 1_000_000;

pub const TIMESTAMP_HINT: &str = "expected YYYY-MM-DD (for example 2026-08-16) or an RFC 3339 timestamp (for example 2026-08-16T07:30:00Z)";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timestamp {
    at: DateTime<Utc>,
    date_only: bool,
}

impl Timestamp {
    pub fn at(value: DateTime<Utc>) -> Self {
        Self {
            at: value,
            date_only: false,
        }
    }

    pub fn parse(value: &str) -> Result<Self, DomainError> {
        let value = value.trim();
        if let Ok(parsed) = DateTime::parse_from_rfc3339(value) {
            return Ok(Self::at(parsed.with_timezone(&Utc)));
        }
        let date = NaiveDate::parse_from_str(value, "%Y-%m-%d")
            .map_err(|_| DomainError::InvalidInput(TIMESTAMP_HINT))?;
        Ok(Self {
            at: date
                .and_hms_opt(0, 0, 0)
                .ok_or(DomainError::InvalidInput(TIMESTAMP_HINT))?
                .and_utc(),
            date_only: true,
        })
    }

    pub fn start(&self) -> DateTime<Utc> {
        self.at
    }

    pub fn end(&self) -> DateTime<Utc> {
        if self.date_only {
            self.at + TimeDelta::days(1) - TimeDelta::nanoseconds(1)
        } else {
            self.at
        }
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(|_| de::Error::custom(TIMESTAMP_HINT))
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateWorkoutSession {
    pub started_at: Timestamp,
    pub label: Option<String>,
    pub activity: CreateActivity,
}

pub type UpdateWorkoutSession = CreateWorkoutSession;

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum CreateActivity {
    Strength,
    Run {
        distance_m: i64,
        duration_sec: i64,
        #[serde(default)]
        elevation_gain_m: i64,
    },
}

#[derive(Clone, Debug, Serialize)]
pub struct WorkoutSessionSummary {
    pub id: Uuid,
    pub started_at: DateTime<Utc>,
    pub label: Option<String>,
    pub activity_type: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorkoutSession {
    pub id: Uuid,
    pub started_at: DateTime<Utc>,
    pub label: Option<String>,
    pub activity: ActivityView,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ActivityView {
    Strength {
        exercises: Vec<SessionExercise>,
    },
    Run {
        distance_m: i64,
        duration_sec: i64,
        elevation_gain_m: i64,
    },
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionFilter {
    pub started_at_from: Option<Timestamp>,
    pub started_at_to: Option<Timestamp>,
    pub activity: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddSessionExercise {
    pub exercise_id: Uuid,
    pub position: Option<u64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateSessionExercise {
    pub exercise_id: Uuid,
    pub position: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct SessionExerciseSummary {
    pub id: Uuid,
    pub session_id: Uuid,
    pub exercise_id: Uuid,
    pub exercise_name: String,
    pub contraction_type: String,
    pub position: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct SessionExercise {
    pub id: Uuid,
    pub session_id: Uuid,
    pub exercise_id: Uuid,
    pub exercise_name: String,
    pub contraction_type: String,
    pub position: u64,
    pub sets: Vec<ExerciseSet>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddExerciseSet {
    pub position: Option<u64>,
    pub set_type: String,
    pub reps: Option<i64>,
    pub hold_sec: Option<i64>,
    #[serde(default)]
    pub load_g: i64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateExerciseSet {
    pub position: u64,
    pub set_type: String,
    pub reps: Option<i64>,
    pub hold_sec: Option<i64>,
    #[serde(default)]
    pub load_g: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct ExerciseSet {
    pub id: Uuid,
    pub session_exercise_id: Uuid,
    pub position: u64,
    pub set_type: String,
    pub reps: Option<i64>,
    pub hold_sec: Option<i64>,
    pub load_g: i64,
}

fn parse_id(value: &str) -> Result<Uuid, DomainError> {
    Uuid::parse_str(value).map_err(|_| DomainError::NotFound)
}

fn mutation_error(error: sea_orm::DbErr) -> DomainError {
    tracing::debug!(error = %error, "workout mutation rejected by database");
    DomainError::Conflict
}

fn make_page<T>(mut items: Vec<T>, offset: u64, limit: u64) -> Page<T> {
    let has_more = items.len() as u64 > limit;
    if has_more {
        items.pop();
    }
    Page {
        items,
        next_offset: has_more.then_some(offset + limit),
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use crate::{
        domain::{
            AuthConfig, Domain, OAuthConfig,
            auth::{Identity, OAuthPrincipal, Principal, PrincipalTransport},
            catalogue::PageRequest,
        },
        migration::Migrator,
    };
    use sea_orm::{ConnectionTrait, Database};
    use sea_orm_migration::MigratorTrait;
    use std::time::Duration;

    pub(crate) fn auth_config() -> AuthConfig {
        AuthConfig {
            session_hmac_key: [3; 32],
            session_key_id: "session".into(),
            password_pepper: b"pepper".to_vec(),
            pepper_key_id: "pepper".into(),
            password_concurrency: 1,
            idle_lifetime: Duration::from_secs(60),
            absolute_lifetime: Duration::from_secs(120),
        }
    }

    pub(crate) fn oauth_config() -> OAuthConfig {
        OAuthConfig {
            hmac_key: [8; 32],
            key_id: "oauth".into(),
        }
    }

    pub(crate) fn principal(user_id: Uuid, role: &str) -> Principal {
        Principal {
            identity: Identity {
                user_id,
                role: role.into(),
                auth_version: 0,
            },
            transport: PrincipalTransport::OAuthAccessToken {
                token_id: Uuid::now_v7(),
                context: OAuthPrincipal {
                    client_id: Uuid::now_v7().to_string(),
                    issuer: "https://frater.example".into(),
                    resource: "https://frater.example/mcp".into(),
                    scope: "workouts:read workouts:write".into(),
                },
            },
        }
    }

    pub(crate) async fn fixture(
        database: sea_orm::DatabaseConnection,
    ) -> (
        Domain,
        sea_orm::DatabaseConnection,
        Principal,
        Principal,
        Principal,
        Uuid,
        Uuid,
    ) {
        let owner_id = Uuid::now_v7();
        let other_id = Uuid::now_v7();
        let super_id = Uuid::now_v7();
        for (id, email, role) in [
            (owner_id, "owner@example.com", "user"),
            (other_id, "other@example.com", "user"),
            (super_id, "root@example.com", "superuser"),
        ] {
            database
                .execute_unprepared(&format!(
                    "INSERT INTO users(id,email_normalized,email_display,role,status,auth_version,created_at,updated_at) VALUES('{id}','{email}','{email}','{role}','active',0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')"
                ))
                .await
                .unwrap();
        }
        let dynamic = Uuid::now_v7();
        let isometric = Uuid::now_v7();
        database
            .execute_unprepared(&format!(
                "INSERT INTO exercises(id,name,contraction_type) VALUES('{dynamic}','Squat','dynamic'),('{isometric}','Plank','isometric')"
            ))
            .await
            .unwrap();
        let domain = Domain::new(database.clone(), auth_config(), oauth_config())
            .await
            .unwrap();
        (
            domain,
            database,
            principal(owner_id, "user"),
            principal(other_id, "user"),
            principal(super_id, "superuser"),
            dynamic,
            isometric,
        )
    }

    pub(crate) async fn memory_domain() -> (
        Domain,
        sea_orm::DatabaseConnection,
        Principal,
        Principal,
        Principal,
        Uuid,
        Uuid,
    ) {
        let database = Database::connect("sqlite::memory:").await.unwrap();
        database
            .execute_unprepared("PRAGMA foreign_keys=ON")
            .await
            .unwrap();
        Migrator::up(&database, None).await.unwrap();
        fixture(database).await
    }

    pub(crate) fn strength() -> CreateWorkoutSession {
        CreateWorkoutSession {
            started_at: Timestamp::parse("2026-01-02T03:04:05Z").unwrap(),
            label: Some("strength".into()),
            activity: CreateActivity::Strength,
        }
    }

    pub(crate) fn run(distance_m: i64) -> CreateWorkoutSession {
        CreateWorkoutSession {
            started_at: Timestamp::parse("2026-01-03T03:04:05Z").unwrap(),
            label: Some("run".into()),
            activity: CreateActivity::Run {
                distance_m,
                duration_sec: 1_800,
                elevation_gain_m: 25,
            },
        }
    }

    pub(crate) fn dynamic_set(position: Option<u64>, load_g: i64) -> AddExerciseSet {
        AddExerciseSet {
            position,
            set_type: "working".into(),
            reps: Some(5),
            hold_sec: None,
            load_g,
        }
    }

    pub(crate) async fn assert_dense_children(
        domain: &Domain,
        owner: &Principal,
        session_id: Uuid,
    ) {
        let page = domain
            .list_session_exercises(
                owner,
                session_id,
                PageRequest {
                    offset: 0,
                    limit: Some(100),
                },
            )
            .await
            .unwrap();
        assert_eq!(
            page.items
                .iter()
                .map(|item| item.position)
                .collect::<Vec<_>>(),
            (0..page.items.len() as u64).collect::<Vec<_>>()
        );
    }

    pub(crate) async fn assert_dense_sets(domain: &Domain, owner: &Principal, parent_id: Uuid) {
        let page = domain
            .list_exercise_sets(
                owner,
                parent_id,
                PageRequest {
                    offset: 0,
                    limit: Some(100),
                },
            )
            .await
            .unwrap();
        assert_eq!(
            page.items
                .iter()
                .map(|item| item.position)
                .collect::<Vec<_>>(),
            (0..page.items.len() as u64).collect::<Vec<_>>()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;
    use crate::domain::catalogue::PageRequest;

    #[tokio::test]
    async fn all_foreign_workout_operations_are_not_found_even_for_superuser() {
        let (domain, _, owner, other, superuser, dynamic, _) = memory_domain().await;
        let session = domain.create_session(&owner, strength()).await.unwrap();
        let child = domain
            .add_session_exercise(
                &owner,
                session.id,
                AddSessionExercise {
                    exercise_id: dynamic,
                    position: None,
                },
            )
            .await
            .unwrap();
        let set = domain
            .add_exercise_set(&owner, child.id, dynamic_set(None, 1))
            .await
            .unwrap();

        for stranger in [&other, &superuser] {
            assert!(matches!(
                domain.get_session(stranger, session.id).await,
                Err(DomainError::NotFound)
            ));
            assert!(matches!(
                domain
                    .update_session(stranger, session.id, strength())
                    .await,
                Err(DomainError::NotFound)
            ));
            assert!(matches!(
                domain.delete_session(stranger, session.id).await,
                Err(DomainError::NotFound)
            ));
            assert!(matches!(
                domain
                    .list_session_exercises(stranger, session.id, PageRequest::default())
                    .await,
                Err(DomainError::NotFound)
            ));
            assert!(matches!(
                domain.get_session_exercise(stranger, child.id).await,
                Err(DomainError::NotFound)
            ));
            assert!(matches!(
                domain
                    .update_session_exercise(
                        stranger,
                        child.id,
                        UpdateSessionExercise {
                            exercise_id: dynamic,
                            position: 0
                        }
                    )
                    .await,
                Err(DomainError::NotFound)
            ));
            assert!(matches!(
                domain.remove_session_exercise(stranger, child.id).await,
                Err(DomainError::NotFound)
            ));
            assert!(matches!(
                domain
                    .list_exercise_sets(stranger, child.id, PageRequest::default())
                    .await,
                Err(DomainError::NotFound)
            ));
            assert!(matches!(
                domain.get_exercise_set(stranger, set.id).await,
                Err(DomainError::NotFound)
            ));
            assert!(matches!(
                domain
                    .update_exercise_set(
                        stranger,
                        set.id,
                        UpdateExerciseSet {
                            position: 0,
                            set_type: "working".into(),
                            reps: Some(5),
                            hold_sec: None,
                            load_g: 2
                        }
                    )
                    .await,
                Err(DomainError::NotFound)
            ));
            assert!(matches!(
                domain.remove_exercise_set(stranger, set.id).await,
                Err(DomainError::NotFound)
            ));
        }
        assert_eq!(
            domain
                .list_sessions(&other, SessionFilter::default(), PageRequest::default())
                .await
                .unwrap()
                .items
                .len(),
            0
        );
        assert!(domain.get_session(&owner, session.id).await.is_ok());
        assert!(domain.get_exercise_set(&owner, set.id).await.is_ok());
    }
}
