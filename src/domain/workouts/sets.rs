use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
use uuid::Uuid;

use super::{ExerciseSet, MAX_EXERCISE_SETS, parse_id};
use crate::domain::{Domain, auth::Principal, entity::exercise_sets, error::DomainError};

pub(super) fn validate_set(
    contraction: &str,
    set_type: &str,
    reps: Option<i64>,
    hold_sec: Option<i64>,
) -> Result<(), DomainError> {
    if !matches!(set_type, "warmup" | "working" | "amrap" | "drop") {
        return Err(DomainError::InvalidInput("invalid set_type"));
    }
    match contraction {
        "dynamic" if reps.is_some_and(|value| value > 0) && hold_sec.is_none() => Ok(()),
        "isometric" if hold_sec.is_some_and(|value| value > 0) && reps.is_none() => Ok(()),
        "dynamic" => Err(DomainError::InvalidInput(
            "dynamic sets require positive reps and no hold_sec",
        )),
        "isometric" => Err(DomainError::InvalidInput(
            "isometric sets require positive hold_sec and no reps",
        )),
        _ => Err(DomainError::Conflict),
    }
}

fn set_view(model: exercise_sets::Model) -> Result<ExerciseSet, DomainError> {
    Ok(ExerciseSet {
        id: parse_id(&model.id)?,
        session_exercise_id: parse_id(&model.session_exercise_id)?,
        position: u64::try_from(model.position).map_err(|_| DomainError::NotFound)?,
        set_type: model.set_type,
        reps: model.reps,
        hold_sec: model.hold_sec,
        load_g: model.load_g,
        notes: model.notes,
    })
}

impl Domain {
    pub(super) async fn all_exercise_sets<C: ConnectionTrait>(
        &self,
        connection: &C,
        principal: &Principal,
        session_exercise_id: Uuid,
    ) -> Result<Vec<ExerciseSet>, DomainError> {
        let models = exercise_sets::Entity::find()
            .filter(exercise_sets::Column::SessionExerciseId.eq(session_exercise_id.to_string()))
            .filter(exercise_sets::Column::UserId.eq(principal.user_id().to_string()))
            .order_by_asc(exercise_sets::Column::Position)
            .order_by_asc(exercise_sets::Column::Id)
            .limit((MAX_EXERCISE_SETS + 1) as u64)
            .all(connection)
            .await?;
        if models.len() > MAX_EXERCISE_SETS {
            return Err(DomainError::Conflict);
        }
        models.into_iter().map(set_view).collect()
    }
}
