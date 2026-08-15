use chrono::{DateTime, Utc};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect, Set,
};
use uuid::Uuid;

use super::{
    ActivityView, CreateActivity, CreateWorkoutSession, SessionFilter, UpdateWorkoutSession,
    WorkoutSession, WorkoutSessionSummary, make_page, mutation_error, parse_id,
};
use crate::domain::{
    Domain,
    auth::Principal,
    catalogue::{Page, PageRequest},
    entity::{runs, session_exercises, sessions},
    error::DomainError,
};

fn session_summary(model: sessions::Model) -> Result<WorkoutSessionSummary, DomainError> {
    Ok(WorkoutSessionSummary {
        id: parse_id(&model.id)?,
        started_at: DateTime::parse_from_rfc3339(&model.started_at)
            .map_err(|_| DomainError::NotFound)?
            .with_timezone(&Utc),
        label: model.label,
        activity_type: model.activity_type,
    })
}

fn validate_session(input: &CreateWorkoutSession) -> Result<(), DomainError> {
    if input
        .label
        .as_ref()
        .is_some_and(|value| value.len() > 256 || value.chars().any(char::is_control))
    {
        return Err(DomainError::InvalidInput(
            "label too long or contains control characters",
        ));
    }
    if let CreateActivity::Run {
        distance_m,
        duration_sec,
        elevation_gain_m,
    } = input.activity
        && (distance_m <= 0 || duration_sec <= 0 || elevation_gain_m < 0)
    {
        return Err(DomainError::InvalidInput("invalid run details"));
    }
    Ok(())
}

impl Domain {
    pub async fn create_session(
        &self,
        principal: &Principal,
        input: CreateWorkoutSession,
    ) -> Result<WorkoutSession, DomainError> {
        validate_session(&input)?;
        let id = Uuid::now_v7();
        let kind = match input.activity {
            CreateActivity::Strength => "strength",
            CreateActivity::Run { .. } => "run",
        };
        let tx = self.begin_immediate().await?;
        sessions::ActiveModel {
            id: Set(id.to_string()),
            user_id: Set(principal.user_id().to_string()),
            started_at: Set(input.started_at.start().to_rfc3339()),
            activity_type: Set(kind.to_owned()),
            label: Set(input.label.clone()),
        }
        .insert(&tx)
        .await
        .map_err(mutation_error)?;
        if let CreateActivity::Run {
            distance_m,
            duration_sec,
            elevation_gain_m,
        } = input.activity
        {
            runs::ActiveModel {
                session_id: Set(id.to_string()),
                user_id: Set(principal.user_id().to_string()),
                activity_type: Set("run".to_owned()),
                distance_m: Set(distance_m),
                duration_sec: Set(duration_sec),
                elevation_gain_m: Set(elevation_gain_m),
            }
            .insert(&tx)
            .await
            .map_err(mutation_error)?;
        }
        let result = self.get_session_on(&tx, principal, id).await?;
        tx.commit().await.map_err(mutation_error)?;
        Ok(result)
    }

    pub async fn list_sessions(
        &self,
        principal: &Principal,
        filter: SessionFilter,
        request: PageRequest,
    ) -> Result<Page<WorkoutSessionSummary>, DomainError> {
        if let Some(activity) = filter.activity.as_deref()
            && !matches!(activity, "strength" | "run")
        {
            return Err(DomainError::InvalidInput("invalid activity filter"));
        }
        if filter
            .started_at_from
            .zip(filter.started_at_to)
            .is_some_and(|(from, to)| from.start() > to.end())
        {
            return Err(DomainError::InvalidInput(
                "started_at_from must not exceed started_at_to",
            ));
        }
        let (offset, limit) = request.bounded()?;
        let mut select = sessions::Entity::find()
            .filter(sessions::Column::UserId.eq(principal.user_id().to_string()));
        if let Some(from) = filter.started_at_from {
            select = select.filter(sessions::Column::StartedAt.gte(from.start().to_rfc3339()));
        }
        if let Some(to) = filter.started_at_to {
            select = select.filter(sessions::Column::StartedAt.lte(to.end().to_rfc3339()));
        }
        if let Some(activity) = filter.activity {
            select = select.filter(sessions::Column::ActivityType.eq(activity));
        }
        let models = select
            .order_by_desc(sessions::Column::StartedAt)
            .order_by_desc(sessions::Column::Id)
            .offset(offset)
            .limit(limit + 1)
            .all(&self.db)
            .await?;
        let items = models
            .into_iter()
            .map(session_summary)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(make_page(items, offset, limit))
    }

    pub async fn get_session(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<WorkoutSession, DomainError> {
        self.get_session_on(&self.db, principal, id).await
    }

    pub(super) async fn get_session_on<C: ConnectionTrait>(
        &self,
        connection: &C,
        principal: &Principal,
        id: Uuid,
    ) -> Result<WorkoutSession, DomainError> {
        let model = sessions::Entity::find_by_id(id.to_string())
            .filter(sessions::Column::UserId.eq(principal.user_id().to_string()))
            .one(connection)
            .await?
            .ok_or(DomainError::NotFound)?;
        self.session_from_model(connection, principal, model).await
    }

    async fn session_from_model<C: ConnectionTrait>(
        &self,
        connection: &C,
        principal: &Principal,
        model: sessions::Model,
    ) -> Result<WorkoutSession, DomainError> {
        let id = parse_id(&model.id)?;
        let activity = match model.activity_type.as_str() {
            "strength" => ActivityView::Strength {
                exercises: self
                    .all_session_exercises(connection, principal, id)
                    .await?,
            },
            "run" => {
                let run = runs::Entity::find_by_id(model.id.clone())
                    .filter(runs::Column::UserId.eq(principal.user_id().to_string()))
                    .one(connection)
                    .await?
                    .ok_or(DomainError::NotFound)?;
                ActivityView::Run {
                    distance_m: run.distance_m,
                    duration_sec: run.duration_sec,
                    elevation_gain_m: run.elevation_gain_m,
                }
            }
            _ => return Err(DomainError::NotFound),
        };
        Ok(WorkoutSession {
            id,
            started_at: DateTime::parse_from_rfc3339(&model.started_at)
                .map_err(|_| DomainError::NotFound)?
                .with_timezone(&Utc),
            label: model.label,
            activity,
        })
    }

    pub async fn update_session(
        &self,
        principal: &Principal,
        id: Uuid,
        input: UpdateWorkoutSession,
    ) -> Result<WorkoutSession, DomainError> {
        validate_session(&input)?;
        let tx = self.begin_immediate().await?;
        let Some(model) = sessions::Entity::find_by_id(id.to_string())
            .filter(sessions::Column::UserId.eq(principal.user_id().to_string()))
            .one(&tx)
            .await?
        else {
            return Err(DomainError::NotFound);
        };
        let new_kind = match input.activity {
            CreateActivity::Strength => "strength",
            CreateActivity::Run { .. } => "run",
        };
        if model.activity_type != new_kind {
            runs::Entity::delete_many()
                .filter(runs::Column::SessionId.eq(id.to_string()))
                .filter(runs::Column::UserId.eq(principal.user_id().to_string()))
                .exec(&tx)
                .await
                .map_err(mutation_error)?;
            session_exercises::Entity::delete_many()
                .filter(session_exercises::Column::SessionId.eq(id.to_string()))
                .filter(session_exercises::Column::UserId.eq(principal.user_id().to_string()))
                .exec(&tx)
                .await
                .map_err(mutation_error)?;
        }
        let mut active: sessions::ActiveModel = model.into();
        active.started_at = Set(input.started_at.start().to_rfc3339());
        active.label = Set(input.label.clone());
        active.activity_type = Set(new_kind.to_owned());
        active.update(&tx).await.map_err(mutation_error)?;
        match input.activity {
            CreateActivity::Strength => {
                runs::Entity::delete_many()
                    .filter(runs::Column::SessionId.eq(id.to_string()))
                    .filter(runs::Column::UserId.eq(principal.user_id().to_string()))
                    .exec(&tx)
                    .await
                    .map_err(mutation_error)?;
            }
            CreateActivity::Run {
                distance_m,
                duration_sec,
                elevation_gain_m,
            } => {
                let existing = runs::Entity::find_by_id(id.to_string()).one(&tx).await?;
                let active = runs::ActiveModel {
                    session_id: Set(id.to_string()),
                    user_id: Set(principal.user_id().to_string()),
                    activity_type: Set("run".to_owned()),
                    distance_m: Set(distance_m),
                    duration_sec: Set(duration_sec),
                    elevation_gain_m: Set(elevation_gain_m),
                };
                if existing.is_some() {
                    active.update(&tx).await.map_err(mutation_error)?;
                } else {
                    active.insert(&tx).await.map_err(mutation_error)?;
                }
            }
        }
        let result = self.get_session_on(&tx, principal, id).await?;
        tx.commit().await.map_err(mutation_error)?;
        Ok(result)
    }

    pub async fn delete_session(&self, principal: &Principal, id: Uuid) -> Result<(), DomainError> {
        let result = sessions::Entity::delete_many()
            .filter(sessions::Column::Id.eq(id.to_string()))
            .filter(sessions::Column::UserId.eq(principal.user_id().to_string()))
            .exec(&self.db)
            .await
            .map_err(mutation_error)?;
        if result.rows_affected == 0 {
            Err(DomainError::NotFound)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::super::{
        ActivityView, AddSessionExercise, CreateActivity, CreateWorkoutSession, SessionFilter,
    };
    use crate::domain::{catalogue::PageRequest, error::DomainError};

    #[tokio::test]
    async fn list_dtos_are_shallow_while_item_reads_are_complete() {
        let (domain, _, owner, _, _, dynamic, _) = memory_domain().await;
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
        domain
            .add_exercise_set(&owner, child.id, dynamic_set(None, -20_000))
            .await
            .unwrap();

        let sessions = serde_json::to_value(
            domain
                .list_sessions(&owner, SessionFilter::default(), PageRequest::default())
                .await
                .unwrap(),
        )
        .unwrap();
        assert!(sessions["items"][0].get("activity").is_none());
        assert!(sessions["items"][0].get("exercises").is_none());
        let children = serde_json::to_value(
            domain
                .list_session_exercises(&owner, session.id, PageRequest::default())
                .await
                .unwrap(),
        )
        .unwrap();
        assert!(children["items"][0].get("sets").is_none());
        let complete =
            serde_json::to_value(domain.get_session(&owner, session.id).await.unwrap()).unwrap();
        assert_eq!(
            complete["activity"]["exercises"][0]["sets"][0]["load_g"],
            -20_000
        );
    }

    #[tokio::test]
    async fn strength_and_run_subtypes_update_atomically() {
        let (domain, _, owner, _, _, dynamic, _) = memory_domain().await;
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
        domain
            .add_exercise_set(&owner, child.id, dynamic_set(None, 10_000))
            .await
            .unwrap();

        let converted = domain
            .update_session(&owner, session.id, run(5_000))
            .await
            .unwrap();
        assert!(matches!(
            converted.activity,
            ActivityView::Run {
                distance_m: 5_000,
                duration_sec: 1_800,
                elevation_gain_m: 25
            }
        ));
        assert!(matches!(
            domain.get_session_exercise(&owner, child.id).await,
            Err(DomainError::NotFound)
        ));
        let changed = domain
            .update_session(&owner, session.id, run(10_000))
            .await
            .unwrap();
        assert!(matches!(
            changed.activity,
            ActivityView::Run {
                distance_m: 10_000,
                ..
            }
        ));

        let invalid = CreateWorkoutSession {
            activity: CreateActivity::Run {
                distance_m: 0,
                duration_sec: 10,
                elevation_gain_m: 0,
            },
            ..run(1)
        };
        assert!(matches!(
            domain.update_session(&owner, session.id, invalid).await,
            Err(DomainError::InvalidInput("invalid run details"))
        ));
        assert!(matches!(
            domain
                .get_session(&owner, session.id)
                .await
                .unwrap()
                .activity,
            ActivityView::Run {
                distance_m: 10_000,
                ..
            }
        ));

        let converted_back = domain
            .update_session(&owner, session.id, strength())
            .await
            .unwrap();
        assert!(
            matches!(converted_back.activity, ActivityView::Strength { ref exercises } if exercises.is_empty())
        );
    }
}
