use chrono::{DateTime, Utc};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect, Set,
};
use uuid::Uuid;

use super::{
    ActivityView, CreateActivity, CreateWorkoutSession, MAX_RUN_SPLITS, RunSplit, SessionFilter,
    WorkoutSession, WorkoutSessionSummary, make_page, mutation_error, parse_id, resolve_run_splits,
    run_totals, validate_notes,
};
use crate::domain::{
    Domain,
    auth::Principal,
    catalogue::{Page, PageRequest},
    entity::{run_splits, runs, sessions},
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
    Ok(())
}

pub(super) async fn insert_run_splits<C: ConnectionTrait>(
    connection: &C,
    user_id: &str,
    session_id: Uuid,
    splits: &[RunSplit],
) -> Result<(), DomainError> {
    for (position, split) in splits.iter().enumerate() {
        run_splits::ActiveModel {
            id: Set(Uuid::now_v7().to_string()),
            session_id: Set(session_id.to_string()),
            user_id: Set(user_id.to_owned()),
            activity_type: Set("run".to_owned()),
            position: Set(position as i64),
            distance_m: Set(split.distance_m),
            duration_sec: Set(split.duration_sec),
        }
        .insert(connection)
        .await
        .map_err(mutation_error)?;
    }
    Ok(())
}

async fn all_run_splits<C: ConnectionTrait>(
    connection: &C,
    principal: &Principal,
    session_id: Uuid,
) -> Result<Vec<RunSplit>, DomainError> {
    let models = run_splits::Entity::find()
        .filter(run_splits::Column::SessionId.eq(session_id.to_string()))
        .filter(run_splits::Column::UserId.eq(principal.user_id().to_string()))
        .order_by_asc(run_splits::Column::Position)
        .limit((MAX_RUN_SPLITS + 1) as u64)
        .all(connection)
        .await?;
    if models.len() > MAX_RUN_SPLITS {
        return Err(DomainError::Conflict);
    }
    Ok(models
        .into_iter()
        .map(|model| RunSplit {
            distance_m: model.distance_m,
            duration_sec: model.duration_sec,
        })
        .collect())
}

impl Domain {
    pub async fn create_session(
        &self,
        principal: &Principal,
        input: CreateWorkoutSession,
    ) -> Result<WorkoutSession, DomainError> {
        validate_session(&input)?;
        let splits = match &input.activity {
            CreateActivity::Strength => Vec::new(),
            CreateActivity::Run {
                distance_m,
                duration_sec,
                elevation_gain_m,
                splits,
            } => resolve_run_splits(splits, *distance_m, *duration_sec, *elevation_gain_m)?,
        };
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
            elevation_gain_m, ..
        } = &input.activity
        {
            runs::ActiveModel {
                session_id: Set(id.to_string()),
                user_id: Set(principal.user_id().to_string()),
                activity_type: Set("run".to_owned()),
                elevation_gain_m: Set(*elevation_gain_m),
            }
            .insert(&tx)
            .await
            .map_err(mutation_error)?;
            insert_run_splits(&tx, &principal.user_id().to_string(), id, &splits).await?;
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
                let splits = all_run_splits(connection, principal, id).await?;
                let (distance_m, duration_sec) = run_totals(&splits);
                ActivityView::Run {
                    distance_m,
                    duration_sec,
                    elevation_gain_m: run.elevation_gain_m,
                    splits,
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
    use super::super::test_support::*;
    use super::super::{MAX_RUN_SPLITS, SessionFilter};
    use crate::domain::{catalogue::PageRequest, error::DomainError};

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

    #[tokio::test]
    async fn a_run_keeps_its_splits_in_order_and_derives_its_totals_from_them() {
        let (domain, _, owner, _, _, _, _) = memory_domain().await;
        let laps = vec![split(1_000, 295), split(1_000, 301), split(3_000, 904)];

        let created = domain
            .create_session(&owner, run_from(None, None, laps))
            .await
            .unwrap();
        assert_eq!(
            splits_of(&created),
            vec![(1_000, 295), (1_000, 301), (3_000, 904)]
        );
        assert_eq!(totals_of(&created), (5_000, 1_500));
        let read = domain.get_session(&owner, created.id).await.unwrap();
        assert_eq!(splits_of(&read), splits_of(&created));
        assert_eq!(totals_of(&read), (5_000, 1_500));
    }

    #[tokio::test]
    async fn a_run_given_only_its_totals_is_stored_as_one_split() {
        let (domain, _, owner, _, _, _, _) = memory_domain().await;
        let created = domain
            .create_session(&owner, run_from(Some(8_000), Some(2_400), Vec::new()))
            .await
            .unwrap();
        assert_eq!(splits_of(&created), vec![(8_000, 2_400)]);
        assert_eq!(totals_of(&created), (8_000, 2_400));
    }

    #[tokio::test]
    async fn a_run_needs_its_splits_or_its_totals_and_they_must_agree() {
        let (domain, _, owner, _, _, _, _) = memory_domain().await;
        let laps = || vec![split(4_000, 1_220), split(1_000, 280)];

        assert_eq!(
            splits_of(
                &domain
                    .create_session(&owner, run_from(Some(5_000), Some(1_500), laps()))
                    .await
                    .unwrap()
            ),
            vec![(4_000, 1_220), (1_000, 280)]
        );

        for absent in [
            run_from(None, None, Vec::new()),
            run_from(Some(5_000), None, Vec::new()),
            run_from(None, Some(1_500), Vec::new()),
        ] {
            assert!(matches!(
                domain.create_session(&owner, absent).await,
                Err(DomainError::InvalidInput(
                    "a run needs a distance_m and a duration_sec, as its splits or as its totals"
                ))
            ));
        }

        assert!(matches!(
            domain
                .create_session(&owner, run_from(Some(4_999), Some(1_500), laps()))
                .await,
            Err(DomainError::InvalidInput(message)) if message.contains("distance_m")
        ));
        assert!(matches!(
            domain
                .create_session(&owner, run_from(Some(5_000), Some(1_501), laps()))
                .await,
            Err(DomainError::InvalidInput(message)) if message.contains("duration_sec")
        ));
    }

    #[tokio::test]
    async fn a_split_needs_a_positive_distance_and_duration_and_the_list_is_bounded() {
        let (domain, _, owner, _, _, _, _) = memory_domain().await;
        for laps in [
            vec![split(0, 295)],
            vec![split(-1_000, 295)],
            vec![split(1_000, 0)],
            vec![split(1_000, -295)],
        ] {
            assert!(matches!(
                domain.create_session(&owner, run_with_splits(laps)).await,
                Err(DomainError::InvalidInput(
                    "each run split needs a positive distance_m and duration_sec"
                ))
            ));
        }

        let over_limit = vec![split(1_000, 295); MAX_RUN_SPLITS + 1];
        assert!(matches!(
            domain
                .create_session(&owner, run_with_splits(over_limit))
                .await,
            Err(DomainError::InvalidInput("run split limit reached"))
        ));
        let at_limit = vec![split(50, 15); MAX_RUN_SPLITS];
        assert_eq!(
            splits_of(
                &domain
                    .create_session(&owner, run_with_splits(at_limit))
                    .await
                    .unwrap()
            )
            .len(),
            MAX_RUN_SPLITS
        );
    }

    #[tokio::test]
    async fn the_splits_of_a_run_must_sum_to_its_distance_and_duration() {
        let (domain, _, owner, _, _, _, _) = memory_domain().await;
        assert!(
            domain
                .create_session(
                    &owner,
                    run_with_splits(vec![split(4_000, 1_220), split(1_000, 280)])
                )
                .await
                .is_ok()
        );

        for laps in [
            vec![split(4_000, 1_220), split(999, 280)],
            vec![split(4_000, 1_220), split(1_001, 280)],
        ] {
            assert!(matches!(
                domain.create_session(&owner, run_with_splits(laps)).await,
                Err(DomainError::InvalidInput(message)) if message.contains("distance_m")
            ));
        }

        for laps in [
            vec![split(4_000, 1_220), split(1_000, 279)],
            vec![split(4_000, 1_220), split(1_000, 281)],
        ] {
            assert!(matches!(
                domain.create_session(&owner, run_with_splits(laps)).await,
                Err(DomainError::InvalidInput(message)) if message.contains("duration_sec")
            ));
        }
    }

    #[tokio::test]
    async fn list_workouts_reports_a_run_it_can_page_alongside_its_derived_totals() {
        let (domain, _, owner, _, _, _, _) = memory_domain().await;
        let created = domain
            .create_session(
                &owner,
                run_from(None, None, vec![split(1_000, 295), split(7_000, 2_105)]),
            )
            .await
            .unwrap();
        let listed = domain
            .list_workouts(&owner, SessionFilter::default(), PageRequest::default())
            .await
            .unwrap();
        assert_eq!(listed.items.len(), 1);
        assert_eq!(listed.items[0].id, created.id);
        assert_eq!(
            totals_of(&domain.get_session(&owner, created.id).await.unwrap()),
            (8_000, 2_400)
        );
    }

    #[tokio::test]
    async fn the_splits_of_a_run_are_invisible_to_another_user() {
        let (domain, _, owner, other, superuser, _, _) = memory_domain().await;
        let session = domain
            .create_session(&owner, run_with_splits(vec![split(5_000, 1_500)]))
            .await
            .unwrap();
        for stranger in [&other, &superuser] {
            assert!(matches!(
                domain.get_session(stranger, session.id).await,
                Err(DomainError::NotFound)
            ));
        }
        assert_eq!(
            splits_of(&domain.get_session(&owner, session.id).await.unwrap()),
            vec![(5_000, 1_500)]
        );
    }
}
