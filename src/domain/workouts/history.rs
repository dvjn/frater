use std::collections::HashMap;

use chrono::{DateTime, Utc};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{SessionFilter, Timestamp, make_page, parse_id};
use crate::domain::{
    Domain,
    auth::Principal,
    catalogue::{Page, PageRequest},
    entity::{exercise_sets, exercises, session_exercises, sessions},
    error::DomainError,
};

pub const MAX_HISTORY_SESSIONS: u64 = 365;
const ID_CHUNK: usize = 200;

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatsRange {
    pub from: Option<Timestamp>,
    pub to: Option<Timestamp>,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorkoutListEntry {
    pub id: Uuid,
    pub started_at: DateTime<Utc>,
    pub label: Option<String>,
    pub notes: Option<String>,
    pub activity_type: String,
    pub exercise_count: u64,
    pub set_count: u64,
    pub volume_g: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct ExerciseHistorySet {
    pub position: u64,
    pub set_type: String,
    pub reps: Option<i64>,
    pub hold_sec: Option<i64>,
    pub load_g: i64,
    pub effective_load_g: i64,
    pub estimated_1rm_g: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ExerciseHistoryEntry {
    pub session_id: Uuid,
    pub session_exercise_id: Uuid,
    pub started_at: DateTime<Utc>,
    pub label: Option<String>,
    pub volume_g: i64,
    pub sets: Vec<ExerciseHistorySet>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PersonalRecord {
    pub exercise_id: Uuid,
    pub exercise_name: String,
    pub max_load_g: Option<i64>,
    pub max_load_reps: Option<i64>,
    pub best_estimated_1rm_g: Option<i64>,
    pub best_estimated_1rm_load_g: Option<i64>,
    pub best_estimated_1rm_reps: Option<i64>,
    pub longest_hold_sec: Option<i64>,
    pub set_count: u64,
    pub last_performed_at: DateTime<Utc>,
}

/// What an exercise moves besides its external load: the share of bodyweight the
/// movement carries, and the bodyweight that applied on the day of the session.
#[derive(Clone, Copy, Debug, Default)]
struct LoadContext {
    bodyweight_share: i64,
    bodyweight_g: Option<i64>,
}

type LoadedSet<'a> = (LoadContext, &'a exercise_sets::Model);

fn effective_load_g(context: LoadContext, model: &exercise_sets::Model) -> i64 {
    crate::domain::bodyweight::effective_load_g(
        context.bodyweight_g.unwrap_or(0),
        context.bodyweight_share,
        model.load_g,
    )
}

fn estimated_1rm_g(load_g: i64, reps: Option<i64>) -> Option<i64> {
    let reps = reps?;
    if load_g <= 0 || reps <= 0 {
        return None;
    }
    Some(load_g.saturating_mul(30 + reps) / 30)
}

fn set_volume_g(context: LoadContext, model: &exercise_sets::Model) -> i64 {
    model.reps.filter(|reps| *reps > 0).map_or(0, |reps| {
        effective_load_g(context, model).saturating_mul(reps)
    })
}

/// A single set is already allowed to hold `i64::MAX` grams, so a plain sum
/// would overflow on the second one.
fn total_volume_g<'a>(sets: impl Iterator<Item = LoadedSet<'a>>) -> i64 {
    sets.fold(0, |total, (context, set)| {
        total.saturating_add(set_volume_g(context, set))
    })
}

fn started_at(model: &sessions::Model) -> Result<DateTime<Utc>, DomainError> {
    Ok(DateTime::parse_from_rfc3339(&model.started_at)
        .map_err(|_| DomainError::NotFound)?
        .with_timezone(&Utc))
}

struct HistoryExercise {
    model: session_exercises::Model,
    sets: Vec<exercise_sets::Model>,
    load: LoadContext,
}

struct HistorySession {
    model: sessions::Model,
    exercises: Vec<HistoryExercise>,
}

impl HistorySession {
    fn loaded_sets(&self) -> impl Iterator<Item = LoadedSet<'_>> {
        self.exercises.iter().flat_map(HistoryExercise::loaded_sets)
    }
}

impl HistoryExercise {
    fn loaded_sets(&self) -> impl Iterator<Item = LoadedSet<'_>> {
        self.sets.iter().map(|set| (self.load, set))
    }
}

impl Domain {
    async fn load_history(
        &self,
        principal: &Principal,
        range: StatsRange,
        exercise_id: Option<Uuid>,
        limit: u64,
    ) -> Result<Vec<HistorySession>, DomainError> {
        if range
            .from
            .zip(range.to)
            .is_some_and(|(from, to)| from.start() > to.end())
        {
            return Err(DomainError::InvalidInput("from must not be later than to"));
        }
        if !(1..=MAX_HISTORY_SESSIONS).contains(&limit) {
            return Err(DomainError::InvalidInput(
                "limit must be between 1 and 365 sessions",
            ));
        }
        let user_id = principal.user_id().to_string();
        let mut select =
            sessions::Entity::find().filter(sessions::Column::UserId.eq(user_id.clone()));
        if let Some(from) = range.from {
            select = select.filter(sessions::Column::StartedAt.gte(from.start().to_rfc3339()));
        }
        if let Some(to) = range.to {
            select = select.filter(sessions::Column::StartedAt.lte(to.end().to_rfc3339()));
        }
        let session_models = select
            .order_by_desc(sessions::Column::StartedAt)
            .order_by_desc(sessions::Column::Id)
            .limit(limit)
            .all(&self.db)
            .await?;
        self.hydrate_sessions(&user_id, session_models, exercise_id)
            .await
    }

    /// Loads the exercises and sets of already selected sessions in bounded
    /// chunks, so one query per session is never issued.
    async fn hydrate_sessions(
        &self,
        user_id: &str,
        session_models: Vec<sessions::Model>,
        exercise_id: Option<Uuid>,
    ) -> Result<Vec<HistorySession>, DomainError> {
        if session_models.is_empty() {
            return Ok(Vec::new());
        }

        let session_ids = session_models
            .iter()
            .map(|model| model.id.clone())
            .collect::<Vec<_>>();
        let mut exercise_models = Vec::new();
        for chunk in session_ids.chunks(ID_CHUNK) {
            let mut select = session_exercises::Entity::find()
                .filter(session_exercises::Column::UserId.eq(user_id.to_owned()))
                .filter(session_exercises::Column::SessionId.is_in(chunk.to_vec()));
            if let Some(exercise_id) = exercise_id {
                select = select
                    .filter(session_exercises::Column::ExerciseId.eq(exercise_id.to_string()));
            }
            exercise_models.extend(
                select
                    .order_by_asc(session_exercises::Column::Position)
                    .order_by_asc(session_exercises::Column::Id)
                    .all(&self.db)
                    .await?,
            );
        }

        let exercise_ids = exercise_models
            .iter()
            .map(|model| model.id.clone())
            .collect::<Vec<_>>();
        let mut sets_by_exercise: HashMap<String, Vec<exercise_sets::Model>> = HashMap::new();
        for chunk in exercise_ids.chunks(ID_CHUNK) {
            let models = exercise_sets::Entity::find()
                .filter(exercise_sets::Column::UserId.eq(user_id.to_owned()))
                .filter(exercise_sets::Column::SessionExerciseId.is_in(chunk.to_vec()))
                .order_by_asc(exercise_sets::Column::Position)
                .order_by_asc(exercise_sets::Column::Id)
                .all(&self.db)
                .await?;
            for model in models {
                sets_by_exercise
                    .entry(model.session_exercise_id.clone())
                    .or_default()
                    .push(model);
            }
        }

        let mut shares: HashMap<String, i64> = HashMap::new();
        let catalogue_ids = exercise_models
            .iter()
            .map(|model| model.exercise_id.clone())
            .collect::<Vec<_>>();
        for chunk in catalogue_ids.chunks(ID_CHUNK) {
            for model in exercises::Entity::find()
                .filter(exercises::Column::Id.is_in(chunk.to_vec()))
                .all(&self.db)
                .await?
            {
                shares.insert(model.id, model.bodyweight_share);
            }
        }

        let dates = session_models
            .iter()
            .map(|model| Ok(started_at(model)?.date_naive().to_string()))
            .collect::<Result<Vec<_>, DomainError>>()?;
        let timeline = self.bodyweight_timeline(user_id, &dates).await?;
        let bodyweight_by_session = session_models
            .iter()
            .zip(dates)
            .map(|(model, date)| (model.id.clone(), timeline.on(&date)))
            .collect::<HashMap<_, _>>();

        let mut exercises_by_session: HashMap<String, Vec<HistoryExercise>> = HashMap::new();
        for model in exercise_models {
            let sets = sets_by_exercise.remove(&model.id).unwrap_or_default();
            let load = LoadContext {
                bodyweight_share: shares.get(&model.exercise_id).copied().unwrap_or(0),
                bodyweight_g: bodyweight_by_session
                    .get(&model.session_id)
                    .copied()
                    .flatten(),
            };
            exercises_by_session
                .entry(model.session_id.clone())
                .or_default()
                .push(HistoryExercise { model, sets, load });
        }

        Ok(session_models
            .into_iter()
            .filter_map(|model| {
                let exercises = exercises_by_session.remove(&model.id).unwrap_or_default();
                if exercise_id.is_some() && exercises.is_empty() {
                    return None;
                }
                Some(HistorySession { model, exercises })
            })
            .collect())
    }

    /// One listing for the whole workout log: session filters with per-session
    /// totals.
    pub async fn list_workouts(
        &self,
        principal: &Principal,
        filter: SessionFilter,
        request: PageRequest,
    ) -> Result<Page<WorkoutListEntry>, DomainError> {
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
        let user_id = principal.user_id().to_string();
        let mut select =
            sessions::Entity::find().filter(sessions::Column::UserId.eq(user_id.clone()));
        if let Some(from) = filter.started_at_from {
            select = select.filter(sessions::Column::StartedAt.gte(from.start().to_rfc3339()));
        }
        if let Some(to) = filter.started_at_to {
            select = select.filter(sessions::Column::StartedAt.lte(to.end().to_rfc3339()));
        }
        if let Some(activity) = filter.activity {
            select = select.filter(sessions::Column::ActivityType.eq(activity));
        }
        let session_models = select
            .order_by_desc(sessions::Column::StartedAt)
            .order_by_desc(sessions::Column::Id)
            .offset(offset)
            .limit(limit + 1)
            .all(&self.db)
            .await?;
        let sessions = self
            .hydrate_sessions(&user_id, session_models, None)
            .await?;
        let entries = sessions
            .into_iter()
            .map(|session| {
                Ok(WorkoutListEntry {
                    id: parse_id(&session.model.id)?,
                    started_at: started_at(&session.model)?,
                    label: session.model.label.clone(),
                    notes: session.model.notes.clone(),
                    activity_type: session.model.activity_type.clone(),
                    exercise_count: session.exercises.len() as u64,
                    set_count: session
                        .exercises
                        .iter()
                        .map(|exercise| exercise.sets.len() as u64)
                        .sum(),
                    volume_g: total_volume_g(session.loaded_sets()),
                })
            })
            .collect::<Result<Vec<_>, DomainError>>()?;
        Ok(make_page(entries, offset, limit))
    }

    pub async fn exercise_history(
        &self,
        principal: &Principal,
        exercise_id: Uuid,
        range: StatsRange,
        limit: Option<u64>,
    ) -> Result<Vec<ExerciseHistoryEntry>, DomainError> {
        let sessions = self
            .load_history(principal, range, Some(exercise_id), limit.unwrap_or(20))
            .await?;
        let mut entries = Vec::new();
        for session in sessions {
            let started_at = started_at(&session.model)?;
            for exercise in session.exercises {
                entries.push(ExerciseHistoryEntry {
                    session_id: parse_id(&session.model.id)?,
                    session_exercise_id: parse_id(&exercise.model.id)?,
                    started_at,
                    label: session.model.label.clone(),
                    volume_g: total_volume_g(exercise.loaded_sets()),
                    sets: exercise
                        .loaded_sets()
                        .map(|(context, set)| {
                            let effective_load_g = effective_load_g(context, set);
                            Ok(ExerciseHistorySet {
                                position: u64::try_from(set.position)
                                    .map_err(|_| DomainError::NotFound)?,
                                set_type: set.set_type.clone(),
                                reps: set.reps,
                                hold_sec: set.hold_sec,
                                load_g: set.load_g,
                                effective_load_g,
                                estimated_1rm_g: estimated_1rm_g(effective_load_g, set.reps),
                            })
                        })
                        .collect::<Result<Vec<_>, DomainError>>()?,
                });
            }
        }
        Ok(entries)
    }

    pub async fn personal_records(
        &self,
        principal: &Principal,
        exercise_id: Option<Uuid>,
        range: StatsRange,
    ) -> Result<Vec<PersonalRecord>, DomainError> {
        let sessions = self
            .load_history(principal, range, exercise_id, MAX_HISTORY_SESSIONS)
            .await?;
        let mut records: HashMap<String, PersonalRecord> = HashMap::new();
        for session in sessions {
            let started_at = started_at(&session.model)?;
            for exercise in session.exercises {
                let key = exercise.model.exercise_id.clone();
                let record = match records.get_mut(&key) {
                    Some(record) => record,
                    None => {
                        let name = exercises::Entity::find_by_id(key.clone())
                            .one(&self.db)
                            .await?
                            .ok_or(DomainError::NotFound)?
                            .name;
                        records.entry(key.clone()).or_insert(PersonalRecord {
                            exercise_id: parse_id(&key)?,
                            exercise_name: name,
                            max_load_g: None,
                            max_load_reps: None,
                            best_estimated_1rm_g: None,
                            best_estimated_1rm_load_g: None,
                            best_estimated_1rm_reps: None,
                            longest_hold_sec: None,
                            set_count: 0,
                            last_performed_at: started_at,
                        })
                    }
                };
                record.set_count += exercise.sets.len() as u64;
                if started_at > record.last_performed_at {
                    record.last_performed_at = started_at;
                }
                for (context, set) in exercise.loaded_sets() {
                    let load_g = effective_load_g(context, set);
                    let carries_load = set.reps.is_some_and(|reps| reps > 0)
                        && (set.load_g >= 0 || context.bodyweight_share > 0);
                    if carries_load && record.max_load_g.is_none_or(|best| load_g > best) {
                        record.max_load_g = Some(load_g);
                        record.max_load_reps = set.reps;
                    }
                    if carries_load
                        && let Some(estimate) = estimated_1rm_g(load_g, set.reps)
                        && record
                            .best_estimated_1rm_g
                            .is_none_or(|best| estimate > best)
                    {
                        record.best_estimated_1rm_g = Some(estimate);
                        record.best_estimated_1rm_load_g = Some(load_g);
                        record.best_estimated_1rm_reps = set.reps;
                    }
                    if let Some(hold_sec) = set.hold_sec
                        && record.longest_hold_sec.is_none_or(|best| hold_sec > best)
                    {
                        record.longest_hold_sec = Some(hold_sec);
                    }
                }
            }
        }
        let mut records = records.into_values().collect::<Vec<_>>();
        records.sort_by(|left, right| left.exercise_name.cmp(&right.exercise_name));
        Ok(records)
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use crate::domain::{
        LogBodyweight,
        workouts::{AddExerciseSet, LogWorkout, LogWorkoutExercise},
    };
    use sea_orm::ConnectionTrait;

    fn working(reps: i64, load_g: i64) -> AddExerciseSet {
        AddExerciseSet {
            position: None,
            set_type: "working".into(),
            reps: Some(reps),
            hold_sec: None,
            load_g,
            notes: None,
        }
    }

    fn hold(hold_sec: i64) -> AddExerciseSet {
        AddExerciseSet {
            position: None,
            set_type: "working".into(),
            reps: None,
            hold_sec: Some(hold_sec),
            load_g: 0,
            notes: None,
        }
    }

    fn range(from: &str, to: &str) -> StatsRange {
        StatsRange {
            from: Some(Timestamp::parse(from).unwrap()),
            to: Some(Timestamp::parse(to).unwrap()),
        }
    }

    fn between(from: &str, to: &str) -> SessionFilter {
        SessionFilter {
            started_at_from: Some(Timestamp::parse(from).unwrap()),
            started_at_to: Some(Timestamp::parse(to).unwrap()),
            activity: None,
        }
    }

    async fn logged() -> (crate::domain::Domain, crate::domain::Principal, Uuid) {
        let (domain, _, owner, _, _, dynamic_id, _) = memory_domain().await;
        for (day, load) in [("2026-03-01", 60_000), ("2026-03-08", 70_000)] {
            domain
                .log_workout(
                    &owner,
                    LogWorkout {
                        started_at: Timestamp::parse(day).unwrap(),
                        label: Some("Leg day".into()),
                        notes: None,
                        exercises: vec![LogWorkoutExercise {
                            exercise_id: dynamic_id,
                            notes: None,
                            sets: vec![working(5, load), working(5, load)],
                        }],
                    },
                )
                .await
                .unwrap();
        }
        (domain, owner, dynamic_id)
    }

    #[tokio::test]
    async fn list_workouts_totals_filters_and_paginates() {
        let (domain, owner, _) = logged().await;
        let all = domain
            .list_workouts(&owner, SessionFilter::default(), PageRequest::default())
            .await
            .unwrap();
        assert_eq!(all.items.len(), 2);
        assert_eq!(all.items[0].set_count, 2);
        assert_eq!(all.items[0].volume_g, 700_000);
        assert_eq!(all.items[0].exercise_count, 1);
        assert_eq!(all.next_offset, None);

        let first_only = domain
            .list_workouts(
                &owner,
                between("2026-03-01", "2026-03-01"),
                PageRequest::default(),
            )
            .await
            .unwrap();
        assert_eq!(first_only.items.len(), 1);
        assert_eq!(first_only.items[0].volume_g, 600_000);

        let runs = domain
            .list_workouts(
                &owner,
                SessionFilter {
                    activity: Some("run".into()),
                    ..SessionFilter::default()
                },
                PageRequest::default(),
            )
            .await
            .unwrap();
        assert!(runs.items.is_empty());

        let page = domain
            .list_workouts(
                &owner,
                SessionFilter::default(),
                PageRequest {
                    offset: 0,
                    limit: Some(1),
                },
            )
            .await
            .unwrap();
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.next_offset, Some(1));
        let second = domain
            .list_workouts(
                &owner,
                SessionFilter::default(),
                PageRequest {
                    offset: 1,
                    limit: Some(1),
                },
            )
            .await
            .unwrap();
        assert_eq!(second.items.len(), 1);
        assert_ne!(second.items[0].id, page.items[0].id);

        assert!(matches!(
            domain
                .list_workouts(
                    &owner,
                    between("2026-03-08", "2026-03-01"),
                    PageRequest::default()
                )
                .await,
            Err(DomainError::InvalidInput(
                "started_at_from must not exceed started_at_to"
            ))
        ));
        assert!(matches!(
            domain
                .list_workouts(
                    &owner,
                    SessionFilter {
                        activity: Some("swim".into()),
                        ..SessionFilter::default()
                    },
                    PageRequest::default()
                )
                .await,
            Err(DomainError::InvalidInput("invalid activity filter"))
        ));
        assert!(matches!(
            domain
                .list_workouts(
                    &owner,
                    SessionFilter::default(),
                    PageRequest {
                        offset: 0,
                        limit: Some(101),
                    }
                )
                .await,
            Err(DomainError::InvalidInput("invalid pagination bounds"))
        ));
    }

    #[tokio::test]
    async fn exercise_history_returns_sets_newest_first() {
        let (domain, owner, exercise_id) = logged().await;
        let history = domain
            .exercise_history(&owner, exercise_id, StatsRange::default(), None)
            .await
            .unwrap();
        assert_eq!(history.len(), 2);
        assert!(history[0].started_at > history[1].started_at);
        assert_eq!(history[0].sets.len(), 2);
        assert_eq!(history[0].sets[0].load_g, 70_000);
        assert_eq!(history[0].sets[0].estimated_1rm_g, Some(81_666));

        let unknown = domain
            .exercise_history(&owner, Uuid::now_v7(), StatsRange::default(), None)
            .await
            .unwrap();
        assert!(unknown.is_empty());
    }

    #[tokio::test]
    async fn personal_records_track_max_load_and_best_estimate() {
        let (domain, owner, exercise_id) = logged().await;
        let records = domain
            .personal_records(&owner, None, StatsRange::default())
            .await
            .unwrap();
        assert_eq!(records.len(), 1);
        let record = &records[0];
        assert_eq!(record.exercise_id, exercise_id);
        assert_eq!(record.max_load_g, Some(70_000));
        assert_eq!(record.max_load_reps, Some(5));
        assert_eq!(record.best_estimated_1rm_g, Some(81_666));
        assert_eq!(record.set_count, 4);

        let early = domain
            .personal_records(&owner, None, range("2026-03-01", "2026-03-02"))
            .await
            .unwrap();
        assert_eq!(early[0].max_load_g, Some(60_000));
    }

    #[tokio::test]
    async fn the_history_range_and_limit_bounds_are_enforced() {
        let (domain, owner, exercise_id) = logged().await;
        assert!(matches!(
            domain
                .exercise_history(&owner, exercise_id, range("2026-03-08", "2026-03-01"), None)
                .await,
            Err(DomainError::InvalidInput("from must not be later than to"))
        ));
        assert!(matches!(
            domain
                .personal_records(&owner, None, range("2026-03-08", "2026-03-01"))
                .await,
            Err(DomainError::InvalidInput("from must not be later than to"))
        ));
        for limit in [0, MAX_HISTORY_SESSIONS + 1] {
            assert!(matches!(
                domain
                    .exercise_history(&owner, exercise_id, StatsRange::default(), Some(limit))
                    .await,
                Err(DomainError::InvalidInput(
                    "limit must be between 1 and 365 sessions"
                ))
            ));
        }
        assert!(
            domain
                .exercise_history(
                    &owner,
                    exercise_id,
                    StatsRange::default(),
                    Some(MAX_HISTORY_SESSIONS)
                )
                .await
                .is_ok()
        );
    }

    /// A date-only `to` covers the whole day, so a session late in that day
    /// must still be inside the range.
    #[tokio::test]
    async fn a_date_only_bound_reaches_the_end_of_its_day() {
        let (domain, _, owner, _, _, dynamic_id, _) = memory_domain().await;
        domain
            .log_workout(
                &owner,
                LogWorkout {
                    started_at: Timestamp::parse("2026-03-08T18:30:00Z").unwrap(),
                    label: Some("evening".into()),
                    notes: None,
                    exercises: vec![LogWorkoutExercise {
                        exercise_id: dynamic_id,
                        notes: None,
                        sets: vec![working(5, 60_000)],
                    }],
                },
            )
            .await
            .unwrap();

        let listed = domain
            .list_workouts(
                &owner,
                between("2026-03-08", "2026-03-08"),
                PageRequest::default(),
            )
            .await
            .unwrap();
        assert_eq!(listed.items.len(), 1);
        assert_eq!(listed.items[0].label.as_deref(), Some("evening"));

        let records = domain
            .personal_records(&owner, None, range("2026-03-08", "2026-03-08"))
            .await
            .unwrap();
        assert_eq!(records.len(), 1);

        let day_before = domain
            .list_workouts(
                &owner,
                between("2026-03-07", "2026-03-07"),
                PageRequest::default(),
            )
            .await
            .unwrap();
        assert!(day_before.items.is_empty());
    }

    /// One set may hold `i64::MAX` grams, so the totals must saturate instead
    /// of overflowing on the second one.
    #[tokio::test]
    async fn a_volume_total_saturates_instead_of_wrapping() {
        let (domain, _, owner, _, _, dynamic_id, _) = memory_domain().await;
        domain
            .log_workout(
                &owner,
                LogWorkout {
                    started_at: Timestamp::parse("2026-03-09").unwrap(),
                    label: None,
                    notes: None,
                    exercises: vec![LogWorkoutExercise {
                        exercise_id: dynamic_id,
                        notes: None,
                        sets: vec![working(1, i64::MAX), working(1, i64::MAX)],
                    }],
                },
            )
            .await
            .unwrap();

        let listed = domain
            .list_workouts(&owner, SessionFilter::default(), PageRequest::default())
            .await
            .unwrap();
        assert_eq!(listed.items[0].volume_g, i64::MAX);

        let history = domain
            .exercise_history(&owner, dynamic_id, StatsRange::default(), None)
            .await
            .unwrap();
        assert_eq!(history[0].volume_g, i64::MAX);
    }

    #[tokio::test]
    async fn list_workouts_reports_the_session_notes() {
        let (domain, _, owner, _, _, dynamic_id, _) = memory_domain().await;
        domain
            .log_workout(
                &owner,
                LogWorkout {
                    started_at: Timestamp::parse("2026-03-10").unwrap(),
                    label: None,
                    notes: Some("slept badly".into()),
                    exercises: vec![LogWorkoutExercise {
                        exercise_id: dynamic_id,
                        notes: None,
                        sets: vec![working(5, 60_000)],
                    }],
                },
            )
            .await
            .unwrap();
        let listed = domain
            .list_workouts(&owner, SessionFilter::default(), PageRequest::default())
            .await
            .unwrap();
        assert_eq!(listed.items[0].notes.as_deref(), Some("slept badly"));
    }

    async fn mixed_loads() -> (crate::domain::Domain, crate::domain::Principal, Uuid, Uuid) {
        let (domain, _, owner, _, _, dynamic_id, isometric_id) = memory_domain().await;
        domain
            .log_workout(
                &owner,
                LogWorkout {
                    started_at: Timestamp::parse("2026-03-01").unwrap(),
                    label: None,
                    notes: None,
                    exercises: vec![
                        LogWorkoutExercise {
                            exercise_id: dynamic_id,
                            notes: None,
                            sets: vec![working(5, 60_000), working(8, -27_000), working(10, 0)],
                        },
                        LogWorkoutExercise {
                            exercise_id: isometric_id,
                            notes: None,
                            sets: vec![hold(45)],
                        },
                    ],
                },
            )
            .await
            .unwrap();
        (domain, owner, dynamic_id, isometric_id)
    }

    #[tokio::test]
    async fn assistance_sets_are_excluded_from_volume() {
        let (domain, owner, _, _) = mixed_loads().await;
        let sessions = domain
            .list_workouts(&owner, SessionFilter::default(), PageRequest::default())
            .await
            .unwrap()
            .items;
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].set_count, 4);
        assert_eq!(sessions[0].volume_g, 300_000);
    }

    #[tokio::test]
    async fn exercise_history_keeps_negative_load_but_drops_it_from_metrics() {
        let (domain, owner, exercise_id, _) = mixed_loads().await;
        let history = domain
            .exercise_history(&owner, exercise_id, StatsRange::default(), None)
            .await
            .unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].volume_g, 300_000);

        let assistance = history[0]
            .sets
            .iter()
            .find(|set| set.load_g < 0)
            .expect("assistance set is still reported");
        assert_eq!(assistance.load_g, -27_000);
        assert_eq!(assistance.reps, Some(8));
        assert_eq!(assistance.estimated_1rm_g, None);

        let bodyweight = history[0]
            .sets
            .iter()
            .find(|set| set.load_g == 0)
            .expect("bodyweight set is reported");
        assert_eq!(bodyweight.estimated_1rm_g, None);
    }

    #[tokio::test]
    async fn assistance_sets_never_become_personal_records() {
        let (domain, owner, dynamic_id, isometric_id) = mixed_loads().await;
        let records = domain
            .personal_records(&owner, Some(dynamic_id), StatsRange::default())
            .await
            .unwrap();
        assert_eq!(records[0].max_load_g, Some(60_000));
        assert_eq!(records[0].max_load_reps, Some(5));
        assert_eq!(records[0].best_estimated_1rm_load_g, Some(60_000));
        assert_eq!(records[0].set_count, 3);

        let isometric = domain
            .personal_records(&owner, Some(isometric_id), StatsRange::default())
            .await
            .unwrap();
        assert_eq!(isometric[0].longest_hold_sec, Some(45));
        assert_eq!(isometric[0].max_load_g, None);
    }

    #[tokio::test]
    async fn assistance_only_exercise_has_no_max_load_and_zero_volume() {
        let (domain, _, owner, _, _, dynamic_id, _) = memory_domain().await;
        domain
            .log_workout(
                &owner,
                LogWorkout {
                    started_at: Timestamp::parse("2026-03-01").unwrap(),
                    label: None,
                    notes: None,
                    exercises: vec![LogWorkoutExercise {
                        exercise_id: dynamic_id,
                        notes: None,
                        sets: vec![working(8, -27_000), working(8, -13_000)],
                    }],
                },
            )
            .await
            .unwrap();

        let history = domain
            .exercise_history(&owner, dynamic_id, StatsRange::default(), None)
            .await
            .unwrap();
        assert_eq!(history[0].volume_g, 0);
        assert_eq!(
            history[0]
                .sets
                .iter()
                .map(|set| set.load_g)
                .collect::<Vec<_>>(),
            vec![-27_000, -13_000]
        );

        let records = domain
            .personal_records(&owner, None, StatsRange::default())
            .await
            .unwrap();
        assert_eq!(records[0].max_load_g, None);
        assert_eq!(records[0].best_estimated_1rm_g, None);
        assert_eq!(records[0].set_count, 2);
    }

    struct BodyweightFixture {
        domain: crate::domain::Domain,
        owner: crate::domain::Principal,
        lat_pulldown: Uuid,
        pull_up: Uuid,
        push_up: Uuid,
    }

    async fn bodyweight_fixture() -> BodyweightFixture {
        let (domain, database, owner, _, _, lat_pulldown, _) = memory_domain().await;
        let pull_up = Uuid::now_v7();
        let push_up = Uuid::now_v7();
        database
            .execute_unprepared(&format!(
                "INSERT INTO exercises(id,name,contraction_type,bodyweight_share) VALUES('{pull_up}','Pull-up','dynamic',100),('{push_up}','Push-up','dynamic',65)"
            ))
            .await
            .unwrap();
        BodyweightFixture {
            domain,
            owner,
            lat_pulldown,
            pull_up,
            push_up,
        }
    }

    async fn weigh(fixture: &BodyweightFixture, day: &str, mass_g: i64) {
        fixture
            .domain
            .log_bodyweight(
                &fixture.owner,
                LogBodyweight {
                    recorded_on: Timestamp::parse(day).unwrap(),
                    mass_g,
                },
            )
            .await
            .unwrap();
    }

    async fn log(fixture: &BodyweightFixture, day: &str, exercises: Vec<LogWorkoutExercise>) {
        fixture
            .domain
            .log_workout(
                &fixture.owner,
                LogWorkout {
                    started_at: Timestamp::parse(day).unwrap(),
                    label: None,
                    notes: None,
                    exercises,
                },
            )
            .await
            .unwrap();
    }

    fn entry(exercise_id: Uuid, sets: Vec<AddExerciseSet>) -> LogWorkoutExercise {
        LogWorkoutExercise {
            exercise_id,
            notes: None,
            sets,
        }
    }

    async fn volume_of(fixture: &BodyweightFixture, exercise_id: Uuid) -> i64 {
        fixture
            .domain
            .exercise_history(&fixture.owner, exercise_id, StatsRange::default(), None)
            .await
            .unwrap()[0]
            .volume_g
    }

    #[tokio::test]
    async fn the_share_of_bodyweight_an_exercise_carries_becomes_volume() {
        let fixture = bodyweight_fixture().await;
        weigh(&fixture, "2026-03-01", 70_000).await;
        log(
            &fixture,
            "2026-03-01",
            vec![
                entry(fixture.lat_pulldown, vec![working(10, 32_000)]),
                entry(
                    fixture.pull_up,
                    vec![working(10, 0), working(3, 5_000), working(8, -27_000)],
                ),
                entry(fixture.push_up, vec![working(20, 0)]),
            ],
        )
        .await;

        assert_eq!(volume_of(&fixture, fixture.lat_pulldown).await, 320_000);
        assert_eq!(volume_of(&fixture, fixture.push_up).await, 910_000);
        // 70 kg x 10, then 75 kg x 3, then 43 kg x 8.
        assert_eq!(volume_of(&fixture, fixture.pull_up).await, 1_269_000);

        let history = fixture
            .domain
            .exercise_history(&fixture.owner, fixture.pull_up, StatsRange::default(), None)
            .await
            .unwrap();
        assert_eq!(
            history[0]
                .sets
                .iter()
                .map(|set| (set.load_g, set.effective_load_g))
                .collect::<Vec<_>>(),
            vec![(0, 70_000), (5_000, 75_000), (-27_000, 43_000)]
        );
    }

    #[tokio::test]
    async fn assistance_beyond_bodyweight_never_reduces_a_volume() {
        let fixture = bodyweight_fixture().await;
        weigh(&fixture, "2026-03-01", 70_000).await;
        log(
            &fixture,
            "2026-03-01",
            vec![entry(fixture.pull_up, vec![working(8, -90_000)])],
        )
        .await;
        assert_eq!(volume_of(&fixture, fixture.pull_up).await, 0);
        let history = fixture
            .domain
            .exercise_history(&fixture.owner, fixture.pull_up, StatsRange::default(), None)
            .await
            .unwrap();
        assert_eq!(history[0].sets[0].effective_load_g, 0);
    }

    /// Without a reading a bodyweight exercise falls back to its external
    /// load, which for a plain pull-up is nothing.
    #[tokio::test]
    async fn a_share_without_a_reading_counts_only_the_external_load() {
        let fixture = bodyweight_fixture().await;
        log(
            &fixture,
            "2026-03-01",
            vec![
                entry(fixture.pull_up, vec![working(10, 0), working(3, 5_000)]),
                entry(fixture.lat_pulldown, vec![working(5, 32_000)]),
            ],
        )
        .await;
        assert_eq!(volume_of(&fixture, fixture.pull_up).await, 15_000);
        assert_eq!(volume_of(&fixture, fixture.lat_pulldown).await, 160_000);

        weigh(&fixture, "2026-03-01", 70_000).await;
        assert_eq!(volume_of(&fixture, fixture.pull_up).await, 925_000);
    }

    /// An exercise that carries no bodyweight must report exactly the volume it
    /// reported before bodyweight existed, reading or no reading.
    #[tokio::test]
    async fn a_share_of_zero_keeps_the_volume_it_always_had() {
        let fixture = bodyweight_fixture().await;
        log(
            &fixture,
            "2026-03-01",
            vec![entry(
                fixture.lat_pulldown,
                vec![working(5, 60_000), working(5, 60_000)],
            )],
        )
        .await;
        assert_eq!(volume_of(&fixture, fixture.lat_pulldown).await, 600_000);
        weigh(&fixture, "2026-03-01", 70_000).await;
        assert_eq!(volume_of(&fixture, fixture.lat_pulldown).await, 600_000);
    }

    /// A 0 kg record is truthful, and it must stay distinguishable from an
    /// exercise that was never performed.
    #[tokio::test]
    async fn a_share_of_zero_with_no_load_still_earns_a_max_load_record() {
        let fixture = bodyweight_fixture().await;
        log(
            &fixture,
            "2026-03-01",
            vec![entry(fixture.lat_pulldown, vec![working(5, 0)])],
        )
        .await;
        let records = fixture
            .domain
            .personal_records(
                &fixture.owner,
                Some(fixture.lat_pulldown),
                StatsRange::default(),
            )
            .await
            .unwrap();
        assert_eq!(records[0].max_load_g, Some(0));
        assert_eq!(records[0].max_load_reps, Some(5));
    }

    /// Band assistance lowers the effective load but does not remove it, so the
    /// record follows the effective load and both branches see the same set.
    #[tokio::test]
    async fn an_assisted_pull_up_earns_a_max_load_from_its_effective_load() {
        let fixture = bodyweight_fixture().await;
        weigh(&fixture, "2026-03-01", 70_000).await;
        log(
            &fixture,
            "2026-03-01",
            vec![entry(fixture.pull_up, vec![working(5, -20_000)])],
        )
        .await;
        let records = fixture
            .domain
            .personal_records(&fixture.owner, Some(fixture.pull_up), StatsRange::default())
            .await
            .unwrap();
        assert_eq!(records[0].max_load_g, Some(50_000));
        assert_eq!(records[0].max_load_reps, Some(5));
        assert_eq!(records[0].best_estimated_1rm_load_g, Some(50_000));
        assert_eq!(records[0].best_estimated_1rm_reps, Some(5));
    }

    #[tokio::test]
    async fn a_pull_up_earns_a_max_load_and_an_epley_estimate() {
        let fixture = bodyweight_fixture().await;
        weigh(&fixture, "2026-03-01", 70_000).await;
        log(
            &fixture,
            "2026-03-01",
            vec![entry(
                fixture.pull_up,
                vec![working(10, 0), working(3, 5_000)],
            )],
        )
        .await;
        let records = fixture
            .domain
            .personal_records(&fixture.owner, Some(fixture.pull_up), StatsRange::default())
            .await
            .unwrap();
        assert_eq!(records[0].max_load_g, Some(75_000));
        assert_eq!(records[0].max_load_reps, Some(3));
        assert_eq!(records[0].best_estimated_1rm_g, Some(93_333));
        assert_eq!(records[0].best_estimated_1rm_load_g, Some(70_000));
        assert_eq!(records[0].best_estimated_1rm_reps, Some(10));
    }

    /// The reading of the day of the workout, not the newest one on record.
    #[tokio::test]
    async fn a_workout_between_two_readings_uses_the_earlier_one() {
        let fixture = bodyweight_fixture().await;
        weigh(&fixture, "2026-03-01", 70_000).await;
        weigh(&fixture, "2026-03-15", 80_000).await;
        for day in ["2026-02-20", "2026-03-08", "2026-03-20"] {
            log(
                &fixture,
                day,
                vec![entry(fixture.pull_up, vec![working(1, 0)])],
            )
            .await;
        }
        let history = fixture
            .domain
            .exercise_history(&fixture.owner, fixture.pull_up, StatsRange::default(), None)
            .await
            .unwrap();
        assert_eq!(
            history
                .iter()
                .map(|entry| entry.volume_g)
                .collect::<Vec<_>>(),
            // Newest first: after both readings, between them, and before both.
            vec![80_000, 70_000, 70_000]
        );
    }
}
