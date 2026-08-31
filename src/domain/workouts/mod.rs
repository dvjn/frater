mod dashboard;
mod exercises;
mod history;
mod log;
mod sessions;
mod sets;

use chrono::{DateTime, NaiveDate, TimeDelta, Utc};
use serde::{Deserialize, Deserializer, Serialize, de};
use uuid::Uuid;

use super::{catalogue::Page, error::DomainError};

pub use dashboard::{Dashboard, RunListEntry, RunRecord};
pub use history::{PersonalRecord, StatsRange, WorkoutListEntry};
pub use log::{LogWorkout, LogWorkoutExercise, ReplaceRun};

pub const MAX_SESSION_EXERCISES: usize = 100;
pub const MAX_EXERCISE_SETS: usize = 100;
pub const MAX_RUN_SPLITS: usize = 100;
pub const MAX_NOTES: usize = 1_000;

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

    pub fn is_date_only(&self) -> bool {
        self.date_only
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
    #[serde(default)]
    pub notes: Option<String>,
    pub activity: CreateActivity,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum CreateActivity {
    Strength,
    Run {
        #[serde(default)]
        distance_m: Option<i64>,
        #[serde(default)]
        duration_sec: Option<i64>,
        #[serde(default)]
        elevation_gain_m: i64,
        #[serde(default)]
        splits: Vec<RunSplit>,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RunSplit {
    pub distance_m: i64,
    pub duration_sec: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorkoutSessionSummary {
    pub id: Uuid,
    pub started_at: DateTime<Utc>,
    pub label: Option<String>,
    pub notes: Option<String>,
    pub activity_type: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorkoutSession {
    pub id: Uuid,
    pub started_at: DateTime<Utc>,
    pub label: Option<String>,
    pub notes: Option<String>,
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
        splits: Vec<RunSplit>,
    },
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionFilter {
    pub started_at_from: Option<Timestamp>,
    pub started_at_to: Option<Timestamp>,
    pub activity: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SessionExercise {
    pub id: Uuid,
    pub session_id: Uuid,
    pub exercise_id: Uuid,
    pub exercise_name: String,
    pub contraction_type: String,
    pub position: u64,
    pub notes: Option<String>,
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
    #[serde(default)]
    pub notes: Option<String>,
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
    pub notes: Option<String>,
}

fn validate_notes(notes: Option<&String>) -> Result<(), DomainError> {
    if notes.is_some_and(|value| value.chars().count() > MAX_NOTES) {
        return Err(DomainError::InvalidInput(
            "notes must be at most 1000 characters",
        ));
    }
    Ok(())
}

fn resolve_run_splits(
    splits: &[RunSplit],
    distance_m: Option<i64>,
    duration_sec: Option<i64>,
    elevation_gain_m: i64,
) -> Result<Vec<RunSplit>, DomainError> {
    if distance_m.is_some_and(|value| value <= 0)
        || duration_sec.is_some_and(|value| value <= 0)
        || elevation_gain_m < 0
    {
        return Err(DomainError::InvalidInput("invalid run details"));
    }
    if splits.len() > MAX_RUN_SPLITS {
        return Err(DomainError::InvalidInput("run split limit reached"));
    }
    if splits
        .iter()
        .any(|split| split.distance_m <= 0 || split.duration_sec <= 0)
    {
        return Err(DomainError::InvalidInput(
            "each run split needs a positive distance_m and duration_sec",
        ));
    }
    if splits.is_empty() {
        let (Some(distance_m), Some(duration_sec)) = (distance_m, duration_sec) else {
            return Err(DomainError::InvalidInput(
                "a run needs a distance_m and a duration_sec, as its splits or as its totals",
            ));
        };
        return Ok(vec![RunSplit {
            distance_m,
            duration_sec,
        }]);
    }
    let (distance_sum, duration_sum) = run_totals(splits);
    if distance_m.is_some_and(|value| value != distance_sum) {
        return Err(DomainError::InvalidInput(
            "the run splits must sum to the distance_m of the run; cover any laps you do not know as one remainder split",
        ));
    }
    if duration_sec.is_some_and(|value| value != duration_sum) {
        return Err(DomainError::InvalidInput(
            "the run splits must sum to the duration_sec of the run; cover any laps you do not know as one remainder split",
        ));
    }
    Ok(splits.to_vec())
}

pub(crate) fn run_totals(splits: &[RunSplit]) -> (i64, i64) {
    splits
        .iter()
        .fold((0i64, 0i64), |(distance, duration), split| {
            (
                distance.saturating_add(split.distance_m),
                duration.saturating_add(split.duration_sec),
            )
        })
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

    pub(crate) fn run(distance_m: i64) -> CreateWorkoutSession {
        CreateWorkoutSession {
            started_at: Timestamp::parse("2026-01-03T03:04:05Z").unwrap(),
            label: Some("run".into()),
            notes: None,
            activity: CreateActivity::Run {
                distance_m: Some(distance_m),
                duration_sec: Some(1_800),
                elevation_gain_m: 25,
                splits: Vec::new(),
            },
        }
    }

    pub(crate) fn run_from(
        distance_m: Option<i64>,
        duration_sec: Option<i64>,
        splits: Vec<RunSplit>,
    ) -> CreateWorkoutSession {
        CreateWorkoutSession {
            activity: CreateActivity::Run {
                distance_m,
                duration_sec,
                elevation_gain_m: 25,
                splits,
            },
            ..run(5_000)
        }
    }

    pub(crate) fn run_with_splits(splits: Vec<RunSplit>) -> CreateWorkoutSession {
        run_from(Some(5_000), Some(1_500), splits)
    }

    pub(crate) fn totals_of(session: &WorkoutSession) -> (i64, i64) {
        match &session.activity {
            ActivityView::Run {
                distance_m,
                duration_sec,
                ..
            } => (*distance_m, *duration_sec),
            ActivityView::Strength { .. } => panic!("expected a run session"),
        }
    }

    pub(crate) fn split(distance_m: i64, duration_sec: i64) -> RunSplit {
        RunSplit {
            distance_m,
            duration_sec,
        }
    }

    pub(crate) fn splits_of(session: &WorkoutSession) -> Vec<(i64, i64)> {
        match &session.activity {
            ActivityView::Run { splits, .. } => splits
                .iter()
                .map(|split| (split.distance_m, split.duration_sec))
                .collect(),
            ActivityView::Strength { .. } => panic!("expected a run session"),
        }
    }

    pub(crate) async fn log_one_set(
        domain: &Domain,
        owner: &Principal,
        exercise_id: Uuid,
        load_g: i64,
    ) -> WorkoutSession {
        domain
            .log_workout(
                owner,
                LogWorkout {
                    started_at: Timestamp::parse("2026-01-02T03:04:05Z").unwrap(),
                    label: Some("strength".into()),
                    notes: None,
                    exercises: vec![LogWorkoutExercise {
                        exercise_id,
                        notes: None,
                        sets: vec![AddExerciseSet {
                            position: None,
                            set_type: "working".into(),
                            reps: Some(5),
                            hold_sec: None,
                            load_g,
                            notes: None,
                        }],
                    }],
                },
            )
            .await
            .unwrap()
    }

    /// Positions must stay dense from zero, for the exercises of a session and
    /// for the sets of each of them.
    pub(crate) async fn assert_dense_children(
        domain: &Domain,
        owner: &Principal,
        session_id: Uuid,
    ) {
        let dense = |positions: Vec<u64>| {
            assert_eq!(positions, (0..positions.len() as u64).collect::<Vec<_>>());
        };
        let ActivityView::Strength { exercises } = domain
            .get_session(owner, session_id)
            .await
            .unwrap()
            .activity
        else {
            panic!("expected a strength session");
        };
        dense(exercises.iter().map(|exercise| exercise.position).collect());
        for exercise in &exercises {
            dense(exercise.sets.iter().map(|set| set.position).collect());
        }
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
        let session = log_one_set(&domain, &owner, dynamic, 1).await;
        let replacement = || LogWorkout {
            started_at: Timestamp::parse("2026-01-02T03:04:05Z").unwrap(),
            label: None,
            notes: None,
            exercises: vec![LogWorkoutExercise {
                exercise_id: dynamic,
                notes: None,
                sets: vec![],
            }],
        };

        for stranger in [&other, &superuser] {
            assert!(matches!(
                domain.get_session(stranger, session.id).await,
                Err(DomainError::NotFound)
            ));
            assert!(matches!(
                domain.delete_session(stranger, session.id).await,
                Err(DomainError::NotFound)
            ));
            assert!(matches!(
                domain
                    .replace_workout(stranger, session.id, replacement())
                    .await,
                Err(DomainError::NotFound)
            ));
            assert!(matches!(
                domain
                    .replace_run(
                        stranger,
                        session.id,
                        ReplaceRun {
                            started_at: Timestamp::parse("2026-01-02T03:04:05Z").unwrap(),
                            label: None,
                            notes: None,
                            distance_m: Some(5_000),
                            duration_sec: Some(1_800),
                            elevation_gain_m: 0,
                            splits: Vec::new(),
                        }
                    )
                    .await,
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
    }
}
