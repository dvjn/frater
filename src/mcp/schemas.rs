use rmcp::model::JsonObject;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::domain::{
    AddExerciseSet, BodyweightFilter, MAX_BODYWEIGHT_G, MAX_RUN_SPLITS, PageRequest, SessionFilter,
    StatsRange, Timestamp,
};

/// A batch resolver answers one entry per name, so the list is bounded by the
/// same 100 items that every other array argument allows.
pub(super) const MAX_BATCH_REFERENCES: usize = 100;

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
pub(super) struct NamesArg {
    pub(super) names: Vec<String>,
}
/// The API-layer shape of an exercise: three flat reference lists, each entry a
/// UUID or a catalogue name. The database keeps `(exercise_id, muscle_id, role)`.
///
/// `bodyweight_share` is optional here so `create_exercise` can default it to 0.
/// `update_exercise` replaces the whole exercise, and a missing share would
/// rewrite every past volume and record, so that tool rejects the omission.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ExerciseArg {
    pub(super) name: String,
    pub(super) contraction_type: String,
    #[serde(default)]
    pub(super) bodyweight_share: Option<i64>,
    #[serde(default)]
    pub(super) primary_muscles: Vec<String>,
    #[serde(default)]
    pub(super) secondary_muscles: Vec<String>,
    #[serde(default)]
    pub(super) equipment: Vec<String>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateExerciseArg {
    pub(super) id: Uuid,
    #[serde(flatten)]
    pub(super) input: ExerciseArg,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ListWorkoutsArgs {
    started_at_from: Option<Timestamp>,
    started_at_to: Option<Timestamp>,
    activity: Option<String>,
    offset: Option<u64>,
    limit: Option<u64>,
}
impl ListWorkoutsArgs {
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
pub(super) struct LogWorkoutArg {
    pub(super) started_at: Timestamp,
    pub(super) label: Option<String>,
    #[serde(default)]
    pub(super) notes: Option<String>,
    pub(super) exercises: Vec<LogWorkoutExerciseArg>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReplaceWorkoutArg {
    pub(super) id: Uuid,
    pub(super) started_at: Timestamp,
    pub(super) label: Option<String>,
    #[serde(default)]
    pub(super) notes: Option<String>,
    #[serde(default)]
    pub(super) exercises: Option<Vec<LogWorkoutExerciseArg>>,
    /// `get_workout_session` nests the exercises inside `activity`, so the
    /// output it returns is accepted here without any reshaping.
    #[serde(default)]
    pub(super) activity: Option<ReplaceActivityArg>,
}
#[derive(Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum ReplaceActivityArg {
    Strength {
        exercises: Vec<LogWorkoutExerciseArg>,
    },
    Run {
        #[serde(default)]
        distance_m: Option<i64>,
        #[serde(default)]
        duration_sec: Option<i64>,
        #[serde(default)]
        elevation_gain_m: i64,
        #[serde(default)]
        splits: Vec<RunSplitArg>,
    },
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RunSplitArg {
    pub(super) distance_m: i64,
    pub(super) duration_sec: i64,
}
/// The identity fields `id`, `session_id` and `contraction_type` carry no
/// instruction: they exist so that the output of `get_workout_session` can be
/// sent back unchanged. `exercise_name` is not one of them; see `reference`.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LogWorkoutExerciseArg {
    #[serde(default)]
    pub(super) exercise: Option<String>,
    #[serde(default)]
    pub(super) exercise_id: Option<String>,
    #[serde(default)]
    pub(super) exercise_name: Option<String>,
    #[serde(default)]
    pub(super) position: Option<u64>,
    #[serde(default)]
    pub(super) notes: Option<String>,
    #[serde(default)]
    pub(super) sets: Vec<WorkoutSetArg>,
    #[serde(default, rename = "id")]
    _id: Option<Uuid>,
    #[serde(default, rename = "session_id")]
    _session_id: Option<Uuid>,
    #[serde(default, rename = "contraction_type")]
    _contraction_type: Option<String>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WorkoutSetArg {
    pub(super) position: Option<u64>,
    pub(super) set_type: String,
    pub(super) reps: Option<i64>,
    pub(super) hold_sec: Option<i64>,
    #[serde(default)]
    pub(super) load_g: i64,
    #[serde(default)]
    pub(super) notes: Option<String>,
    #[serde(default, rename = "id")]
    _id: Option<Uuid>,
    #[serde(default, rename = "session_exercise_id")]
    _session_exercise_id: Option<Uuid>,
}

pub(super) const EXERCISE_REFERENCE_CONFLICT: &str = "send either 'exercise', or the 'exercise_id' and 'exercise_name' that get_workout_session returns, but not both";
pub(super) const EXERCISE_REFERENCE_MISSING: &str = "each exercise entry needs 'exercise' (an id or a name), or the 'exercise_id' that get_workout_session returns";

impl LogWorkoutExerciseArg {
    /// The read shape names the exercise twice, as `exercise_id` and
    /// `exercise_name`; the id is resolved, and `expected_name` makes the
    /// caller confirm that the name agrees with it.
    pub(super) fn reference(&self) -> Result<&str, &'static str> {
        let read_shape = self
            .exercise_id
            .as_deref()
            .or(self.exercise_name.as_deref());
        match (self.exercise.as_deref(), read_shape) {
            (Some(_), Some(_)) => Err(EXERCISE_REFERENCE_CONFLICT),
            (Some(reference), None) | (None, Some(reference)) => Ok(reference),
            (None, None) => Err(EXERCISE_REFERENCE_MISSING),
        }
    }

    /// The name that `reference` did not resolve by, present only when both
    /// read-shape fields are sent. An agent that edits `exercise_name` alone
    /// means to change the movement, so a name that disagrees with the id must
    /// fail rather than lose the edit.
    pub(super) fn expected_name(&self) -> Option<&str> {
        match (&self.exercise_id, self.exercise_name.as_deref()) {
            (Some(_), Some(name)) => Some(name),
            _ => None,
        }
    }
}

impl WorkoutSetArg {
    pub(super) fn input(&self) -> AddExerciseSet {
        AddExerciseSet {
            position: self.position,
            set_type: self.set_type.clone(),
            reps: self.reps,
            hold_sec: self.hold_sec,
            load_g: self.load_g,
            notes: self.notes.clone(),
        }
    }
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
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ListBodyweightArgs {
    from: Option<Timestamp>,
    to: Option<Timestamp>,
    offset: Option<u64>,
    limit: Option<u64>,
}
impl ListBodyweightArgs {
    pub(super) fn filter(&self) -> BodyweightFilter {
        BodyweightFilter {
            from: self.from,
            to: self.to,
        }
    }
    pub(super) fn page(&self) -> PageRequest {
        PageRequest {
            offset: self.offset.unwrap_or(0),
            limit: self.limit,
        }
    }
}

/// Name, description, and the one scope the tool needs. The scope is data, not
/// a rule inferred from the name, so adding a tool cannot silently give it the
/// wrong one. `dispatch_tool` enforces it, and `list_tools` decides visibility
/// from it: discovery, never enforcement.
pub(super) const TOOL_SPECS: &[(&str, &str, &str)] = &[
    (
        "list_muscles",
        "Search the muscle catalogue by name. Input: optional query, offset, limit. Returns {items:[{id,name}], next_offset}.",
        "catalogue:read",
    ),
    (
        "get_muscle",
        "Look up one muscle by id or name; names are case-insensitive and may be a unique prefix. Returns {id,name}.",
        "catalogue:read",
    ),
    (
        "create_muscle",
        "Add a muscle to the shared catalogue. Input: name. Returns {id,name}.",
        "catalogue:write",
    ),
    (
        "update_muscle",
        "Rename a muscle. Input: id, name. Returns {id,name}.",
        "catalogue:write",
    ),
    (
        "delete_muscle",
        "Delete a muscle that no exercise uses. Input: id. Returns {deleted,id}.",
        "catalogue:write",
    ),
    (
        "list_equipment",
        "Search the equipment catalogue by name. Input: optional query, offset, limit. Returns {items:[{id,name}], next_offset}.",
        "catalogue:read",
    ),
    (
        "get_equipment",
        "Look up one equipment item by id or name; names are case-insensitive and may be a unique prefix. Returns {id,name}.",
        "catalogue:read",
    ),
    (
        "create_equipment",
        "Add equipment to the shared catalogue. Input: name. Returns {id,name}.",
        "catalogue:write",
    ),
    (
        "update_equipment",
        "Rename an equipment item. Input: id, name. Returns {id,name}.",
        "catalogue:write",
    ),
    (
        "delete_equipment",
        "Delete equipment that no exercise uses. Input: id. Returns {deleted,id}.",
        "catalogue:write",
    ),
    (
        "list_exercises",
        "Find exercises by name before logging a workout. Input: optional query, offset, limit. Returns {items:[{id,name,contraction_type}], next_offset}.",
        "catalogue:read",
    ),
    (
        "get_exercise",
        "Get one exercise with its muscles and equipment. Input: id or name. Returns {id,name,contraction_type,primary_muscles,secondary_muscles,equipment}.",
        "catalogue:read",
    ),
    (
        "resolve_exercises",
        "Resolve many exercise references in one read-only call before logging a workout. Input: names, each a UUID or a name. Returns {results:[{query,status,...}]} in input order, one entry per name: status 'found' with match {id,name,contraction_type}, 'ambiguous' with candidates, 'missing', or 'invalid' with a message when the name itself is unusable (blank or too long). One unresolvable name never fails the call.",
        "catalogue:read",
    ),
    (
        "create_exercise",
        "Add an exercise and its muscle and equipment links in one atomic call. Input: name, contraction_type, optional bodyweight_share, optional primary_muscles, secondary_muscles, equipment. Each list entry is a UUID or a catalogue name. bodyweight_share is the percentage of bodyweight the movement moves, 0 when the load is entirely external.",
        "catalogue:write",
    ),
    (
        "update_exercise",
        "Replace an exercise and all of its links in one atomic call. Input: id, name, contraction_type, bodyweight_share, optional primary_muscles, secondary_muscles, equipment. Each list entry is a UUID or a catalogue name, and lists you leave out become empty. bodyweight_share is required here, because a new share changes the volume and the records already reported for past workouts.",
        "catalogue:write",
    ),
    (
        "delete_exercise",
        "Delete an exercise that no session uses. Input: id. Returns {deleted,id}.",
        "catalogue:write",
    ),
    (
        "list_workouts",
        "List and summarise your workouts, newest first. Input: optional started_at_from and started_at_to (YYYY-MM-DD or RFC 3339), optional activity ('strength' or 'run'), offset, limit. Returns {items:[{id,started_at,label,notes,activity_type,exercise_count,set_count,volume_g}], next_offset}. volume_g counts the bodyweight an exercise carries, plus any added load and minus any assistance, using the user's latest bodyweight reading on or before that date, or the earliest later one when none is earlier.",
        "workouts:read",
    ),
    (
        "get_workout_session",
        "Read one of your sessions in full, with every exercise and set. Input: id. Returns the session with nested exercises and sets.",
        "workouts:read",
    ),
    (
        "create_workout_session",
        "Create an empty strength session or a run. Input: started_at (YYYY-MM-DD or RFC 3339), optional label, activity. A run is its splits[{distance_m,duration_sec}], its laps in order: they are what is stored, and the distance and duration of the run are their sums. For a run you have no laps for, send just distance_m and duration_sec instead and it is recorded as a single split. Send both and the totals must equal the sums, so cover any laps you do not know as one remainder split. Every run needs one or the other. To record a complete strength workout in one call use log_workout instead.",
        "workouts:write",
    ),
    (
        "delete_workout_session",
        "Delete one of your sessions with all of its exercises and sets. Input: id. Returns {deleted,id}.",
        "workouts:write",
    ),
    (
        "log_workout",
        "Record a finished strength workout in one atomic call: session, exercises, and sets together. Input: started_at (YYYY-MM-DD or RFC 3339), optional label, optional notes, exercises[{exercise (id or name), optional notes, sets[]}]. Array order sets the order: an explicit position that disagrees with the index of its array element is rejected, so reorder the array instead of editing a position. Each exercise entry may name its exercise with 'exercise', or with the 'exercise_id' and 'exercise_name' that get_workout_session returns, but not with both; when you send exercise_id and exercise_name together they must name the same exercise, so to change the movement send only the one field you changed. The remaining identity fields (id, session_id, session_exercise_id, contraction_type) are accepted and ignored. Nothing is stored if any part is invalid.",
        "workouts:write",
    ),
    (
        "replace_workout",
        "Correct a logged workout by submitting the whole thing again, in one atomic call. Read it with get_workout_session, change what is wrong, and send the complete session back: the output of that tool is accepted unchanged. The session keeps its id; every exercise and set id is reissued. Input: id, started_at, optional label and notes, then either activity (the nested {type:'strength', exercises[]} or {type:'run', optional elevation_gain_m, splits[{distance_m,duration_sec}]} that get_workout_session returns; a run is its splits, its laps in order, and its distance and duration are their sums, so for a run you have no laps for send just distance_m and duration_sec instead and it is recorded as a single split, and when you send both they must agree, with any laps you do not know folded into one remainder split) or a top-level exercises[{exercise (id or name), optional notes, sets[]}] as log_workout takes it — send one or the other, never both. Each exercise entry may name its exercise with 'exercise', or with the 'exercise_id' and 'exercise_name' that get_workout_session returns, but not with both; when you send exercise_id and exercise_name together they must name the same exercise, so to change the movement send only the one field you changed. The remaining identity fields (id, session_id, session_exercise_id, contraction_type) are accepted and ignored. Array order sets the order: an explicit position that disagrees with the index of its array element is rejected, so reorder the array instead of editing a position. Exercises, sets, and run splits you leave out are removed. A strength payload against a run session, or a run payload against a strength session, changes the activity type of the session and discards the previous activity detail. Nothing changes if any part is invalid.",
        "workouts:write",
    ),
    (
        "exercise_history",
        "Track how one exercise progressed over time. Input: exercise (id or name), optional from and to, optional limit. Returns items[], each performance newest first, with its sets and estimated 1RM in grams. Each set reports load_g as you logged it (negative is assistance) and effective_load_g, which adds the bodyweight the exercise carries and is what volume_g and the estimated 1RM use, so rising effective_load_g is progress even while load_g stays negative.",
        "workouts:read",
    ),
    (
        "personal_records",
        "Report best efforts: heaviest set, best estimated one-repetition maximum (Epley), and longest hold. Input: optional exercise (id or name), optional from and to. Returns items[]. Loads in grams, and they include the bodyweight an exercise carries, so a pull-up records the weight it moved.",
        "workouts:read",
    ),
    (
        "log_bodyweight",
        "Record the user's bodyweight for one date, so that bodyweight exercises carry a load. Input: recorded_on (YYYY-MM-DD; a timestamp is rejected, because it names a different day outside UTC), mass_g in grams. One reading per date: sending the same date again corrects that reading instead of adding another. Returns {id,recorded_on,mass_g}.",
        "workouts:write",
    ),
    (
        "list_bodyweight",
        "List the user's bodyweight readings, newest first. Input: optional from and to (YYYY-MM-DD or RFC 3339), offset, limit. Returns {items:[{id,recorded_on,mass_g}], next_offset}.",
        "workouts:read",
    ),
    (
        "delete_bodyweight",
        "Delete one of the user's bodyweight readings. Input: id. Returns {deleted,id}. Deleting a reading changes the volume and the records already reported for the workouts that used it.",
        "workouts:write",
    ),
];

pub(super) fn schema_for_tool(name: &str) -> JsonObject {
    let uuid = json!({"type":"string", "format":"uuid"});
    let page = json!({"offset":{"type":"integer","minimum":0,"maximum":100000},"limit":{"type":"integer","minimum":1,"maximum":100}});
    let named = json!({"name":{"type":"string","minLength":1,"maxLength":128}});
    let id_only = json!({"id":uuid.clone()});
    let reference = json!({"type":"string","minLength":1,"maxLength":128,"description":"A UUID, or a catalogue name (case-insensitive, a unique prefix is enough)."});
    let reference_list = json!({"type":"array","maxItems":MAX_BATCH_REFERENCES,"uniqueItems":true,"items":reference.clone()});
    let exercise = json!({
        "name":{"type":"string","minLength":1,"maxLength":128},
        "contraction_type":{"type":"string","enum":["dynamic","isometric"]},
        "bodyweight_share":{"type":"integer","minimum":0,"maximum":100,"description":"The percentage of bodyweight the movement moves, 0 when the load is entirely external."},
        "primary_muscles":reference_list.clone(),
        "secondary_muscles":reference_list.clone(),
        "equipment":reference_list.clone()
    });
    let run_splits = json!({"type":"array","maxItems":MAX_RUN_SPLITS,"items":{"type":"object","additionalProperties":false,"required":["distance_m","duration_sec"],"properties":{"distance_m":{"type":"integer","minimum":1},"duration_sec":{"type":"integer","minimum":1}}},"description":"The laps of the run, in order, and the source of truth for its distance and duration. Send them, or send distance_m and duration_sec instead and the run is recorded as one split; send both and the totals must equal the sums. Cover laps you do not know as one remainder split."});
    let activity = json!({"oneOf":[
        {"type":"object","additionalProperties":false,"required":["type"],"properties":{"type":{"const":"strength"}}},
        {"type":"object","additionalProperties":false,"required":["type"],"properties":{"type":{"const":"run"},"distance_m":{"type":"integer","minimum":1},"duration_sec":{"type":"integer","minimum":1},"elevation_gain_m":{"type":"integer","minimum":0},"splits":run_splits.clone()}}
    ]});
    let notes = json!({"type":["string","null"],"maxLength":1000});
    let session = json!({"started_at":{"type":"string","description":"YYYY-MM-DD or an RFC 3339 timestamp."},"label":{"type":["string","null"],"maxLength":256},"notes":notes.clone(),"activity":activity});
    let set = json!({"position":{"type":"integer","minimum":0,"maximum":99},"set_type":{"type":"string","enum":["warmup","working","amrap","drop"]},"reps":{"type":["integer","null"],"minimum":1},"hold_sec":{"type":["integer","null"],"minimum":1},"load_g":{"type":"integer"},"notes":notes.clone()});

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
        "resolve_exercises" => (
            json!({"names":{"type":"array","minItems":1,"maxItems":MAX_BATCH_REFERENCES,"items":reference.clone()}}),
            vec!["names"],
        ),
        "log_workout" | "replace_workout" => {
            let workout_set = merge(
                set.clone(),
                json!({"id":uuid.clone(),"session_exercise_id":uuid.clone()}),
            );
            let entry = json!({"type":"object","additionalProperties":false,"anyOf":[
                {"required":["exercise"]},
                {"required":["exercise_id"]},
                {"required":["exercise_name"]}
            ],"properties":{
                "exercise":reference.clone(),
                "exercise_id":uuid.clone(),
                "exercise_name":{"type":"string","minLength":1,"maxLength":128},
                "id":uuid.clone(),
                "session_id":uuid.clone(),
                "contraction_type":{"type":"string","enum":["dynamic","isometric"]},
                "position":{"type":"integer","minimum":0,"maximum":99},
                "notes":notes.clone(),
                "sets":{"type":"array","maxItems":100,"items":{"type":"object","additionalProperties":false,"required":["set_type"],"properties":workout_set}}
            }});
            // A session with no exercises is readable, so replace_workout must
            // accept the empty list that get_workout_session returns for it.
            let exercises = if name == "replace_workout" {
                json!({"type":"array","maxItems":100,"items":entry.clone()})
            } else {
                json!({"type":"array","minItems":1,"maxItems":100,"items":entry.clone()})
            };
            let workout = json!({
                "started_at":stamp.clone(),
                "label":{"type":["string","null"],"maxLength":256},
                "notes":notes.clone(),
                "exercises":exercises.clone()
            });
            if name == "replace_workout" {
                let activity = json!({"oneOf":[
                    {"type":"object","additionalProperties":false,"required":["type","exercises"],"properties":{"type":{"const":"strength"},"exercises":exercises}},
                    {"type":"object","additionalProperties":false,"required":["type"],"properties":{"type":{"const":"run"},"distance_m":{"type":"integer","minimum":1},"duration_sec":{"type":"integer","minimum":1},"elevation_gain_m":{"type":"integer","minimum":0},"splits":run_splits.clone()}}
                ]});
                (
                    merge(
                        id_only.clone(),
                        merge(workout, json!({"activity":activity})),
                    ),
                    vec!["id", "started_at"],
                )
            } else {
                (workout, vec!["started_at", "exercises"])
            }
        }
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
        "personal_records" => (merge(json!({"exercise":reference.clone()}), range), vec![]),
        "delete_muscle"
        | "delete_equipment"
        | "delete_exercise"
        | "get_workout_session"
        | "delete_workout_session"
        | "delete_bodyweight" => (id_only, vec!["id"]),
        "log_bodyweight" => (
            json!({"recorded_on":{"type":"string","format":"date","description":"YYYY-MM-DD. A timestamp is rejected, because it would name a different day outside UTC."},"mass_g":{"type":"integer","minimum":1,"maximum":MAX_BODYWEIGHT_G}}),
            vec!["recorded_on", "mass_g"],
        ),
        "list_bodyweight" => (merge(range.clone(), page), vec![]),
        "create_muscle" | "create_equipment" => (named, vec!["name"]),
        "update_muscle" | "update_equipment" => {
            (merge(json!({"id":uuid.clone()}), named), vec!["id", "name"])
        }
        "create_exercise" => (exercise, vec!["name", "contraction_type"]),
        "update_exercise" => (
            merge(json!({"id":uuid.clone()}), exercise),
            vec!["id", "name", "contraction_type", "bodyweight_share"],
        ),
        "list_workouts" => (
            merge(
                json!({"started_at_from":stamp.clone(),"started_at_to":stamp,"activity":{"type":"string","enum":["strength","run"]}}),
                page,
            ),
            vec![],
        ),
        "create_workout_session" => (session, vec!["started_at", "activity"]),
        _ => (json!({}), vec![]),
    };
    let mut schema = JsonObject::new();
    schema.insert("type".into(), json!("object"));
    schema.insert("additionalProperties".into(), json!(false));
    schema.insert("properties".into(), properties);
    if !required.is_empty() {
        schema.insert("required".into(), json!(required));
    }
    // Two subschemas that name only a required property make `oneOf` mean
    // "exactly one of them": sending both matches both, which fails oneOf.
    if name == "replace_workout" {
        schema.insert(
            "oneOf".into(),
            json!([{"required":["exercises"]},{"required":["activity"]}]),
        );
    }
    schema
}

/// Every shape a tool result can hold, named once so that the 27 output
/// schemas share them instead of restating them.
fn output_definitions() -> serde_json::Map<String, Value> {
    let uuid = json!({"type":"string","format":"uuid"});
    let stamp = json!({"type":"string","format":"date-time"});
    let nullable_string = json!({"type":["string","null"]});
    let count = json!({"type":"integer","minimum":0});
    let nullable_integer = json!({"type":["integer","null"]});
    let nullable_stamp = json!({"type":["string","null"],"format":"date-time"});
    let entity_properties =
        json!({"id":uuid.clone(),"name":{"type":"string"},"contraction_type":{"type":"string"}});

    let named_entity = json!({
        "type":"object","additionalProperties":false,"required":["id","name"],
        "properties":{"id":uuid.clone(),"name":{"type":"string"}}
    });
    let exercise_summary = json!({
        "type":"object","additionalProperties":false,
        "required":["id","name","contraction_type"],
        "properties":entity_properties
    });
    let named_list = json!({"type":"array","items":{"$ref":"#/$defs/named_entity"}});
    let exercise = json!({
        "type":"object","additionalProperties":false,
        "required":["id","name","contraction_type","bodyweight_share","primary_muscles","secondary_muscles","equipment"],
        "properties":{
            "id":uuid.clone(),"name":{"type":"string"},
            "contraction_type":{"type":"string"},
            "bodyweight_share":{"type":"integer","minimum":0,"maximum":100},
            "primary_muscles":named_list.clone(),
            "secondary_muscles":named_list.clone(),
            "equipment":named_list
        }
    });
    let exercise_set = json!({
        "type":"object","additionalProperties":false,
        "required":["id","session_exercise_id","position","set_type","reps","hold_sec","load_g","notes"],
        "properties":{
            "id":uuid.clone(),"session_exercise_id":uuid.clone(),
            "position":count.clone(),"set_type":{"type":"string"},
            "reps":nullable_integer.clone(),"hold_sec":nullable_integer.clone(),
            "load_g":{"type":"integer"},"notes":nullable_string.clone()
        }
    });
    let session_exercise_properties = json!({
        "id":uuid.clone(),"session_id":uuid.clone(),"exercise_id":uuid.clone(),
        "exercise_name":{"type":"string"},"contraction_type":{"type":"string"},
        "position":count.clone(),"notes":nullable_string.clone()
    });
    let session_exercise = json!({
        "type":"object","additionalProperties":false,
        "required":["id","session_id","exercise_id","exercise_name","contraction_type","position","notes","sets"],
        "properties":merge(
            session_exercise_properties,
            json!({"sets":{"type":"array","items":{"$ref":"#/$defs/exercise_set"}}})
        )
    });
    let activity = json!({"oneOf":[
        {"type":"object","additionalProperties":false,"required":["type","exercises"],
         "properties":{"type":{"const":"strength"},
                       "exercises":{"type":"array","items":{"$ref":"#/$defs/session_exercise"}}}},
        {"type":"object","additionalProperties":false,
         "required":["type","distance_m","duration_sec","elevation_gain_m","splits"],
         "properties":{"type":{"const":"run"},"distance_m":{"type":"integer"},
                       "duration_sec":{"type":"integer"},"elevation_gain_m":{"type":"integer"},
                       "splits":{"type":"array","items":{"type":"object","additionalProperties":false,
                                 "required":["distance_m","duration_sec"],
                                 "properties":{"distance_m":{"type":"integer"},"duration_sec":{"type":"integer"}}}}}}
    ]});
    let workout_session = json!({
        "type":"object","additionalProperties":false,
        "required":["id","started_at","label","notes","activity"],
        "properties":{
            "id":uuid.clone(),"started_at":stamp.clone(),
            "label":nullable_string.clone(),"notes":nullable_string.clone(),
            "activity":activity
        }
    });
    let workout_list_entry = json!({
        "type":"object","additionalProperties":false,
        "required":["id","started_at","label","notes","activity_type","exercise_count","set_count","volume_g"],
        "properties":{
            "id":uuid.clone(),"started_at":stamp.clone(),
            "label":nullable_string.clone(),"notes":nullable_string.clone(),
            "activity_type":{"type":"string"},
            "exercise_count":count.clone(),"set_count":count.clone(),
            "volume_g":{"type":"integer"}
        }
    });
    let exercise_history_set = json!({
        "type":"object","additionalProperties":false,
        "required":["position","set_type","reps","hold_sec","load_g","effective_load_g","estimated_1rm_g"],
        "properties":{
            "position":count.clone(),"set_type":{"type":"string"},
            "reps":nullable_integer.clone(),"hold_sec":nullable_integer.clone(),
            "load_g":{"type":"integer"},"effective_load_g":{"type":"integer","minimum":0},
            "estimated_1rm_g":nullable_integer.clone()
        }
    });
    let exercise_history_entry = json!({
        "type":"object","additionalProperties":false,
        "required":["session_id","session_exercise_id","started_at","label","volume_g","sets"],
        "properties":{
            "session_id":uuid.clone(),"session_exercise_id":uuid.clone(),
            "started_at":stamp.clone(),"label":nullable_string,
            "volume_g":{"type":"integer"},
            "sets":{"type":"array","items":{"$ref":"#/$defs/exercise_history_set"}}
        }
    });
    let personal_record = json!({
        "type":"object","additionalProperties":false,
        "required":["exercise_id","exercise_name","max_load_g","max_load_reps","best_estimated_1rm_g","best_estimated_1rm_load_g","best_estimated_1rm_reps","longest_hold_sec","set_count","last_performed_at","max_load_at","longest_hold_at"],
        "properties":{
            "exercise_id":uuid.clone(),"exercise_name":{"type":"string"},
            "max_load_g":nullable_integer.clone(),"max_load_reps":nullable_integer.clone(),
            "best_estimated_1rm_g":nullable_integer.clone(),
            "best_estimated_1rm_load_g":nullable_integer.clone(),
            "best_estimated_1rm_reps":nullable_integer.clone(),
            "longest_hold_sec":nullable_integer,
            "set_count":count,"last_performed_at":stamp,
            "max_load_at":nullable_stamp.clone(),"longest_hold_at":nullable_stamp
        }
    });
    let bodyweight_reading = json!({
        "type":"object","additionalProperties":false,
        "required":["id","recorded_on","mass_g"],
        "properties":{
            "id":uuid.clone(),
            "recorded_on":{"type":"string","format":"date"},
            "mass_g":{"type":"integer","minimum":1,"maximum":MAX_BODYWEIGHT_G}
        }
    });
    let deleted = json!({
        "type":"object","additionalProperties":false,"required":["deleted","id"],
        "properties":{"deleted":{"const":true},"id":uuid.clone()}
    });
    let resolution = |describe: &str| {
        json!({
            "type":"object","additionalProperties":false,"required":["query","status"],
            "properties":{
                "query":{"type":"string"},
                "status":{"type":"string","enum":["found","missing","ambiguous","invalid"]},
                "match":{"$ref":format!("#/$defs/{describe}")},
                "candidates":{"type":"array","items":{"$ref":format!("#/$defs/{describe}")}},
                "message":{"type":"string"}
            }
        })
    };
    // Shared by every tool. `error` and `message` are always both present, and
    // no success shape carries either, so the `oneOf` of an output schema is
    // discriminated by them in one direction and by `additionalProperties`
    // false in the other.
    let error = json!({
        "type":"object","additionalProperties":false,"required":["error","message"],
        "properties":{
            "error":{"type":"string","enum":[
                "ambiguous","conflict","forbidden","internal",
                "invalid_input","invalid_params","not_found","unavailable"
            ]},
            "message":{"type":"string"},
            "candidates":{"type":"array","items":{"$ref":"#/$defs/named_entity"}},
            "missing":{"type":"array","items":{"type":"string"}},
            "ambiguous":{"type":"array","items":{
                "type":"object","additionalProperties":false,"required":["query","candidates"],
                "properties":{
                    "query":{"type":"string"},
                    "candidates":{"type":"array","items":{"$ref":"#/$defs/named_entity"}}
                }
            }},
            "valid_example":{
                "type":"object","additionalProperties":false,"required":["name","arguments"],
                "properties":{"name":{"type":"string"},"arguments":{"type":"object"}}
            }
        }
    });

    let mut definitions = serde_json::Map::new();
    for (name, definition) in [
        ("named_entity", named_entity),
        ("exercise_summary", exercise_summary),
        ("exercise", exercise),
        ("exercise_set", exercise_set),
        ("session_exercise", session_exercise),
        ("workout_session", workout_session),
        ("workout_list_entry", workout_list_entry),
        ("exercise_history_set", exercise_history_set),
        ("exercise_history_entry", exercise_history_entry),
        ("personal_record", personal_record),
        ("bodyweight_reading", bodyweight_reading),
        ("deleted", deleted),
        ("exercise_resolution", resolution("exercise_summary")),
        ("error", error),
    ] {
        definitions.insert(name.into(), definition);
    }
    definitions
}

/// The success half of a tool's output schema. The entity shapes are inlined
/// rather than referenced so that the branch itself carries the
/// `additionalProperties` false that discriminates it from an error.
fn success_schema_for_tool(name: &str, definitions: &serde_json::Map<String, Value>) -> Value {
    let inline = |definition: &str| definitions[definition].clone();
    let page = |definition: &str| {
        json!({
            "type":"object","additionalProperties":false,"required":["items","next_offset"],
            "properties":{
                "items":{"type":"array","items":{"$ref":format!("#/$defs/{definition}")}},
                "next_offset":{"type":["integer","null"],"minimum":0}
            }
        })
    };
    let results = |definition: &str| {
        json!({
            "type":"object","additionalProperties":false,"required":["results"],
            "properties":{"results":{"type":"array","items":{"$ref":format!("#/$defs/{definition}")}}}
        })
    };
    let sequence = |definition: &str| {
        json!({
            "type":"object","additionalProperties":false,"required":["items"],
            "properties":{"items":{"type":"array","items":{"$ref":format!("#/$defs/{definition}")}}}
        })
    };

    match name {
        "list_muscles" | "list_equipment" => page("named_entity"),
        "get_muscle" | "create_muscle" | "update_muscle" | "get_equipment" | "create_equipment"
        | "update_equipment" => inline("named_entity"),
        "list_exercises" => page("exercise_summary"),
        "get_exercise" | "create_exercise" | "update_exercise" => inline("exercise"),
        "resolve_exercises" => results("exercise_resolution"),
        "list_workouts" => page("workout_list_entry"),
        "get_workout_session" | "create_workout_session" | "log_workout" | "replace_workout" => {
            inline("workout_session")
        }
        "exercise_history" => sequence("exercise_history_entry"),
        "personal_records" => sequence("personal_record"),
        "log_bodyweight" => inline("bodyweight_reading"),
        "list_bodyweight" => page("bodyweight_reading"),
        "delete_muscle"
        | "delete_equipment"
        | "delete_exercise"
        | "delete_workout_session"
        | "delete_bodyweight" => inline("deleted"),
        _ => json!({"type":"object"}),
    }
}

/// Copies into `wanted` every definition that `schema` reaches, so that a
/// standalone per-tool document never holds a `$ref` it cannot resolve.
fn collect_definitions(
    schema: &Value,
    definitions: &serde_json::Map<String, Value>,
    wanted: &mut serde_json::Map<String, Value>,
) {
    match schema {
        Value::Object(fields) => {
            for (key, value) in fields {
                if key == "$ref" {
                    let Some(reference) = value
                        .as_str()
                        .and_then(|value| value.strip_prefix("#/$defs/").map(str::to_owned))
                    else {
                        continue;
                    };
                    if wanted.contains_key(&reference) {
                        continue;
                    }
                    let definition = definitions[&reference].clone();
                    wanted.insert(reference, definition.clone());
                    collect_definitions(&definition, definitions, wanted);
                } else {
                    collect_definitions(value, definitions, wanted);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_definitions(item, definitions, wanted);
            }
        }
        _ => {}
    }
}

pub(super) fn output_schema_for_tool(name: &str) -> JsonObject {
    let definitions = output_definitions();
    let success = success_schema_for_tool(name, &definitions);
    let error = json!({"$ref":"#/$defs/error"});
    let mut wanted = serde_json::Map::new();
    collect_definitions(&success, &definitions, &mut wanted);
    collect_definitions(&error, &definitions, &mut wanted);
    let mut schema = JsonObject::new();
    schema.insert("oneOf".into(), json!([success, error]));
    schema.insert("$defs".into(), Value::Object(wanted));
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
        | "delete_bodyweight" => json!({"id":id}),
        "log_bodyweight" => json!({"recorded_on":"2026-08-16","mass_g":72500}),
        "list_bodyweight" => json!({"from":"2026-07-01","limit":20}),
        "resolve_exercises" => json!({"names":["Back squat","Lat pulldown"]}),
        "create_exercise" => json!({
            "name":"Back squat","contraction_type":"dynamic","bodyweight_share":0,
            "primary_muscles":["Quadriceps"],"secondary_muscles":["Glutes"],
            "equipment":["Barbell"]
        }),
        "update_exercise" => json!({
            "id":id,"name":"Back squat","contraction_type":"dynamic","bodyweight_share":0,
            "primary_muscles":["Quadriceps"],"secondary_muscles":[],"equipment":[]
        }),
        "list_workouts" => {
            json!({"started_at_from":"2026-08-01","started_at_to":"2026-08-16","activity":"strength"})
        }
        "create_workout_session" => {
            json!({"started_at":"2026-08-16","label":"Morning run","activity":{
                "type":"run","elevation_gain_m":40,
                "splits":[
                    {"distance_m":1000,"duration_sec":295},
                    {"distance_m":1000,"duration_sec":301},
                    {"distance_m":6000,"duration_sec":1804}
                ]
            }})
        }
        "log_workout" => json!({
            "started_at":"2026-08-16","label":"Leg day",
            "exercises":[{"exercise":"Back squat","sets":[
                {"set_type":"warmup","reps":8,"load_g":40000},
                {"set_type":"working","reps":5,"load_g":60000}
            ]}]
        }),
        "replace_workout" => json!({
            "id":id,"started_at":"2026-08-16","label":"Morning run",
            "activity":{
                "type":"run","distance_m":8000,"duration_sec":2400,"elevation_gain_m":40,
                "splits":[{"distance_m":1000,"duration_sec":295},{"distance_m":7000,"duration_sec":2105}]
            }
        }),
        "exercise_history" => json!({"exercise":"Back squat","from":"2026-07-01","limit":10}),
        "personal_records" => json!({"exercise":"Back squat"}),
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

    /// `deny_unknown_fields` names every field it accepts in the error it
    /// raises for one it does not, so the accepted set comes from the struct
    /// itself and cannot go stale. `probe` selects the variant to ask.
    fn accepted_fields<T: serde::de::DeserializeOwned>(
        probe: Value,
    ) -> std::collections::BTreeSet<String> {
        let mut probe = probe;
        probe
            .as_object_mut()
            .unwrap()
            .insert("a field no struct accepts".into(), Value::Null);
        let error = serde_json::from_value::<T>(probe)
            .err()
            .expect("the struct must deny unknown fields")
            .to_string();
        let listed = error
            .split_once("expected")
            .unwrap_or_else(|| panic!("{error}"))
            .1;
        listed
            .split('`')
            .skip(1)
            .step_by(2)
            .map(str::to_owned)
            .collect()
    }

    fn published_fields(schema: &Value) -> std::collections::BTreeSet<String> {
        schema["properties"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect()
    }

    fn published_required(schema: &Value) -> std::collections::BTreeSet<String> {
        schema["required"]
            .as_array()
            .map(|names| {
                names
                    .iter()
                    .map(|name| name.as_str().unwrap().to_owned())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The fields of `complete` the struct refuses to do without, found by
    /// deserialising it once per field with that field removed.
    fn required_fields<T: serde::de::DeserializeOwned>(
        complete: &Value,
    ) -> std::collections::BTreeSet<String> {
        complete
            .as_object()
            .unwrap()
            .keys()
            .filter(|field| {
                let mut probe = complete.clone();
                probe.as_object_mut().unwrap().remove(field.as_str());
                serde_json::from_value::<T>(probe).is_err()
            })
            .cloned()
            .collect()
    }

    /// A schema that advertises a field the argument struct rejects, or a
    /// struct that accepts a field the schema never publishes, is what made
    /// the documented `replace_workout` round-trip impossible, so the two sets
    /// are compared in both directions.
    #[test]
    fn the_nested_workout_schemas_and_their_argument_structs_agree() {
        let id = "0199a1f0-0000-7000-8000-000000000000";
        let complete_set = json!({
            "id":id,"session_exercise_id":id,"position":0,"set_type":"working",
            "reps":5,"hold_sec":null,"load_g":60000,"notes":"a note"
        });
        let complete_entry = json!({
            "exercise":id,"exercise_id":id,"exercise_name":"Back squat",
            "id":id,"session_id":id,"contraction_type":"dynamic","position":0,
            "notes":"a note","sets":[complete_set.clone()]
        });
        for tool in ["log_workout", "replace_workout"] {
            let schema = schema_for_tool(tool);
            let entry = &schema["properties"]["exercises"]["items"];
            assert_eq!(
                published_fields(entry),
                accepted_fields::<LogWorkoutExerciseArg>(json!({})),
                "{tool} exercise entry"
            );
            assert_eq!(
                published_fields(&entry["properties"]["sets"]["items"]),
                accepted_fields::<WorkoutSetArg>(json!({})),
                "{tool} set"
            );
            // Names alone let a field be required in the schema while serde
            // quietly defaults it, which is what hid the mismatch on
            // ReplaceActivityArg::Strength.exercises.
            assert_eq!(
                published_required(entry),
                required_fields::<LogWorkoutExerciseArg>(&complete_entry),
                "{tool} exercise entry required"
            );
            assert_eq!(
                published_required(&entry["properties"]["sets"]["items"]),
                required_fields::<WorkoutSetArg>(&complete_set),
                "{tool} set required"
            );
        }

        let schema = schema_for_tool("replace_workout");
        assert_eq!(
            published_fields(&Value::Object(schema.clone())),
            accepted_fields::<ReplaceWorkoutArg>(json!({})),
        );
        assert_eq!(
            published_required(&Value::Object(schema.clone())),
            required_fields::<ReplaceWorkoutArg>(&json!({
                "id":id,"started_at":"2026-08-16","label":"Leg day","notes":"a note",
                "exercises":[complete_entry.clone()],
                "activity":{"type":"strength","exercises":[complete_entry.clone()]}
            })),
        );
        // The internally tagged enum consumes 'type' before it reports the
        // fields of the variant, so the tag is never in the accepted set.
        for (branch, probe) in [(0, json!({"type":"strength"})), (1, json!({"type":"run"}))] {
            let branch = &schema["properties"]["activity"]["oneOf"][branch];
            let mut accepted = accepted_fields::<ReplaceActivityArg>(probe.clone());
            accepted.insert("type".into());
            assert_eq!(published_fields(branch), accepted);
            let mut complete = probe;
            let complete_object = complete.as_object_mut().unwrap();
            for (field, value) in [
                ("exercises", json!([complete_entry.clone()])),
                ("distance_m", json!(8000)),
                ("duration_sec", json!(2400)),
                ("elevation_gain_m", json!(40)),
            ] {
                if published_fields(branch).contains(field) {
                    complete_object.insert(field.into(), value);
                }
            }
            assert_eq!(
                published_required(branch),
                required_fields::<ReplaceActivityArg>(&complete),
            );
        }

        let entry = json!({
            "exercise_id":id,"exercise_name":"Back squat",
            "id":id,"session_id":id,"contraction_type":"dynamic","position":0,
            "notes":"a note","sets":[complete_set]
        });
        serde_json::from_value::<LogWorkoutExerciseArg>(entry.clone()).unwrap();
        serde_json::from_value::<ReplaceWorkoutArg>(json!({
            "id":id,"started_at":"2026-08-16","label":"Leg day","notes":"a note",
            "activity":{"type":"strength","exercises":[entry]}
        }))
        .unwrap();
    }

    #[test]
    fn tool_surface_is_complete_and_schemas_reject_unknown_fields() {
        assert_eq!(TOOL_SPECS.len(), 27);
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
        assert_eq!(names.len(), 27);
        for (name, _, _) in TOOL_SPECS {
            let schema = schema_for_tool(name);
            assert_eq!(schema.get("type"), Some(&json!("object")));
            assert_eq!(schema.get("additionalProperties"), Some(&json!(false)));

            let schema = Value::Object(output_schema_for_tool(name));
            let branches = schema["oneOf"].as_array().unwrap();
            assert_eq!(branches.len(), 2, "tool {name}");
            assert_eq!(branches[1], json!({"$ref":"#/$defs/error"}), "tool {name}");
            assert_eq!(
                branches[0].get("additionalProperties"),
                Some(&json!(false)),
                "tool {name} success branch"
            );
            for reference in references(&schema) {
                assert!(
                    schema["$defs"].get(&reference).is_some(),
                    "tool {name} refers to {reference}, which its $defs does not hold"
                );
            }
        }
    }

    fn references(schema: &Value) -> Vec<String> {
        let mut found = Vec::new();
        let mut pending = vec![schema.clone()];
        while let Some(value) = pending.pop() {
            match value {
                Value::Object(fields) => {
                    for (key, value) in fields {
                        match (key.as_str(), value.as_str()) {
                            ("$ref", Some(reference)) => found.push(
                                reference
                                    .strip_prefix("#/$defs/")
                                    .unwrap_or_else(|| panic!("unresolvable $ref {reference}"))
                                    .to_owned(),
                            ),
                            _ => pending.push(value),
                        }
                    }
                }
                Value::Array(items) => pending.extend(items),
                _ => {}
            }
        }
        found
    }

    /// One branch of an output schema, as a document of its own, so that a
    /// payload can be tested against the success shape and the error shape
    /// separately.
    fn branch(schema: &Value, index: usize) -> Value {
        let mut document = serde_json::Map::new();
        document.insert("$defs".into(), schema["$defs"].clone());
        document.extend(schema["oneOf"][index].as_object().unwrap().clone());
        Value::Object(document)
    }

    /// Hand-written schemas go stale, so the declared shape is checked against
    /// what the tools really answer rather than against the structs by eye.
    #[tokio::test]
    async fn every_tool_result_validates_against_its_declared_output_schema() {
        let (server, read_only, ordinary_admin, superuser, _) =
            super::super::test_support::server_fixture().await;
        let recorded: std::cell::RefCell<Vec<(String, Value)>> = Default::default();
        let call = async |name: &str, arguments: Value, principal: &crate::domain::Principal| {
            let arguments = serde_json::from_value(arguments).unwrap();
            let result = server
                .dispatch_tool(name, arguments, Some(principal.clone()))
                .await
                .unwrap();
            let value = super::super::test_support::structured_value(&result);
            recorded.borrow_mut().push((name.to_owned(), value.clone()));
            value
        };
        let root = &superuser;

        let quadriceps = call("create_muscle", json!({"name":"Quadriceps"}), root).await;
        call("create_muscle", json!({"name":"Glutes"}), root).await;
        let spare_muscle = call("create_muscle", json!({"name":"Spare muscle"}), root).await;
        call(
            "update_muscle",
            json!({"id":quadriceps["id"],"name":"Quadriceps"}),
            root,
        )
        .await;
        call("get_muscle", json!({"id":"Quadriceps"}), root).await;
        call("list_muscles", json!({}), root).await;
        call("delete_muscle", json!({"id":spare_muscle["id"]}), root).await;

        let barbell = call("create_equipment", json!({"name":"Barbell"}), root).await;
        let spare_equipment = call("create_equipment", json!({"name":"Sled"}), root).await;
        call(
            "update_equipment",
            json!({"id":barbell["id"],"name":"Barbell"}),
            root,
        )
        .await;
        call("get_equipment", json!({"id":"Barbell"}), root).await;
        call("list_equipment", json!({}), root).await;
        call(
            "delete_equipment",
            json!({"id":spare_equipment["id"]}),
            root,
        )
        .await;

        let squat = call(
            "create_exercise",
            json!({
                "name":"Back squat","contraction_type":"dynamic",
                "primary_muscles":["Quadriceps"],"secondary_muscles":["Glutes"],
                "equipment":["Barbell"]
            }),
            root,
        )
        .await;
        call(
            "create_exercise",
            json!({"name":"Back extension","contraction_type":"dynamic"}),
            root,
        )
        .await;
        let spare_exercise = call(
            "create_exercise",
            json!({"name":"Spare lift","contraction_type":"dynamic"}),
            root,
        )
        .await;
        call(
            "update_exercise",
            json!({
                "id":squat["id"],"name":"Back squat","contraction_type":"dynamic",
                "bodyweight_share":0,
                "primary_muscles":["Quadriceps"],"secondary_muscles":["Glutes"],
                "equipment":["Barbell"]
            }),
            root,
        )
        .await;
        call("get_exercise", json!({"id":"Back squat"}), root).await;
        call("list_exercises", json!({}), root).await;
        call(
            "resolve_exercises",
            json!({"names":["Back squat","Hack squat","back"]}),
            root,
        )
        .await;
        call("delete_exercise", json!({"id":spare_exercise["id"]}), root).await;

        let strength = call(
            "create_workout_session",
            json!({"started_at":"2026-08-01","activity":{"type":"strength"}}),
            root,
        )
        .await;
        call("get_workout_session", json!({"id":strength["id"]}), root).await;

        let logged = call(
            "log_workout",
            json!({
                "started_at":"2026-08-10","label":"Leg day","notes":"slept badly",
                "exercises":[{"exercise":"Back squat","notes":"belt on","sets":[
                    {"set_type":"warmup","reps":8,"load_g":40000},
                    {"set_type":"working","reps":5,"load_g":60000}
                ]}]
            }),
            root,
        )
        .await;
        call(
            "replace_workout",
            json!({
                "id":logged["id"],"started_at":"2026-08-10","label":"Leg day",
                "exercises":[{"exercise":"Back squat","sets":[
                    {"set_type":"working","reps":5,"load_g":62500}
                ]}]
            }),
            root,
        )
        .await;
        let reading = call(
            "log_bodyweight",
            json!({"recorded_on":"2026-08-09","mass_g":72500}),
            root,
        )
        .await;
        call("list_bodyweight", json!({}), root).await;
        call("delete_bodyweight", json!({"id":reading["id"]}), root).await;
        call("list_workouts", json!({}), root).await;
        call("exercise_history", json!({"exercise":"Back squat"}), root).await;
        call("personal_records", json!({}), root).await;
        call("delete_workout_session", json!({"id":strength["id"]}), root).await;

        // Error shapes: the plain two-field payload, the one that carries
        // candidates, and the one that carries missing, ambiguous and
        // valid_example together.
        let no_scope = call(
            "create_workout_session",
            json!({"started_at":"2026-08-01","activity":{"type":"strength"}}),
            &read_only,
        )
        .await;
        let no_role = call("create_muscle", json!({"name":"Denied"}), &ordinary_admin).await;
        // Both authorization failures share one code, so only the message can
        // tell a client which of the two remedies applies.
        assert_eq!(no_scope["error"], "forbidden");
        assert_eq!(no_role["error"], "forbidden");
        assert_ne!(no_scope["message"], no_role["message"]);
        call("get_exercise", json!({"id":"back"}), root).await;
        call("get_exercise", json!({"id":"Nothing at all"}), root).await;
        call(
            "log_workout",
            json!({
                "started_at":"2026-08-14",
                "exercises":[
                    {"exercise":"Hack squat","sets":[]},
                    {"exercise":"back","sets":[]}
                ]
            }),
            root,
        )
        .await;
        for (name, _, _) in TOOL_SPECS {
            call(name, json!({"definitely_unknown":true}), root).await;
        }

        let recorded = recorded.borrow();
        let mut with_success = std::collections::BTreeSet::new();
        let mut with_error = std::collections::BTreeSet::new();
        for (name, payload) in recorded.iter() {
            let schema = Value::Object(output_schema_for_tool(name));
            let whole = jsonschema::validator_for(&schema).unwrap();
            assert!(
                whole.is_valid(payload),
                "tool {name} answered {payload}, which its output schema rejects: {:?}",
                whole.validate(payload).err().map(|error| error.to_string())
            );
            let success = jsonschema::validator_for(&branch(&schema, 0)).unwrap();
            let error = jsonschema::validator_for(&branch(&schema, 1)).unwrap();
            let matches_error = error.is_valid(payload);
            assert_ne!(
                success.is_valid(payload),
                matches_error,
                "tool {name} answered {payload}, which does not pick exactly one branch"
            );
            if matches_error {
                with_error.insert(name.clone());
            } else {
                with_success.insert(name.clone());
            }
        }
        let every_tool = TOOL_SPECS
            .iter()
            .map(|(name, _, _)| (*name).to_owned())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            with_success, every_tool,
            "these tools have no validated success result"
        );
        assert_eq!(
            with_error, every_tool,
            "these tools have no validated error result"
        );
    }
}
