use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, QuerySelect, Set,
};
use uuid::Uuid;

use super::{
    AddSessionExercise, MAX_POSITION, MAX_SESSION_EXERCISES, SessionExercise,
    SessionExerciseSummary, TEMP_POSITION_BASE, UpdateSessionExercise, make_page, mutation_error,
    parse_id,
};
use crate::domain::{
    Domain,
    auth::Principal,
    catalogue::{Page, PageRequest},
    entity::{exercise_sets, exercises, session_exercises, sessions},
    error::DomainError,
};

impl Domain {
    async fn owned_strength_session<C: ConnectionTrait>(
        &self,
        connection: &C,
        principal: &Principal,
        id: Uuid,
    ) -> Result<(), DomainError> {
        let exists = sessions::Entity::find_by_id(id.to_string())
            .filter(sessions::Column::UserId.eq(principal.user_id().to_string()))
            .filter(sessions::Column::ActivityType.eq("strength"))
            .one(connection)
            .await?
            .is_some();
        if exists {
            Ok(())
        } else {
            Err(DomainError::NotFound)
        }
    }

    pub async fn list_session_exercises(
        &self,
        principal: &Principal,
        session_id: Uuid,
        request: PageRequest,
    ) -> Result<Page<SessionExerciseSummary>, DomainError> {
        self.owned_strength_session(&self.db, principal, session_id)
            .await?;
        let (offset, limit) = request.bounded()?;
        let models = session_exercises::Entity::find()
            .filter(session_exercises::Column::SessionId.eq(session_id.to_string()))
            .filter(session_exercises::Column::UserId.eq(principal.user_id().to_string()))
            .order_by_asc(session_exercises::Column::Position)
            .order_by_asc(session_exercises::Column::Id)
            .offset(offset)
            .limit(limit + 1)
            .all(&self.db)
            .await?;
        let mut items = Vec::with_capacity(models.len());
        for model in models {
            items.push(self.session_exercise_summary(&self.db, model).await?);
        }
        Ok(make_page(items, offset, limit))
    }

    async fn session_exercise_summary<C: ConnectionTrait>(
        &self,
        connection: &C,
        model: session_exercises::Model,
    ) -> Result<SessionExerciseSummary, DomainError> {
        let exercise = exercises::Entity::find_by_id(model.exercise_id.clone())
            .one(connection)
            .await?
            .ok_or(DomainError::NotFound)?;
        Ok(SessionExerciseSummary {
            id: parse_id(&model.id)?,
            session_id: parse_id(&model.session_id)?,
            exercise_id: parse_id(&model.exercise_id)?,
            exercise_name: exercise.name,
            contraction_type: model.contraction_type,
            position: u64::try_from(model.position).map_err(|_| DomainError::NotFound)?,
        })
    }

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

    pub async fn get_session_exercise(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<SessionExercise, DomainError> {
        self.get_session_exercise_on(&self.db, principal, id).await
    }

    async fn get_session_exercise_on<C: ConnectionTrait>(
        &self,
        connection: &C,
        principal: &Principal,
        id: Uuid,
    ) -> Result<SessionExercise, DomainError> {
        let model = session_exercises::Entity::find_by_id(id.to_string())
            .filter(session_exercises::Column::UserId.eq(principal.user_id().to_string()))
            .one(connection)
            .await?
            .ok_or(DomainError::NotFound)?;
        self.session_exercise_from_model(connection, principal, model)
            .await
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
            sets: self.all_exercise_sets(connection, principal, id).await?,
        })
    }

    pub async fn add_session_exercise(
        &self,
        principal: &Principal,
        session_id: Uuid,
        input: AddSessionExercise,
    ) -> Result<SessionExercise, DomainError> {
        let tx = self.begin_immediate().await?;
        self.owned_strength_session(&tx, principal, session_id)
            .await?;
        let exercise = exercises::Entity::find_by_id(input.exercise_id.to_string())
            .one(&tx)
            .await?
            .ok_or(DomainError::InvalidInput("unknown exercise_id"))?;
        let mut siblings = session_exercises::Entity::find()
            .filter(session_exercises::Column::SessionId.eq(session_id.to_string()))
            .filter(session_exercises::Column::UserId.eq(principal.user_id().to_string()))
            .order_by_asc(session_exercises::Column::Position)
            .all(&tx)
            .await?;
        if siblings.len() >= MAX_SESSION_EXERCISES {
            return Err(DomainError::InvalidInput("session exercise limit reached"));
        }
        let desired = input.position.unwrap_or(siblings.len() as u64);
        if desired > siblings.len() as u64 || desired > MAX_POSITION {
            return Err(DomainError::InvalidInput("invalid position"));
        }
        move_session_exercises_to_temporary(&tx, &mut siblings).await?;
        let id = Uuid::now_v7();
        session_exercises::ActiveModel {
            id: Set(id.to_string()),
            session_id: Set(session_id.to_string()),
            user_id: Set(principal.user_id().to_string()),
            exercise_id: Set(input.exercise_id.to_string()),
            activity_type: Set("strength".to_owned()),
            contraction_type: Set(exercise.contraction_type),
            position: Set(desired as i64),
        }
        .insert(&tx)
        .await
        .map_err(mutation_error)?;
        restore_session_exercise_positions(&tx, siblings, None, desired).await?;
        let result = self.get_session_exercise_on(&tx, principal, id).await?;
        tx.commit().await.map_err(mutation_error)?;
        Ok(result)
    }

    pub async fn update_session_exercise(
        &self,
        principal: &Principal,
        id: Uuid,
        input: UpdateSessionExercise,
    ) -> Result<SessionExercise, DomainError> {
        if input.position > MAX_POSITION {
            return Err(DomainError::InvalidInput("invalid position"));
        }
        let tx = self.begin_immediate().await?;
        let Some(model) = session_exercises::Entity::find_by_id(id.to_string())
            .filter(session_exercises::Column::UserId.eq(principal.user_id().to_string()))
            .one(&tx)
            .await?
        else {
            return Err(DomainError::NotFound);
        };
        let exercise = exercises::Entity::find_by_id(input.exercise_id.to_string())
            .one(&tx)
            .await?
            .ok_or(DomainError::InvalidInput("unknown exercise_id"))?;
        let set_count = exercise_sets::Entity::find()
            .filter(exercise_sets::Column::SessionExerciseId.eq(id.to_string()))
            .count(&tx)
            .await?;
        if set_count > 0 && exercise.contraction_type != model.contraction_type {
            return Err(DomainError::Conflict);
        }
        let mut siblings = session_exercises::Entity::find()
            .filter(session_exercises::Column::SessionId.eq(model.session_id.clone()))
            .filter(session_exercises::Column::UserId.eq(principal.user_id().to_string()))
            .order_by_asc(session_exercises::Column::Position)
            .all(&tx)
            .await?;
        siblings.retain(|item| item.id != model.id);
        if input.position > siblings.len() as u64 {
            return Err(DomainError::InvalidInput("invalid position"));
        }
        let mut all = siblings.clone();
        all.push(model.clone());
        move_session_exercises_to_temporary(&tx, &mut all).await?;
        let mut active: session_exercises::ActiveModel = model.into();
        active.exercise_id = Set(input.exercise_id.to_string());
        active.contraction_type = Set(exercise.contraction_type);
        active.position = Set(input.position as i64);
        active.update(&tx).await.map_err(mutation_error)?;
        restore_session_exercise_positions(&tx, siblings, Some(id), input.position).await?;
        let result = self.get_session_exercise_on(&tx, principal, id).await?;
        tx.commit().await.map_err(mutation_error)?;
        Ok(result)
    }

    pub async fn remove_session_exercise(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<(), DomainError> {
        let tx = self.begin_immediate().await?;
        let Some(model) = session_exercises::Entity::find_by_id(id.to_string())
            .filter(session_exercises::Column::UserId.eq(principal.user_id().to_string()))
            .one(&tx)
            .await?
        else {
            return Err(DomainError::NotFound);
        };
        session_exercises::Entity::delete_by_id(id.to_string())
            .exec(&tx)
            .await
            .map_err(mutation_error)?;
        let mut siblings = session_exercises::Entity::find()
            .filter(session_exercises::Column::SessionId.eq(model.session_id))
            .filter(session_exercises::Column::UserId.eq(principal.user_id().to_string()))
            .order_by_asc(session_exercises::Column::Position)
            .all(&tx)
            .await?;
        move_session_exercises_to_temporary(&tx, &mut siblings).await?;
        restore_session_exercise_positions(&tx, siblings, None, u64::MAX).await?;
        tx.commit().await.map_err(mutation_error)?;
        Ok(())
    }
}

async fn move_session_exercises_to_temporary<C: ConnectionTrait>(
    connection: &C,
    items: &mut [session_exercises::Model],
) -> Result<(), DomainError> {
    for (index, item) in items.iter().enumerate() {
        let mut active: session_exercises::ActiveModel = item.clone().into();
        active.position = Set(TEMP_POSITION_BASE + index as i64);
        active.update(connection).await.map_err(mutation_error)?;
    }
    Ok(())
}

async fn restore_session_exercise_positions<C: ConnectionTrait>(
    connection: &C,
    items: Vec<session_exercises::Model>,
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
        let mut active: session_exercises::ActiveModel = item.into();
        active.position = Set(index as i64);
        active.update(connection).await.map_err(mutation_error)?;
        index += 1;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::super::{AddSessionExercise, MAX_EXERCISE_SETS, MAX_SESSION_EXERCISES};
    use crate::{
        db,
        domain::{DomainError, catalogue::PageRequest},
    };
    use std::{fs, path::PathBuf, sync::Arc};
    use uuid::Uuid;

    #[tokio::test]
    async fn child_limits_cannot_be_bypassed_by_front_insertion() {
        let (domain, _, owner, _, _, dynamic, _) = memory_domain().await;
        let session = domain.create_session(&owner, strength()).await.unwrap();
        for _ in 0..MAX_SESSION_EXERCISES {
            domain
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
        }
        assert!(matches!(
            domain
                .add_session_exercise(
                    &owner,
                    session.id,
                    AddSessionExercise {
                        exercise_id: dynamic,
                        position: Some(0),
                    },
                )
                .await,
            Err(DomainError::InvalidInput("session exercise limit reached"))
        ));
        assert_dense_children(&domain, &owner, session.id).await;

        let parent = domain
            .list_session_exercises(
                &owner,
                session.id,
                PageRequest {
                    offset: 0,
                    limit: Some(100),
                },
            )
            .await
            .unwrap()
            .items[0]
            .id;
        for _ in 0..MAX_EXERCISE_SETS {
            domain
                .add_exercise_set(&owner, parent, dynamic_set(Some(0), i64::MIN))
                .await
                .unwrap();
        }
        assert!(matches!(
            domain
                .add_exercise_set(&owner, parent, dynamic_set(Some(0), i64::MAX))
                .await,
            Err(DomainError::InvalidInput("exercise set limit reached"))
        ));
        let sets = domain
            .list_exercise_sets(
                &owner,
                parent,
                PageRequest {
                    offset: 0,
                    limit: Some(100),
                },
            )
            .await
            .unwrap();
        assert_eq!(sets.items.len(), 100);
        assert!(sets.items.iter().all(|item| item.load_g == i64::MIN));
        assert_dense_sets(&domain, &owner, parent).await;
    }

    struct TempDatabase {
        directory: PathBuf,
        path: PathBuf,
    }

    impl TempDatabase {
        fn new() -> Self {
            let directory = std::env::temp_dir().join(format!("frater-reorder-{}", Uuid::now_v7()));
            fs::create_dir(&directory).unwrap();
            Self {
                path: directory.join("frater.db"),
                directory,
            }
        }
    }

    impl Drop for TempDatabase {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }

    #[tokio::test]
    async fn file_backed_wal_concurrent_reorders_are_serialized_or_controlled() {
        let temp = TempDatabase::new();
        let database = db::connect(&format!("sqlite://{}?mode=rwc", temp.path.display()))
            .await
            .unwrap();
        let (domain, database, owner, _, _, dynamic, _) = fixture(database).await;
        let domain = Arc::new(domain);
        let session = domain.create_session(&owner, strength()).await.unwrap();
        let add = || {
            let domain = domain.clone();
            let owner = owner.clone();
            async move {
                domain
                    .add_session_exercise(
                        &owner,
                        session.id,
                        AddSessionExercise {
                            exercise_id: dynamic,
                            position: Some(0),
                        },
                    )
                    .await
            }
        };
        let (first, second) = tokio::join!(add(), add());
        for result in [&first, &second] {
            assert!(result.is_ok() || matches!(result, Err(DomainError::TemporarilyUnavailable)));
        }
        let success_count = usize::from(first.is_ok()) + usize::from(second.is_ok());
        assert!(success_count >= 1);
        let page = domain
            .list_session_exercises(
                &owner,
                session.id,
                PageRequest {
                    offset: 0,
                    limit: Some(100),
                },
            )
            .await
            .unwrap();
        assert_eq!(page.items.len(), success_count);
        assert_eq!(
            page.items
                .iter()
                .map(|item| item.position)
                .collect::<Vec<_>>(),
            (0..success_count as u64).collect::<Vec<_>>()
        );
        drop(domain);
        database.close().await.unwrap();
    }
}
