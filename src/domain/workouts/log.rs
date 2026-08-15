use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect, Set,
};
use serde::Deserialize;
use uuid::Uuid;

use super::{
    ActivityView, AddExerciseSet, MAX_EXERCISE_SETS, MAX_SESSION_EXERCISES, Timestamp,
    WorkoutSession, mutation_error, parse_id, sets::validate_set,
};
use crate::domain::{
    Domain,
    auth::Principal,
    entity::{exercise_sets, exercises, session_exercises, sessions},
    error::DomainError,
};

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogWorkout {
    pub started_at: Timestamp,
    pub label: Option<String>,
    pub exercises: Vec<LogWorkoutExercise>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogWorkoutExercise {
    pub exercise_id: Uuid,
    #[serde(default)]
    pub sets: Vec<AddExerciseSet>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepeatLastWorkout {
    pub started_at: Timestamp,
    pub label: Option<String>,
    pub like_label: Option<String>,
}

fn validate_label(label: Option<&String>) -> Result<(), DomainError> {
    if label.is_some_and(|value| value.len() > 256 || value.chars().any(char::is_control)) {
        return Err(DomainError::InvalidInput(
            "label must be at most 256 characters and contain no control characters",
        ));
    }
    Ok(())
}

impl Domain {
    pub async fn log_workout(
        &self,
        principal: &Principal,
        input: LogWorkout,
    ) -> Result<WorkoutSession, DomainError> {
        validate_label(input.label.as_ref())?;
        if input.exercises.is_empty() {
            return Err(DomainError::InvalidInput(
                "exercises must contain at least one entry",
            ));
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

        let session_id = Uuid::now_v7();
        let user_id = principal.user_id().to_string();
        let tx = self.begin_immediate().await?;
        sessions::ActiveModel {
            id: Set(session_id.to_string()),
            user_id: Set(user_id.clone()),
            started_at: Set(input.started_at.start().to_rfc3339()),
            activity_type: Set("strength".to_owned()),
            label: Set(input.label.clone()),
        }
        .insert(&tx)
        .await
        .map_err(mutation_error)?;

        for (position, entry) in input.exercises.iter().enumerate() {
            let exercise = exercises::Entity::find_by_id(entry.exercise_id.to_string())
                .one(&tx)
                .await?
                .ok_or(DomainError::InvalidInput("unknown exercise_id"))?;
            let session_exercise_id = Uuid::now_v7();
            session_exercises::ActiveModel {
                id: Set(session_exercise_id.to_string()),
                session_id: Set(session_id.to_string()),
                user_id: Set(user_id.clone()),
                exercise_id: Set(entry.exercise_id.to_string()),
                activity_type: Set("strength".to_owned()),
                contraction_type: Set(exercise.contraction_type.clone()),
                position: Set(position as i64),
            }
            .insert(&tx)
            .await
            .map_err(mutation_error)?;
            for (set_position, set) in entry.sets.iter().enumerate() {
                validate_set(
                    &exercise.contraction_type,
                    &set.set_type,
                    set.reps,
                    set.hold_sec,
                )?;
                exercise_sets::ActiveModel {
                    id: Set(Uuid::now_v7().to_string()),
                    session_exercise_id: Set(session_exercise_id.to_string()),
                    user_id: Set(user_id.clone()),
                    contraction_type: Set(exercise.contraction_type.clone()),
                    position: Set(set_position as i64),
                    set_type: Set(set.set_type.clone()),
                    reps: Set(set.reps),
                    hold_sec: Set(set.hold_sec),
                    load_g: Set(set.load_g),
                }
                .insert(&tx)
                .await
                .map_err(mutation_error)?;
            }
        }

        let result = self.get_session_on(&tx, principal, session_id).await?;
        tx.commit().await.map_err(mutation_error)?;
        Ok(result)
    }

    pub async fn repeat_last_workout(
        &self,
        principal: &Principal,
        input: RepeatLastWorkout,
    ) -> Result<WorkoutSession, DomainError> {
        validate_label(input.label.as_ref())?;
        let mut select = sessions::Entity::find()
            .filter(sessions::Column::UserId.eq(principal.user_id().to_string()))
            .filter(sessions::Column::ActivityType.eq("strength"));
        if let Some(like_label) = input.like_label.as_deref() {
            let like_label = like_label.trim();
            if like_label.is_empty() || like_label.len() > 256 {
                return Err(DomainError::InvalidInput(
                    "like_label must be 1-256 characters",
                ));
            }
            select = select.filter(sessions::Column::Label.contains(like_label));
        }
        let previous = select
            .order_by_desc(sessions::Column::StartedAt)
            .order_by_desc(sessions::Column::Id)
            .limit(1)
            .one(&self.db)
            .await?
            .ok_or(DomainError::NotFound)?;

        let previous_id = parse_id(&previous.id)?;
        let source = self.get_session(principal, previous_id).await?;
        let ActivityView::Strength { exercises } = source.activity else {
            return Err(DomainError::NotFound);
        };
        let template = LogWorkout {
            started_at: input.started_at,
            label: input.label.or(previous.label),
            exercises: exercises
                .into_iter()
                .map(|exercise| LogWorkoutExercise {
                    exercise_id: exercise.exercise_id,
                    sets: exercise
                        .sets
                        .into_iter()
                        .map(|set| AddExerciseSet {
                            position: None,
                            set_type: set.set_type,
                            reps: set.reps,
                            hold_sec: set.hold_sec,
                            load_g: set.load_g,
                        })
                        .collect(),
                })
                .collect(),
        };
        if template.exercises.is_empty() {
            return Err(DomainError::InvalidInput(
                "the previous session has no exercises to repeat",
            ));
        }
        self.log_workout(principal, template).await
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use crate::domain::{
        catalogue::PageRequest,
        workouts::{ExerciseSet, SessionFilter},
    };

    fn dynamic(reps: i64, load_g: i64) -> AddExerciseSet {
        AddExerciseSet {
            position: None,
            set_type: "working".into(),
            reps: Some(reps),
            hold_sec: None,
            load_g,
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
                    exercises: vec![
                        LogWorkoutExercise {
                            exercise_id: dynamic_id,
                            sets: vec![dynamic(5, 60_000), dynamic(5, 65_000)],
                        },
                        LogWorkoutExercise {
                            exercise_id: isometric_id,
                            sets: vec![AddExerciseSet {
                                position: None,
                                set_type: "working".into(),
                                reps: None,
                                hold_sec: Some(60),
                                load_g: 0,
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
                    exercises: vec![LogWorkoutExercise {
                        exercise_id: dynamic_id,
                        sets: vec![
                            dynamic(5, 1_000),
                            AddExerciseSet {
                                position: None,
                                set_type: "working".into(),
                                reps: None,
                                hold_sec: Some(30),
                                load_g: 0,
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
    async fn repeat_last_workout_copies_the_latest_matching_session() {
        let (domain, _, owner, _, _, dynamic_id, _) = memory_domain().await;
        domain
            .log_workout(
                &owner,
                LogWorkout {
                    started_at: Timestamp::parse("2026-02-01").unwrap(),
                    label: Some("Leg day".into()),
                    exercises: vec![LogWorkoutExercise {
                        exercise_id: dynamic_id,
                        sets: vec![dynamic(5, 60_000)],
                    }],
                },
            )
            .await
            .unwrap();
        domain
            .log_workout(
                &owner,
                LogWorkout {
                    started_at: Timestamp::parse("2026-02-03").unwrap(),
                    label: Some("Push day".into()),
                    exercises: vec![LogWorkoutExercise {
                        exercise_id: dynamic_id,
                        sets: vec![dynamic(3, 80_000)],
                    }],
                },
            )
            .await
            .unwrap();

        let latest = domain
            .repeat_last_workout(
                &owner,
                RepeatLastWorkout {
                    started_at: Timestamp::parse("2026-02-05").unwrap(),
                    label: None,
                    like_label: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(latest.label.as_deref(), Some("Push day"));
        assert_eq!(sets_of(&latest, 0)[0].load_g, 80_000);

        let filtered = domain
            .repeat_last_workout(
                &owner,
                RepeatLastWorkout {
                    started_at: Timestamp::parse("2026-02-06").unwrap(),
                    label: Some("Leg day again".into()),
                    like_label: Some("Leg".into()),
                },
            )
            .await
            .unwrap();
        assert_eq!(filtered.label.as_deref(), Some("Leg day again"));
        assert_eq!(sets_of(&filtered, 0)[0].load_g, 60_000);

        assert!(matches!(
            domain
                .repeat_last_workout(
                    &owner,
                    RepeatLastWorkout {
                        started_at: Timestamp::parse("2026-02-07").unwrap(),
                        label: None,
                        like_label: Some("nothing matches".into()),
                    },
                )
                .await,
            Err(DomainError::NotFound)
        ));
    }
}
