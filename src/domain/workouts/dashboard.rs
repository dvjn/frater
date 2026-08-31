use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};

use super::history::{
    HistorySession, MAX_HISTORY_SESSIONS, PersonalRecord, StatsRange, WorkoutListEntry, started_at,
    total_volume_g,
};
use super::parse_id;
use crate::domain::{
    Domain,
    auth::Principal,
    entity::{run_splits, sessions},
    error::DomainError,
};

pub const DASHBOARD_LOG_LIMIT: usize = 20;
pub const RECORD_DISTANCES_M: [i64; 5] = [1_000, 5_000, 10_000, 21_098, 42_195];

const ID_CHUNK: usize = 200;

#[derive(Clone, Debug, Default)]
pub struct DashboardStats {
    pub strength_count: u64,
    pub run_count: u64,
    pub volume_g: i64,
    pub exercise_count: u64,
    pub distance_m: i64,
    pub duration_sec: i64,
}

#[derive(Clone, Debug)]
pub struct RunListEntry {
    pub started_at: DateTime<Utc>,
    pub label: Option<String>,
    pub distance_m: i64,
    pub duration_sec: i64,
}

#[derive(Clone, Debug)]
pub struct RunRecord {
    pub distance_m: i64,
    pub duration_sec: i64,
    pub started_at: DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub struct Dashboard {
    pub stats: DashboardStats,
    pub exercise_records: Vec<PersonalRecord>,
    pub run_records: Vec<RunRecord>,
    pub workouts: Vec<WorkoutListEntry>,
    pub runs: Vec<RunListEntry>,
}

fn even_pace_estimate_sec(duration_sec: i64, distance_m: i64, record_distance_m: i64) -> i64 {
    let scaled =
        i128::from(duration_sec) * i128::from(record_distance_m) + i128::from(distance_m) / 2;
    i64::try_from(scaled / i128::from(distance_m)).unwrap_or(i64::MAX)
}

impl Domain {
    pub async fn dashboard(
        &self,
        principal: &Principal,
        range: StatsRange,
    ) -> Result<Dashboard, DomainError> {
        let sessions = self
            .load_history(principal, range, None, MAX_HISTORY_SESSIONS)
            .await?;
        let mut stats = DashboardStats::default();
        let mut workouts = Vec::new();
        let mut run_sessions = Vec::new();
        let mut exercise_ids = HashSet::new();
        for session in &sessions {
            let started_at = started_at(&session.model)?;
            if session.model.activity_type == "run" {
                run_sessions.push((session.model.id.clone(), started_at, &session.model.label));
                continue;
            }
            stats.strength_count += 1;
            stats.volume_g = stats
                .volume_g
                .saturating_add(total_volume_g(session.loaded_sets()));
            for exercise in &session.exercises {
                if !exercise.sets.is_empty() {
                    exercise_ids.insert(exercise.model.exercise_id.clone());
                }
            }
            if workouts.len() < DASHBOARD_LOG_LIMIT {
                workouts.push(workout_entry(session, started_at)?);
            }
        }
        stats.exercise_count = exercise_ids.len() as u64;

        let user_id = principal.user_id().to_string();
        let ids = run_sessions
            .iter()
            .map(|(id, _, _)| id.clone())
            .collect::<Vec<_>>();
        let totals = self.run_totals_by_session(&user_id, &ids).await?;
        let mut runs = Vec::new();
        for (id, started_at, label) in run_sessions {
            let (distance_m, duration_sec) = totals.get(&id).copied().unwrap_or((0, 0));
            stats.run_count += 1;
            stats.distance_m = stats.distance_m.saturating_add(distance_m);
            stats.duration_sec = stats.duration_sec.saturating_add(duration_sec);
            if runs.len() < DASHBOARD_LOG_LIMIT {
                runs.push(RunListEntry {
                    started_at,
                    label: label.clone(),
                    distance_m,
                    duration_sec,
                });
            }
        }

        let exercise_records = self
            .personal_records(principal, None, StatsRange::default())
            .await?;
        let run_records = self.run_records(principal).await?;
        Ok(Dashboard {
            stats,
            exercise_records,
            run_records,
            workouts,
            runs,
        })
    }

    pub async fn run_records(&self, principal: &Principal) -> Result<Vec<RunRecord>, DomainError> {
        let user_id = principal.user_id().to_string();
        let session_models = sessions::Entity::find()
            .filter(sessions::Column::UserId.eq(user_id.clone()))
            .filter(sessions::Column::ActivityType.eq("run"))
            .order_by_desc(sessions::Column::StartedAt)
            .order_by_desc(sessions::Column::Id)
            .limit(MAX_HISTORY_SESSIONS)
            .all(&self.db)
            .await?;
        let ids = session_models
            .iter()
            .map(|model| model.id.clone())
            .collect::<Vec<_>>();
        let totals = self.run_totals_by_session(&user_id, &ids).await?;
        let mut records: HashMap<i64, RunRecord> = HashMap::new();
        for model in &session_models {
            let Some(&(distance_m, duration_sec)) = totals.get(&model.id) else {
                continue;
            };
            if distance_m <= 0 || duration_sec <= 0 {
                continue;
            }
            let run_started_at = started_at(model)?;
            for record_distance_m in RECORD_DISTANCES_M {
                if distance_m < record_distance_m {
                    continue;
                }
                let estimate = even_pace_estimate_sec(duration_sec, distance_m, record_distance_m);
                let record = records.entry(record_distance_m).or_insert(RunRecord {
                    distance_m: record_distance_m,
                    duration_sec: estimate,
                    started_at: run_started_at,
                });
                if estimate < record.duration_sec {
                    record.duration_sec = estimate;
                    record.started_at = run_started_at;
                }
            }
        }
        let mut records = records.into_values().collect::<Vec<_>>();
        records.sort_by_key(|record| record.distance_m);
        Ok(records)
    }

    async fn run_totals_by_session(
        &self,
        user_id: &str,
        session_ids: &[String],
    ) -> Result<HashMap<String, (i64, i64)>, DomainError> {
        let mut totals: HashMap<String, (i64, i64)> = HashMap::new();
        for chunk in session_ids.chunks(ID_CHUNK) {
            let models = run_splits::Entity::find()
                .filter(run_splits::Column::UserId.eq(user_id.to_owned()))
                .filter(run_splits::Column::SessionId.is_in(chunk.to_vec()))
                .all(&self.db)
                .await?;
            for model in models {
                let entry = totals.entry(model.session_id).or_insert((0, 0));
                entry.0 = entry.0.saturating_add(model.distance_m);
                entry.1 = entry.1.saturating_add(model.duration_sec);
            }
        }
        Ok(totals)
    }
}

fn workout_entry(
    session: &HistorySession,
    started_at: DateTime<Utc>,
) -> Result<WorkoutListEntry, DomainError> {
    Ok(WorkoutListEntry {
        id: parse_id(&session.model.id)?,
        started_at,
        label: session.model.label.clone(),
        notes: session.model.notes.clone(),
        activity_type: session.model.activity_type.clone(),
        exercise_count: session.exercises.len() as u64,
        set_count: session
            .exercises
            .iter()
            .map(|exercise| exercise.sets.len() as u64)
            .sum(),
        volume_g: total_volume_g(session.loaded_sets()),
    })
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use crate::domain::workouts::{CreateActivity, CreateWorkoutSession, RunSplit, Timestamp};

    fn run_on(day: &str, distance_m: i64, duration_sec: i64) -> CreateWorkoutSession {
        CreateWorkoutSession {
            started_at: Timestamp::parse(day).unwrap(),
            label: Some("Run".into()),
            notes: None,
            activity: CreateActivity::Run {
                distance_m: Some(distance_m),
                duration_sec: Some(duration_sec),
                elevation_gain_m: 0,
                splits: Vec::new(),
            },
        }
    }

    #[tokio::test]
    async fn run_records_pick_the_fastest_pace_per_standard_distance() {
        let (domain, _, owner, _, _, _, _) = memory_domain().await;
        domain
            .create_session(&owner, run_on("2026-02-01", 10_000, 3_000))
            .await
            .unwrap();
        domain
            .create_session(&owner, run_on("2026-02-08", 5_000, 1_200))
            .await
            .unwrap();
        let records = domain.run_records(&owner).await.unwrap();
        let by_distance = records
            .iter()
            .map(|record| (record.distance_m, record.duration_sec))
            .collect::<Vec<_>>();
        assert_eq!(
            by_distance,
            vec![(1_000, 240), (5_000, 1_200), (10_000, 3_000)]
        );
        assert_eq!(
            records[0].started_at,
            Timestamp::parse("2026-02-08").unwrap().start()
        );
        assert_eq!(
            records[2].started_at,
            Timestamp::parse("2026-02-01").unwrap().start()
        );
    }

    #[tokio::test]
    async fn run_records_use_splits_and_round_to_the_nearest_second() {
        let (domain, _, owner, _, _, _, _) = memory_domain().await;
        domain
            .create_session(
                &owner,
                CreateWorkoutSession {
                    activity: CreateActivity::Run {
                        distance_m: None,
                        duration_sec: None,
                        elevation_gain_m: 0,
                        splits: vec![
                            RunSplit {
                                distance_m: 1_500,
                                duration_sec: 500,
                            },
                            RunSplit {
                                distance_m: 1_500,
                                duration_sec: 501,
                            },
                        ],
                    },
                    ..run_on("2026-02-01", 0, 0)
                },
            )
            .await
            .unwrap();
        let records = domain.run_records(&owner).await.unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].distance_m, 1_000);
        assert_eq!(records[0].duration_sec, 334);
    }

    #[tokio::test]
    async fn dashboard_totals_split_by_activity_and_honor_the_range() {
        let (domain, _, owner, _, _, dynamic_id, _) = memory_domain().await;
        domain
            .log_workout(
                &owner,
                crate::domain::workouts::LogWorkout {
                    started_at: Timestamp::parse("2026-02-02").unwrap(),
                    label: Some("Leg day".into()),
                    notes: None,
                    exercises: vec![crate::domain::workouts::LogWorkoutExercise {
                        exercise_id: dynamic_id,
                        notes: None,
                        sets: vec![crate::domain::workouts::AddExerciseSet {
                            position: None,
                            set_type: "working".into(),
                            reps: Some(5),
                            hold_sec: None,
                            load_g: 100_000,
                            notes: None,
                        }],
                    }],
                },
            )
            .await
            .unwrap();
        domain
            .create_session(&owner, run_on("2026-02-03", 8_200, 2_460))
            .await
            .unwrap();
        let all = domain
            .dashboard(&owner, StatsRange::default())
            .await
            .unwrap();
        assert_eq!(all.stats.strength_count, 1);
        assert_eq!(all.stats.run_count, 1);
        assert_eq!(all.stats.volume_g, 500_000);
        assert_eq!(all.stats.exercise_count, 1);
        assert_eq!(all.stats.distance_m, 8_200);
        assert_eq!(all.stats.duration_sec, 2_460);
        assert_eq!(all.workouts.len(), 1);
        assert_eq!(all.runs.len(), 1);
        assert_eq!(all.exercise_records.len(), 1);
        assert_eq!(all.run_records.len(), 2);

        let later = domain
            .dashboard(
                &owner,
                StatsRange {
                    from: Some(Timestamp::parse("2026-02-03").unwrap()),
                    to: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(later.stats.strength_count, 0);
        assert_eq!(later.stats.run_count, 1);
        assert!(later.workouts.is_empty());
        assert_eq!(later.exercise_records.len(), 1);
        assert_eq!(later.run_records.len(), 2);
    }
}
