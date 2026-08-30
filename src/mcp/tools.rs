use rmcp::{
    ErrorData,
    model::{CallToolResult, JsonObject},
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::domain::{
    CreateWorkoutSession, DomainError, ExerciseInput, ExerciseMuscleInput, LogBodyweight,
    LogWorkout, LogWorkoutExercise, Lookup, MAX_EXERCISE_ASSOCIATIONS, NamedInput, Principal,
    ReplaceRun, RunSplit,
};

use super::McpServer;
use super::schemas::{
    ExerciseArg, ExerciseHistoryArg, IdArg, ListArgs, ListBodyweightArgs, ListWorkoutsArgs,
    LogWorkoutArg, LogWorkoutExerciseArg, MAX_BATCH_REFERENCES, NamesArg, PersonalRecordsArg,
    ReferenceArg, ReplaceActivityArg, ReplaceWorkoutArg, UpdateExerciseArg, UpdateNameArg,
    WorkoutSetArg, example_for_tool,
};

impl McpServer {
    pub(super) async fn dispatch_tool(
        &self,
        name: &str,
        arguments: JsonObject,
        principal: Option<Principal>,
    ) -> Result<CallToolResult, ErrorData> {
        let Some(principal) = principal else {
            return Ok(tool_error(
                "forbidden",
                "authenticated OAuth principal required",
            ));
        };
        // An unauthenticated caller must not learn which tool names exist, so
        // this stays behind the principal check above.
        let Some(required_scope) = required_scope(name) else {
            return Ok(tool_error("not_found", "tool not found"));
        };
        if !principal
            .oauth()
            .is_some_and(|context| context.has_scope(required_scope))
        {
            // One code for both authorization failures, so a client never has
            // to branch on two. The remedies differ, so the message must say
            // which applies: a missing scope the client can re-authorize for,
            // a missing role only a catalogue administrator can grant.
            return Ok(tool_error(
                "forbidden",
                &format!("{required_scope} scope required"),
            ));
        }
        if required_scope == "catalogue:write" && principal.role() != "superuser" {
            return Ok(tool_error(
                "forbidden",
                "catalogue mutation requires the superuser role",
            ));
        }

        macro_rules! input {
            ($type:ty) => {
                match parse_arguments::<$type>(name, arguments.clone()) {
                    Ok(value) => value,
                    Err(result) => return Ok(result),
                }
            };
        }
        macro_rules! output {
            ($future:expr) => {
                match $future.await {
                    Ok(value) => structured(&value),
                    Err(error) => domain_tool_error(name, error),
                }
            };
        }
        /// The MCP specification types `structuredContent` as an object, so a
        /// sequence result travels under one key instead of at the top level.
        macro_rules! items {
            ($future:expr) => {
                match $future.await {
                    Ok(items) => structured(&json!({"items": items})),
                    Err(error) => domain_tool_error(name, error),
                }
            };
        }
        macro_rules! exercise_id {
            ($reference:expr) => {
                match self.resolve_exercise_id(name, $reference).await {
                    Ok(value) => value,
                    Err(result) => return Ok(result),
                }
            };
        }

        let result = match name {
            "list_muscles" => {
                let input = input!(ListArgs);
                output!(
                    self.domain
                        .list_muscles(input.query.as_deref(), input.page())
                )
            }
            "get_muscle" => {
                let input = input!(ReferenceArg);
                match self.domain.resolve_muscle(&input.id).await {
                    Ok(Lookup::Found(muscle)) => structured(&muscle),
                    Ok(other) => lookup_error(name, "muscle", &input.id, other, |item| {
                        (item.id, item.name.clone())
                    }),
                    Err(error) => domain_tool_error(name, error),
                }
            }
            "create_muscle" => {
                let input = input!(NamedInput);
                output!(self.domain.create_muscle(&principal, input))
            }
            "update_muscle" => {
                let input = input!(UpdateNameArg);
                output!(self.domain.update_muscle(
                    &principal,
                    input.id,
                    NamedInput { name: input.name }
                ))
            }
            "delete_muscle" => {
                let input = input!(IdArg);
                match self.domain.delete_muscle(&principal, input.id).await {
                    Ok(()) => deleted(input.id),
                    Err(error) => domain_tool_error(name, error),
                }
            }
            "list_equipment" => {
                let input = input!(ListArgs);
                output!(
                    self.domain
                        .list_equipment(input.query.as_deref(), input.page())
                )
            }
            "get_equipment" => {
                let input = input!(ReferenceArg);
                match self.domain.resolve_equipment(&input.id).await {
                    Ok(Lookup::Found(equipment)) => structured(&equipment),
                    Ok(other) => lookup_error(name, "equipment", &input.id, other, |item| {
                        (item.id, item.name.clone())
                    }),
                    Err(error) => domain_tool_error(name, error),
                }
            }
            "create_equipment" => {
                let input = input!(NamedInput);
                output!(self.domain.create_equipment(&principal, input))
            }
            "update_equipment" => {
                let input = input!(UpdateNameArg);
                output!(self.domain.update_equipment(
                    &principal,
                    input.id,
                    NamedInput { name: input.name }
                ))
            }
            "delete_equipment" => {
                let input = input!(IdArg);
                match self.domain.delete_equipment(&principal, input.id).await {
                    Ok(()) => deleted(input.id),
                    Err(error) => domain_tool_error(name, error),
                }
            }
            "list_exercises" => {
                let input = input!(ListArgs);
                output!(
                    self.domain
                        .list_exercises(input.query.as_deref(), input.page())
                )
            }
            "get_exercise" => {
                let input = input!(ReferenceArg);
                let id = exercise_id!(&input.id);
                output!(self.domain.get_exercise(id))
            }
            // Every name gets its own entry: one unresolvable reference must
            // not cost the caller the answers for the others.
            "resolve_exercises" => {
                let input = input!(NamesArg);
                if input.names.len() > MAX_BATCH_REFERENCES {
                    return Ok(detailed_error(
                        name,
                        "invalid_input",
                        "names must hold at most 100 references",
                        None,
                    ));
                }
                let mut results = Vec::with_capacity(input.names.len());
                for query in input.names {
                    match self.domain.resolve_exercise(&query).await {
                        Ok(lookup) => results.push(resolution_entry(&query, lookup, |item| {
                            json!({"id":item.id,"name":item.name,"contraction_type":item.contraction_type})
                        })),
                        Err(DomainError::InvalidInput(message)) => results.push(
                            json!({"query": query, "status": "invalid", "message": message}),
                        ),
                        Err(error) => return Ok(domain_tool_error(name, error)),
                    }
                }
                structured(&json!({"results": results}))
            }
            "create_exercise" => {
                let input = input!(ExerciseArg);
                let bodyweight_share = input.bodyweight_share.unwrap_or(0);
                let input = match self
                    .resolve_exercise_input(name, input, bodyweight_share)
                    .await
                {
                    Ok(value) => value,
                    Err(result) => return Ok(result),
                };
                output!(self.domain.create_exercise(&principal, input))
            }
            "update_exercise" => {
                let argument = input!(UpdateExerciseArg);
                let Some(bodyweight_share) = argument.input.bodyweight_share else {
                    return Ok(detailed_error(
                        name,
                        "invalid_input",
                        "bodyweight_share is required; omitting it would reset the share and change the volume and records already reported for past workouts",
                        None,
                    ));
                };
                let input = match self
                    .resolve_exercise_input(name, argument.input, bodyweight_share)
                    .await
                {
                    Ok(value) => value,
                    Err(result) => return Ok(result),
                };
                output!(self.domain.update_exercise(&principal, argument.id, input))
            }
            "delete_exercise" => {
                let input = input!(IdArg);
                match self.domain.delete_exercise(&principal, input.id).await {
                    Ok(()) => deleted(input.id),
                    Err(error) => domain_tool_error(name, error),
                }
            }
            "list_workouts" => {
                let input = input!(ListWorkoutsArgs);
                output!(
                    self.domain
                        .list_workouts(&principal, input.filter(), input.page())
                )
            }
            "get_workout_session" => {
                let input = input!(IdArg);
                output!(self.domain.get_session(&principal, input.id))
            }
            "create_workout_session" => {
                let input = input!(CreateWorkoutSession);
                output!(self.domain.create_session(&principal, input))
            }
            "delete_workout_session" => {
                let input = input!(IdArg);
                match self.domain.delete_session(&principal, input.id).await {
                    Ok(()) => deleted(input.id),
                    Err(error) => domain_tool_error(name, error),
                }
            }
            "log_workout" => {
                let input = input!(LogWorkoutArg);
                let exercises = match self.resolve_workout_exercises(name, input.exercises).await {
                    Ok(value) => value,
                    Err(result) => return Ok(result),
                };
                output!(self.domain.log_workout(
                    &principal,
                    LogWorkout {
                        started_at: input.started_at,
                        label: input.label,
                        notes: input.notes,
                        exercises,
                    }
                ))
            }
            "replace_workout" => {
                let input = input!(ReplaceWorkoutArg);
                match replacement_payload(name, &input) {
                    Ok(Replacement::Run {
                        distance_m,
                        duration_sec,
                        elevation_gain_m,
                        splits,
                    }) => output!(self.domain.replace_run(
                        &principal,
                        input.id,
                        ReplaceRun {
                            started_at: input.started_at,
                            label: input.label.clone(),
                            notes: input.notes.clone(),
                            distance_m,
                            duration_sec,
                            elevation_gain_m,
                            splits,
                        }
                    )),
                    Ok(Replacement::Strength(entries)) => {
                        let exercises = match self.resolve_workout_exercises(name, entries).await {
                            Ok(value) => value,
                            Err(result) => return Ok(result),
                        };
                        output!(self.domain.replace_workout(
                            &principal,
                            input.id,
                            LogWorkout {
                                started_at: input.started_at,
                                label: input.label.clone(),
                                notes: input.notes.clone(),
                                exercises,
                            }
                        ))
                    }
                    Err(result) => return Ok(result),
                }
            }
            "exercise_history" => {
                let input = input!(ExerciseHistoryArg);
                let id = exercise_id!(&input.exercise);
                items!(
                    self.domain
                        .exercise_history(&principal, id, input.range(), input.limit)
                )
            }
            "personal_records" => {
                let input = input!(PersonalRecordsArg);
                let exercise_id = match input.exercise.as_deref() {
                    Some(reference) => Some(exercise_id!(reference)),
                    None => None,
                };
                items!(
                    self.domain
                        .personal_records(&principal, exercise_id, input.range())
                )
            }
            "log_bodyweight" => {
                let input = input!(LogBodyweight);
                output!(self.domain.log_bodyweight(&principal, input))
            }
            "list_bodyweight" => {
                let input = input!(ListBodyweightArgs);
                output!(
                    self.domain
                        .list_bodyweight(&principal, input.filter(), input.page())
                )
            }
            "delete_bodyweight" => {
                let input = input!(IdArg);
                match self.domain.delete_bodyweight(&principal, input.id).await {
                    Ok(()) => deleted(input.id),
                    Err(error) => domain_tool_error(name, error),
                }
            }
            _ => tool_error("not_found", "tool not found"),
        };
        Ok(result)
    }

    async fn resolve_exercise_id(
        &self,
        tool: &str,
        reference: &str,
    ) -> Result<Uuid, CallToolResult> {
        match self.domain.resolve_exercise(reference).await {
            Ok(Lookup::Found(exercise)) => Ok(exercise.id),
            Ok(other) => Err(lookup_error(tool, "exercise", reference, other, |item| {
                (item.id, item.name.clone())
            })),
            Err(error) => Err(domain_tool_error(tool, error)),
        }
    }

    /// Turns the three flat reference lists of the API into the id-and-role
    /// associations the database stores. Everything resolves before any write.
    async fn resolve_exercise_input(
        &self,
        tool: &str,
        input: ExerciseArg,
        bodyweight_share: i64,
    ) -> Result<ExerciseInput, CallToolResult> {
        if input.primary_muscles.len() + input.secondary_muscles.len() > MAX_EXERCISE_ASSOCIATIONS
            || input.equipment.len() > MAX_EXERCISE_ASSOCIATIONS
        {
            return Err(detailed_error(
                tool,
                "invalid_input",
                "too many exercise associations",
                None,
            ));
        }
        let mut muscles = Vec::new();
        for (role, references) in [
            ("primary", &input.primary_muscles),
            ("secondary", &input.secondary_muscles),
        ] {
            for reference in references {
                let muscle = match self.domain.resolve_muscle(reference).await {
                    Ok(Lookup::Found(muscle)) => muscle,
                    Ok(other) => {
                        return Err(lookup_error(tool, "muscle", reference, other, |item| {
                            (item.id, item.name.clone())
                        }));
                    }
                    Err(error) => return Err(domain_tool_error(tool, error)),
                };
                if muscles
                    .iter()
                    .any(|held: &ExerciseMuscleInput| held.muscle_id == muscle.id)
                {
                    return Err(detailed_error(
                        tool,
                        "invalid_input",
                        &format!(
                            "'{}' is listed more than once; a muscle is either primary or secondary, never both",
                            muscle.name
                        ),
                        None,
                    ));
                }
                muscles.push(ExerciseMuscleInput {
                    muscle_id: muscle.id,
                    role: role.to_owned(),
                });
            }
        }
        let mut equipment_ids = Vec::new();
        for reference in &input.equipment {
            match self.domain.resolve_equipment(reference).await {
                Ok(Lookup::Found(item)) => equipment_ids.push(item.id),
                Ok(other) => {
                    return Err(lookup_error(tool, "equipment", reference, other, |item| {
                        (item.id, item.name.clone())
                    }));
                }
                Err(error) => return Err(domain_tool_error(tool, error)),
            }
        }
        Ok(ExerciseInput {
            name: input.name,
            contraction_type: input.contraction_type,
            bodyweight_share,
            muscles,
            equipment_ids,
        })
    }

    /// Resolves every reference in a nested workout before the caller writes
    /// anything, and reports all failures together. Failing on the first bad
    /// reference would make an agent diagnose a long workout one call at a
    /// time.
    async fn resolve_workout_exercises(
        &self,
        tool: &str,
        entries: Vec<LogWorkoutExerciseArg>,
    ) -> Result<Vec<LogWorkoutExercise>, CallToolResult> {
        let mut resolved = Vec::with_capacity(entries.len());
        let mut missing: Vec<String> = Vec::new();
        let mut ambiguous: Vec<Value> = Vec::new();
        for (index, entry) in entries.iter().enumerate() {
            let reference = match entry.reference() {
                Ok(reference) => reference,
                Err(message) => {
                    return Err(detailed_error(tool, "invalid_input", message, None));
                }
            };
            if let Some(result) = position_mismatch(tool, reference, index, entry) {
                return Err(result);
            }
            match self.domain.resolve_exercise(reference).await {
                Ok(Lookup::Found(exercise)) => {
                    if let Some(name) = entry.expected_name()
                        && !name.eq_ignore_ascii_case(&exercise.name)
                    {
                        return Err(detailed_error(
                            tool,
                            "invalid_input",
                            &format!(
                                "exercise entry {index} sends exercise_id {} ('{}') and exercise_name '{}', which name different exercises; send only the field you changed",
                                exercise.id, exercise.name, name
                            ),
                            None,
                        ));
                    }
                    resolved.push(LogWorkoutExercise {
                        exercise_id: exercise.id,
                        notes: entry.notes.clone(),
                        sets: entry.sets.iter().map(WorkoutSetArg::input).collect(),
                    })
                }
                Ok(Lookup::Missing) => {
                    let reference = reference.to_owned();
                    if !missing.contains(&reference) {
                        missing.push(reference);
                    }
                }
                Ok(Lookup::Ambiguous(candidates)) => ambiguous.push(json!({
                    "query": reference,
                    "candidates": candidates
                        .iter()
                        .map(|item| json!({"id": item.id, "name": item.name}))
                        .collect::<Vec<_>>(),
                })),
                Err(error) => return Err(domain_tool_error(tool, error)),
            }
        }
        if missing.is_empty() && ambiguous.is_empty() {
            return Ok(resolved);
        }
        Err(unresolved_exercises_error(tool, missing, ambiguous))
    }
}

enum Replacement {
    Strength(Vec<LogWorkoutExerciseArg>),
    Run {
        distance_m: Option<i64>,
        duration_sec: Option<i64>,
        elevation_gain_m: i64,
        splits: Vec<RunSplit>,
    },
}

/// `replace_workout` takes either the nested `activity` that
/// `get_workout_session` returns or the flat `exercises` that `log_workout`
/// takes. Both together cannot be honoured, so neither is guessed at.
fn replacement_payload(
    tool: &str,
    input: &ReplaceWorkoutArg,
) -> Result<Replacement, CallToolResult> {
    let invalid = |message: &str| detailed_error(tool, "invalid_input", message, None);
    match (&input.activity, &input.exercises) {
        (Some(_), Some(_)) => Err(invalid(
            "send either 'activity' or a top-level 'exercises', not both",
        )),
        (Some(ReplaceActivityArg::Strength { exercises }), None) => {
            Ok(Replacement::Strength(exercises.clone()))
        }
        (
            Some(ReplaceActivityArg::Run {
                distance_m,
                duration_sec,
                elevation_gain_m,
                splits,
            }),
            None,
        ) => Ok(Replacement::Run {
            distance_m: *distance_m,
            duration_sec: *duration_sec,
            elevation_gain_m: *elevation_gain_m,
            splits: splits
                .iter()
                .map(|split| RunSplit {
                    distance_m: split.distance_m,
                    duration_sec: split.duration_sec,
                })
                .collect(),
        }),
        (None, Some(exercises)) => Ok(Replacement::Strength(exercises.clone())),
        (None, None) => Err(invalid(
            "send the 'activity' that get_workout_session returns, or a top-level 'exercises'",
        )),
    }
}

/// Array order is the only order a whole-workout write can honour, so a
/// position that disagrees with its index is refused rather than discarded.
fn position_mismatch(
    tool: &str,
    reference: &str,
    index: usize,
    entry: &LogWorkoutExerciseArg,
) -> Option<CallToolResult> {
    let mismatch = |message: String| Some(detailed_error(tool, "invalid_input", &message, None));
    if let Some(position) = entry.position.filter(|position| *position != index as u64) {
        return mismatch(format!(
            "exercise '{reference}' declares position {position} but is element {index} of the exercises array; reorder the array to reorder the exercises"
        ));
    }
    for (set_index, set) in entry.sets.iter().enumerate() {
        if let Some(position) = set
            .position
            .filter(|position| *position != set_index as u64)
        {
            return mismatch(format!(
                "set {set_index} of exercise '{reference}' declares position {position}; reorder the sets array to reorder the sets"
            ));
        }
    }
    None
}

/// `not_found` when any name is missing, because that is terminal: the exercise
/// catalogue is global and only a catalogue administrator can add to it, so a
/// retry with the same name can never succeed.
fn unresolved_exercises_error(
    tool: &str,
    missing: Vec<String>,
    ambiguous: Vec<Value>,
) -> CallToolResult {
    let mut sentences = Vec::new();
    if !missing.is_empty() {
        let names = missing
            .iter()
            .map(|value| format!("'{value}'"))
            .collect::<Vec<_>>()
            .join(", ");
        sentences.push(format!(
            "no exercise matches {names}. Retrying cannot fix this: the exercise catalogue is shared and only a catalogue administrator can add an entry to it. Ask the user to have these exercises added, or use exercises the catalogue already holds (find them with list_exercises)"
        ));
    }
    if !ambiguous.is_empty() {
        let reports = ambiguous
            .iter()
            .map(|item| {
                let names = item["candidates"]
                    .as_array()
                    .map(|candidates| {
                        candidates
                            .iter()
                            .filter_map(|candidate| candidate["name"].as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default();
                format!("{} matches {names}", item["query"])
            })
            .collect::<Vec<_>>()
            .join("; ");
        sentences.push(format!(
            "{reports}. Send the exact name or the id for each of these"
        ));
    }
    let code = if missing.is_empty() {
        "ambiguous"
    } else {
        "not_found"
    };
    CallToolResult::structured_error(json!({
        "error": code,
        "message": sentences.join(". "),
        "missing": missing,
        "ambiguous": ambiguous,
        "valid_example": {"name": tool, "arguments": example_for_tool(tool)},
    }))
}

fn resolution_entry<T>(query: &str, lookup: Lookup<T>, describe: impl Fn(&T) -> Value) -> Value {
    match lookup {
        Lookup::Found(item) => {
            json!({"query": query, "status": "found", "match": describe(&item)})
        }
        Lookup::Missing => json!({"query": query, "status": "missing"}),
        Lookup::Ambiguous(candidates) => json!({
            "query": query,
            "status": "ambiguous",
            "candidates": candidates.iter().map(describe).collect::<Vec<_>>(),
        }),
    }
}

fn lookup_error<T>(
    tool: &str,
    kind: &str,
    reference: &str,
    lookup: Lookup<T>,
    describe: impl Fn(&T) -> (Uuid, String),
) -> CallToolResult {
    match lookup {
        Lookup::Found(_) => tool_error("internal", "unexpected lookup state"),
        Lookup::Missing => detailed_error(
            tool,
            "not_found",
            &format!(
                "no {kind} matches '{reference}'. Pass an id, or find the name with {}",
                match kind {
                    "muscle" => "list_muscles",
                    "equipment" => "list_equipment",
                    _ => "list_exercises",
                }
            ),
            None,
        ),
        Lookup::Ambiguous(candidates) => {
            let candidates = candidates
                .iter()
                .map(|item| {
                    let (id, name) = describe(item);
                    json!({"id":id, "name":name})
                })
                .collect::<Vec<_>>();
            let names = candidates
                .iter()
                .filter_map(|item| item["name"].as_str())
                .collect::<Vec<_>>()
                .join(", ");
            detailed_error(
                tool,
                "ambiguous",
                &format!(
                    "'{reference}' matches {} {kind} entries ({names}). Call again with the exact name or the id",
                    candidates.len()
                ),
                Some(candidates),
            )
        }
    }
}

fn parse_arguments<T: DeserializeOwned>(
    tool: &str,
    arguments: JsonObject,
) -> Result<T, CallToolResult> {
    serde_json::from_value(Value::Object(arguments))
        .map_err(|error| detailed_error(tool, "invalid_params", &error.to_string(), None))
}

fn detailed_error(
    tool: &str,
    code: &str,
    message: &str,
    candidates: Option<Vec<Value>>,
) -> CallToolResult {
    let mut payload = json!({
        "error": code,
        "message": message,
        "valid_example": {"name": tool, "arguments": example_for_tool(tool)},
    });
    if let Some(candidates) = candidates {
        payload["candidates"] = Value::Array(candidates);
    }
    CallToolResult::structured_error(payload)
}

fn structured<T: Serialize>(value: &T) -> CallToolResult {
    match serde_json::to_value(value) {
        Ok(value) => CallToolResult::structured(value),
        Err(_) => tool_error("internal", "could not serialize fitness result"),
    }
}

fn deleted(id: Uuid) -> CallToolResult {
    CallToolResult::structured(json!({"deleted": true, "id": id}))
}

fn tool_error(code: &str, message: &str) -> CallToolResult {
    CallToolResult::structured_error(json!({"error": code, "message": message}))
}

fn domain_tool_error(tool: &str, error: DomainError) -> CallToolResult {
    match error {
        DomainError::NotFound => detailed_error(
            tool,
            "not_found",
            "no record matches the given id, or it belongs to another user",
            None,
        ),
        DomainError::Conflict => tool_error(
            "conflict",
            "operation conflicts with existing fitness history or data",
        ),
        DomainError::Forbidden => tool_error("forbidden", "forbidden"),
        DomainError::InvalidInput(message) => detailed_error(tool, "invalid_input", message, None),
        DomainError::InvalidCredentials => tool_error("forbidden", "forbidden"),
        error @ (DomainError::TemporarilyUnavailable | DomainError::Internal(_)) => {
            tracing::error!(tool, ?error, "tool call failed");
            tool_error("unavailable", "fitness service unavailable")
        }
    }
}

/// `None` for a name the table does not hold. An earlier version fell back to
/// the least privileged scope, which let a typo dispatch as a workout read.
pub(super) fn required_scope(name: &str) -> Option<&'static str> {
    super::schemas::TOOL_SPECS
        .iter()
        .find(|(spec_name, _, _)| *spec_name == name)
        .map(|(_, _, scope)| *scope)
}

#[cfg(test)]
mod tests {
    use super::super::schemas::TOOL_SPECS;
    use super::super::test_support::{server_fixture, structured_value};
    use crate::domain::test_oauth_principal;
    use rmcp::model::JsonObject;
    use serde_json::json;

    #[tokio::test]
    async fn every_advertised_tool_dispatches_and_rejects_unknown_arguments() {
        let (server, _, _, superuser, _) = server_fixture().await;
        for (name, _, _) in TOOL_SPECS {
            let arguments = serde_json::from_value(json!({"definitely_unknown": true})).unwrap();
            let result = server
                .dispatch_tool(name, arguments, Some(superuser.clone()))
                .await
                .unwrap();
            let value = structured_value(&result);
            assert_eq!(value["error"], "invalid_params", "tool {name}");
            assert_ne!(value["error"], "not_found", "tool {name}");
        }
        let unknown = server
            .dispatch_tool("unknown_tool", JsonObject::new(), Some(superuser))
            .await
            .unwrap();
        assert_eq!(structured_value(&unknown)["error"], "not_found");
    }

    #[tokio::test]
    async fn workflow_tools_log_and_report_by_exercise_name() {
        let (server, _, _, superuser, _) = server_fixture().await;
        let call = async |name: &str, arguments: serde_json::Value| {
            structured_value(
                &server
                    .dispatch_tool(
                        name,
                        serde_json::from_value(arguments).unwrap(),
                        Some(superuser.clone()),
                    )
                    .await
                    .unwrap(),
            )
        };

        call(
            "create_exercise",
            json!({"name":"Back squat","contraction_type":"dynamic"}),
        )
        .await;
        call(
            "create_exercise",
            json!({"name":"Back extension","contraction_type":"dynamic"}),
        )
        .await;

        let logged = call(
            "log_workout",
            json!({
                "started_at":"2026-08-10",
                "label":"Leg day",
                "exercises":[{"exercise":"back squat","sets":[
                    {"set_type":"warmup","reps":8,"load_g":40000},
                    {"set_type":"working","reps":5,"load_g":60000}
                ]}]
            }),
        )
        .await;
        assert_eq!(logged["label"], "Leg day");
        assert_eq!(
            logged["activity"]["exercises"][0]["sets"][1]["load_g"],
            60000
        );

        let history = call("list_workouts", json!({"started_at_from":"2026-08-10"})).await;
        assert_eq!(history["items"].as_array().unwrap().len(), 1);
        assert_eq!(history["items"][0]["set_count"], 2);

        let single_day = call(
            "list_workouts",
            json!({"started_at_from":"2026-08-10","started_at_to":"2026-08-10"}),
        )
        .await;
        assert_eq!(single_day["items"].as_array().unwrap().len(), 1);

        let progress = call(
            "exercise_history",
            json!({"exercise":"Back squat","limit":5}),
        )
        .await;
        assert_eq!(progress["items"][0]["sets"][1]["estimated_1rm_g"], 70000);

        let records = call("personal_records", json!({"exercise":"Back squat"})).await;
        assert_eq!(records["items"][0]["max_load_g"], 60000);

        let ambiguous = call("exercise_history", json!({"exercise":"back"})).await;
        assert_eq!(ambiguous["error"], "ambiguous");
        assert_eq!(ambiguous["candidates"].as_array().unwrap().len(), 2);
        assert!(ambiguous["valid_example"]["arguments"].is_object());

        let unknown = call("personal_records", json!({"exercise":"deadlift"})).await;
        assert_eq!(unknown["error"], "not_found");
        assert!(
            unknown["message"]
                .as_str()
                .unwrap()
                .contains("list_exercises")
        );

        let bad_date = call(
            "log_workout",
            json!({"started_at":"10 August 2026","exercises":[{"exercise":"Back squat"}]}),
        )
        .await;
        assert_eq!(bad_date["error"], "invalid_params");
        assert!(bad_date["message"].as_str().unwrap().contains("YYYY-MM-DD"));
        assert_eq!(bad_date["valid_example"]["name"], "log_workout");
    }

    #[tokio::test]
    async fn log_workout_accepts_notes_at_every_level_and_rejects_a_long_one() {
        let (server, _, _, superuser, _) = server_fixture().await;
        let call = async |name: &str, arguments: serde_json::Value| {
            structured_value(
                &server
                    .dispatch_tool(
                        name,
                        serde_json::from_value(arguments).unwrap(),
                        Some(superuser.clone()),
                    )
                    .await
                    .unwrap(),
            )
        };

        call(
            "create_exercise",
            json!({"name":"Back squat","contraction_type":"dynamic"}),
        )
        .await;

        let logged = call(
            "log_workout",
            json!({
                "started_at":"2026-08-11",
                "notes":"session note",
                "exercises":[{"exercise":"Back squat","notes":"exercise note","sets":[
                    {"set_type":"working","reps":5,"load_g":60000,"notes":"set note"}
                ]}]
            }),
        )
        .await;
        assert_eq!(logged["notes"], "session note");
        assert_eq!(logged["activity"]["exercises"][0]["notes"], "exercise note");
        assert_eq!(
            logged["activity"]["exercises"][0]["sets"][0]["notes"],
            "set note"
        );

        let without_notes = call(
            "log_workout",
            json!({
                "started_at":"2026-08-12",
                "exercises":[{"exercise":"Back squat","sets":[
                    {"set_type":"working","reps":5,"load_g":60000}
                ]}]
            }),
        )
        .await;
        assert!(without_notes["notes"].is_null());
        assert!(without_notes["activity"]["exercises"][0]["notes"].is_null());

        let too_long = call(
            "log_workout",
            json!({
                "started_at":"2026-08-13",
                "notes":"n".repeat(1001),
                "exercises":[{"exercise":"Back squat"}]
            }),
        )
        .await;
        assert_eq!(too_long["error"], "invalid_input");
    }

    #[tokio::test]
    async fn a_nested_workout_reports_every_unresolved_exercise_in_one_answer() {
        let (server, _, _, superuser, _) = server_fixture().await;
        let call = async |name: &str, arguments: serde_json::Value| {
            structured_value(
                &server
                    .dispatch_tool(
                        name,
                        serde_json::from_value(arguments).unwrap(),
                        Some(superuser.clone()),
                    )
                    .await
                    .unwrap(),
            )
        };
        for name in ["Back squat", "Back extension"] {
            call(
                "create_exercise",
                json!({"name":name,"contraction_type":"dynamic"}),
            )
            .await;
        }
        let working = json!([{"set_type":"working","reps":5,"load_g":60000}]);

        let all_missing = call(
            "log_workout",
            json!({
                "started_at":"2026-08-14",
                "exercises":[
                    {"exercise":"Back squat","sets":working},
                    {"exercise":"Hack squat","sets":working},
                    {"exercise":"Sissy squat","sets":working}
                ]
            }),
        )
        .await;
        assert_eq!(all_missing["error"], "not_found");
        assert_eq!(all_missing["missing"], json!(["Hack squat", "Sissy squat"]));
        assert_eq!(all_missing["ambiguous"], json!([]));
        let message = all_missing["message"].as_str().unwrap();
        assert!(message.contains("catalogue administrator"));
        assert!(message.contains("Retrying cannot fix this"));

        let mixed = call(
            "log_workout",
            json!({
                "started_at":"2026-08-14",
                "exercises":[
                    {"exercise":"back","sets":working},
                    {"exercise":"Hack squat","sets":working}
                ]
            }),
        )
        .await;
        assert_eq!(mixed["error"], "not_found");
        assert_eq!(mixed["missing"], json!(["Hack squat"]));
        assert_eq!(mixed["ambiguous"][0]["query"], "back");
        assert_eq!(
            mixed["ambiguous"][0]["candidates"]
                .as_array()
                .unwrap()
                .len(),
            2
        );

        let only_ambiguous = call(
            "log_workout",
            json!({
                "started_at":"2026-08-14",
                "exercises":[{"exercise":"back","sets":working}]
            }),
        )
        .await;
        assert_eq!(only_ambiguous["error"], "ambiguous");
        assert_eq!(only_ambiguous["missing"], json!([]));
        assert!(
            only_ambiguous["message"]
                .as_str()
                .unwrap()
                .contains("exact name or the id")
        );

        let logged = call(
            "log_workout",
            json!({
                "started_at":"2026-08-14",
                "exercises":[{"exercise":"Back squat","sets":working}]
            }),
        )
        .await;
        let session_id = logged["id"].as_str().unwrap().to_owned();
        let replace_missing = call(
            "replace_workout",
            json!({
                "id":session_id,
                "started_at":"2026-08-14",
                "exercises":[
                    {"exercise":"Hack squat","sets":working},
                    {"exercise":"Sissy squat","sets":working}
                ]
            }),
        )
        .await;
        assert_eq!(replace_missing["error"], "not_found");
        assert_eq!(
            replace_missing["missing"],
            json!(["Hack squat", "Sissy squat"])
        );
        assert_eq!(replace_missing["valid_example"]["name"], "replace_workout");

        let replaced = call(
            "replace_workout",
            json!({
                "id":session_id,
                "started_at":"2026-08-15",
                "label":"corrected",
                "exercises":[{"exercise":"Back extension","sets":[
                    {"set_type":"working","reps":8,"load_g":20000}
                ]}]
            }),
        )
        .await;
        assert_eq!(replaced["id"], session_id);
        assert_eq!(replaced["label"], "corrected");
        let exercises = replaced["activity"]["exercises"].as_array().unwrap();
        assert_eq!(exercises.len(), 1);
        assert_eq!(exercises[0]["exercise_name"], "Back extension");
    }

    /// Replacing a workout writes new rows, so only the identity of the session
    /// survives; everything the caller sent back must come out the same.
    fn without_row_ids(session: &serde_json::Value) -> serde_json::Value {
        let mut session = session.clone();
        if let Some(exercises) = session["activity"]["exercises"].as_array_mut() {
            for exercise in exercises {
                exercise["id"] = serde_json::Value::Null;
                for set in exercise["sets"].as_array_mut().into_iter().flatten() {
                    set["id"] = serde_json::Value::Null;
                    set["session_exercise_id"] = serde_json::Value::Null;
                }
            }
        }
        session
    }

    #[tokio::test]
    async fn what_get_workout_session_returns_replaces_the_workout_unchanged() {
        let (server, _, _, superuser, _) = server_fixture().await;
        let call = async |name: &str, arguments: serde_json::Value| {
            structured_value(
                &server
                    .dispatch_tool(
                        name,
                        serde_json::from_value(arguments).unwrap(),
                        Some(superuser.clone()),
                    )
                    .await
                    .unwrap(),
            )
        };
        for (name, contraction) in [("Back squat", "dynamic"), ("Plank", "isometric")] {
            call(
                "create_exercise",
                json!({"name":name,"contraction_type":contraction}),
            )
            .await;
        }

        let logged = call(
            "log_workout",
            json!({
                "started_at":"2026-08-20T18:30:00Z",
                "label":"Leg day",
                "notes":"slept badly",
                "exercises":[
                    {"exercise":"Back squat","notes":"belt on","sets":[
                        {"set_type":"warmup","reps":8,"load_g":40000},
                        {"set_type":"working","reps":5,"load_g":60000,"notes":"left knee twinge"}
                    ]},
                    {"exercise":"Plank","sets":[{"set_type":"working","hold_sec":45}]}
                ]
            }),
        )
        .await;
        let read = call("get_workout_session", json!({"id":logged["id"]})).await;

        let replaced = call("replace_workout", read.clone()).await;
        assert!(replaced.get("error").is_none(), "{replaced}");
        assert_eq!(without_row_ids(&replaced), without_row_ids(&read));
        let read_again = call("get_workout_session", json!({"id":logged["id"]})).await;
        assert_eq!(without_row_ids(&read_again), without_row_ids(&read));
    }

    #[tokio::test]
    async fn an_exercise_id_and_exercise_name_that_disagree_are_refused() {
        let (server, _, _, superuser, _) = server_fixture().await;
        let call = async |name: &str, arguments: serde_json::Value| {
            structured_value(
                &server
                    .dispatch_tool(
                        name,
                        serde_json::from_value(arguments).unwrap(),
                        Some(superuser.clone()),
                    )
                    .await
                    .unwrap(),
            )
        };
        for name in ["Back squat", "Front squat"] {
            call(
                "create_exercise",
                json!({"name":name,"contraction_type":"dynamic"}),
            )
            .await;
        }
        let logged = call(
            "log_workout",
            json!({
                "started_at":"2026-08-22",
                "exercises":[{"exercise":"Back squat","sets":[
                    {"set_type":"working","reps":5,"load_g":60000}
                ]}]
            }),
        )
        .await;
        let read = call("get_workout_session", json!({"id":logged["id"]})).await;

        // The agreeing pair that get_workout_session emits must keep working.
        let unchanged = call("replace_workout", read.clone()).await;
        assert!(unchanged.get("error").is_none(), "{unchanged}");

        let mut edited = read.clone();
        edited["activity"]["exercises"][0]["exercise_name"] = json!("Front squat");
        let rejected = call("replace_workout", edited).await;
        assert_eq!(rejected["error"], "invalid_input");
        let message = rejected["message"].as_str().unwrap();
        assert!(message.contains("exercise entry 0"), "{message}");
        assert!(message.contains("'Front squat'"), "{message}");
        assert!(
            message.contains("send only the field you changed"),
            "{message}"
        );

        let stored = call("get_workout_session", json!({"id":logged["id"]})).await;
        assert_eq!(
            stored["activity"]["exercises"][0]["exercise_name"],
            json!("Back squat")
        );
    }

    #[tokio::test]
    async fn a_position_that_disagrees_with_the_array_order_is_refused() {
        let (server, _, _, superuser, _) = server_fixture().await;
        let call = async |name: &str, arguments: serde_json::Value| {
            structured_value(
                &server
                    .dispatch_tool(
                        name,
                        serde_json::from_value(arguments).unwrap(),
                        Some(superuser.clone()),
                    )
                    .await
                    .unwrap(),
            )
        };
        call(
            "create_exercise",
            json!({"name":"Back squat","contraction_type":"dynamic"}),
        )
        .await;

        let reordered_sets = call(
            "log_workout",
            json!({
                "started_at":"2026-08-21",
                "exercises":[{"exercise":"Back squat","sets":[
                    {"position":1,"set_type":"working","reps":5,"load_g":60000},
                    {"position":0,"set_type":"working","reps":5,"load_g":62500}
                ]}]
            }),
        )
        .await;
        assert_eq!(reordered_sets["error"], "invalid_input");
        let message = reordered_sets["message"].as_str().unwrap();
        assert!(
            message.contains("set 0 of exercise 'Back squat'"),
            "{message}"
        );
        assert!(message.contains("declares position 1"), "{message}");
        assert!(message.contains("reorder the sets array"), "{message}");

        let reordered_exercises = call(
            "log_workout",
            json!({
                "started_at":"2026-08-21",
                "exercises":[{"exercise":"Back squat","position":3,"sets":[
                    {"set_type":"working","reps":5,"load_g":60000}
                ]}]
            }),
        )
        .await;
        assert_eq!(reordered_exercises["error"], "invalid_input");
        assert!(
            reordered_exercises["message"]
                .as_str()
                .unwrap()
                .contains("reorder the array to reorder the exercises")
        );

        let matching = call(
            "log_workout",
            json!({
                "started_at":"2026-08-21",
                "exercises":[{"exercise":"Back squat","position":0,"sets":[
                    {"position":0,"set_type":"working","reps":5,"load_g":60000},
                    {"position":1,"set_type":"working","reps":5,"load_g":62500}
                ]}]
            }),
        )
        .await;
        assert!(matching.get("error").is_none(), "{matching}");
    }

    /// create_workout_session can open a strength session with no exercises, so
    /// get_workout_session on it must round-trip through replace_workout.
    #[tokio::test]
    async fn replace_workout_round_trips_an_empty_strength_session() {
        let (server, _, _, superuser, _) = server_fixture().await;
        let call = async |name: &str, arguments: serde_json::Value| {
            structured_value(
                &server
                    .dispatch_tool(
                        name,
                        serde_json::from_value(arguments).unwrap(),
                        Some(superuser.clone()),
                    )
                    .await
                    .unwrap(),
            )
        };
        let session = call(
            "create_workout_session",
            json!({"started_at":"2026-08-24","activity":{"type":"strength"}}),
        )
        .await;
        let read = call("get_workout_session", json!({"id":session["id"]})).await;
        assert_eq!(
            read["activity"]["exercises"].as_array().unwrap().len(),
            0,
            "an empty session must read back with an empty exercises list"
        );
        assert_eq!(call("replace_workout", read.clone()).await, read);
    }

    #[tokio::test]
    async fn replace_workout_corrects_a_run_and_changes_the_activity_type() {
        let (server, _, _, superuser, _) = server_fixture().await;
        let call = async |name: &str, arguments: serde_json::Value| {
            structured_value(
                &server
                    .dispatch_tool(
                        name,
                        serde_json::from_value(arguments).unwrap(),
                        Some(superuser.clone()),
                    )
                    .await
                    .unwrap(),
            )
        };
        call(
            "create_exercise",
            json!({"name":"Back squat","contraction_type":"dynamic"}),
        )
        .await;
        let working = json!([{"set_type":"working","reps":5,"load_g":60000}]);

        let run = call(
            "create_workout_session",
            json!({
                "started_at":"2026-08-22T07:00:00Z","label":"Morning run",
                "activity":{"type":"run","distance_m":5000,"duration_sec":1800}
            }),
        )
        .await;
        let read = call("get_workout_session", json!({"id":run["id"]})).await;
        assert_eq!(call("replace_workout", read.clone()).await, read);

        let corrected = call(
            "replace_workout",
            json!({
                "id":run["id"],"started_at":"2026-08-22T07:05:00Z","label":"Morning run",
                "activity":{"type":"run","distance_m":6000,"duration_sec":1900,"elevation_gain_m":40}
            }),
        )
        .await;
        assert_eq!(corrected["activity"]["distance_m"], 6000);
        assert_eq!(corrected["activity"]["duration_sec"], 1900);
        assert_eq!(corrected["activity"]["elevation_gain_m"], 40);

        // A mistyped activity is correctable in place, so the session keeps its
        // id instead of having to be deleted and logged again.
        let exercises_against_a_run = call(
            "replace_workout",
            json!({
                "id":run["id"],"started_at":"2026-08-22",
                "exercises":[{"exercise":"Back squat","sets":working}]
            }),
        )
        .await;
        assert_eq!(exercises_against_a_run["id"], run["id"]);
        assert_eq!(exercises_against_a_run["activity"]["type"], "strength");
        assert_eq!(
            exercises_against_a_run["activity"]["exercises"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            call("get_workout_session", json!({"id":run["id"]})).await,
            exercises_against_a_run
        );

        let strength = call(
            "log_workout",
            json!({
                "started_at":"2026-08-23",
                "exercises":[{"exercise":"Back squat","sets":working}]
            }),
        )
        .await;
        let run_against_a_workout = call(
            "replace_workout",
            json!({
                "id":strength["id"],"started_at":"2026-08-23",
                "activity":{"type":"run","distance_m":1000,"duration_sec":600}
            }),
        )
        .await;
        assert_eq!(run_against_a_workout["id"], strength["id"]);
        assert_eq!(run_against_a_workout["activity"]["type"], "run");
        assert_eq!(run_against_a_workout["activity"]["distance_m"], 1000);
        assert_eq!(
            call("get_workout_session", json!({"id":strength["id"]})).await,
            run_against_a_workout
        );

        let both = call(
            "replace_workout",
            json!({
                "id":strength["id"],"started_at":"2026-08-23",
                "activity":{"type":"strength","exercises":[{"exercise":"Back squat","sets":working}]},
                "exercises":[{"exercise":"Back squat","sets":working}]
            }),
        )
        .await;
        assert_eq!(both["error"], "invalid_input");
        assert!(both["message"].as_str().unwrap().contains("not both"));

        let neither = call(
            "replace_workout",
            json!({"id":strength["id"],"started_at":"2026-08-23"}),
        )
        .await;
        assert_eq!(neither["error"], "invalid_input");

        let two_references = call(
            "replace_workout",
            json!({
                "id":strength["id"],"started_at":"2026-08-23",
                "exercises":[{"exercise":"Back squat","exercise_id":strength["id"],"sets":working}]
            }),
        )
        .await;
        assert_eq!(two_references["error"], "invalid_input");
        assert!(
            two_references["message"]
                .as_str()
                .unwrap()
                .contains("but not both")
        );
    }

    #[tokio::test]
    async fn batch_resolvers_answer_every_name_in_order_without_failing_wholesale() {
        let (server, _, _, superuser, _) = server_fixture().await;
        let call = async |name: &str, arguments: serde_json::Value| {
            structured_value(
                &server
                    .dispatch_tool(
                        name,
                        serde_json::from_value(arguments).unwrap(),
                        Some(superuser.clone()),
                    )
                    .await
                    .unwrap(),
            )
        };
        for name in ["Back squat", "Back extension"] {
            call(
                "create_exercise",
                json!({"name":name,"contraction_type":"dynamic"}),
            )
            .await;
        }
        let exercises = call(
            "resolve_exercises",
            json!({"names":["Back squat","back","Hack squat"]}),
        )
        .await;
        let results = exercises["results"].as_array().unwrap();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0]["query"], "Back squat");
        assert_eq!(results[0]["status"], "found");
        assert_eq!(results[0]["match"]["name"], "Back squat");
        assert_eq!(results[0]["match"]["contraction_type"], "dynamic");
        assert_eq!(results[1]["query"], "back");
        assert_eq!(results[1]["status"], "ambiguous");
        assert_eq!(results[1]["candidates"].as_array().unwrap().len(), 2);
        assert_eq!(results[2]["query"], "Hack squat");
        assert_eq!(results[2]["status"], "missing");

        let by_id = call("resolve_exercises", json!({"names":["Back squat"]})).await;
        let id = by_id["results"][0]["match"]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let again = call("resolve_exercises", json!({"names":[id.clone()]})).await;
        assert_eq!(again["results"][0]["match"]["id"], id);

        let blank = call("resolve_exercises", json!({"names":["   ","Back squat"]})).await;
        assert_eq!(blank["results"][0]["status"], "invalid");
        assert_eq!(blank["results"][1]["status"], "found");

        let too_many = call(
            "resolve_exercises",
            json!({"names":(0..101).map(|index| format!("name {index}")).collect::<Vec<_>>()}),
        )
        .await;
        assert_eq!(too_many["error"], "invalid_input");
    }

    #[tokio::test]
    async fn exercise_writes_take_names_or_ids_and_round_trip_through_the_read_shape() {
        let (server, _, _, superuser, _) = server_fixture().await;
        let call = async |name: &str, arguments: serde_json::Value| {
            structured_value(
                &server
                    .dispatch_tool(
                        name,
                        serde_json::from_value(arguments).unwrap(),
                        Some(superuser.clone()),
                    )
                    .await
                    .unwrap(),
            )
        };
        let quadriceps = call("create_muscle", json!({"name":"Rectus Abdominis"})).await;
        let obliques = call("create_muscle", json!({"name":"Obliques"})).await;
        let cable = call("create_equipment", json!({"name":"Cable Machine"})).await;

        let created = call(
            "create_exercise",
            json!({
                "name":"Cable crunch","contraction_type":"dynamic",
                "primary_muscles":["Rectus Abdominis"],
                "secondary_muscles":["Obliques"],
                "equipment":["Cable Machine"]
            }),
        )
        .await;
        assert_eq!(created["primary_muscles"][0]["id"], quadriceps["id"]);
        assert_eq!(created["secondary_muscles"][0]["id"], obliques["id"]);
        assert_eq!(created["equipment"][0]["id"], cable["id"]);

        let read = call("get_exercise", json!({"id":"Cable crunch"})).await;
        assert_eq!(read, created);

        let by_id = call(
            "update_exercise",
            json!({
                "id":created["id"],"name":"Cable crunch","contraction_type":"dynamic",
                "bodyweight_share":0,
                "primary_muscles":[quadriceps["id"]],
                "secondary_muscles":[obliques["id"]],
                "equipment":[cable["id"]]
            }),
        )
        .await;
        assert_eq!(by_id, created);

        let replaced = call(
            "update_exercise",
            json!({
                "id":created["id"],"name":"Cable crunch","contraction_type":"dynamic",
                "bodyweight_share":0,
                "primary_muscles":["Obliques"]
            }),
        )
        .await;
        assert_eq!(replaced["primary_muscles"][0]["id"], obliques["id"]);
        assert!(replaced["secondary_muscles"].as_array().unwrap().is_empty());
        assert!(replaced["equipment"].as_array().unwrap().is_empty());

        let both_roles = call(
            "create_exercise",
            json!({
                "name":"Contradiction","contraction_type":"dynamic",
                "primary_muscles":["Obliques"],"secondary_muscles":["obliq"]
            }),
        )
        .await;
        assert_eq!(both_roles["error"], "invalid_input");
        assert!(
            both_roles["message"]
                .as_str()
                .unwrap()
                .contains("never both")
        );

        let missing = call(
            "create_exercise",
            json!({"name":"Unknown link","contraction_type":"dynamic","equipment":["Sled"]}),
        )
        .await;
        assert_eq!(missing["error"], "not_found");
        assert!(
            missing["message"]
                .as_str()
                .unwrap()
                .contains("list_equipment")
        );
    }

    #[tokio::test]
    async fn the_removed_and_merged_tools_are_no_longer_dispatchable() {
        let (server, _, _, superuser, _) = server_fixture().await;
        for name in [
            "volume_stats",
            "workout_history",
            "list_workout_sessions",
            "update_workout_session",
            "list_session_exercises",
            "get_session_exercise",
            "add_session_exercise",
            "update_session_exercise",
            "remove_session_exercise",
            "list_exercise_sets",
            "get_exercise_set",
            "add_exercise_set",
            "update_exercise_set",
            "remove_exercise_set",
        ] {
            let result = server
                .dispatch_tool(name, JsonObject::new(), Some(superuser.clone()))
                .await
                .unwrap();
            assert_eq!(
                structured_value(&result)["error"],
                "not_found",
                "tool {name}"
            );
        }
    }

    #[tokio::test]
    async fn tool_scope_and_role_matrix_is_enforced() {
        let (server, read_only, ordinary_admin, superuser, _) = server_fixture().await;
        let superuser_view = superuser.clone();
        let timestamp = "2026-01-01T00:00:00Z";
        let create_session = serde_json::from_value(json!({
            "started_at": timestamp,
            "activity": {"type": "strength"}
        }))
        .unwrap();
        let denied = server
            .dispatch_tool(
                "create_workout_session",
                create_session,
                Some(read_only.clone()),
            )
            .await
            .unwrap();
        assert_eq!(structured_value(&denied)["error"], "forbidden");
        assert_eq!(
            structured_value(&denied)["message"],
            "workouts:write scope required"
        );

        let write_only_for_catalogue =
            test_oauth_principal(read_only.user_id(), "user", "workouts:read workouts:write");
        let denied = server
            .dispatch_tool(
                "create_muscle",
                serde_json::from_value(json!({"name": "Denied"})).unwrap(),
                Some(write_only_for_catalogue),
            )
            .await
            .unwrap();
        assert_eq!(structured_value(&denied)["error"], "forbidden");
        assert_eq!(
            structured_value(&denied)["message"],
            "catalogue:write scope required"
        );

        let denied = server
            .dispatch_tool(
                "create_muscle",
                serde_json::from_value(json!({"name": "Denied"})).unwrap(),
                Some(ordinary_admin),
            )
            .await
            .unwrap();
        assert_eq!(structured_value(&denied)["error"], "forbidden");

        let created = server
            .dispatch_tool(
                "create_muscle",
                serde_json::from_value(json!({"name": "Allowed"})).unwrap(),
                Some(superuser),
            )
            .await
            .unwrap();
        assert_eq!(structured_value(&created)["name"], "Allowed");

        let listed = server
            .dispatch_tool("list_muscles", JsonObject::new(), Some(read_only.clone()))
            .await
            .unwrap();
        assert_eq!(structured_value(&listed)["items"][0]["name"], "Allowed");

        let workouts_only =
            test_oauth_principal(read_only.user_id(), "user", "workouts:read workouts:write");
        let denied = server
            .dispatch_tool("list_muscles", JsonObject::new(), Some(workouts_only))
            .await
            .unwrap();
        assert_eq!(structured_value(&denied)["error"], "forbidden");
        assert_eq!(
            structured_value(&denied)["message"],
            "catalogue:read scope required"
        );

        let catalogue_writer = test_oauth_principal(
            read_only.user_id(),
            "user",
            "workouts:read catalogue:read catalogue:write",
        );
        let listed = server
            .dispatch_tool(
                "list_muscles",
                JsonObject::new(),
                Some(catalogue_writer.clone()),
            )
            .await
            .unwrap();
        assert_eq!(structured_value(&listed)["items"][0]["name"], "Allowed");

        // What a principal sees in tools/list, against what it can call.
        let seen = |principal: &crate::domain::Principal| {
            server
                .discoverable_tools(principal)
                .into_iter()
                .map(|tool| tool.name.to_string())
                .collect::<std::collections::HashSet<_>>()
        };
        let read_only_view = seen(&read_only);
        assert_eq!(
            read_only_view,
            [
                "list_muscles",
                "get_muscle",
                "list_equipment",
                "get_equipment",
                "list_exercises",
                "get_exercise",
                "resolve_exercises",
                "list_workouts",
                "get_workout_session",
                "exercise_history",
                "personal_records",
                "list_bodyweight",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect::<std::collections::HashSet<_>>()
        );

        // Discovery now matches reality: holding catalogue:write without the
        // superuser role no longer advertises a tool the call would refuse.
        assert!(!seen(&catalogue_writer).contains("create_muscle"));
        assert!(seen(&superuser_view).contains("create_muscle"));
        assert!(seen(&superuser_view).contains("list_muscles"));
    }

    #[tokio::test]
    async fn bodyweight_readings_are_upserted_listed_and_scoped_to_their_owner() {
        let (server, _, other, superuser, _) = server_fixture().await;
        let call = async |name: &str,
                          arguments: serde_json::Value,
                          principal: &crate::domain::Principal| {
            structured_value(
                &server
                    .dispatch_tool(
                        name,
                        serde_json::from_value(arguments).unwrap(),
                        Some(principal.clone()),
                    )
                    .await
                    .unwrap(),
            )
        };

        let first = call(
            "log_bodyweight",
            json!({"recorded_on":"2026-08-01","mass_g":72500}),
            &superuser,
        )
        .await;
        assert_eq!(first["recorded_on"], "2026-08-01");
        let corrected = call(
            "log_bodyweight",
            json!({"recorded_on":"2026-08-01","mass_g":72000}),
            &superuser,
        )
        .await;
        assert_eq!(corrected["id"], first["id"]);
        assert_eq!(corrected["mass_g"], 72000);
        call(
            "log_bodyweight",
            json!({"recorded_on":"2026-08-08","mass_g":71500}),
            &superuser,
        )
        .await;

        let listed = call("list_bodyweight", json!({}), &superuser).await;
        assert_eq!(
            listed["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|item| item["recorded_on"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["2026-08-08", "2026-08-01"]
        );
        let window = call(
            "list_bodyweight",
            json!({"from":"2026-08-08","limit":1}),
            &superuser,
        )
        .await;
        assert_eq!(window["items"].as_array().unwrap().len(), 1);
        assert_eq!(window["items"][0]["mass_g"], 71500);

        // Another user neither sees nor deletes these readings.
        assert!(
            call("list_bodyweight", json!({}), &other).await["items"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        let denied = call("delete_bodyweight", json!({"id":first["id"]}), &other).await;
        assert_eq!(denied["error"], "not_found");
        let deleted = call("delete_bodyweight", json!({"id":first["id"]}), &superuser).await;
        assert_eq!(deleted["deleted"], true);
        assert_eq!(
            call("list_bodyweight", json!({}), &superuser).await["items"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn bodyweight_share_round_trips_defaults_on_create_and_is_required_on_update() {
        let (server, _, _, superuser, _) = server_fixture().await;
        let call = async |name: &str, arguments: serde_json::Value| {
            structured_value(
                &server
                    .dispatch_tool(
                        name,
                        serde_json::from_value(arguments).unwrap(),
                        Some(superuser.clone()),
                    )
                    .await
                    .unwrap(),
            )
        };

        let pull_up = call(
            "create_exercise",
            json!({"name":"Pull-up","contraction_type":"dynamic","bodyweight_share":100}),
        )
        .await;
        assert_eq!(pull_up["bodyweight_share"], 100);
        assert_eq!(call("get_exercise", json!({"id":"Pull-up"})).await, pull_up);

        let reduced = call(
            "update_exercise",
            json!({
                "id":pull_up["id"],"name":"Pull-up","contraction_type":"dynamic",
                "bodyweight_share":65
            }),
        )
        .await;
        assert_eq!(reduced["bodyweight_share"], 65);
        let omitted = call(
            "update_exercise",
            json!({"id":pull_up["id"],"name":"Pull up","contraction_type":"dynamic"}),
        )
        .await;
        assert_eq!(omitted["error"], "invalid_input");
        assert_eq!(
            call("get_exercise", json!({"id":pull_up["id"]}))
                .await
                .get("bodyweight_share"),
            Some(&json!(65))
        );

        let barbell = call(
            "create_exercise",
            json!({"name":"Back squat","contraction_type":"dynamic"}),
        )
        .await;
        assert_eq!(barbell["bodyweight_share"], 0);

        for share in [101, -1] {
            let refused = call(
                "create_exercise",
                json!({"name":format!("Impossible {share}"),"contraction_type":"dynamic","bodyweight_share":share}),
            )
            .await;
            assert_eq!(refused["error"], "invalid_input", "share {share}");
        }
    }
}
