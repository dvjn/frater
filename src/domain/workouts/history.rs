use std::collections::HashMap;

use chrono::{DateTime, Utc};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{Timestamp, parse_id};
use crate::domain::{
    Domain,
    auth::Principal,
    entity::{exercise_muscles, exercise_sets, exercises, muscles, session_exercises, sessions},
    error::DomainError,
};

pub const MAX_HISTORY_SESSIONS: u64 = 365;
const ID_CHUNK: usize = 200;

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatsRange {
    pub from: Option<Timestamp>,
    pub to: Option<Timestamp>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VolumeGrouping {
    Exercise,
    Muscle,
}

#[derive(Clone, Debug, Serialize)]
pub struct SessionHistoryEntry {
    pub session_id: Uuid,
    pub started_at: DateTime<Utc>,
    pub label: Option<String>,
    pub activity_type: String,
    pub exercise_count: u64,
    pub set_count: u64,
    pub volume_g: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct ExerciseHistorySet {
    pub position: u64,
    pub set_type: String,
    pub reps: Option<i64>,
    pub hold_sec: Option<i64>,
    pub load_g: i64,
    pub estimated_1rm_g: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ExerciseHistoryEntry {
    pub session_id: Uuid,
    pub session_exercise_id: Uuid,
    pub started_at: DateTime<Utc>,
    pub label: Option<String>,
    pub volume_g: i64,
    pub sets: Vec<ExerciseHistorySet>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PersonalRecord {
    pub exercise_id: Uuid,
    pub exercise_name: String,
    pub max_load_g: Option<i64>,
    pub max_load_reps: Option<i64>,
    pub best_estimated_1rm_g: Option<i64>,
    pub best_estimated_1rm_load_g: Option<i64>,
    pub best_estimated_1rm_reps: Option<i64>,
    pub longest_hold_sec: Option<i64>,
    pub set_count: u64,
    pub last_performed_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize)]
pub struct VolumeEntry {
    pub id: Uuid,
    pub name: String,
    pub set_count: u64,
    pub rep_count: i64,
    pub volume_g: i64,
}

fn estimated_1rm_g(load_g: i64, reps: Option<i64>) -> Option<i64> {
    let reps = reps?;
    if load_g <= 0 || reps <= 0 {
        return None;
    }
    Some(load_g.saturating_mul(30 + reps) / 30)
}

fn set_volume_g(model: &exercise_sets::Model) -> i64 {
    model
        .reps
        .filter(|reps| *reps > 0)
        .map_or(0, |reps| model.load_g.saturating_mul(reps))
}

fn started_at(model: &sessions::Model) -> Result<DateTime<Utc>, DomainError> {
    Ok(DateTime::parse_from_rfc3339(&model.started_at)
        .map_err(|_| DomainError::NotFound)?
        .with_timezone(&Utc))
}

struct HistoryExercise {
    model: session_exercises::Model,
    sets: Vec<exercise_sets::Model>,
}

struct HistorySession {
    model: sessions::Model,
    exercises: Vec<HistoryExercise>,
}

impl Domain {
    async fn load_history(
        &self,
        principal: &Principal,
        range: StatsRange,
        exercise_id: Option<Uuid>,
        limit: u64,
    ) -> Result<Vec<HistorySession>, DomainError> {
        if range
            .from
            .zip(range.to)
            .is_some_and(|(from, to)| from.start() > to.end())
        {
            return Err(DomainError::InvalidInput("from must not be later than to"));
        }
        if !(1..=MAX_HISTORY_SESSIONS).contains(&limit) {
            return Err(DomainError::InvalidInput(
                "limit must be between 1 and 365 sessions",
            ));
        }
        let user_id = principal.user_id().to_string();
        let mut select =
            sessions::Entity::find().filter(sessions::Column::UserId.eq(user_id.clone()));
        if let Some(from) = range.from {
            select = select.filter(sessions::Column::StartedAt.gte(from.start().to_rfc3339()));
        }
        if let Some(to) = range.to {
            select = select.filter(sessions::Column::StartedAt.lte(to.end().to_rfc3339()));
        }
        let session_models = select
            .order_by_desc(sessions::Column::StartedAt)
            .order_by_desc(sessions::Column::Id)
            .limit(limit)
            .all(&self.db)
            .await?;
        if session_models.is_empty() {
            return Ok(Vec::new());
        }

        let session_ids = session_models
            .iter()
            .map(|model| model.id.clone())
            .collect::<Vec<_>>();
        let mut exercise_models = Vec::new();
        for chunk in session_ids.chunks(ID_CHUNK) {
            let mut select = session_exercises::Entity::find()
                .filter(session_exercises::Column::UserId.eq(user_id.clone()))
                .filter(session_exercises::Column::SessionId.is_in(chunk.to_vec()));
            if let Some(exercise_id) = exercise_id {
                select = select
                    .filter(session_exercises::Column::ExerciseId.eq(exercise_id.to_string()));
            }
            exercise_models.extend(
                select
                    .order_by_asc(session_exercises::Column::Position)
                    .order_by_asc(session_exercises::Column::Id)
                    .all(&self.db)
                    .await?,
            );
        }

        let exercise_ids = exercise_models
            .iter()
            .map(|model| model.id.clone())
            .collect::<Vec<_>>();
        let mut sets_by_exercise: HashMap<String, Vec<exercise_sets::Model>> = HashMap::new();
        for chunk in exercise_ids.chunks(ID_CHUNK) {
            let models = exercise_sets::Entity::find()
                .filter(exercise_sets::Column::UserId.eq(user_id.clone()))
                .filter(exercise_sets::Column::SessionExerciseId.is_in(chunk.to_vec()))
                .order_by_asc(exercise_sets::Column::Position)
                .order_by_asc(exercise_sets::Column::Id)
                .all(&self.db)
                .await?;
            for model in models {
                sets_by_exercise
                    .entry(model.session_exercise_id.clone())
                    .or_default()
                    .push(model);
            }
        }

        let mut exercises_by_session: HashMap<String, Vec<HistoryExercise>> = HashMap::new();
        for model in exercise_models {
            let sets = sets_by_exercise.remove(&model.id).unwrap_or_default();
            exercises_by_session
                .entry(model.session_id.clone())
                .or_default()
                .push(HistoryExercise { model, sets });
        }

        Ok(session_models
            .into_iter()
            .filter_map(|model| {
                let exercises = exercises_by_session.remove(&model.id).unwrap_or_default();
                if exercise_id.is_some() && exercises.is_empty() {
                    return None;
                }
                Some(HistorySession { model, exercises })
            })
            .collect())
    }

    pub async fn session_history(
        &self,
        principal: &Principal,
        range: StatsRange,
        limit: Option<u64>,
    ) -> Result<Vec<SessionHistoryEntry>, DomainError> {
        let sessions = self
            .load_history(principal, range, None, limit.unwrap_or(50))
            .await?;
        sessions
            .into_iter()
            .map(|session| {
                Ok(SessionHistoryEntry {
                    session_id: parse_id(&session.model.id)?,
                    started_at: started_at(&session.model)?,
                    label: session.model.label.clone(),
                    activity_type: session.model.activity_type.clone(),
                    exercise_count: session.exercises.len() as u64,
                    set_count: session
                        .exercises
                        .iter()
                        .map(|exercise| exercise.sets.len() as u64)
                        .sum(),
                    volume_g: session
                        .exercises
                        .iter()
                        .flat_map(|exercise| exercise.sets.iter())
                        .map(set_volume_g)
                        .sum(),
                })
            })
            .collect()
    }

    pub async fn exercise_history(
        &self,
        principal: &Principal,
        exercise_id: Uuid,
        range: StatsRange,
        limit: Option<u64>,
    ) -> Result<Vec<ExerciseHistoryEntry>, DomainError> {
        let sessions = self
            .load_history(principal, range, Some(exercise_id), limit.unwrap_or(20))
            .await?;
        let mut entries = Vec::new();
        for session in sessions {
            let started_at = started_at(&session.model)?;
            for exercise in session.exercises {
                entries.push(ExerciseHistoryEntry {
                    session_id: parse_id(&session.model.id)?,
                    session_exercise_id: parse_id(&exercise.model.id)?,
                    started_at,
                    label: session.model.label.clone(),
                    volume_g: exercise.sets.iter().map(set_volume_g).sum(),
                    sets: exercise
                        .sets
                        .iter()
                        .map(|set| {
                            Ok(ExerciseHistorySet {
                                position: u64::try_from(set.position)
                                    .map_err(|_| DomainError::NotFound)?,
                                set_type: set.set_type.clone(),
                                reps: set.reps,
                                hold_sec: set.hold_sec,
                                load_g: set.load_g,
                                estimated_1rm_g: estimated_1rm_g(set.load_g, set.reps),
                            })
                        })
                        .collect::<Result<Vec<_>, DomainError>>()?,
                });
            }
        }
        Ok(entries)
    }

    pub async fn personal_records(
        &self,
        principal: &Principal,
        exercise_id: Option<Uuid>,
        range: StatsRange,
    ) -> Result<Vec<PersonalRecord>, DomainError> {
        let sessions = self
            .load_history(principal, range, exercise_id, MAX_HISTORY_SESSIONS)
            .await?;
        let mut records: HashMap<String, PersonalRecord> = HashMap::new();
        for session in sessions {
            let started_at = started_at(&session.model)?;
            for exercise in session.exercises {
                let key = exercise.model.exercise_id.clone();
                let record = match records.get_mut(&key) {
                    Some(record) => record,
                    None => {
                        let name = exercises::Entity::find_by_id(key.clone())
                            .one(&self.db)
                            .await?
                            .ok_or(DomainError::NotFound)?
                            .name;
                        records.entry(key.clone()).or_insert(PersonalRecord {
                            exercise_id: parse_id(&key)?,
                            exercise_name: name,
                            max_load_g: None,
                            max_load_reps: None,
                            best_estimated_1rm_g: None,
                            best_estimated_1rm_load_g: None,
                            best_estimated_1rm_reps: None,
                            longest_hold_sec: None,
                            set_count: 0,
                            last_performed_at: started_at,
                        })
                    }
                };
                record.set_count += exercise.sets.len() as u64;
                if started_at > record.last_performed_at {
                    record.last_performed_at = started_at;
                }
                for set in &exercise.sets {
                    if set.reps.is_some_and(|reps| reps > 0)
                        && record.max_load_g.is_none_or(|best| set.load_g > best)
                    {
                        record.max_load_g = Some(set.load_g);
                        record.max_load_reps = set.reps;
                    }
                    if let Some(estimate) = estimated_1rm_g(set.load_g, set.reps)
                        && record
                            .best_estimated_1rm_g
                            .is_none_or(|best| estimate > best)
                    {
                        record.best_estimated_1rm_g = Some(estimate);
                        record.best_estimated_1rm_load_g = Some(set.load_g);
                        record.best_estimated_1rm_reps = set.reps;
                    }
                    if let Some(hold_sec) = set.hold_sec
                        && record.longest_hold_sec.is_none_or(|best| hold_sec > best)
                    {
                        record.longest_hold_sec = Some(hold_sec);
                    }
                }
            }
        }
        let mut records = records.into_values().collect::<Vec<_>>();
        records.sort_by(|left, right| left.exercise_name.cmp(&right.exercise_name));
        Ok(records)
    }

    pub async fn volume_stats(
        &self,
        principal: &Principal,
        grouping: VolumeGrouping,
        range: StatsRange,
    ) -> Result<Vec<VolumeEntry>, DomainError> {
        let sessions = self
            .load_history(principal, range, None, MAX_HISTORY_SESSIONS)
            .await?;
        let mut totals: HashMap<String, (u64, i64, i64)> = HashMap::new();
        for session in sessions {
            for exercise in session.exercises {
                let set_count = exercise.sets.len() as u64;
                let rep_count: i64 = exercise
                    .sets
                    .iter()
                    .map(|set| set.reps.unwrap_or(0).max(0))
                    .sum();
                let volume_g: i64 = exercise.sets.iter().map(set_volume_g).sum();
                let keys = match grouping {
                    VolumeGrouping::Exercise => vec![exercise.model.exercise_id.clone()],
                    VolumeGrouping::Muscle => exercise_muscles::Entity::find()
                        .filter(
                            exercise_muscles::Column::ExerciseId
                                .eq(exercise.model.exercise_id.clone()),
                        )
                        .filter(exercise_muscles::Column::Role.eq("primary"))
                        .all(&self.db)
                        .await?
                        .into_iter()
                        .map(|link| link.muscle_id)
                        .collect(),
                };
                for key in keys {
                    let entry = totals.entry(key).or_insert((0, 0, 0));
                    entry.0 += set_count;
                    entry.1 += rep_count;
                    entry.2 += volume_g;
                }
            }
        }

        let mut entries = Vec::with_capacity(totals.len());
        for (key, (set_count, rep_count, volume_g)) in totals {
            let name = match grouping {
                VolumeGrouping::Exercise => {
                    exercises::Entity::find_by_id(key.clone())
                        .one(&self.db)
                        .await?
                        .ok_or(DomainError::NotFound)?
                        .name
                }
                VolumeGrouping::Muscle => {
                    muscles::Entity::find_by_id(key.clone())
                        .one(&self.db)
                        .await?
                        .ok_or(DomainError::NotFound)?
                        .name
                }
            };
            entries.push(VolumeEntry {
                id: parse_id(&key)?,
                name,
                set_count,
                rep_count,
                volume_g,
            });
        }
        entries.sort_by(|left, right| {
            right
                .volume_g
                .cmp(&left.volume_g)
                .then_with(|| left.name.cmp(&right.name))
        });
        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use crate::domain::{
        ExerciseInput, NamedInput,
        catalogue::ExerciseMuscleInput,
        workouts::{AddExerciseSet, LogWorkout, LogWorkoutExercise},
    };

    fn working(reps: i64, load_g: i64) -> AddExerciseSet {
        AddExerciseSet {
            position: None,
            set_type: "working".into(),
            reps: Some(reps),
            hold_sec: None,
            load_g,
        }
    }

    fn range(from: &str, to: &str) -> StatsRange {
        StatsRange {
            from: Some(Timestamp::parse(from).unwrap()),
            to: Some(Timestamp::parse(to).unwrap()),
        }
    }

    async fn logged() -> (crate::domain::Domain, crate::domain::Principal, Uuid) {
        let (domain, _, owner, _, _, dynamic_id, _) = memory_domain().await;
        for (day, load) in [("2026-03-01", 60_000), ("2026-03-08", 70_000)] {
            domain
                .log_workout(
                    &owner,
                    LogWorkout {
                        started_at: Timestamp::parse(day).unwrap(),
                        label: Some("Leg day".into()),
                        exercises: vec![LogWorkoutExercise {
                            exercise_id: dynamic_id,
                            sets: vec![working(5, load), working(5, load)],
                        }],
                    },
                )
                .await
                .unwrap();
        }
        (domain, owner, dynamic_id)
    }

    #[tokio::test]
    async fn session_history_totals_and_date_range_are_inclusive() {
        let (domain, owner, _) = logged().await;
        let all = domain
            .session_history(&owner, StatsRange::default(), None)
            .await
            .unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].set_count, 2);
        assert_eq!(all[0].volume_g, 700_000);

        let first_only = domain
            .session_history(&owner, range("2026-03-01", "2026-03-01"), None)
            .await
            .unwrap();
        assert_eq!(first_only.len(), 1);
        assert_eq!(first_only[0].volume_g, 600_000);

        assert!(matches!(
            domain
                .session_history(&owner, range("2026-03-08", "2026-03-01"), None)
                .await,
            Err(DomainError::InvalidInput("from must not be later than to"))
        ));
        assert!(matches!(
            domain
                .session_history(&owner, StatsRange::default(), Some(0))
                .await,
            Err(DomainError::InvalidInput(_))
        ));
    }

    #[tokio::test]
    async fn exercise_history_returns_sets_newest_first() {
        let (domain, owner, exercise_id) = logged().await;
        let history = domain
            .exercise_history(&owner, exercise_id, StatsRange::default(), None)
            .await
            .unwrap();
        assert_eq!(history.len(), 2);
        assert!(history[0].started_at > history[1].started_at);
        assert_eq!(history[0].sets.len(), 2);
        assert_eq!(history[0].sets[0].load_g, 70_000);
        assert_eq!(history[0].sets[0].estimated_1rm_g, Some(81_666));

        let unknown = domain
            .exercise_history(&owner, Uuid::now_v7(), StatsRange::default(), None)
            .await
            .unwrap();
        assert!(unknown.is_empty());
    }

    #[tokio::test]
    async fn personal_records_track_max_load_and_best_estimate() {
        let (domain, owner, exercise_id) = logged().await;
        let records = domain
            .personal_records(&owner, None, StatsRange::default())
            .await
            .unwrap();
        assert_eq!(records.len(), 1);
        let record = &records[0];
        assert_eq!(record.exercise_id, exercise_id);
        assert_eq!(record.max_load_g, Some(70_000));
        assert_eq!(record.max_load_reps, Some(5));
        assert_eq!(record.best_estimated_1rm_g, Some(81_666));
        assert_eq!(record.set_count, 4);

        let early = domain
            .personal_records(&owner, None, range("2026-03-01", "2026-03-02"))
            .await
            .unwrap();
        assert_eq!(early[0].max_load_g, Some(60_000));
    }

    #[tokio::test]
    async fn volume_stats_group_by_exercise_and_primary_muscle() {
        let (domain, _, owner, _, superuser, _, _) = memory_domain().await;
        let muscle = domain
            .create_muscle(
                &superuser,
                NamedInput {
                    name: "Quads".into(),
                },
            )
            .await
            .unwrap();
        let exercise = domain
            .create_exercise(
                &superuser,
                ExerciseInput {
                    name: "Front squat".into(),
                    contraction_type: "dynamic".into(),
                    muscles: vec![ExerciseMuscleInput {
                        muscle_id: muscle.id,
                        role: "primary".into(),
                    }],
                    equipment_ids: vec![],
                },
            )
            .await
            .unwrap();
        domain
            .log_workout(
                &owner,
                LogWorkout {
                    started_at: Timestamp::parse("2026-03-01").unwrap(),
                    label: None,
                    exercises: vec![LogWorkoutExercise {
                        exercise_id: exercise.id,
                        sets: vec![working(10, 50_000)],
                    }],
                },
            )
            .await
            .unwrap();

        let by_exercise = domain
            .volume_stats(&owner, VolumeGrouping::Exercise, StatsRange::default())
            .await
            .unwrap();
        assert_eq!(by_exercise.len(), 1);
        assert_eq!(by_exercise[0].name, "Front squat");
        assert_eq!(by_exercise[0].rep_count, 10);
        assert_eq!(by_exercise[0].volume_g, 500_000);

        let by_muscle = domain
            .volume_stats(&owner, VolumeGrouping::Muscle, StatsRange::default())
            .await
            .unwrap();
        assert_eq!(by_muscle.len(), 1);
        assert_eq!(by_muscle[0].id, muscle.id);
        assert_eq!(by_muscle[0].volume_g, 500_000);
    }
}
