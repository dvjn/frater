use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect, Set,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{
    Domain,
    auth::Principal,
    catalogue::{Page, PageRequest},
    entity::bodyweight_readings,
    error::DomainError,
    workouts::Timestamp,
};

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogBodyweight {
    pub recorded_on: Timestamp,
    pub mass_g: i64,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BodyweightFilter {
    pub from: Option<Timestamp>,
    pub to: Option<Timestamp>,
}

#[derive(Clone, Debug, Serialize)]
pub struct BodyweightReading {
    pub id: Uuid,
    pub recorded_on: String,
    pub mass_g: i64,
}

/// The share of bodyweight a movement moves, as a percentage of it.
pub const MAX_BODYWEIGHT_SHARE: i64 = 100;

/// Above any real human mass, and low enough that no effective load saturates.
pub const MAX_BODYWEIGHT_G: i64 = 1_000_000;

/// `load_g` is signed: positive is added load, negative is machine or band
/// assistance. Assistance can exceed bodyweight, so the effective load floors
/// at zero rather than crediting negative work.
pub fn effective_load_g(bodyweight_g: i64, bodyweight_share: i64, load_g: i64) -> i64 {
    let carried = bodyweight_g.saturating_mul(bodyweight_share) / 100;
    carried.saturating_add(load_g).max(0)
}

/// Every reading that can answer a date in one batch of workouts, sorted by
/// date. Three range queries cover any batch, so no workout costs a query.
pub(super) struct BodyweightTimeline {
    readings: Vec<(String, i64)>,
}

impl BodyweightTimeline {
    /// The most recent reading on or before `date`. With no earlier reading the
    /// earliest later one is used: a weight measured after the workout is a
    /// closer estimate than no weight at all.
    pub(super) fn on(&self, date: &str) -> Option<i64> {
        self.readings
            .iter()
            .rev()
            .find(|(recorded_on, _)| recorded_on.as_str() <= date)
            .or_else(|| self.readings.first())
            .map(|(_, mass_g)| *mass_g)
    }
}

fn parse_id(value: &str) -> Result<Uuid, DomainError> {
    Uuid::parse_str(value).map_err(|_| DomainError::NotFound)
}

fn mutation_error(error: sea_orm::DbErr) -> DomainError {
    tracing::debug!(error = %error, "bodyweight mutation rejected by database");
    DomainError::Conflict
}

fn recorded_on(value: Timestamp) -> String {
    value.start().date_naive().to_string()
}

fn view(model: bodyweight_readings::Model) -> Result<BodyweightReading, DomainError> {
    Ok(BodyweightReading {
        id: parse_id(&model.id)?,
        recorded_on: model.recorded_on,
        mass_g: model.mass_g,
    })
}

impl Domain {
    pub(super) async fn bodyweight_timeline(
        &self,
        user_id: &str,
        dates: &[String],
    ) -> Result<BodyweightTimeline, DomainError> {
        let Some(first) = dates.iter().min() else {
            return Ok(BodyweightTimeline {
                readings: Vec::new(),
            });
        };
        let last = dates.iter().max().expect("a non-empty batch has a maximum");
        let mine = || {
            bodyweight_readings::Entity::find()
                .filter(bodyweight_readings::Column::UserId.eq(user_id.to_owned()))
        };
        let mut readings = mine()
            .filter(bodyweight_readings::Column::RecordedOn.lte(first.clone()))
            .order_by_desc(bodyweight_readings::Column::RecordedOn)
            .limit(1)
            .all(&self.db)
            .await?;
        readings.extend(
            mine()
                .filter(bodyweight_readings::Column::RecordedOn.gt(first.clone()))
                .filter(bodyweight_readings::Column::RecordedOn.lte(last.clone()))
                .order_by_asc(bodyweight_readings::Column::RecordedOn)
                .all(&self.db)
                .await?,
        );
        readings.extend(
            mine()
                .filter(bodyweight_readings::Column::RecordedOn.gt(last.clone()))
                .order_by_asc(bodyweight_readings::Column::RecordedOn)
                .limit(1)
                .all(&self.db)
                .await?,
        );
        let mut readings = readings
            .into_iter()
            .map(|model| (model.recorded_on, model.mass_g))
            .collect::<Vec<_>>();
        readings.sort_by(|left, right| left.0.cmp(&right.0));
        Ok(BodyweightTimeline { readings })
    }

    pub async fn log_bodyweight(
        &self,
        principal: &Principal,
        input: LogBodyweight,
    ) -> Result<BodyweightReading, DomainError> {
        if !(1..=MAX_BODYWEIGHT_G).contains(&input.mass_g) {
            return Err(DomainError::InvalidInput(
                "mass_g must be between 1 and 1000000",
            ));
        }
        // A timestamp names an instant in UTC, so accepting one would move a
        // reading to the wrong day for any client that is not on UTC.
        if !input.recorded_on.is_date_only() {
            return Err(DomainError::InvalidInput(
                "recorded_on must be a date, as YYYY-MM-DD, not a timestamp",
            ));
        }
        let user_id = principal.user_id().to_string();
        let recorded_on = recorded_on(input.recorded_on);
        let tx = self.begin_immediate().await?;
        let existing = bodyweight_readings::Entity::find()
            .filter(bodyweight_readings::Column::UserId.eq(user_id.clone()))
            .filter(bodyweight_readings::Column::RecordedOn.eq(recorded_on.clone()))
            .one(&tx)
            .await?;
        let model = match existing {
            Some(model) => {
                let mut active: bodyweight_readings::ActiveModel = model.into();
                active.mass_g = Set(input.mass_g);
                active.update(&tx).await.map_err(mutation_error)?
            }
            None => bodyweight_readings::ActiveModel {
                id: Set(Uuid::now_v7().to_string()),
                user_id: Set(user_id),
                recorded_on: Set(recorded_on),
                mass_g: Set(input.mass_g),
            }
            .insert(&tx)
            .await
            .map_err(mutation_error)?,
        };
        let reading = view(model)?;
        tx.commit().await.map_err(mutation_error)?;
        Ok(reading)
    }

    pub async fn list_bodyweight(
        &self,
        principal: &Principal,
        filter: BodyweightFilter,
        request: PageRequest,
    ) -> Result<Page<BodyweightReading>, DomainError> {
        if filter
            .from
            .zip(filter.to)
            .is_some_and(|(from, to)| from.start() > to.end())
        {
            return Err(DomainError::InvalidInput("from must not be later than to"));
        }
        let (offset, limit) = request.bounded()?;
        let mut select = bodyweight_readings::Entity::find()
            .filter(bodyweight_readings::Column::UserId.eq(principal.user_id().to_string()));
        if let Some(from) = filter.from {
            select = select.filter(bodyweight_readings::Column::RecordedOn.gte(recorded_on(from)));
        }
        if let Some(to) = filter.to {
            select = select.filter(
                bodyweight_readings::Column::RecordedOn.lte(to.end().date_naive().to_string()),
            );
        }
        let mut items = select
            .order_by_desc(bodyweight_readings::Column::RecordedOn)
            .order_by_desc(bodyweight_readings::Column::Id)
            .offset(offset)
            .limit(limit + 1)
            .all(&self.db)
            .await?
            .into_iter()
            .map(view)
            .collect::<Result<Vec<_>, DomainError>>()?;
        let has_more = items.len() as u64 > limit;
        if has_more {
            items.pop();
        }
        Ok(Page {
            items,
            next_offset: has_more.then_some(offset + limit),
        })
    }

    pub async fn delete_bodyweight(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<(), DomainError> {
        let result = bodyweight_readings::Entity::delete_many()
            .filter(bodyweight_readings::Column::Id.eq(id.to_string()))
            .filter(bodyweight_readings::Column::UserId.eq(principal.user_id().to_string()))
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
    use super::*;
    use crate::domain::workouts::test_support::memory_domain;

    fn reading(recorded_on: &str, mass_g: i64) -> LogBodyweight {
        LogBodyweight {
            recorded_on: Timestamp::parse(recorded_on).unwrap(),
            mass_g,
        }
    }

    #[test]
    fn the_five_worked_cases_of_effective_load() {
        assert_eq!(effective_load_g(70_000, 0, 32_000), 32_000);
        assert_eq!(effective_load_g(70_000, 100, 0), 70_000);
        assert_eq!(effective_load_g(70_000, 100, 5_000), 75_000);
        assert_eq!(effective_load_g(70_000, 100, -27_000), 43_000);
        assert_eq!(effective_load_g(70_000, 65, 0), 45_500);
    }

    #[test]
    fn assistance_beyond_bodyweight_clamps_to_zero() {
        assert_eq!(effective_load_g(70_000, 100, -90_000), 0);
        assert_eq!(effective_load_g(0, 0, -27_000), 0);
    }

    /// An instant in UTC lands on the previous or the next day for a client in
    /// another zone, so the reading must name the date itself.
    #[tokio::test]
    async fn a_timestamp_is_rejected_rather_than_truncated_to_a_date() {
        let (domain, _, owner, _, _, _, _) = memory_domain().await;
        assert!(matches!(
            domain
                .log_bodyweight(&owner, reading("2026-08-01T23:00:00Z", 70_000))
                .await,
            Err(DomainError::InvalidInput(
                "recorded_on must be a date, as YYYY-MM-DD, not a timestamp"
            ))
        ));
        assert!(
            domain
                .log_bodyweight(&owner, reading("2026-08-01", 70_000))
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn a_reading_is_upserted_for_one_date_and_scoped_to_its_owner() {
        let (domain, _, owner, other, _, _, _) = memory_domain().await;
        let first = domain
            .log_bodyweight(&owner, reading("2026-03-01", 70_000))
            .await
            .unwrap();
        let second = domain
            .log_bodyweight(&owner, reading("2026-03-01", 71_000))
            .await
            .unwrap();
        assert_eq!(first.id, second.id);
        assert_eq!(second.mass_g, 71_000);
        assert_eq!(second.recorded_on, "2026-03-01");
        let listed = domain
            .list_bodyweight(&owner, BodyweightFilter::default(), PageRequest::default())
            .await
            .unwrap();
        assert_eq!(listed.items.len(), 1);

        assert!(
            domain
                .list_bodyweight(&other, BodyweightFilter::default(), PageRequest::default())
                .await
                .unwrap()
                .items
                .is_empty()
        );
        assert!(matches!(
            domain.delete_bodyweight(&other, first.id).await,
            Err(DomainError::NotFound)
        ));
        domain.delete_bodyweight(&owner, first.id).await.unwrap();
        assert!(matches!(
            domain.delete_bodyweight(&owner, first.id).await,
            Err(DomainError::NotFound)
        ));
    }

    #[tokio::test]
    async fn readings_are_filtered_and_paged() {
        let (domain, _, owner, _, _, _, _) = memory_domain().await;
        for (day, mass_g) in [
            ("2026-03-01", 70_000),
            ("2026-03-08", 71_000),
            ("2026-03-15", 72_000),
        ] {
            domain
                .log_bodyweight(&owner, reading(day, mass_g))
                .await
                .unwrap();
        }
        let window = domain
            .list_bodyweight(
                &owner,
                BodyweightFilter {
                    from: Some(Timestamp::parse("2026-03-08").unwrap()),
                    to: Some(Timestamp::parse("2026-03-15").unwrap()),
                },
                PageRequest::default(),
            )
            .await
            .unwrap();
        assert_eq!(
            window
                .items
                .iter()
                .map(|item| item.recorded_on.as_str())
                .collect::<Vec<_>>(),
            vec!["2026-03-15", "2026-03-08"]
        );

        let page = domain
            .list_bodyweight(
                &owner,
                BodyweightFilter::default(),
                PageRequest {
                    offset: 0,
                    limit: Some(2),
                },
            )
            .await
            .unwrap();
        assert_eq!(page.items.len(), 2);
        assert_eq!(page.next_offset, Some(2));

        assert!(matches!(
            domain
                .list_bodyweight(
                    &owner,
                    BodyweightFilter {
                        from: Some(Timestamp::parse("2026-03-15").unwrap()),
                        to: Some(Timestamp::parse("2026-03-01").unwrap()),
                    },
                    PageRequest::default()
                )
                .await,
            Err(DomainError::InvalidInput("from must not be later than to"))
        ));
        assert!(matches!(
            domain
                .log_bodyweight(&owner, reading("2026-03-01", 0))
                .await,
            Err(DomainError::InvalidInput(
                "mass_g must be between 1 and 1000000"
            ))
        ));
    }

    #[tokio::test]
    async fn a_mass_is_bounded_so_that_no_effective_load_saturates() {
        let (domain, _, owner, _, _, _, _) = memory_domain().await;
        let heaviest = domain
            .log_bodyweight(&owner, reading("2026-03-01", MAX_BODYWEIGHT_G))
            .await
            .unwrap();
        assert_eq!(heaviest.mass_g, MAX_BODYWEIGHT_G);
        assert!(matches!(
            domain
                .log_bodyweight(&owner, reading("2026-03-02", MAX_BODYWEIGHT_G + 1))
                .await,
            Err(DomainError::InvalidInput(
                "mass_g must be between 1 and 1000000"
            ))
        ));
    }

    #[tokio::test]
    async fn a_timeline_picks_the_latest_earlier_reading_or_the_earliest_later_one() {
        let (domain, _, owner, _, _, _, _) = memory_domain().await;
        for (day, mass_g) in [("2026-03-01", 70_000), ("2026-03-15", 72_000)] {
            domain
                .log_bodyweight(&owner, reading(day, mass_g))
                .await
                .unwrap();
        }
        let user_id = owner.user_id().to_string();
        let dates = ["2026-02-01", "2026-03-01", "2026-03-08", "2026-03-20"]
            .map(str::to_owned)
            .to_vec();
        let timeline = domain.bodyweight_timeline(&user_id, &dates).await.unwrap();
        assert_eq!(timeline.on("2026-02-01"), Some(70_000));
        assert_eq!(timeline.on("2026-03-01"), Some(70_000));
        assert_eq!(timeline.on("2026-03-08"), Some(70_000));
        assert_eq!(timeline.on("2026-03-20"), Some(72_000));

        let empty = domain.bodyweight_timeline(&user_id, &[]).await.unwrap();
        assert_eq!(empty.on("2026-03-01"), None);
    }
}
