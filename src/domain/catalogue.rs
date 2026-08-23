use std::collections::HashSet;

use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect, Set,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{
    Domain,
    auth::Principal,
    bodyweight::MAX_BODYWEIGHT_SHARE,
    entity::{equipment, exercise_equipment, exercise_muscles, exercises, muscles},
    error::DomainError,
};

pub const DEFAULT_PAGE_LIMIT: u64 = 50;
pub const MAX_PAGE_LIMIT: u64 = 100;
pub const MAX_PAGE_OFFSET: u64 = 100_000;
pub const MAX_EXERCISE_ASSOCIATIONS: usize = 100;

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageRequest {
    #[serde(default)]
    pub offset: u64,
    pub limit: Option<u64>,
}

impl PageRequest {
    pub fn bounded(&self) -> Result<(u64, u64), DomainError> {
        let limit = self.limit.unwrap_or(DEFAULT_PAGE_LIMIT);
        if self.offset > MAX_PAGE_OFFSET || !(1..=MAX_PAGE_LIMIT).contains(&limit) {
            return Err(DomainError::InvalidInput("invalid pagination bounds"));
        }
        Ok((self.offset, limit))
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_offset: Option<u64>,
}

fn page<T>(mut items: Vec<T>, offset: u64, limit: u64) -> Page<T> {
    let has_more = items.len() as u64 > limit;
    if has_more {
        items.pop();
    }
    Page {
        items,
        next_offset: has_more.then_some(offset + limit),
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Muscle {
    pub id: Uuid,
    pub name: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Equipment {
    pub id: Uuid,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamedInput {
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExerciseMuscleInput {
    pub muscle_id: Uuid,
    pub role: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExerciseInput {
    pub name: String,
    pub contraction_type: String,
    pub bodyweight_share: i64,
    #[serde(default)]
    pub muscles: Vec<ExerciseMuscleInput>,
    #[serde(default)]
    pub equipment_ids: Vec<Uuid>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ExerciseSummary {
    pub id: Uuid,
    pub name: String,
    pub contraction_type: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Exercise {
    pub id: Uuid,
    pub name: String,
    pub contraction_type: String,
    pub bodyweight_share: i64,
    pub primary_muscles: Vec<Muscle>,
    pub secondary_muscles: Vec<Muscle>,
    pub equipment: Vec<Equipment>,
}

#[derive(Clone, Debug)]
pub enum Lookup<T> {
    Found(T),
    Missing,
    Ambiguous(Vec<T>),
}

pub const MAX_LOOKUP_CANDIDATES: u64 = 10;

fn rank_candidates<T>(items: Vec<T>, query: &str, name_of: impl Fn(&T) -> &str) -> Lookup<T> {
    let query = query.trim().to_lowercase();
    let mut exact = Vec::new();
    let mut prefix = Vec::new();
    let mut substring = Vec::new();
    for item in items {
        let name = name_of(&item).to_lowercase();
        if name == query {
            exact.push(item);
        } else if name.starts_with(&query) {
            prefix.push(item);
        } else {
            substring.push(item);
        }
    }
    for mut bucket in [exact, prefix, substring] {
        match bucket.len() {
            0 => continue,
            1 => return Lookup::Found(bucket.pop().expect("one match")),
            _ => return Lookup::Ambiguous(bucket),
        }
    }
    Lookup::Missing
}

fn require_catalogue_admin(principal: &Principal) -> Result<(), DomainError> {
    if principal.role() == "superuser" {
        Ok(())
    } else {
        Err(DomainError::Forbidden)
    }
}

fn validate_name(name: &str) -> Result<String, DomainError> {
    let name = name.trim();
    if name.is_empty() || name.len() > 128 || name.chars().any(char::is_control) {
        return Err(DomainError::InvalidInput(
            "name must be 1-128 non-control characters",
        ));
    }
    Ok(name.to_owned())
}

fn validate_exercise(input: &ExerciseInput) -> Result<String, DomainError> {
    let name = validate_name(&input.name)?;
    if input.muscles.len() > MAX_EXERCISE_ASSOCIATIONS
        || input.equipment_ids.len() > MAX_EXERCISE_ASSOCIATIONS
    {
        return Err(DomainError::InvalidInput("too many exercise associations"));
    }
    if !matches!(input.contraction_type.as_str(), "dynamic" | "isometric") {
        return Err(DomainError::InvalidInput("invalid contraction_type"));
    }
    if !(0..=MAX_BODYWEIGHT_SHARE).contains(&input.bodyweight_share) {
        return Err(DomainError::InvalidInput(
            "bodyweight_share must be between 0 and 100",
        ));
    }
    let mut muscles = HashSet::new();
    for association in &input.muscles {
        if !matches!(association.role.as_str(), "primary" | "secondary") {
            return Err(DomainError::InvalidInput("invalid muscle role"));
        }
        if !muscles.insert(association.muscle_id) {
            return Err(DomainError::InvalidInput("duplicate muscle association"));
        }
    }
    let mut equipment = HashSet::new();
    if input.equipment_ids.iter().any(|id| !equipment.insert(*id)) {
        return Err(DomainError::InvalidInput("duplicate equipment association"));
    }
    Ok(name)
}

fn parse_id(value: &str) -> Result<Uuid, DomainError> {
    Uuid::parse_str(value).map_err(|_| DomainError::NotFound)
}

fn mutation_error(error: sea_orm::DbErr) -> DomainError {
    tracing::debug!(error = %error, "catalogue mutation rejected by database");
    DomainError::Conflict
}

fn validate_lookup_query(reference: &str) -> Result<&str, DomainError> {
    let query = reference.trim();
    if query.is_empty() || query.len() > 128 || query.chars().any(char::is_control) {
        return Err(DomainError::InvalidInput(
            "reference must be a UUID or a name of 1-128 non-control characters",
        ));
    }
    Ok(query)
}

fn validate_query(query: Option<&str>) -> Result<Option<&str>, DomainError> {
    let query = query.map(str::trim).filter(|query| !query.is_empty());
    if query.is_some_and(|query| query.len() > 128 || query.chars().any(char::is_control)) {
        return Err(DomainError::InvalidInput("invalid name query"));
    }
    Ok(query)
}

impl Domain {
    pub async fn resolve_muscle(&self, reference: &str) -> Result<Lookup<Muscle>, DomainError> {
        if let Ok(id) = Uuid::parse_str(reference.trim()) {
            return Ok(match self.get_muscle(id).await {
                Ok(muscle) => Lookup::Found(muscle),
                Err(DomainError::NotFound) => Lookup::Missing,
                Err(error) => return Err(error),
            });
        }
        let query = validate_lookup_query(reference)?;
        let items = self
            .list_muscles(
                Some(query),
                PageRequest {
                    offset: 0,
                    limit: Some(MAX_LOOKUP_CANDIDATES),
                },
            )
            .await?
            .items;
        Ok(rank_candidates(items, query, |item| item.name.as_str()))
    }

    pub async fn resolve_equipment(
        &self,
        reference: &str,
    ) -> Result<Lookup<Equipment>, DomainError> {
        if let Ok(id) = Uuid::parse_str(reference.trim()) {
            return Ok(match self.get_equipment(id).await {
                Ok(equipment) => Lookup::Found(equipment),
                Err(DomainError::NotFound) => Lookup::Missing,
                Err(error) => return Err(error),
            });
        }
        let query = validate_lookup_query(reference)?;
        let items = self
            .list_equipment(
                Some(query),
                PageRequest {
                    offset: 0,
                    limit: Some(MAX_LOOKUP_CANDIDATES),
                },
            )
            .await?
            .items;
        Ok(rank_candidates(items, query, |item| item.name.as_str()))
    }

    pub async fn resolve_exercise(
        &self,
        reference: &str,
    ) -> Result<Lookup<ExerciseSummary>, DomainError> {
        if let Ok(id) = Uuid::parse_str(reference.trim()) {
            return Ok(match self.get_exercise(id).await {
                Ok(exercise) => Lookup::Found(ExerciseSummary {
                    id: exercise.id,
                    name: exercise.name,
                    contraction_type: exercise.contraction_type,
                }),
                Err(DomainError::NotFound) => Lookup::Missing,
                Err(error) => return Err(error),
            });
        }
        let query = validate_lookup_query(reference)?;
        let items = self
            .list_exercises(
                Some(query),
                PageRequest {
                    offset: 0,
                    limit: Some(MAX_LOOKUP_CANDIDATES),
                },
            )
            .await?
            .items;
        Ok(rank_candidates(items, query, |item| item.name.as_str()))
    }

    pub async fn list_muscles(
        &self,
        query: Option<&str>,
        request: PageRequest,
    ) -> Result<Page<Muscle>, DomainError> {
        let (offset, limit) = request.bounded()?;
        let mut select = muscles::Entity::find();
        if let Some(query) = validate_query(query)? {
            select = select.filter(muscles::Column::Name.contains(query));
        }
        let models = select
            .order_by_asc(muscles::Column::Name)
            .order_by_asc(muscles::Column::Id)
            .offset(offset)
            .limit(limit + 1)
            .all(&self.db)
            .await?;
        let items = models
            .into_iter()
            .map(|model| {
                Ok(Muscle {
                    id: parse_id(&model.id)?,
                    name: model.name,
                })
            })
            .collect::<Result<Vec<_>, DomainError>>()?;
        Ok(page(items, offset, limit))
    }

    pub async fn get_muscle(&self, id: Uuid) -> Result<Muscle, DomainError> {
        let model = muscles::Entity::find_by_id(id.to_string())
            .one(&self.db)
            .await?
            .ok_or(DomainError::NotFound)?;
        Ok(Muscle {
            id,
            name: model.name,
        })
    }

    pub async fn create_muscle(
        &self,
        principal: &Principal,
        input: NamedInput,
    ) -> Result<Muscle, DomainError> {
        require_catalogue_admin(principal)?;
        let id = Uuid::now_v7();
        let name = validate_name(&input.name)?;
        muscles::ActiveModel {
            id: Set(id.to_string()),
            name: Set(name.clone()),
        }
        .insert(&self.db)
        .await
        .map_err(mutation_error)?;
        Ok(Muscle { id, name })
    }

    pub async fn update_muscle(
        &self,
        principal: &Principal,
        id: Uuid,
        input: NamedInput,
    ) -> Result<Muscle, DomainError> {
        require_catalogue_admin(principal)?;
        let Some(model) = muscles::Entity::find_by_id(id.to_string())
            .one(&self.db)
            .await?
        else {
            return Err(DomainError::NotFound);
        };
        let name = validate_name(&input.name)?;
        let mut active: muscles::ActiveModel = model.into();
        active.name = Set(name.clone());
        active.update(&self.db).await.map_err(mutation_error)?;
        Ok(Muscle { id, name })
    }

    pub async fn delete_muscle(&self, principal: &Principal, id: Uuid) -> Result<(), DomainError> {
        require_catalogue_admin(principal)?;
        let result = muscles::Entity::delete_by_id(id.to_string())
            .exec(&self.db)
            .await
            .map_err(mutation_error)?;
        if result.rows_affected == 0 {
            Err(DomainError::NotFound)
        } else {
            Ok(())
        }
    }

    pub async fn list_equipment(
        &self,
        query: Option<&str>,
        request: PageRequest,
    ) -> Result<Page<Equipment>, DomainError> {
        let (offset, limit) = request.bounded()?;
        let mut select = equipment::Entity::find();
        if let Some(query) = validate_query(query)? {
            select = select.filter(equipment::Column::Name.contains(query));
        }
        let models = select
            .order_by_asc(equipment::Column::Name)
            .order_by_asc(equipment::Column::Id)
            .offset(offset)
            .limit(limit + 1)
            .all(&self.db)
            .await?;
        let items = models
            .into_iter()
            .map(|model| {
                Ok(Equipment {
                    id: parse_id(&model.id)?,
                    name: model.name,
                })
            })
            .collect::<Result<Vec<_>, DomainError>>()?;
        Ok(page(items, offset, limit))
    }

    pub async fn get_equipment(&self, id: Uuid) -> Result<Equipment, DomainError> {
        let model = equipment::Entity::find_by_id(id.to_string())
            .one(&self.db)
            .await?
            .ok_or(DomainError::NotFound)?;
        Ok(Equipment {
            id,
            name: model.name,
        })
    }

    pub async fn create_equipment(
        &self,
        principal: &Principal,
        input: NamedInput,
    ) -> Result<Equipment, DomainError> {
        require_catalogue_admin(principal)?;
        let id = Uuid::now_v7();
        let name = validate_name(&input.name)?;
        equipment::ActiveModel {
            id: Set(id.to_string()),
            name: Set(name.clone()),
        }
        .insert(&self.db)
        .await
        .map_err(mutation_error)?;
        Ok(Equipment { id, name })
    }

    pub async fn update_equipment(
        &self,
        principal: &Principal,
        id: Uuid,
        input: NamedInput,
    ) -> Result<Equipment, DomainError> {
        require_catalogue_admin(principal)?;
        let Some(model) = equipment::Entity::find_by_id(id.to_string())
            .one(&self.db)
            .await?
        else {
            return Err(DomainError::NotFound);
        };
        let name = validate_name(&input.name)?;
        let mut active: equipment::ActiveModel = model.into();
        active.name = Set(name.clone());
        active.update(&self.db).await.map_err(mutation_error)?;
        Ok(Equipment { id, name })
    }

    pub async fn delete_equipment(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<(), DomainError> {
        require_catalogue_admin(principal)?;
        let result = equipment::Entity::delete_by_id(id.to_string())
            .exec(&self.db)
            .await
            .map_err(mutation_error)?;
        if result.rows_affected == 0 {
            Err(DomainError::NotFound)
        } else {
            Ok(())
        }
    }

    pub async fn list_exercises(
        &self,
        query: Option<&str>,
        request: PageRequest,
    ) -> Result<Page<ExerciseSummary>, DomainError> {
        let (offset, limit) = request.bounded()?;
        let mut select = exercises::Entity::find();
        if let Some(query) = validate_query(query)? {
            select = select.filter(exercises::Column::Name.contains(query));
        }
        let models = select
            .order_by_asc(exercises::Column::Name)
            .order_by_asc(exercises::Column::Id)
            .offset(offset)
            .limit(limit + 1)
            .all(&self.db)
            .await?;
        let items = models
            .into_iter()
            .map(|model| {
                Ok(ExerciseSummary {
                    id: parse_id(&model.id)?,
                    name: model.name,
                    contraction_type: model.contraction_type,
                })
            })
            .collect::<Result<Vec<_>, DomainError>>()?;
        Ok(page(items, offset, limit))
    }

    pub async fn get_exercise(&self, id: Uuid) -> Result<Exercise, DomainError> {
        let model = exercises::Entity::find_by_id(id.to_string())
            .one(&self.db)
            .await?
            .ok_or(DomainError::NotFound)?;
        self.exercise_from_model(&self.db, model).await
    }

    async fn exercise_from_model<C: ConnectionTrait>(
        &self,
        connection: &C,
        model: exercises::Model,
    ) -> Result<Exercise, DomainError> {
        let muscle_links = exercise_muscles::Entity::find()
            .filter(exercise_muscles::Column::ExerciseId.eq(model.id.clone()))
            .order_by_asc(exercise_muscles::Column::Role)
            .order_by_asc(exercise_muscles::Column::MuscleId)
            .limit((MAX_EXERCISE_ASSOCIATIONS + 1) as u64)
            .all(connection)
            .await?;
        if muscle_links.len() > MAX_EXERCISE_ASSOCIATIONS {
            return Err(DomainError::Conflict);
        }
        let mut primary_muscles = Vec::new();
        let mut secondary_muscles = Vec::new();
        for link in muscle_links {
            let role = link.role;
            let muscle = muscles::Entity::find_by_id(link.muscle_id)
                .one(connection)
                .await?
                .ok_or(DomainError::NotFound)?;
            let view = Muscle {
                id: parse_id(&muscle.id)?,
                name: muscle.name,
            };
            if role == "primary" {
                primary_muscles.push(view);
            } else {
                secondary_muscles.push(view);
            }
        }
        let equipment_links = exercise_equipment::Entity::find()
            .filter(exercise_equipment::Column::ExerciseId.eq(model.id.clone()))
            .order_by_asc(exercise_equipment::Column::EquipmentId)
            .limit((MAX_EXERCISE_ASSOCIATIONS + 1) as u64)
            .all(connection)
            .await?;
        if equipment_links.len() > MAX_EXERCISE_ASSOCIATIONS {
            return Err(DomainError::Conflict);
        }
        let mut equipment_views = Vec::with_capacity(equipment_links.len());
        for link in equipment_links {
            let item = equipment::Entity::find_by_id(link.equipment_id)
                .one(connection)
                .await?
                .ok_or(DomainError::NotFound)?;
            equipment_views.push(Equipment {
                id: parse_id(&item.id)?,
                name: item.name,
            });
        }
        let by_name = |a: &Muscle, b: &Muscle| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id));
        primary_muscles.sort_by(by_name);
        secondary_muscles.sort_by(by_name);
        equipment_views.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
        Ok(Exercise {
            id: parse_id(&model.id)?,
            name: model.name,
            contraction_type: model.contraction_type,
            bodyweight_share: model.bodyweight_share,
            primary_muscles,
            secondary_muscles,
            equipment: equipment_views,
        })
    }

    pub async fn create_exercise(
        &self,
        principal: &Principal,
        input: ExerciseInput,
    ) -> Result<Exercise, DomainError> {
        require_catalogue_admin(principal)?;
        let name = validate_exercise(&input)?;
        let id = Uuid::now_v7();
        let tx = self.begin_immediate().await?;
        exercises::ActiveModel {
            id: Set(id.to_string()),
            name: Set(name),
            contraction_type: Set(input.contraction_type.clone()),
            bodyweight_share: Set(input.bodyweight_share),
        }
        .insert(&tx)
        .await
        .map_err(mutation_error)?;
        insert_associations(&tx, id, &input).await?;
        let model = exercises::Entity::find_by_id(id.to_string())
            .one(&tx)
            .await?
            .ok_or(DomainError::Conflict)?;
        let result = self.exercise_from_model(&tx, model).await?;
        tx.commit().await.map_err(mutation_error)?;
        Ok(result)
    }

    pub async fn update_exercise(
        &self,
        principal: &Principal,
        id: Uuid,
        input: ExerciseInput,
    ) -> Result<Exercise, DomainError> {
        require_catalogue_admin(principal)?;
        let name = validate_exercise(&input)?;
        let tx = self.begin_immediate().await?;
        let Some(model) = exercises::Entity::find_by_id(id.to_string())
            .one(&tx)
            .await?
        else {
            return Err(DomainError::NotFound);
        };
        let mut active: exercises::ActiveModel = model.into();
        active.name = Set(name);
        active.contraction_type = Set(input.contraction_type.clone());
        active.bodyweight_share = Set(input.bodyweight_share);
        active.update(&tx).await.map_err(mutation_error)?;
        exercise_muscles::Entity::delete_many()
            .filter(exercise_muscles::Column::ExerciseId.eq(id.to_string()))
            .exec(&tx)
            .await
            .map_err(mutation_error)?;
        exercise_equipment::Entity::delete_many()
            .filter(exercise_equipment::Column::ExerciseId.eq(id.to_string()))
            .exec(&tx)
            .await
            .map_err(mutation_error)?;
        insert_associations(&tx, id, &input).await?;
        let model = exercises::Entity::find_by_id(id.to_string())
            .one(&tx)
            .await?
            .ok_or(DomainError::Conflict)?;
        let result = self.exercise_from_model(&tx, model).await?;
        tx.commit().await.map_err(mutation_error)?;
        Ok(result)
    }

    pub async fn delete_exercise(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<(), DomainError> {
        require_catalogue_admin(principal)?;
        let result = exercises::Entity::delete_by_id(id.to_string())
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

async fn insert_associations<C: ConnectionTrait>(
    connection: &C,
    id: Uuid,
    input: &ExerciseInput,
) -> Result<(), DomainError> {
    for association in &input.muscles {
        if muscles::Entity::find_by_id(association.muscle_id.to_string())
            .one(connection)
            .await?
            .is_none()
        {
            return Err(DomainError::InvalidInput("unknown muscle_id"));
        }
        exercise_muscles::ActiveModel {
            exercise_id: Set(id.to_string()),
            muscle_id: Set(association.muscle_id.to_string()),
            role: Set(association.role.clone()),
        }
        .insert(connection)
        .await
        .map_err(mutation_error)?;
    }
    for equipment_id in &input.equipment_ids {
        if equipment::Entity::find_by_id(equipment_id.to_string())
            .one(connection)
            .await?
            .is_none()
        {
            return Err(DomainError::InvalidInput("unknown equipment_id"));
        }
        exercise_equipment::ActiveModel {
            exercise_id: Set(id.to_string()),
            equipment_id: Set(equipment_id.to_string()),
        }
        .insert(connection)
        .await
        .map_err(mutation_error)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{
            AuthConfig, LogWorkout, LogWorkoutExercise, OAuthConfig,
            auth::{Identity, OAuthPrincipal, PrincipalTransport},
        },
        migration::Migrator,
    };

    use sea_orm::{ConnectionTrait, Database};
    use sea_orm_migration::MigratorTrait;
    use std::time::Duration;

    fn principal(user_id: Uuid, role: &str, scope: &str) -> Principal {
        Principal {
            identity: Identity {
                user_id,
                role: role.into(),
                auth_version: 0,
            },
            transport: PrincipalTransport::OAuthAccessToken {
                token_id: Uuid::now_v7(),
                context: OAuthPrincipal {
                    client_id: Uuid::now_v7().to_string(),
                    issuer: "https://frater.example".into(),
                    resource: "https://frater.example/mcp".into(),
                    scope: scope.into(),
                },
            },
        }
    }

    async fn fixture() -> (Domain, sea_orm::DatabaseConnection, Principal, Principal) {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        db.execute_unprepared("PRAGMA foreign_keys=ON")
            .await
            .unwrap();
        Migrator::up(&db, None).await.unwrap();
        let admin_id = Uuid::now_v7();
        let user_id = Uuid::now_v7();
        for (id, email, role) in [
            (admin_id, "admin@example.com", "superuser"),
            (user_id, "user@example.com", "user"),
        ] {
            db.execute_unprepared(&format!(
                "INSERT INTO users(id,email_normalized,email_display,role,status,auth_version,created_at,updated_at) VALUES('{id}','{email}','{email}','{role}','active',0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')"
            ))
            .await
            .unwrap();
        }
        let domain = Domain::new(
            db.clone(),
            AuthConfig {
                session_hmac_key: [1; 32],
                session_key_id: "session".into(),
                password_pepper: b"pepper".to_vec(),
                pepper_key_id: "pepper".into(),
                password_concurrency: 1,
                idle_lifetime: Duration::from_secs(60),
                absolute_lifetime: Duration::from_secs(120),
            },
            OAuthConfig {
                hmac_key: [2; 32],
                key_id: "oauth".into(),
            },
        )
        .await
        .unwrap();
        (
            domain,
            db,
            principal(
                admin_id,
                "superuser",
                "workouts:read catalogue:write workouts:write",
            ),
            principal(user_id, "user", "workouts:read catalogue:write"),
        )
    }

    fn exercise_input(name: &str, muscle_id: Uuid, equipment_id: Uuid) -> ExerciseInput {
        ExerciseInput {
            name: name.into(),
            contraction_type: "dynamic".into(),
            bodyweight_share: 0,
            muscles: vec![ExerciseMuscleInput {
                muscle_id,
                role: "primary".into(),
            }],
            equipment_ids: vec![equipment_id],
        }
    }

    #[tokio::test]
    async fn catalogue_crud_embeds_associations_and_rejects_duplicates() {
        let (domain, _, admin, _) = fixture().await;
        let muscle = domain
            .create_muscle(
                &admin,
                NamedInput {
                    name: " Quads ".into(),
                },
            )
            .await
            .unwrap();
        assert_eq!(muscle.name, "Quads");
        let muscle = domain
            .update_muscle(
                &admin,
                muscle.id,
                NamedInput {
                    name: "Quadriceps".into(),
                },
            )
            .await
            .unwrap();
        let equipment = domain
            .create_equipment(
                &admin,
                NamedInput {
                    name: "Barbell".into(),
                },
            )
            .await
            .unwrap();
        let equipment = domain
            .update_equipment(
                &admin,
                equipment.id,
                NamedInput {
                    name: "Olympic bar".into(),
                },
            )
            .await
            .unwrap();
        let exercise = domain
            .create_exercise(&admin, exercise_input("Squat", muscle.id, equipment.id))
            .await
            .unwrap();
        assert_eq!(exercise.primary_muscles[0].name, "Quadriceps");
        assert!(exercise.secondary_muscles.is_empty());
        assert_eq!(exercise.equipment[0].name, "Olympic bar");
        let updated = domain
            .update_exercise(
                &admin,
                exercise.id,
                ExerciseInput {
                    name: "Paused squat".into(),
                    contraction_type: "isometric".into(),
                    bodyweight_share: 0,
                    muscles: vec![ExerciseMuscleInput {
                        muscle_id: muscle.id,
                        role: "secondary".into(),
                    }],
                    equipment_ids: vec![],
                },
            )
            .await
            .unwrap();
        assert_eq!(updated.name, "Paused squat");
        assert_eq!(updated.contraction_type, "isometric");
        assert!(updated.primary_muscles.is_empty());
        assert_eq!(updated.secondary_muscles[0].name, "Quadriceps");
        assert!(updated.equipment.is_empty());

        assert!(matches!(
            domain
                .create_muscle(
                    &admin,
                    NamedInput {
                        name: "Quadriceps".into()
                    }
                )
                .await,
            Err(DomainError::Conflict)
        ));
        let duplicate_muscles = ExerciseInput {
            name: "Duplicate muscle".into(),
            contraction_type: "dynamic".into(),
            bodyweight_share: 0,
            muscles: vec![
                ExerciseMuscleInput {
                    muscle_id: muscle.id,
                    role: "primary".into(),
                },
                ExerciseMuscleInput {
                    muscle_id: muscle.id,
                    role: "secondary".into(),
                },
            ],
            equipment_ids: vec![],
        };
        assert!(matches!(
            domain.create_exercise(&admin, duplicate_muscles).await,
            Err(DomainError::InvalidInput("duplicate muscle association"))
        ));
        let duplicate_equipment = ExerciseInput {
            name: "Duplicate equipment".into(),
            contraction_type: "dynamic".into(),
            bodyweight_share: 0,
            muscles: vec![],
            equipment_ids: vec![equipment.id, equipment.id],
        };
        assert!(matches!(
            domain.create_exercise(&admin, duplicate_equipment).await,
            Err(DomainError::InvalidInput("duplicate equipment association"))
        ));

        domain.delete_exercise(&admin, exercise.id).await.unwrap();
        domain.delete_muscle(&admin, muscle.id).await.unwrap();
        domain.delete_equipment(&admin, equipment.id).await.unwrap();
        assert!(matches!(
            domain.get_exercise(exercise.id).await,
            Err(DomainError::NotFound)
        ));
    }

    #[tokio::test]
    async fn association_arrays_accept_100_and_reject_101() {
        let (domain, _, admin, _) = fixture().await;
        let mut muscle_ids = Vec::new();
        let mut equipment_ids = Vec::new();
        for index in 0..MAX_EXERCISE_ASSOCIATIONS {
            muscle_ids.push(
                domain
                    .create_muscle(
                        &admin,
                        NamedInput {
                            name: format!("Muscle {index:03}"),
                        },
                    )
                    .await
                    .unwrap()
                    .id,
            );
            equipment_ids.push(
                domain
                    .create_equipment(
                        &admin,
                        NamedInput {
                            name: format!("Equipment {index:03}"),
                        },
                    )
                    .await
                    .unwrap()
                    .id,
            );
        }
        let input = ExerciseInput {
            name: "Fully associated".into(),
            contraction_type: "dynamic".into(),
            bodyweight_share: 0,
            muscles: muscle_ids
                .iter()
                .map(|id| ExerciseMuscleInput {
                    muscle_id: *id,
                    role: "primary".into(),
                })
                .collect(),
            equipment_ids: equipment_ids.clone(),
        };
        let exercise = domain.create_exercise(&admin, input).await.unwrap();
        assert_eq!(exercise.primary_muscles.len(), 100);
        assert_eq!(exercise.equipment.len(), 100);

        let too_many_muscles = ExerciseInput {
            name: "Too many muscles".into(),
            contraction_type: "dynamic".into(),
            bodyweight_share: 0,
            muscles: (0..=MAX_EXERCISE_ASSOCIATIONS)
                .map(|_| ExerciseMuscleInput {
                    muscle_id: Uuid::now_v7(),
                    role: "primary".into(),
                })
                .collect(),
            equipment_ids: vec![],
        };
        assert!(matches!(
            domain.create_exercise(&admin, too_many_muscles).await,
            Err(DomainError::InvalidInput("too many exercise associations"))
        ));
        let too_many_equipment = ExerciseInput {
            name: "Too much equipment".into(),
            contraction_type: "dynamic".into(),
            bodyweight_share: 0,
            muscles: vec![],
            equipment_ids: (0..=MAX_EXERCISE_ASSOCIATIONS)
                .map(|_| Uuid::now_v7())
                .collect(),
        };
        assert!(matches!(
            domain.create_exercise(&admin, too_many_equipment).await,
            Err(DomainError::InvalidInput("too many exercise associations"))
        ));
    }

    #[tokio::test]
    async fn filters_pagination_and_bounds_are_consistent() {
        let (domain, _, admin, _) = fixture().await;
        let alpha = domain
            .create_muscle(
                &admin,
                NamedInput {
                    name: "Alpha muscle".into(),
                },
            )
            .await
            .unwrap();
        let beta = domain
            .create_muscle(
                &admin,
                NamedInput {
                    name: "Beta muscle".into(),
                },
            )
            .await
            .unwrap();
        let bar = domain
            .create_equipment(
                &admin,
                NamedInput {
                    name: "Alpha bar".into(),
                },
            )
            .await
            .unwrap();
        let band = domain
            .create_equipment(
                &admin,
                NamedInput {
                    name: "Beta band".into(),
                },
            )
            .await
            .unwrap();
        domain
            .create_exercise(&admin, exercise_input("Alpha lift", alpha.id, bar.id))
            .await
            .unwrap();
        domain
            .create_exercise(&admin, exercise_input("Beta lift", beta.id, band.id))
            .await
            .unwrap();

        let first = domain
            .list_muscles(
                None,
                PageRequest {
                    offset: 0,
                    limit: Some(1),
                },
            )
            .await
            .unwrap();
        assert_eq!(first.items.len(), 1);
        assert_eq!(first.next_offset, Some(1));
        let second = domain
            .list_muscles(
                None,
                PageRequest {
                    offset: 1,
                    limit: Some(1),
                },
            )
            .await
            .unwrap();
        assert_eq!(second.items.len(), 1);
        assert_eq!(
            domain
                .list_muscles(Some(" alpha "), PageRequest::default())
                .await
                .unwrap()
                .items
                .len(),
            1
        );
        assert_eq!(
            domain
                .list_equipment(Some("Alpha"), PageRequest::default())
                .await
                .unwrap()
                .items
                .len(),
            1
        );
        assert_eq!(
            domain
                .list_exercises(Some("Alpha"), PageRequest::default())
                .await
                .unwrap()
                .items
                .len(),
            1
        );
        for request in [
            PageRequest {
                offset: 0,
                limit: Some(0),
            },
            PageRequest {
                offset: 0,
                limit: Some(101),
            },
            PageRequest {
                offset: MAX_PAGE_OFFSET + 1,
                limit: Some(1),
            },
        ] {
            assert!(matches!(
                domain.list_muscles(None, request).await,
                Err(DomainError::InvalidInput("invalid pagination bounds"))
            ));
        }
    }

    #[tokio::test]
    async fn unknown_associations_roll_back_create_and_update() {
        let (domain, _, admin, _) = fixture().await;
        let muscle = domain
            .create_muscle(
                &admin,
                NamedInput {
                    name: "Quads".into(),
                },
            )
            .await
            .unwrap();
        let equipment = domain
            .create_equipment(
                &admin,
                NamedInput {
                    name: "Barbell".into(),
                },
            )
            .await
            .unwrap();
        let original = domain
            .create_exercise(&admin, exercise_input("Squat", muscle.id, equipment.id))
            .await
            .unwrap();

        let before_count = domain
            .list_exercises(None, PageRequest::default())
            .await
            .unwrap()
            .items
            .len();
        let unknown = Uuid::now_v7();
        assert!(matches!(
            domain
                .create_exercise(&admin, exercise_input("Invalid", unknown, equipment.id))
                .await,
            Err(DomainError::InvalidInput("unknown muscle_id"))
        ));
        assert_eq!(
            domain
                .list_exercises(None, PageRequest::default())
                .await
                .unwrap()
                .items
                .len(),
            before_count
        );
        assert!(matches!(
            domain
                .update_exercise(
                    &admin,
                    original.id,
                    exercise_input("Corrupted", muscle.id, unknown)
                )
                .await,
            Err(DomainError::InvalidInput("unknown equipment_id"))
        ));
        let unchanged = domain.get_exercise(original.id).await.unwrap();
        assert_eq!(unchanged.name, "Squat");
        assert_eq!(unchanged.primary_muscles[0].id, muscle.id);
        assert_eq!(unchanged.equipment[0].id, equipment.id);
    }

    #[tokio::test]
    async fn deletions_are_restricted_by_associations_and_history() {
        let (domain, _, admin, _) = fixture().await;
        let muscle = domain
            .create_muscle(
                &admin,
                NamedInput {
                    name: "Quads".into(),
                },
            )
            .await
            .unwrap();
        let equipment = domain
            .create_equipment(
                &admin,
                NamedInput {
                    name: "Barbell".into(),
                },
            )
            .await
            .unwrap();
        let exercise = domain
            .create_exercise(&admin, exercise_input("Squat", muscle.id, equipment.id))
            .await
            .unwrap();
        assert!(matches!(
            domain.delete_muscle(&admin, muscle.id).await,
            Err(DomainError::Conflict)
        ));
        assert!(matches!(
            domain.delete_equipment(&admin, equipment.id).await,
            Err(DomainError::Conflict)
        ));

        let session = domain
            .log_workout(
                &admin,
                LogWorkout {
                    started_at: crate::domain::Timestamp::parse("2026-01-01T00:00:00Z").unwrap(),
                    label: None,
                    notes: None,
                    exercises: vec![LogWorkoutExercise {
                        exercise_id: exercise.id,
                        notes: None,
                        sets: vec![],
                    }],
                },
            )
            .await
            .unwrap();
        assert!(matches!(
            domain.delete_exercise(&admin, exercise.id).await,
            Err(DomainError::Conflict)
        ));
        domain.delete_session(&admin, session.id).await.unwrap();
        domain.delete_exercise(&admin, exercise.id).await.unwrap();
        domain.delete_muscle(&admin, muscle.id).await.unwrap();
        domain.delete_equipment(&admin, equipment.id).await.unwrap();
    }

    #[tokio::test]
    async fn references_resolve_by_id_exact_name_prefix_and_report_ambiguity() {
        let (domain, _, admin, _) = fixture().await;
        let squat = domain
            .create_exercise(
                &admin,
                ExerciseInput {
                    name: "Back squat".into(),
                    contraction_type: "dynamic".into(),
                    bodyweight_share: 0,
                    muscles: vec![],
                    equipment_ids: vec![],
                },
            )
            .await
            .unwrap();
        let extension = domain
            .create_exercise(
                &admin,
                ExerciseInput {
                    name: "Back extension".into(),
                    contraction_type: "dynamic".into(),
                    bodyweight_share: 0,
                    muscles: vec![],
                    equipment_ids: vec![],
                },
            )
            .await
            .unwrap();
        let muscle = domain
            .create_muscle(
                &admin,
                NamedInput {
                    name: "Quads".into(),
                },
            )
            .await
            .unwrap();
        let equipment = domain
            .create_equipment(
                &admin,
                NamedInput {
                    name: "Barbell".into(),
                },
            )
            .await
            .unwrap();

        for (reference, expected) in [
            (squat.id.to_string(), squat.id),
            ("Back squat".to_owned(), squat.id),
            ("back SQUAT".to_owned(), squat.id),
            ("back sq".to_owned(), squat.id),
            ("extension".to_owned(), extension.id),
        ] {
            let Lookup::Found(found) = domain.resolve_exercise(&reference).await.unwrap() else {
                panic!("expected a single match for {reference}");
            };
            assert_eq!(found.id, expected);
        }

        let Lookup::Ambiguous(candidates) = domain.resolve_exercise("back").await.unwrap() else {
            panic!("expected an ambiguous match");
        };
        assert_eq!(candidates.len(), 2);
        assert!(matches!(
            domain.resolve_exercise("deadlift").await.unwrap(),
            Lookup::Missing
        ));
        assert!(matches!(
            domain
                .resolve_exercise(&Uuid::now_v7().to_string())
                .await
                .unwrap(),
            Lookup::Missing
        ));
        assert!(matches!(
            domain.resolve_exercise("  ").await,
            Err(DomainError::InvalidInput(_))
        ));

        let Lookup::Found(found) = domain.resolve_muscle("quad").await.unwrap() else {
            panic!("expected one muscle");
        };
        assert_eq!(found.id, muscle.id);
        let Lookup::Found(found) = domain
            .resolve_equipment(&equipment.id.to_string())
            .await
            .unwrap()
        else {
            panic!("expected one equipment item");
        };
        assert_eq!(found.name, "Barbell");
    }

    #[tokio::test]
    async fn catalogue_admin_scope_does_not_replace_superuser_role() {
        let (domain, _, _, ordinary_admin) = fixture().await;
        assert!(matches!(
            domain
                .create_muscle(
                    &ordinary_admin,
                    NamedInput {
                        name: "Denied".into()
                    }
                )
                .await,
            Err(DomainError::Forbidden)
        ));
        assert!(matches!(
            domain
                .create_equipment(
                    &ordinary_admin,
                    NamedInput {
                        name: "Denied".into()
                    }
                )
                .await,
            Err(DomainError::Forbidden)
        ));
        assert!(matches!(
            domain
                .create_exercise(
                    &ordinary_admin,
                    ExerciseInput {
                        name: "Denied".into(),
                        contraction_type: "dynamic".into(),
                        bodyweight_share: 0,
                        muscles: vec![],
                        equipment_ids: vec![]
                    }
                )
                .await,
            Err(DomainError::Forbidden)
        ));
    }
}
