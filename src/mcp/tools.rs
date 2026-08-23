use rmcp::{
    ErrorData,
    model::{CallToolResult, JsonObject},
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::domain::{
    AddSessionExercise, CreateWorkoutSession, DomainError, ExerciseInput, LogWorkout,
    LogWorkoutExercise, Lookup, NamedInput, Principal, RepeatLastWorkout, UpdateSessionExercise,
};

use super::McpServer;
use super::schemas::{
    AddExerciseSetArg, AddSessionExerciseArg, ExerciseHistoryArg, IdArg, ListArgs, ListChildArgs,
    ListSessionArgs, LogWorkoutArg, PersonalRecordsArg, ReferenceArg, SessionHistoryArg,
    UpdateExerciseArg, UpdateExerciseSetArg, UpdateNameArg, UpdateSessionArg,
    UpdateSessionExerciseArg, VolumeStatsArg, example_for_tool,
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
            return Ok(tool_error(
                "insufficient_scope",
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
            "create_exercise" => {
                let input = input!(ExerciseInput);
                output!(self.domain.create_exercise(&principal, input))
            }
            "update_exercise" => {
                let input = input!(UpdateExerciseArg);
                output!(
                    self.domain
                        .update_exercise(&principal, input.id, input.input)
                )
            }
            "delete_exercise" => {
                let input = input!(IdArg);
                match self.domain.delete_exercise(&principal, input.id).await {
                    Ok(()) => deleted(input.id),
                    Err(error) => domain_tool_error(name, error),
                }
            }
            "list_workout_sessions" => {
                let input = input!(ListSessionArgs);
                output!(
                    self.domain
                        .list_sessions(&principal, input.filter(), input.page())
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
            "update_workout_session" => {
                let input = input!(UpdateSessionArg);
                output!(
                    self.domain
                        .update_session(&principal, input.id, input.input)
                )
            }
            "delete_workout_session" => {
                let input = input!(IdArg);
                match self.domain.delete_session(&principal, input.id).await {
                    Ok(()) => deleted(input.id),
                    Err(error) => domain_tool_error(name, error),
                }
            }
            "list_session_exercises" => {
                let input = input!(ListChildArgs);
                output!(self.domain.list_session_exercises(
                    &principal,
                    input.parent_id,
                    input.page()
                ))
            }
            "get_session_exercise" => {
                let input = input!(IdArg);
                output!(self.domain.get_session_exercise(&principal, input.id))
            }
            "add_session_exercise" => {
                let input = input!(AddSessionExerciseArg);
                let exercise_id = exercise_id!(&input.exercise);
                output!(self.domain.add_session_exercise(
                    &principal,
                    input.session_id,
                    AddSessionExercise {
                        exercise_id,
                        position: input.position,
                    }
                ))
            }
            "update_session_exercise" => {
                let input = input!(UpdateSessionExerciseArg);
                let exercise_id = exercise_id!(&input.exercise);
                output!(self.domain.update_session_exercise(
                    &principal,
                    input.id,
                    UpdateSessionExercise {
                        exercise_id,
                        position: input.position,
                    }
                ))
            }
            "remove_session_exercise" => {
                let input = input!(IdArg);
                match self
                    .domain
                    .remove_session_exercise(&principal, input.id)
                    .await
                {
                    Ok(()) => deleted(input.id),
                    Err(error) => domain_tool_error(name, error),
                }
            }
            "list_exercise_sets" => {
                let input = input!(ListChildArgs);
                output!(
                    self.domain
                        .list_exercise_sets(&principal, input.parent_id, input.page())
                )
            }
            "get_exercise_set" => {
                let input = input!(IdArg);
                output!(self.domain.get_exercise_set(&principal, input.id))
            }
            "add_exercise_set" => {
                let input = input!(AddExerciseSetArg);
                output!(self.domain.add_exercise_set(
                    &principal,
                    input.session_exercise_id,
                    input.input
                ))
            }
            "update_exercise_set" => {
                let input = input!(UpdateExerciseSetArg);
                output!(
                    self.domain
                        .update_exercise_set(&principal, input.id, input.input)
                )
            }
            "remove_exercise_set" => {
                let input = input!(IdArg);
                match self.domain.remove_exercise_set(&principal, input.id).await {
                    Ok(()) => deleted(input.id),
                    Err(error) => domain_tool_error(name, error),
                }
            }
            "log_workout" => {
                let input = input!(LogWorkoutArg);
                let mut exercises = Vec::with_capacity(input.exercises.len());
                for entry in input.exercises {
                    exercises.push(LogWorkoutExercise {
                        exercise_id: exercise_id!(&entry.exercise),
                        sets: entry.sets,
                    });
                }
                output!(self.domain.log_workout(
                    &principal,
                    LogWorkout {
                        started_at: input.started_at,
                        label: input.label,
                        exercises,
                    }
                ))
            }
            "repeat_last_workout" => {
                let input = input!(RepeatLastWorkout);
                output!(self.domain.repeat_last_workout(&principal, input))
            }
            "workout_history" => {
                let input = input!(SessionHistoryArg);
                output!(
                    self.domain
                        .session_history(&principal, input.range(), input.limit)
                )
            }
            "exercise_history" => {
                let input = input!(ExerciseHistoryArg);
                let id = exercise_id!(&input.exercise);
                output!(
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
                output!(
                    self.domain
                        .personal_records(&principal, exercise_id, input.range())
                )
            }
            "volume_stats" => {
                let input = input!(VolumeStatsArg);
                output!(
                    self.domain
                        .volume_stats(&principal, input.group_by, input.range())
                )
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
    async fn workflow_tools_log_repeat_and_report_by_exercise_name() {
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

        let repeated = call(
            "repeat_last_workout",
            json!({"started_at":"2026-08-12","like_label":"Leg"}),
        )
        .await;
        assert_eq!(
            repeated["activity"]["exercises"][0]["sets"][1]["load_g"],
            60000
        );

        let history = call("workout_history", json!({"from":"2026-08-10"})).await;
        assert_eq!(history.as_array().unwrap().len(), 2);
        assert_eq!(history[0]["set_count"], 2);

        let single_day = call(
            "workout_history",
            json!({"from":"2026-08-10","to":"2026-08-10"}),
        )
        .await;
        assert_eq!(single_day.as_array().unwrap().len(), 1);

        let progress = call(
            "exercise_history",
            json!({"exercise":"Back squat","limit":5}),
        )
        .await;
        assert_eq!(progress[0]["sets"][1]["estimated_1rm_g"], 70000);

        let records = call("personal_records", json!({"exercise":"Back squat"})).await;
        assert_eq!(records[0]["max_load_g"], 60000);

        let volume = call("volume_stats", json!({"group_by":"exercise"})).await;
        assert_eq!(volume[0]["volume_g"], 1_240_000);

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
    async fn tool_scope_and_role_matrix_is_enforced() {
        let (server, read_only, ordinary_admin, superuser, _) = server_fixture().await;
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
        assert_eq!(structured_value(&denied)["error"], "insufficient_scope");
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
        assert_eq!(structured_value(&denied)["error"], "insufficient_scope");
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
        assert_eq!(structured_value(&denied)["error"], "insufficient_scope");
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
            .dispatch_tool("list_muscles", JsonObject::new(), Some(catalogue_writer))
            .await
            .unwrap();
        assert_eq!(structured_value(&listed)["items"][0]["name"], "Allowed");
    }
}
