use sea_orm::{ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, Set};
use serde::Deserialize;
use uuid::Uuid;

use super::{
    AddExerciseSet, MAX_EXERCISE_SETS, MAX_SESSION_EXERCISES, Timestamp, WorkoutSession,
    mutation_error, sets::validate_set, validate_notes,
};
use crate::domain::{
    Domain,
    auth::Principal,
    entity::{exercise_sets, exercises, runs, session_exercises, sessions},
    error::DomainError,
};

pub const SET_POSITION_MISMATCH: &str = "a set position must equal the index of its element in the sets array; reorder the array to reorder the sets";

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogWorkout {
    pub started_at: Timestamp,
    pub label: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    pub exercises: Vec<LogWorkoutExercise>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogWorkoutExercise {
    pub exercise_id: Uuid,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub sets: Vec<AddExerciseSet>,
}

/// The run counterpart of `LogWorkout`: everything `replace_run` overwrites.
#[derive(Clone, Debug)]
pub struct ReplaceRun {
    pub started_at: Timestamp,
    pub label: Option<String>,
    pub notes: Option<String>,
    pub distance_m: i64,
    pub duration_sec: i64,
    pub elevation_gain_m: i64,
}

fn validate_label(label: Option<&String>) -> Result<(), DomainError> {
    if label.is_some_and(|value| value.len() > 256 || value.chars().any(char::is_control)) {
        return Err(DomainError::InvalidInput(
            "label must be at most 256 characters and contain no control characters",
        ));
    }
    Ok(())
}

fn validate_workout(input: &LogWorkout) -> Result<(), DomainError> {
    validate_label(input.label.as_ref())?;
    validate_notes(input.notes.as_ref())?;
    for entry in &input.exercises {
        validate_notes(entry.notes.as_ref())?;
        for (index, set) in entry.sets.iter().enumerate() {
            validate_notes(set.notes.as_ref())?;
            // A whole-workout write takes its order from the array, so an
            // explicit position that disagrees would have to be discarded.
            if set
                .position
                .is_some_and(|position| position != index as u64)
            {
                return Err(DomainError::InvalidInput(SET_POSITION_MISMATCH));
            }
        }
    }
    if input.exercises.len() > MAX_SESSION_EXERCISES {
        return Err(DomainError::InvalidInput("session exercise limit reached"));
    }
    if input
        .exercises
        .iter()
        .any(|entry| entry.sets.len() > MAX_EXERCISE_SETS)
    {
        return Err(DomainError::InvalidInput("exercise set limit reached"));
    }
    Ok(())
}

impl Domain {
    /// Loads every referenced exercise and validates every set before the
    /// caller writes anything, so a bad reference cannot leave a half-written
    /// workout behind.
    async fn prepare_workout_exercises<C: ConnectionTrait>(
        &self,
        connection: &C,
        input: &LogWorkout,
    ) -> Result<Vec<exercises::Model>, DomainError> {
        let mut prepared = Vec::with_capacity(input.exercises.len());
        for entry in &input.exercises {
            let exercise = exercises::Entity::find_by_id(entry.exercise_id.to_string())
                .one(connection)
                .await?
                .ok_or(DomainError::InvalidInput("unknown exercise_id"))?;
            for set in &entry.sets {
                validate_set(
                    &exercise.contraction_type,
                    &set.set_type,
                    set.reps,
                    set.hold_sec,
                )?;
            }
            prepared.push(exercise);
        }
        Ok(prepared)
    }

    async fn insert_workout_exercises<C: ConnectionTrait>(
        &self,
        connection: &C,
        user_id: &str,
        session_id: Uuid,
        input: &LogWorkout,
        prepared: &[exercises::Model],
    ) -> Result<(), DomainError> {
        for (position, (entry, exercise)) in input.exercises.iter().zip(prepared).enumerate() {
            let session_exercise_id = Uuid::now_v7();
            session_exercises::ActiveModel {
                id: Set(session_exercise_id.to_string()),
                session_id: Set(session_id.to_string()),
                user_id: Set(user_id.to_owned()),
                exercise_id: Set(entry.exercise_id.to_string()),
                activity_type: Set("strength".to_owned()),
                contraction_type: Set(exercise.contraction_type.clone()),
                position: Set(position as i64),
                notes: Set(entry.notes.clone()),
            }
            .insert(connection)
            .await
            .map_err(mutation_error)?;
            for (set_position, set) in entry.sets.iter().enumerate() {
                exercise_sets::ActiveModel {
                    id: Set(Uuid::now_v7().to_string()),
                    session_exercise_id: Set(session_exercise_id.to_string()),
                    user_id: Set(user_id.to_owned()),
                    contraction_type: Set(exercise.contraction_type.clone()),
                    position: Set(set_position as i64),
                    set_type: Set(set.set_type.clone()),
                    reps: Set(set.reps),
                    hold_sec: Set(set.hold_sec),
                    load_g: Set(set.load_g),
                    notes: Set(set.notes.clone()),
                }
                .insert(connection)
                .await
                .map_err(mutation_error)?;
            }
        }
        Ok(())
    }

    pub async fn log_workout(
        &self,
        principal: &Principal,
        input: LogWorkout,
    ) -> Result<WorkoutSession, DomainError> {
        validate_workout(&input)?;
        // An empty log is far more likely a mistake than an intent, and
        // create_workout_session already opens a session with no exercises.
        if input.exercises.is_empty() {
            return Err(DomainError::InvalidInput(
                "exercises must contain at least one entry",
            ));
        }

        let session_id = Uuid::now_v7();
        let user_id = principal.user_id().to_string();
        let tx = self.begin_immediate().await?;
        let prepared = self.prepare_workout_exercises(&tx, &input).await?;
        sessions::ActiveModel {
            id: Set(session_id.to_string()),
            user_id: Set(user_id.clone()),
            started_at: Set(input.started_at.start().to_rfc3339()),
            activity_type: Set("strength".to_owned()),
            label: Set(input.label.clone()),
            notes: Set(input.notes.clone()),
        }
        .insert(&tx)
        .await
        .map_err(mutation_error)?;
        self.insert_workout_exercises(&tx, &user_id, session_id, &input, &prepared)
            .await?;

        let result = self.get_session_on(&tx, principal, session_id).await?;
        tx.commit().await.map_err(mutation_error)?;
        Ok(result)
    }

    /// Replaces a strength session whole: its fields, its exercises, and all
    /// their sets. Anything absent from `input` is removed. A run session
    /// becomes a strength session, so a mistyped activity is correctable
    /// without losing the session id.
    pub async fn replace_workout(
        &self,
        principal: &Principal,
        session_id: Uuid,
        input: LogWorkout,
    ) -> Result<WorkoutSession, DomainError> {
        validate_workout(&input)?;

        let user_id = principal.user_id().to_string();
        let tx = self.begin_immediate().await?;
        let session = sessions::Entity::find_by_id(session_id.to_string())
            .filter(sessions::Column::UserId.eq(user_id.clone()))
            .one(&tx)
            .await?
            .ok_or(DomainError::NotFound)?;
        let was_run = session.activity_type == "run";
        let prepared = self.prepare_workout_exercises(&tx, &input).await?;

        // Removing the old rows before inserting the new ones keeps the
        // (session_id, position) and (session_exercise_id, position) unique
        // indexes satisfied without parking positions in the temporary band.
        let replaced = session_exercises::Entity::find()
            .filter(session_exercises::Column::SessionId.eq(session_id.to_string()))
            .filter(session_exercises::Column::UserId.eq(user_id.clone()))
            .all(&tx)
            .await?;
        for item in replaced {
            exercise_sets::Entity::delete_many()
                .filter(exercise_sets::Column::SessionExerciseId.eq(item.id.clone()))
                .exec(&tx)
                .await
                .map_err(mutation_error)?;
            session_exercises::Entity::delete_by_id(item.id)
                .exec(&tx)
                .await
                .map_err(mutation_error)?;
        }

        if was_run {
            runs::Entity::delete_many()
                .filter(runs::Column::SessionId.eq(session_id.to_string()))
                .filter(runs::Column::UserId.eq(user_id.clone()))
                .exec(&tx)
                .await
                .map_err(mutation_error)?;
        }

        let mut active: sessions::ActiveModel = session.into();
        active.started_at = Set(input.started_at.start().to_rfc3339());
        active.label = Set(input.label.clone());
        active.notes = Set(input.notes.clone());
        active.activity_type = Set("strength".to_owned());
        active.update(&tx).await.map_err(mutation_error)?;

        // The session must already read as strength: session_exercises rows are
        // pinned to a strength session by a database check.
        self.insert_workout_exercises(&tx, &user_id, session_id, &input, &prepared)
            .await?;

        let result = self.get_session_on(&tx, principal, session_id).await?;
        tx.commit().await.map_err(mutation_error)?;
        Ok(result)
    }

    /// Replaces a run session whole: its fields and its run detail. The run
    /// counterpart of `replace_workout`, so that an agent can correct a run
    /// without deleting and recreating it.
    pub async fn replace_run(
        &self,
        principal: &Principal,
        session_id: Uuid,
        input: ReplaceRun,
    ) -> Result<WorkoutSession, DomainError> {
        validate_label(input.label.as_ref())?;
        validate_notes(input.notes.as_ref())?;
        if input.distance_m <= 0 || input.duration_sec <= 0 || input.elevation_gain_m < 0 {
            return Err(DomainError::InvalidInput("invalid run details"));
        }

        let user_id = principal.user_id().to_string();
        let tx = self.begin_immediate().await?;
        let session = sessions::Entity::find_by_id(session_id.to_string())
            .filter(sessions::Column::UserId.eq(user_id.clone()))
            .one(&tx)
            .await?
            .ok_or(DomainError::NotFound)?;
        let was_run = session.activity_type == "run";
        if !was_run {
            let replaced = session_exercises::Entity::find()
                .filter(session_exercises::Column::SessionId.eq(session_id.to_string()))
                .filter(session_exercises::Column::UserId.eq(user_id.clone()))
                .all(&tx)
                .await?;
            for item in replaced {
                exercise_sets::Entity::delete_many()
                    .filter(exercise_sets::Column::SessionExerciseId.eq(item.id.clone()))
                    .exec(&tx)
                    .await
                    .map_err(mutation_error)?;
                session_exercises::Entity::delete_by_id(item.id)
                    .exec(&tx)
                    .await
                    .map_err(mutation_error)?;
            }
        }

        let mut active: sessions::ActiveModel = session.into();
        active.started_at = Set(input.started_at.start().to_rfc3339());
        active.label = Set(input.label.clone());
        active.notes = Set(input.notes.clone());
        active.activity_type = Set("run".to_owned());
        active.update(&tx).await.map_err(mutation_error)?;

        let run = runs::ActiveModel {
            session_id: Set(session_id.to_string()),
            user_id: Set(user_id),
            activity_type: Set("run".to_owned()),
            distance_m: Set(input.distance_m),
            duration_sec: Set(input.duration_sec),
            elevation_gain_m: Set(input.elevation_gain_m),
        };
        // Every path that sets activity_type='run' writes the runs row with it,
        // so only a session that was not a run needs the row created.
        if was_run {
            run.update(&tx).await.map_err(mutation_error)?;
        } else {
            run.insert(&tx).await.map_err(mutation_error)?;
        }

        let result = self.get_session_on(&tx, principal, session_id).await?;
        tx.commit().await.map_err(mutation_error)?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use crate::domain::{
        catalogue::PageRequest,
        workouts::{ActivityView, ExerciseSet, MAX_NOTES, SessionFilter},
    };

    fn dynamic(reps: i64, load_g: i64) -> AddExerciseSet {
        AddExerciseSet {
            position: None,
            set_type: "working".into(),
            reps: Some(reps),
            hold_sec: None,
            load_g,
            notes: None,
        }
    }

    fn sets_of(session: &WorkoutSession, index: usize) -> Vec<ExerciseSet> {
        match &session.activity {
            ActivityView::Strength { exercises } => exercises[index].sets.clone(),
            ActivityView::Run { .. } => panic!("expected a strength session"),
        }
    }

    #[tokio::test]
    async fn log_workout_stores_the_whole_session_in_order() {
        let (domain, _, owner, _, _, dynamic_id, isometric_id) = memory_domain().await;
        let session = domain
            .log_workout(
                &owner,
                LogWorkout {
                    started_at: Timestamp::parse("2026-02-01").unwrap(),
                    label: Some("push".into()),
                    notes: None,
                    exercises: vec![
                        LogWorkoutExercise {
                            exercise_id: dynamic_id,
                            notes: None,
                            sets: vec![dynamic(5, 60_000), dynamic(5, 65_000)],
                        },
                        LogWorkoutExercise {
                            exercise_id: isometric_id,
                            notes: None,
                            sets: vec![AddExerciseSet {
                                position: None,
                                set_type: "working".into(),
                                reps: None,
                                hold_sec: Some(60),
                                load_g: 0,
                                notes: None,
                            }],
                        },
                    ],
                },
            )
            .await
            .unwrap();
        assert_eq!(session.label.as_deref(), Some("push"));
        assert_eq!(sets_of(&session, 0).len(), 2);
        assert_eq!(sets_of(&session, 0)[1].load_g, 65_000);
        assert_eq!(sets_of(&session, 1)[0].hold_sec, Some(60));
        assert_dense_children(&domain, &owner, session.id).await;
    }

    #[tokio::test]
    async fn log_workout_rolls_back_when_any_set_is_invalid() {
        let (domain, _, owner, _, _, dynamic_id, _) = memory_domain().await;
        let before = domain
            .list_sessions(&owner, SessionFilter::default(), PageRequest::default())
            .await
            .unwrap()
            .items
            .len();
        let invalid = domain
            .log_workout(
                &owner,
                LogWorkout {
                    started_at: Timestamp::parse("2026-02-01").unwrap(),
                    label: None,
                    notes: None,
                    exercises: vec![LogWorkoutExercise {
                        exercise_id: dynamic_id,
                        notes: None,
                        sets: vec![
                            dynamic(5, 1_000),
                            AddExerciseSet {
                                position: None,
                                set_type: "working".into(),
                                reps: None,
                                hold_sec: Some(30),
                                load_g: 0,
                                notes: None,
                            },
                        ],
                    }],
                },
            )
            .await;
        assert!(matches!(invalid, Err(DomainError::InvalidInput(_))));
        let after = domain
            .list_sessions(&owner, SessionFilter::default(), PageRequest::default())
            .await
            .unwrap()
            .items
            .len();
        assert_eq!(before, after);
    }

    #[tokio::test]
    async fn log_workout_round_trips_notes_at_every_level() {
        let (domain, _, owner, _, _, dynamic_id, _) = memory_domain().await;
        let session = domain
            .log_workout(
                &owner,
                LogWorkout {
                    started_at: Timestamp::parse("2026-04-01").unwrap(),
                    label: Some("push".into()),
                    notes: Some("slept badly".into()),
                    exercises: vec![LogWorkoutExercise {
                        exercise_id: dynamic_id,
                        notes: Some("belt from set two".into()),
                        sets: vec![AddExerciseSet {
                            notes: Some("left knee twinge".into()),
                            ..dynamic(5, 60_000)
                        }],
                    }],
                },
            )
            .await
            .unwrap();
        assert_eq!(session.notes.as_deref(), Some("slept badly"));

        let stored = domain.get_session(&owner, session.id).await.unwrap();
        assert_eq!(stored.notes.as_deref(), Some("slept badly"));
        let ActivityView::Strength { exercises } = &stored.activity else {
            panic!("expected a strength session");
        };
        assert_eq!(exercises[0].notes.as_deref(), Some("belt from set two"));
        assert_eq!(
            exercises[0].sets[0].notes.as_deref(),
            Some("left knee twinge")
        );

        let summary = domain
            .list_sessions(&owner, SessionFilter::default(), PageRequest::default())
            .await
            .unwrap();
        assert_eq!(summary.items[0].notes.as_deref(), Some("slept badly"));
    }

    #[tokio::test]
    async fn log_workout_without_notes_leaves_them_empty() {
        let (domain, _, owner, _, _, dynamic_id, _) = memory_domain().await;
        let session = domain
            .log_workout(
                &owner,
                LogWorkout {
                    started_at: Timestamp::parse("2026-04-05").unwrap(),
                    label: None,
                    notes: None,
                    exercises: vec![LogWorkoutExercise {
                        exercise_id: dynamic_id,
                        notes: None,
                        sets: vec![dynamic(5, 60_000)],
                    }],
                },
            )
            .await
            .unwrap();
        assert!(session.notes.is_none());
        let ActivityView::Strength { exercises } = &session.activity else {
            panic!("expected a strength session");
        };
        assert!(exercises[0].notes.is_none());
        assert!(exercises[0].sets[0].notes.is_none());
    }

    #[tokio::test]
    async fn replace_workout_drops_the_exercises_and_sets_that_are_left_out() {
        let (domain, _, owner, _, _, dynamic_id, isometric_id) = memory_domain().await;
        let session = domain
            .log_workout(
                &owner,
                LogWorkout {
                    started_at: Timestamp::parse("2026-05-01").unwrap(),
                    label: Some("push".into()),
                    notes: None,
                    exercises: vec![
                        LogWorkoutExercise {
                            exercise_id: dynamic_id,
                            notes: None,
                            sets: vec![dynamic(5, 60_000), dynamic(5, 65_000), dynamic(5, 70_000)],
                        },
                        LogWorkoutExercise {
                            exercise_id: isometric_id,
                            notes: None,
                            sets: vec![AddExerciseSet {
                                position: None,
                                set_type: "working".into(),
                                reps: None,
                                hold_sec: Some(60),
                                load_g: 0,
                                notes: None,
                            }],
                        },
                    ],
                },
            )
            .await
            .unwrap();

        let replaced = domain
            .replace_workout(
                &owner,
                session.id,
                LogWorkout {
                    started_at: Timestamp::parse("2026-05-02").unwrap(),
                    label: Some("pull".into()),
                    notes: None,
                    exercises: vec![LogWorkoutExercise {
                        exercise_id: dynamic_id,
                        notes: None,
                        sets: vec![dynamic(3, 80_000)],
                    }],
                },
            )
            .await
            .unwrap();
        assert_eq!(replaced.id, session.id);
        assert_eq!(replaced.label.as_deref(), Some("pull"));
        let ActivityView::Strength { exercises } = &replaced.activity else {
            panic!("expected a strength session");
        };
        assert_eq!(exercises.len(), 1);
        assert_eq!(exercises[0].sets.len(), 1);
        assert_eq!(exercises[0].sets[0].load_g, 80_000);
        assert_dense_children(&domain, &owner, session.id).await;

        let stored = domain.get_session(&owner, session.id).await.unwrap();
        let ActivityView::Strength { exercises } = &stored.activity else {
            panic!("expected a strength session");
        };
        assert_eq!(exercises.len(), 1);
        assert_eq!(exercises[0].sets.len(), 1);
    }

    #[tokio::test]
    async fn replace_workout_round_trips_the_session_it_reads_back() {
        let (domain, _, owner, _, _, dynamic_id, isometric_id) = memory_domain().await;
        let session = domain
            .log_workout(
                &owner,
                LogWorkout {
                    started_at: Timestamp::parse("2026-05-05").unwrap(),
                    label: Some("push".into()),
                    notes: Some("slept badly".into()),
                    exercises: vec![
                        LogWorkoutExercise {
                            exercise_id: dynamic_id,
                            notes: Some("belt from set two".into()),
                            sets: vec![
                                AddExerciseSet {
                                    notes: Some("left knee twinge".into()),
                                    ..dynamic(5, 60_000)
                                },
                                dynamic(5, 65_000),
                            ],
                        },
                        LogWorkoutExercise {
                            exercise_id: isometric_id,
                            notes: Some("shoulders tight".into()),
                            sets: vec![AddExerciseSet {
                                position: None,
                                set_type: "working".into(),
                                reps: None,
                                hold_sec: Some(45),
                                load_g: 0,
                                notes: Some("elbows forward".into()),
                            }],
                        },
                    ],
                },
            )
            .await
            .unwrap();

        let read_back = domain.get_session(&owner, session.id).await.unwrap();
        let ActivityView::Strength { exercises } = read_back.activity.clone() else {
            panic!("expected a strength session");
        };
        let submitted = LogWorkout {
            started_at: Timestamp::at(read_back.started_at),
            label: read_back.label.clone(),
            notes: read_back.notes.clone(),
            exercises: exercises
                .into_iter()
                .map(|exercise| LogWorkoutExercise {
                    exercise_id: exercise.exercise_id,
                    notes: exercise.notes,
                    sets: exercise
                        .sets
                        .into_iter()
                        .map(|set| AddExerciseSet {
                            position: None,
                            set_type: set.set_type,
                            reps: set.reps,
                            hold_sec: set.hold_sec,
                            load_g: set.load_g,
                            notes: set.notes,
                        })
                        .collect(),
                })
                .collect(),
        };
        let replaced = domain
            .replace_workout(&owner, session.id, submitted)
            .await
            .unwrap();

        let equivalent = |session: &WorkoutSession| {
            let ActivityView::Strength { exercises } = &session.activity else {
                panic!("expected a strength session");
            };
            (
                session.started_at,
                session.label.clone(),
                session.notes.clone(),
                exercises
                    .iter()
                    .map(|exercise| {
                        (
                            exercise.exercise_id,
                            exercise.position,
                            exercise.notes.clone(),
                            exercise
                                .sets
                                .iter()
                                .map(|set| {
                                    (
                                        set.position,
                                        set.set_type.clone(),
                                        set.reps,
                                        set.hold_sec,
                                        set.load_g,
                                        set.notes.clone(),
                                    )
                                })
                                .collect::<Vec<_>>(),
                        )
                    })
                    .collect::<Vec<_>>(),
            )
        };
        assert_eq!(equivalent(&read_back), equivalent(&replaced));
    }

    #[tokio::test]
    async fn replace_workout_reorders_and_regrows_without_a_position_collision() {
        let (domain, _, owner, _, _, dynamic_id, isometric_id) = memory_domain().await;
        let session = domain
            .log_workout(
                &owner,
                LogWorkout {
                    started_at: Timestamp::parse("2026-05-10").unwrap(),
                    label: None,
                    notes: None,
                    exercises: vec![
                        LogWorkoutExercise {
                            exercise_id: dynamic_id,
                            notes: None,
                            sets: vec![dynamic(5, 60_000)],
                        },
                        LogWorkoutExercise {
                            exercise_id: isometric_id,
                            notes: None,
                            sets: vec![AddExerciseSet {
                                position: None,
                                set_type: "working".into(),
                                reps: None,
                                hold_sec: Some(30),
                                load_g: 0,
                                notes: None,
                            }],
                        },
                    ],
                },
            )
            .await
            .unwrap();

        let replaced = domain
            .replace_workout(
                &owner,
                session.id,
                LogWorkout {
                    started_at: Timestamp::parse("2026-05-10").unwrap(),
                    label: None,
                    notes: None,
                    exercises: vec![
                        LogWorkoutExercise {
                            exercise_id: isometric_id,
                            notes: None,
                            sets: vec![AddExerciseSet {
                                position: None,
                                set_type: "working".into(),
                                reps: None,
                                hold_sec: Some(30),
                                load_g: 0,
                                notes: None,
                            }],
                        },
                        LogWorkoutExercise {
                            exercise_id: dynamic_id,
                            notes: None,
                            sets: vec![dynamic(5, 60_000), dynamic(5, 62_500), dynamic(5, 65_000)],
                        },
                    ],
                },
            )
            .await
            .unwrap();
        let ActivityView::Strength { exercises } = &replaced.activity else {
            panic!("expected a strength session");
        };
        assert_eq!(exercises[0].exercise_id, isometric_id);
        assert_eq!(exercises[1].exercise_id, dynamic_id);
        assert_eq!(
            exercises
                .iter()
                .map(|exercise| exercise.position)
                .collect::<Vec<_>>(),
            vec![0, 1]
        );
        assert_eq!(
            exercises[1]
                .sets
                .iter()
                .map(|set| set.position)
                .collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        assert_dense_children(&domain, &owner, session.id).await;
    }

    #[tokio::test]
    async fn a_rejected_replacement_leaves_the_workout_untouched() {
        let (domain, _, owner, _, _, dynamic_id, isometric_id) = memory_domain().await;
        let session = domain
            .log_workout(
                &owner,
                LogWorkout {
                    started_at: Timestamp::parse("2026-05-15").unwrap(),
                    label: Some("push".into()),
                    notes: Some("felt strong".into()),
                    exercises: vec![LogWorkoutExercise {
                        exercise_id: dynamic_id,
                        notes: Some("belt on".into()),
                        sets: vec![dynamic(5, 60_000), dynamic(5, 65_000)],
                    }],
                },
            )
            .await
            .unwrap();
        let before = serde_json::to_value(domain.get_session(&owner, session.id).await.unwrap())
            .unwrap()
            .to_string();

        let rejected = [
            LogWorkout {
                started_at: Timestamp::parse("2026-05-16").unwrap(),
                label: None,
                notes: None,
                exercises: vec![LogWorkoutExercise {
                    exercise_id: Uuid::now_v7(),
                    notes: None,
                    sets: vec![dynamic(5, 60_000)],
                }],
            },
            LogWorkout {
                started_at: Timestamp::parse("2026-05-16").unwrap(),
                label: None,
                notes: None,
                exercises: vec![LogWorkoutExercise {
                    exercise_id: isometric_id,
                    notes: None,
                    sets: vec![dynamic(5, 60_000)],
                }],
            },
            LogWorkout {
                started_at: Timestamp::parse("2026-05-16").unwrap(),
                label: None,
                notes: Some("n".repeat(MAX_NOTES + 1)),
                exercises: vec![LogWorkoutExercise {
                    exercise_id: dynamic_id,
                    notes: None,
                    sets: vec![dynamic(5, 60_000)],
                }],
            },
            LogWorkout {
                started_at: Timestamp::parse("2026-05-16").unwrap(),
                label: None,
                notes: None,
                exercises: vec![LogWorkoutExercise {
                    exercise_id: dynamic_id,
                    notes: None,
                    sets: vec![
                        AddExerciseSet {
                            position: Some(1),
                            ..dynamic(5, 60_000)
                        },
                        AddExerciseSet {
                            position: Some(0),
                            ..dynamic(5, 65_000)
                        },
                    ],
                }],
            },
        ];
        for input in rejected {
            assert!(matches!(
                domain.replace_workout(&owner, session.id, input).await,
                Err(DomainError::InvalidInput(_))
            ));
            let after = serde_json::to_value(domain.get_session(&owner, session.id).await.unwrap())
                .unwrap()
                .to_string();
            assert_eq!(before, after);
        }
    }

    #[tokio::test]
    async fn replace_workout_refuses_a_foreign_session() {
        let (domain, _, owner, other, _, dynamic_id, _) = memory_domain().await;
        let workout = || LogWorkout {
            started_at: Timestamp::parse("2026-05-20").unwrap(),
            label: None,
            notes: None,
            exercises: vec![LogWorkoutExercise {
                exercise_id: dynamic_id,
                notes: None,
                sets: vec![dynamic(5, 60_000)],
            }],
        };
        let strength = domain.log_workout(&owner, workout()).await.unwrap();
        assert!(matches!(
            domain.replace_workout(&other, strength.id, workout()).await,
            Err(DomainError::NotFound)
        ));
        assert!(matches!(
            domain
                .replace_workout(&owner, Uuid::now_v7(), workout())
                .await,
            Err(DomainError::NotFound)
        ));
    }

    #[tokio::test]
    async fn log_workout_still_needs_at_least_one_exercise() {
        let (domain, _, owner, _, _, _, _) = memory_domain().await;
        assert!(matches!(
            domain
                .log_workout(
                    &owner,
                    LogWorkout {
                        started_at: Timestamp::parse("2026-05-21").unwrap(),
                        label: None,
                        notes: None,
                        exercises: vec![],
                    },
                )
                .await,
            Err(DomainError::InvalidInput(
                "exercises must contain at least one entry"
            ))
        ));
    }

    /// get_workout_session returns an empty exercises list for a session with
    /// no exercises, so replace_workout must accept one back.
    #[tokio::test]
    async fn replace_workout_accepts_an_empty_exercise_list() {
        let (domain, _, owner, _, _, dynamic_id, _) = memory_domain().await;
        let session = domain
            .log_workout(
                &owner,
                LogWorkout {
                    started_at: Timestamp::parse("2026-05-21").unwrap(),
                    label: None,
                    notes: None,
                    exercises: vec![LogWorkoutExercise {
                        exercise_id: dynamic_id,
                        notes: None,
                        sets: vec![dynamic(5, 60_000)],
                    }],
                },
            )
            .await
            .unwrap();
        let emptied = domain
            .replace_workout(
                &owner,
                session.id,
                LogWorkout {
                    started_at: Timestamp::parse("2026-05-21").unwrap(),
                    label: None,
                    notes: None,
                    exercises: vec![],
                },
            )
            .await
            .unwrap();
        assert_eq!(emptied.id, session.id);
        match emptied.activity {
            ActivityView::Strength { exercises } => assert!(exercises.is_empty()),
            ActivityView::Run { .. } => panic!("expected a strength session"),
        }
    }

    #[tokio::test]
    async fn replace_workout_turns_a_run_into_a_strength_session() {
        let (domain, _, owner, _, _, dynamic_id, _) = memory_domain().await;
        let run = domain.create_session(&owner, run(5_000)).await.unwrap();
        let corrected = domain
            .replace_workout(
                &owner,
                run.id,
                LogWorkout {
                    started_at: Timestamp::parse("2026-05-22").unwrap(),
                    label: Some("pull".into()),
                    notes: None,
                    exercises: vec![LogWorkoutExercise {
                        exercise_id: dynamic_id,
                        notes: None,
                        sets: vec![dynamic(5, 60_000)],
                    }],
                },
            )
            .await
            .unwrap();
        assert_eq!(corrected.id, run.id);
        assert_eq!(sets_of(&corrected, 0).len(), 1);
        let reread = domain.get_session(&owner, run.id).await.unwrap();
        assert!(matches!(reread.activity, ActivityView::Strength { .. }));
    }

    #[tokio::test]
    async fn replace_run_turns_a_strength_session_into_a_run() {
        let (domain, _, owner, _, _, dynamic_id, _) = memory_domain().await;
        let session = domain
            .log_workout(
                &owner,
                LogWorkout {
                    started_at: Timestamp::parse("2026-05-23").unwrap(),
                    label: None,
                    notes: None,
                    exercises: vec![LogWorkoutExercise {
                        exercise_id: dynamic_id,
                        notes: None,
                        sets: vec![dynamic(5, 60_000)],
                    }],
                },
            )
            .await
            .unwrap();
        let corrected = domain
            .replace_run(
                &owner,
                session.id,
                ReplaceRun {
                    started_at: Timestamp::parse("2026-05-23").unwrap(),
                    label: None,
                    notes: None,
                    distance_m: 8_000,
                    duration_sec: 2_400,
                    elevation_gain_m: 40,
                },
            )
            .await
            .unwrap();
        assert_eq!(corrected.id, session.id);
        assert!(matches!(
            corrected.activity,
            ActivityView::Run {
                distance_m: 8_000,
                ..
            }
        ));
        // A run session must not keep the strength children behind: a database
        // check pins session_exercises rows to a strength session.
        assert!(
            session_exercises::Entity::find()
                .filter(session_exercises::Column::SessionId.eq(session.id.to_string()))
                .all(&domain.db)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn notes_longer_than_the_limit_are_rejected_at_every_level() {
        let (domain, _, owner, _, _, dynamic_id, _) = memory_domain().await;
        let long = "n".repeat(MAX_NOTES + 1);
        let template = |session_notes, exercise_notes, set_notes: Option<String>| LogWorkout {
            started_at: Timestamp::parse("2026-04-07").unwrap(),
            label: None,
            notes: session_notes,
            exercises: vec![LogWorkoutExercise {
                exercise_id: dynamic_id,
                notes: exercise_notes,
                sets: vec![AddExerciseSet {
                    notes: set_notes,
                    ..dynamic(5, 60_000)
                }],
            }],
        };
        for input in [
            template(Some(long.clone()), None, None),
            template(None, Some(long.clone()), None),
            template(None, None, Some(long.clone())),
        ] {
            assert!(matches!(
                domain.log_workout(&owner, input).await,
                Err(DomainError::InvalidInput(
                    "notes must be at most 1000 characters"
                ))
            ));
        }
        assert!(
            domain
                .log_workout(&owner, template(Some("n".repeat(MAX_NOTES)), None, None))
                .await
                .is_ok()
        );
    }
}
