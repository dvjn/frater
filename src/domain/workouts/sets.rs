use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect, Set,
};
use uuid::Uuid;

use super::{
    AddExerciseSet, ExerciseSet, MAX_EXERCISE_SETS, MAX_POSITION, TEMP_POSITION_BASE,
    UpdateExerciseSet, make_page, mutation_error, parse_id,
};
use crate::domain::{
    Domain,
    auth::Principal,
    catalogue::{Page, PageRequest},
    entity::{exercise_sets, session_exercises},
    error::DomainError,
};

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
    })
}

impl Domain {
    pub async fn list_exercise_sets(
        &self,
        principal: &Principal,
        session_exercise_id: Uuid,
        request: PageRequest,
    ) -> Result<Page<ExerciseSet>, DomainError> {
        if session_exercises::Entity::find_by_id(session_exercise_id.to_string())
            .filter(session_exercises::Column::UserId.eq(principal.user_id().to_string()))
            .one(&self.db)
            .await?
            .is_none()
        {
            return Err(DomainError::NotFound);
        }
        let (offset, limit) = request.bounded()?;
        let models = exercise_sets::Entity::find()
            .filter(exercise_sets::Column::SessionExerciseId.eq(session_exercise_id.to_string()))
            .filter(exercise_sets::Column::UserId.eq(principal.user_id().to_string()))
            .order_by_asc(exercise_sets::Column::Position)
            .order_by_asc(exercise_sets::Column::Id)
            .offset(offset)
            .limit(limit + 1)
            .all(&self.db)
            .await?;
        let items = models
            .into_iter()
            .map(set_view)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(make_page(items, offset, limit))
    }

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

    pub async fn get_exercise_set(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<ExerciseSet, DomainError> {
        self.get_exercise_set_on(&self.db, principal, id).await
    }

    async fn get_exercise_set_on<C: ConnectionTrait>(
        &self,
        connection: &C,
        principal: &Principal,
        id: Uuid,
    ) -> Result<ExerciseSet, DomainError> {
        let model = exercise_sets::Entity::find_by_id(id.to_string())
            .filter(exercise_sets::Column::UserId.eq(principal.user_id().to_string()))
            .one(connection)
            .await?
            .ok_or(DomainError::NotFound)?;
        set_view(model)
    }

    pub async fn add_exercise_set(
        &self,
        principal: &Principal,
        session_exercise_id: Uuid,
        input: AddExerciseSet,
    ) -> Result<ExerciseSet, DomainError> {
        let tx = self.begin_immediate().await?;
        let parent = session_exercises::Entity::find_by_id(session_exercise_id.to_string())
            .filter(session_exercises::Column::UserId.eq(principal.user_id().to_string()))
            .one(&tx)
            .await?
            .ok_or(DomainError::NotFound)?;
        validate_set(
            &parent.contraction_type,
            &input.set_type,
            input.reps,
            input.hold_sec,
        )?;
        let mut siblings = exercise_sets::Entity::find()
            .filter(exercise_sets::Column::SessionExerciseId.eq(session_exercise_id.to_string()))
            .filter(exercise_sets::Column::UserId.eq(principal.user_id().to_string()))
            .order_by_asc(exercise_sets::Column::Position)
            .all(&tx)
            .await?;
        if siblings.len() >= MAX_EXERCISE_SETS {
            return Err(DomainError::InvalidInput("exercise set limit reached"));
        }
        let desired = input.position.unwrap_or(siblings.len() as u64);
        if desired > siblings.len() as u64 || desired > MAX_POSITION {
            return Err(DomainError::InvalidInput("invalid position"));
        }
        move_sets_to_temporary(&tx, &mut siblings).await?;
        let id = Uuid::now_v7();
        exercise_sets::ActiveModel {
            id: Set(id.to_string()),
            session_exercise_id: Set(session_exercise_id.to_string()),
            user_id: Set(principal.user_id().to_string()),
            contraction_type: Set(parent.contraction_type),
            position: Set(desired as i64),
            set_type: Set(input.set_type),
            reps: Set(input.reps),
            hold_sec: Set(input.hold_sec),
            load_g: Set(input.load_g),
        }
        .insert(&tx)
        .await
        .map_err(mutation_error)?;
        restore_set_positions(&tx, siblings, None, desired).await?;
        let result = self.get_exercise_set_on(&tx, principal, id).await?;
        tx.commit().await.map_err(mutation_error)?;
        Ok(result)
    }

    pub async fn update_exercise_set(
        &self,
        principal: &Principal,
        id: Uuid,
        input: UpdateExerciseSet,
    ) -> Result<ExerciseSet, DomainError> {
        if input.position > MAX_POSITION {
            return Err(DomainError::InvalidInput("invalid position"));
        }
        let tx = self.begin_immediate().await?;
        let Some(model) = exercise_sets::Entity::find_by_id(id.to_string())
            .filter(exercise_sets::Column::UserId.eq(principal.user_id().to_string()))
            .one(&tx)
            .await?
        else {
            return Err(DomainError::NotFound);
        };
        validate_set(
            &model.contraction_type,
            &input.set_type,
            input.reps,
            input.hold_sec,
        )?;
        let mut siblings = exercise_sets::Entity::find()
            .filter(exercise_sets::Column::SessionExerciseId.eq(model.session_exercise_id.clone()))
            .filter(exercise_sets::Column::UserId.eq(principal.user_id().to_string()))
            .order_by_asc(exercise_sets::Column::Position)
            .all(&tx)
            .await?;
        siblings.retain(|item| item.id != model.id);
        if input.position > siblings.len() as u64 {
            return Err(DomainError::InvalidInput("invalid position"));
        }
        let mut all = siblings.clone();
        all.push(model.clone());
        move_sets_to_temporary(&tx, &mut all).await?;
        let mut active: exercise_sets::ActiveModel = model.into();
        active.position = Set(input.position as i64);
        active.set_type = Set(input.set_type);
        active.reps = Set(input.reps);
        active.hold_sec = Set(input.hold_sec);
        active.load_g = Set(input.load_g);
        active.update(&tx).await.map_err(mutation_error)?;
        restore_set_positions(&tx, siblings, Some(id), input.position).await?;
        let result = self.get_exercise_set_on(&tx, principal, id).await?;
        tx.commit().await.map_err(mutation_error)?;
        Ok(result)
    }

    pub async fn remove_exercise_set(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<(), DomainError> {
        let tx = self.begin_immediate().await?;
        let Some(model) = exercise_sets::Entity::find_by_id(id.to_string())
            .filter(exercise_sets::Column::UserId.eq(principal.user_id().to_string()))
            .one(&tx)
            .await?
        else {
            return Err(DomainError::NotFound);
        };
        exercise_sets::Entity::delete_by_id(id.to_string())
            .exec(&tx)
            .await
            .map_err(mutation_error)?;
        let mut siblings = exercise_sets::Entity::find()
            .filter(exercise_sets::Column::SessionExerciseId.eq(model.session_exercise_id))
            .filter(exercise_sets::Column::UserId.eq(principal.user_id().to_string()))
            .order_by_asc(exercise_sets::Column::Position)
            .all(&tx)
            .await?;
        move_sets_to_temporary(&tx, &mut siblings).await?;
        restore_set_positions(&tx, siblings, None, u64::MAX).await?;
        tx.commit().await.map_err(mutation_error)?;
        Ok(())
    }
}

async fn move_sets_to_temporary<C: ConnectionTrait>(
    connection: &C,
    items: &mut [exercise_sets::Model],
) -> Result<(), DomainError> {
    for (index, item) in items.iter().enumerate() {
        let mut active: exercise_sets::ActiveModel = item.clone().into();
        active.position = Set(TEMP_POSITION_BASE + index as i64);
        active.update(connection).await.map_err(mutation_error)?;
    }
    Ok(())
}

async fn restore_set_positions<C: ConnectionTrait>(
    connection: &C,
    items: Vec<exercise_sets::Model>,
    moved: Option<Uuid>,
    desired: u64,
) -> Result<(), DomainError> {
    let mut index = 0_u64;
    for item in items {
        if index == desired {
            index += 1;
        }
        if moved.is_some_and(|id| item.id == id.to_string()) {
            continue;
        }
        let mut active: exercise_sets::ActiveModel = item.into();
        active.position = Set(index as i64);
        active.update(connection).await.map_err(mutation_error)?;
        index += 1;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::super::{AddSessionExercise, UpdateExerciseSet, UpdateSessionExercise};

    #[tokio::test]
    async fn insert_move_delete_keep_dense_order_and_preserve_full_i64_loads() {
        let (domain, _, owner, _, _, dynamic, _) = memory_domain().await;
        let session = domain.create_session(&owner, strength()).await.unwrap();
        let a = domain
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
        let b = domain
            .add_session_exercise(
                &owner,
                session.id,
                AddSessionExercise {
                    exercise_id: dynamic,
                    position: Some(0),
                },
            )
            .await
            .unwrap();
        let c = domain
            .add_session_exercise(
                &owner,
                session.id,
                AddSessionExercise {
                    exercise_id: dynamic,
                    position: Some(1),
                },
            )
            .await
            .unwrap();
        assert_dense_children(&domain, &owner, session.id).await;
        domain
            .update_session_exercise(
                &owner,
                a.id,
                UpdateSessionExercise {
                    exercise_id: dynamic,
                    position: 0,
                },
            )
            .await
            .unwrap();
        domain.remove_session_exercise(&owner, c.id).await.unwrap();
        assert_dense_children(&domain, &owner, session.id).await;

        let low = domain
            .add_exercise_set(&owner, b.id, dynamic_set(None, i64::MIN))
            .await
            .unwrap();
        let high = domain
            .add_exercise_set(&owner, b.id, dynamic_set(Some(0), i64::MAX))
            .await
            .unwrap();
        let zero = domain
            .add_exercise_set(&owner, b.id, dynamic_set(Some(1), 0))
            .await
            .unwrap();
        domain
            .update_exercise_set(
                &owner,
                low.id,
                UpdateExerciseSet {
                    position: 0,
                    set_type: "amrap".into(),
                    reps: Some(1),
                    hold_sec: None,
                    load_g: i64::MIN,
                },
            )
            .await
            .unwrap();
        domain.remove_exercise_set(&owner, zero.id).await.unwrap();
        assert_dense_sets(&domain, &owner, b.id).await;
        assert_eq!(
            domain
                .get_exercise_set(&owner, low.id)
                .await
                .unwrap()
                .load_g,
            i64::MIN
        );
        assert_eq!(
            domain
                .get_exercise_set(&owner, high.id)
                .await
                .unwrap()
                .load_g,
            i64::MAX
        );
    }
}
