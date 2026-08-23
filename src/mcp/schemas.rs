use rmcp::model::JsonObject;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::domain::{
    AddExerciseSet, ExerciseInput, PageRequest, SessionFilter, StatsRange, Timestamp,
    UpdateExerciseSet, UpdateWorkoutSession, VolumeGrouping,
};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct IdArg {
    pub(super) id: Uuid,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReferenceArg {
    pub(super) id: String,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateNameArg {
    pub(super) id: Uuid,
    pub(super) name: String,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ListArgs {
    pub(super) query: Option<String>,
    offset: Option<u64>,
    limit: Option<u64>,
}
impl ListArgs {
    pub(super) fn page(&self) -> PageRequest {
        PageRequest {
            offset: self.offset.unwrap_or(0),
            limit: self.limit,
        }
    }
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateExerciseArg {
    pub(super) id: Uuid,
    #[serde(flatten)]
    pub(super) input: ExerciseInput,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ListSessionArgs {
    started_at_from: Option<Timestamp>,
    started_at_to: Option<Timestamp>,
    activity: Option<String>,
    offset: Option<u64>,
    limit: Option<u64>,
}
impl ListSessionArgs {
    pub(super) fn filter(&self) -> SessionFilter {
        SessionFilter {
            started_at_from: self.started_at_from,
            started_at_to: self.started_at_to,
            activity: self.activity.clone(),
        }
    }
    pub(super) fn page(&self) -> PageRequest {
        PageRequest {
            offset: self.offset.unwrap_or(0),
            limit: self.limit,
        }
    }
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateSessionArg {
    pub(super) id: Uuid,
    #[serde(flatten)]
    pub(super) input: UpdateWorkoutSession,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ListChildArgs {
    #[serde(alias = "session_id", alias = "session_exercise_id")]
    pub(super) parent_id: Uuid,
    offset: Option<u64>,
    limit: Option<u64>,
}
impl ListChildArgs {
    pub(super) fn page(&self) -> PageRequest {
        PageRequest {
            offset: self.offset.unwrap_or(0),
            limit: self.limit,
        }
    }
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AddSessionExerciseArg {
    pub(super) session_id: Uuid,
    pub(super) exercise: String,
    pub(super) position: Option<u64>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateSessionExerciseArg {
    pub(super) id: Uuid,
    pub(super) exercise: String,
    pub(super) position: u64,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LogWorkoutArg {
    pub(super) started_at: Timestamp,
    pub(super) label: Option<String>,
    pub(super) exercises: Vec<LogWorkoutExerciseArg>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LogWorkoutExerciseArg {
    pub(super) exercise: String,
    #[serde(default)]
    pub(super) sets: Vec<AddExerciseSet>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SessionHistoryArg {
    from: Option<Timestamp>,
    to: Option<Timestamp>,
    pub(super) limit: Option<u64>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ExerciseHistoryArg {
    pub(super) exercise: String,
    from: Option<Timestamp>,
    to: Option<Timestamp>,
    pub(super) limit: Option<u64>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PersonalRecordsArg {
    pub(super) exercise: Option<String>,
    from: Option<Timestamp>,
    to: Option<Timestamp>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct VolumeStatsArg {
    pub(super) group_by: VolumeGrouping,
    from: Option<Timestamp>,
    to: Option<Timestamp>,
}
impl SessionHistoryArg {
    pub(super) fn range(&self) -> StatsRange {
        StatsRange {
            from: self.from,
            to: self.to,
        }
    }
}
impl ExerciseHistoryArg {
    pub(super) fn range(&self) -> StatsRange {
        StatsRange {
            from: self.from,
            to: self.to,
        }
    }
}
impl PersonalRecordsArg {
    pub(super) fn range(&self) -> StatsRange {
        StatsRange {
            from: self.from,
            to: self.to,
        }
    }
}
impl VolumeStatsArg {
    pub(super) fn range(&self) -> StatsRange {
        StatsRange {
            from: self.from,
            to: self.to,
        }
    }
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AddExerciseSetArg {
    pub(super) session_exercise_id: Uuid,
    #[serde(flatten)]
    pub(super) input: AddExerciseSet,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateExerciseSetArg {
    pub(super) id: Uuid,
    #[serde(flatten)]
    pub(super) input: UpdateExerciseSet,
}

/// Name, description, and the one scope the tool needs. The scope is data, not
/// a rule inferred from the name, so adding a tool cannot silently give it the
/// wrong scope. `list_tools` filters on it and `dispatch_tool` enforces it.
pub(super) const TOOL_SPECS: &[(&str, &str, &str)] = &[
    (
        "list_muscles",
        "Search the muscle catalogue by name. Input: optional query, offset, limit. Returns {items:[{id,name}], next_offset}. Needs the catalogue:read scope.",
        "catalogue:read",
    ),
    (
        "get_muscle",
        "Look up one muscle by id or name; names are case-insensitive and may be a unique prefix. Returns {id,name}. Needs the catalogue:read scope.",
        "catalogue:read",
    ),
    (
        "create_muscle",
        "Add a muscle to the shared catalogue. Input: name. Returns {id,name}. Needs the catalogue:write scope and the superuser role.",
        "catalogue:write",
    ),
    (
        "update_muscle",
        "Rename a muscle. Input: id, name. Returns {id,name}. Needs the catalogue:write scope and the superuser role.",
        "catalogue:write",
    ),
    (
        "delete_muscle",
        "Delete a muscle that no exercise uses. Input: id. Returns {deleted,id}. Needs the catalogue:write scope and the superuser role.",
        "catalogue:write",
    ),
    (
        "list_equipment",
        "Search the equipment catalogue by name. Input: optional query, offset, limit. Returns {items:[{id,name}], next_offset}. Needs the catalogue:read scope.",
        "catalogue:read",
    ),
    (
        "get_equipment",
        "Look up one equipment item by id or name; names are case-insensitive and may be a unique prefix. Returns {id,name}. Needs the catalogue:read scope.",
        "catalogue:read",
    ),
    (
        "create_equipment",
        "Add equipment to the shared catalogue. Input: name. Returns {id,name}. Needs the catalogue:write scope and the superuser role.",
        "catalogue:write",
    ),
    (
        "update_equipment",
        "Rename an equipment item. Input: id, name. Returns {id,name}. Needs the catalogue:write scope and the superuser role.",
        "catalogue:write",
    ),
    (
        "delete_equipment",
        "Delete equipment that no exercise uses. Input: id. Returns {deleted,id}. Needs the catalogue:write scope and the superuser role.",
        "catalogue:write",
    ),
    (
        "list_exercises",
        "Find exercises by name before logging a workout. Input: optional query, offset, limit. Returns {items:[{id,name,contraction_type}], next_offset}. Needs the catalogue:read scope.",
        "catalogue:read",
    ),
    (
        "get_exercise",
        "Get one exercise with its muscles and equipment. Input: id or name. Returns {id,name,contraction_type,muscles,equipment}. Needs the catalogue:read scope.",
        "catalogue:read",
    ),
    (
        "create_exercise",
        "Add an exercise and its muscle and equipment links in one atomic call. Input: name, contraction_type, optional muscles and equipment_ids. Needs the catalogue:write scope and the superuser role.",
        "catalogue:write",
    ),
    (
        "update_exercise",
        "Replace an exercise and all of its links in one atomic call. Input: id, name, contraction_type, muscles, equipment_ids. Needs the catalogue:write scope and the superuser role.",
        "catalogue:write",
    ),
    (
        "delete_exercise",
        "Delete an exercise that no session uses. Input: id. Returns {deleted,id}. Needs the catalogue:write scope and the superuser role.",
        "catalogue:write",
    ),
    (
        "list_workout_sessions",
        "List your sessions, newest first, filtered by date and activity. Dates accept YYYY-MM-DD. Returns {items:[{id,started_at,label,activity_type}], next_offset}. Use workout_history for totals.",
        "workouts:read",
    ),
    (
        "get_workout_session",
        "Read one of your sessions in full, with every exercise and set. Input: id. Returns the session with nested exercises and sets.",
        "workouts:read",
    ),
    (
        "create_workout_session",
        "Create an empty strength session or a run. Input: started_at (YYYY-MM-DD or RFC 3339), optional label, activity. To record a complete strength workout in one call use log_workout instead. Needs the workouts:write scope.",
        "workouts:write",
    ),
    (
        "update_workout_session",
        "Change the date, label, or activity of one of your sessions. Changing the activity type deletes the previous activity detail. Needs the workouts:write scope.",
        "workouts:write",
    ),
    (
        "delete_workout_session",
        "Delete one of your sessions with all of its exercises and sets. Input: id. Returns {deleted,id}. Needs the workouts:write scope.",
        "workouts:write",
    ),
    (
        "list_session_exercises",
        "List the ordered exercises of one of your sessions without their sets. Input: session_id, offset, limit.",
        "workouts:read",
    ),
    (
        "get_session_exercise",
        "Read one exercise of your session with its ordered sets. Input: id.",
        "workouts:read",
    ),
    (
        "add_session_exercise",
        "Append or insert an exercise in one of your strength sessions. Input: session_id, exercise (id or name), optional position. Needs the workouts:write scope.",
        "workouts:write",
    ),
    (
        "update_session_exercise",
        "Replace or reorder an exercise inside your session. Input: id, exercise (id or name), position. Needs the workouts:write scope.",
        "workouts:write",
    ),
    (
        "remove_session_exercise",
        "Remove one exercise and its sets from your session. Input: id. Returns {deleted,id}. Needs the workouts:write scope.",
        "workouts:write",
    ),
    (
        "list_exercise_sets",
        "List the ordered sets of one exercise in your session. Input: session_exercise_id, offset, limit.",
        "workouts:read",
    ),
    (
        "get_exercise_set",
        "Read one of your recorded sets. Input: id.",
        "workouts:read",
    ),
    (
        "add_exercise_set",
        "Add one set to an exercise in your session. Dynamic exercises need reps, isometric exercises need hold_sec. load_g is grams. For unilateral (per-side or per-arm) exercises, one set covers both sides: reps counts the repetitions of one side, and load_g is the load one side moves. Needs the workouts:write scope.",
        "workouts:write",
    ),
    (
        "update_exercise_set",
        "Correct or reorder one of your recorded sets. Input: id, position, set_type, reps or hold_sec, load_g. Needs the workouts:write scope.",
        "workouts:write",
    ),
    (
        "remove_exercise_set",
        "Remove one of your recorded sets. Input: id. Returns {deleted,id}. Needs the workouts:write scope.",
        "workouts:write",
    ),
    (
        "log_workout",
        "Record a finished strength workout in one atomic call: session, exercises, and sets together. Input: started_at (YYYY-MM-DD or RFC 3339), optional label, exercises[{exercise (id or name), sets[]}]. Nothing is stored if any part is invalid. Prefer this over create_workout_session plus add_* calls. Needs the workouts:write scope.",
        "workouts:write",
    ),
    (
        "repeat_last_workout",
        "Start a new session pre-filled from the user's most recent strength session, copying its sets as targets. Input: started_at, optional label, optional like_label to repeat the latest session whose label contains that text. Needs the workouts:write scope.",
        "workouts:write",
    ),
    (
        "workout_history",
        "Summarise the user's sessions in a date range, newest first. Input: optional from and to (YYYY-MM-DD), optional limit. Returns per session: id, started_at, label, activity_type, exercise_count, set_count, volume_g.",
        "workouts:read",
    ),
    (
        "exercise_history",
        "Track how one exercise progressed over time. Input: exercise (id or name), optional from and to, optional limit. Returns each performance, newest first, with its sets and estimated 1RM in grams.",
        "workouts:read",
    ),
    (
        "personal_records",
        "Report best efforts: heaviest set, best estimated one-repetition maximum (Epley), and longest hold. Input: optional exercise (id or name), optional from and to. Loads in grams.",
        "workouts:read",
    ),
    (
        "volume_stats",
        "Compare training volume (load times reps, in grams) over a date range. Input: group_by ('exercise' or 'muscle'), optional from and to. Muscle grouping counts the primary muscle of each exercise.",
        "workouts:read",
    ),
];

pub(super) fn schema_for_tool(name: &str) -> JsonObject {
    let uuid = json!({"type":"string", "format":"uuid"});
    let page = json!({"offset":{"type":"integer","minimum":0,"maximum":100000},"limit":{"type":"integer","minimum":1,"maximum":100}});
    let named = json!({"name":{"type":"string","minLength":1,"maxLength":128}});
    let id_only = json!({"id":uuid.clone()});
    let exercise = json!({
        "name":{"type":"string","minLength":1,"maxLength":128},
        "contraction_type":{"type":"string","enum":["dynamic","isometric"]},
        "muscles":{"type":"array","maxItems":100,"uniqueItems":true,"items":{"type":"object","additionalProperties":false,"required":["muscle_id","role"],"properties":{"muscle_id":uuid.clone(),"role":{"type":"string","enum":["primary","secondary"]}}}},
        "equipment_ids":{"type":"array","maxItems":100,"uniqueItems":true,"items":uuid.clone()}
    });
    let activity = json!({"oneOf":[
        {"type":"object","additionalProperties":false,"required":["type"],"properties":{"type":{"const":"strength"}}},
        {"type":"object","additionalProperties":false,"required":["type","distance_m","duration_sec"],"properties":{"type":{"const":"run"},"distance_m":{"type":"integer","minimum":1},"duration_sec":{"type":"integer","minimum":1},"elevation_gain_m":{"type":"integer","minimum":0}}}
    ]});
    let session = json!({"started_at":{"type":"string","description":"YYYY-MM-DD or an RFC 3339 timestamp."},"label":{"type":["string","null"],"maxLength":256},"activity":activity});
    let set = json!({"position":{"type":"integer","minimum":0,"maximum":99},"set_type":{"type":"string","enum":["warmup","working","amrap","drop"]},"reps":{"type":["integer","null"],"minimum":1},"hold_sec":{"type":["integer","null"],"minimum":1},"load_g":{"type":"integer"}});

    let reference = json!({"type":"string","minLength":1,"maxLength":128,"description":"A UUID, or a catalogue name (case-insensitive, a unique prefix is enough)."});
    let stamp = json!({"type":"string","description":"YYYY-MM-DD or an RFC 3339 timestamp."});
    let range = json!({"from":stamp.clone(),"to":stamp.clone()});

    let (properties, required): (Value, Vec<&str>) = match name {
        "list_muscles" | "list_equipment" | "list_exercises" => (
            merge(json!({"query":{"type":"string","maxLength":128}}), page),
            vec![],
        ),
        "get_muscle" | "get_equipment" | "get_exercise" => {
            (json!({"id":reference.clone()}), vec!["id"])
        }
        "log_workout" => (
            json!({
                "started_at":stamp.clone(),
                "label":{"type":["string","null"],"maxLength":256},
                "exercises":{"type":"array","minItems":1,"maxItems":100,"items":{"type":"object","additionalProperties":false,"required":["exercise"],"properties":{
                    "exercise":reference.clone(),
                    "sets":{"type":"array","maxItems":100,"items":{"type":"object","additionalProperties":false,"required":["set_type"],"properties":set.clone()}}
                }}}
            }),
            vec!["started_at", "exercises"],
        ),
        "repeat_last_workout" => (
            json!({"started_at":stamp.clone(),"label":{"type":["string","null"],"maxLength":256},"like_label":{"type":["string","null"],"maxLength":256}}),
            vec!["started_at"],
        ),
        "workout_history" => (
            merge(
                range.clone(),
                json!({"limit":{"type":"integer","minimum":1,"maximum":365}}),
            ),
            vec![],
        ),
        "exercise_history" => (
            merge(
                json!({"exercise":reference.clone()}),
                merge(
                    range.clone(),
                    json!({"limit":{"type":"integer","minimum":1,"maximum":365}}),
                ),
            ),
            vec!["exercise"],
        ),
        "personal_records" => (
            merge(json!({"exercise":reference.clone()}), range.clone()),
            vec![],
        ),
        "volume_stats" => (
            merge(
                json!({"group_by":{"type":"string","enum":["exercise","muscle"]}}),
                range,
            ),
            vec!["group_by"],
        ),
        "delete_muscle"
        | "delete_equipment"
        | "delete_exercise"
        | "get_workout_session"
        | "delete_workout_session"
        | "get_session_exercise"
        | "remove_session_exercise"
        | "get_exercise_set"
        | "remove_exercise_set" => (id_only, vec!["id"]),
        "create_muscle" | "create_equipment" => (named, vec!["name"]),
        "update_muscle" | "update_equipment" => {
            (merge(json!({"id":uuid.clone()}), named), vec!["id", "name"])
        }
        "create_exercise" => (exercise, vec!["name", "contraction_type"]),
        "update_exercise" => (
            merge(json!({"id":uuid.clone()}), exercise),
            vec!["id", "name", "contraction_type"],
        ),
        "list_workout_sessions" => (
            merge(
                json!({"started_at_from":stamp.clone(),"started_at_to":stamp,"activity":{"type":"string","enum":["strength","run"]}}),
                page,
            ),
            vec![],
        ),
        "create_workout_session" => (session, vec!["started_at", "activity"]),
        "update_workout_session" => (
            merge(json!({"id":uuid.clone()}), session),
            vec!["id", "started_at", "activity"],
        ),
        "list_session_exercises" => (
            merge(json!({"session_id":uuid.clone()}), page),
            vec!["session_id"],
        ),
        "add_session_exercise" => (
            json!({"session_id":uuid.clone(),"exercise":reference.clone(),"position":{"type":"integer","minimum":0,"maximum":99}}),
            vec!["session_id", "exercise"],
        ),
        "update_session_exercise" => (
            json!({"id":uuid.clone(),"exercise":reference,"position":{"type":"integer","minimum":0,"maximum":99}}),
            vec!["id", "exercise", "position"],
        ),
        "list_exercise_sets" => (
            merge(json!({"session_exercise_id":uuid.clone()}), page),
            vec!["session_exercise_id"],
        ),
        "add_exercise_set" => (
            merge(json!({"session_exercise_id":uuid.clone()}), set),
            vec!["session_exercise_id", "set_type"],
        ),
        "update_exercise_set" => (
            merge(json!({"id":uuid}), set),
            vec!["id", "position", "set_type"],
        ),
        _ => (json!({}), vec![]),
    };
    let mut schema = JsonObject::new();
    schema.insert("type".into(), json!("object"));
    schema.insert("additionalProperties".into(), json!(false));
    schema.insert("properties".into(), properties);
    if !required.is_empty() {
        schema.insert("required".into(), json!(required));
    }
    schema
}

pub(super) fn example_for_tool(name: &str) -> Value {
    let id = "0199a1f0-0000-7000-8000-000000000000";
    match name {
        "list_muscles" | "list_equipment" | "list_exercises" => json!({"query":"squat","limit":20}),
        "get_muscle" | "get_equipment" | "get_exercise" => json!({"id":"Back squat"}),
        "create_muscle" | "create_equipment" => json!({"name":"Quadriceps"}),
        "update_muscle" | "update_equipment" => json!({"id":id,"name":"Quadriceps"}),
        "delete_muscle"
        | "delete_equipment"
        | "delete_exercise"
        | "get_workout_session"
        | "delete_workout_session"
        | "get_session_exercise"
        | "remove_session_exercise"
        | "get_exercise_set"
        | "remove_exercise_set" => json!({"id":id}),
        "create_exercise" => json!({
            "name":"Back squat","contraction_type":"dynamic",
            "muscles":[{"muscle_id":id,"role":"primary"}],"equipment_ids":[id]
        }),
        "update_exercise" => json!({
            "id":id,"name":"Back squat","contraction_type":"dynamic","muscles":[],"equipment_ids":[]
        }),
        "list_workout_sessions" => {
            json!({"started_at_from":"2026-08-01","started_at_to":"2026-08-16","activity":"strength"})
        }
        "create_workout_session" => {
            json!({"started_at":"2026-08-16","label":"Leg day","activity":{"type":"strength"}})
        }
        "update_workout_session" => {
            json!({"id":id,"started_at":"2026-08-16","activity":{"type":"run","distance_m":5000,"duration_sec":1800}})
        }
        "list_session_exercises" => json!({"session_id":id,"limit":20}),
        "add_session_exercise" => json!({"session_id":id,"exercise":"Back squat"}),
        "update_session_exercise" => json!({"id":id,"exercise":"Back squat","position":0}),
        "list_exercise_sets" => json!({"session_exercise_id":id,"limit":20}),
        "add_exercise_set" => {
            json!({"session_exercise_id":id,"set_type":"working","reps":5,"load_g":60000})
        }
        "update_exercise_set" => {
            json!({"id":id,"position":0,"set_type":"working","reps":5,"load_g":60000})
        }
        "log_workout" => json!({
            "started_at":"2026-08-16","label":"Leg day",
            "exercises":[{"exercise":"Back squat","sets":[
                {"set_type":"warmup","reps":8,"load_g":40000},
                {"set_type":"working","reps":5,"load_g":60000}
            ]}]
        }),
        "repeat_last_workout" => json!({"started_at":"2026-08-16","like_label":"Leg"}),
        "workout_history" => json!({"from":"2026-07-01","to":"2026-08-16","limit":30}),
        "exercise_history" => json!({"exercise":"Back squat","from":"2026-07-01","limit":10}),
        "personal_records" => json!({"exercise":"Back squat"}),
        "volume_stats" => json!({"group_by":"muscle","from":"2026-07-01","to":"2026-08-16"}),
        _ => json!({}),
    }
}

fn merge(left: Value, right: Value) -> Value {
    let mut left = left.as_object().cloned().unwrap_or_default();
    left.extend(right.as_object().cloned().unwrap_or_default());
    Value::Object(left)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_surface_is_complete_and_schemas_reject_unknown_fields() {
        assert_eq!(TOOL_SPECS.len(), 36);
        // A scope the OAuth layer would reject can never be granted, so a tool
        // carrying one would be unreachable rather than merely misconfigured.
        for (name, _, scope) in TOOL_SPECS {
            assert!(
                crate::domain::resource_scopes().contains(scope),
                "tool {name} needs {scope}, which is not a resource scope"
            );
        }
        let names = TOOL_SPECS
            .iter()
            .map(|(name, _, _)| *name)
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(names.len(), 36);
        for (name, _, _) in TOOL_SPECS {
            let schema = schema_for_tool(name);
            assert_eq!(schema.get("type"), Some(&json!("object")));
            assert_eq!(schema.get("additionalProperties"), Some(&json!(false)));
        }
    }
}
