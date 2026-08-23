use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
use uuid::Uuid;

use super::{MAX_SESSION_EXERCISES, SessionExercise, parse_id};
use crate::domain::{
    Domain,
    auth::Principal,
    entity::{exercises, session_exercises},
    error::DomainError,
};

impl Domain {
    pub(super) async fn all_session_exercises<C: ConnectionTrait>(
        &self,
        connection: &C,
        principal: &Principal,
        session_id: Uuid,
    ) -> Result<Vec<SessionExercise>, DomainError> {
        let models = session_exercises::Entity::find()
            .filter(session_exercises::Column::SessionId.eq(session_id.to_string()))
            .filter(session_exercises::Column::UserId.eq(principal.user_id().to_string()))
            .order_by_asc(session_exercises::Column::Position)
            .order_by_asc(session_exercises::Column::Id)
            .limit((MAX_SESSION_EXERCISES + 1) as u64)
            .all(connection)
            .await?;
        if models.len() > MAX_SESSION_EXERCISES {
            return Err(DomainError::Conflict);
        }
        let mut items = Vec::with_capacity(models.len());
        for model in models {
            items.push(
                self.session_exercise_from_model(connection, principal, model)
                    .await?,
            );
        }
        Ok(items)
    }

    async fn session_exercise_from_model<C: ConnectionTrait>(
        &self,
        connection: &C,
        principal: &Principal,
        model: session_exercises::Model,
    ) -> Result<SessionExercise, DomainError> {
        let exercise = exercises::Entity::find_by_id(model.exercise_id.clone())
            .one(connection)
            .await?
            .ok_or(DomainError::NotFound)?;
        let id = parse_id(&model.id)?;
        Ok(SessionExercise {
            id,
            session_id: parse_id(&model.session_id)?,
            exercise_id: parse_id(&model.exercise_id)?,
            exercise_name: exercise.name,
            contraction_type: model.contraction_type,
            position: u64::try_from(model.position).map_err(|_| DomainError::NotFound)?,
            notes: model.notes,
            sets: self.all_exercise_sets(connection, principal, id).await?,
        })
    }
}
