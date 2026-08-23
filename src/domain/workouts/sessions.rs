use chrono::{DateTime, Utc};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect, Set,
};
use uuid::Uuid;

use super::{
    ActivityView, CreateActivity, CreateWorkoutSession, SessionFilter, WorkoutSession,
    WorkoutSessionSummary, make_page, mutation_error, parse_id, validate_notes,
};
use crate::domain::{
    Domain,
    auth::Principal,
    catalogue::{Page, PageRequest},
    entity::{runs, sessions},
    error::DomainError,
};

fn session_summary(model: sessions::Model) -> Result<WorkoutSessionSummary, DomainError> {
    Ok(WorkoutSessionSummary {
        id: parse_id(&model.id)?,
        started_at: DateTime::parse_from_rfc3339(&model.started_at)
            .map_err(|_| DomainError::NotFound)?
            .with_timezone(&Utc),
        label: model.label,
        notes: model.notes,
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
    validate_notes(input.notes.as_ref())?;
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
            notes: Set(input.notes.clone()),
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
            notes: model.notes,
            activity,
        })
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
    use super::super::SessionFilter;
    use super::super::test_support::*;
    use crate::domain::catalogue::PageRequest;

    #[tokio::test]
    async fn list_dtos_are_shallow_while_item_reads_are_complete() {
        let (domain, _, owner, _, _, dynamic, _) = memory_domain().await;
        let session = log_one_set(&domain, &owner, dynamic, -20_000).await;

        let sessions = serde_json::to_value(
            domain
                .list_sessions(&owner, SessionFilter::default(), PageRequest::default())
                .await
                .unwrap(),
        )
        .unwrap();
        assert!(sessions["items"][0].get("activity").is_none());
        assert!(sessions["items"][0].get("exercises").is_none());
        let complete =
            serde_json::to_value(domain.get_session(&owner, session.id).await.unwrap()).unwrap();
        assert_eq!(
            complete["activity"]["exercises"][0]["sets"][0]["load_g"],
            -20_000
        );
    }
}
