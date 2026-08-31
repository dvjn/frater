use chrono::{DateTime, Utc};
use maud::{Markup, html};

use super::{layout_with_nav, signed_in_nav};
use crate::domain::{Dashboard, PersonalRecord, RunListEntry, RunRecord, WorkoutListEntry};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Range {
    Week,
    Month,
    Year,
    All,
}

impl Range {
    pub fn parse(value: Option<&str>) -> Self {
        match value {
            Some("month") => Self::Month,
            Some("year") => Self::Year,
            Some("all") => Self::All,
            _ => Self::Week,
        }
    }

    pub fn days(self) -> Option<i64> {
        match self {
            Self::Week => Some(7),
            Self::Month => Some(30),
            Self::Year => Some(365),
            Self::All => None,
        }
    }

    fn options() -> [(Self, &'static str, &'static str); 4] {
        [
            (Self::Week, "Week", "/"),
            (Self::Month, "Month", "/?range=month"),
            (Self::Year, "Year", "/?range=year"),
            (Self::All, "All", "/?range=all"),
        ]
    }
}

fn moment(value: DateTime<Utc>) -> String {
    value.format("%Y-%m-%d").to_string()
}

fn group_thousands(value: i64) -> String {
    let digits = value.unsigned_abs().to_string();
    let mut grouped = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    if value < 0 {
        format!("-{grouped}")
    } else {
        grouped
    }
}

fn format_kg(grams: i64) -> String {
    format!("{} kg", group_thousands((grams + 500) / 1_000))
}

fn format_km(meters: i64) -> String {
    let tenths = (meters + 50) / 100;
    format!("{}.{} km", group_thousands(tenths / 10), tenths % 10)
}

fn format_duration(seconds: i64) -> String {
    let seconds = seconds.max(0);
    if seconds < 3_600 {
        format!("{}:{:02}", seconds / 60, seconds % 60)
    } else {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3_600,
            seconds % 3_600 / 60,
            seconds % 60
        )
    }
}

fn format_pace(duration_sec: i64, distance_m: i64) -> String {
    if distance_m <= 0 {
        return "0:00 /km".to_owned();
    }
    let scaled = i128::from(duration_sec) * 1_000 + i128::from(distance_m) / 2;
    let sec_per_km = i64::try_from(scaled / i128::from(distance_m)).unwrap_or(i64::MAX);
    format!("{} /km", format_duration(sec_per_km))
}

fn record_distance_label(distance_m: i64) -> String {
    match distance_m {
        21_098 => "Half marathon".to_owned(),
        42_195 => "Marathon".to_owned(),
        other => format!("{} km", other / 1_000),
    }
}

fn best_set(record: &PersonalRecord) -> Option<(String, DateTime<Utc>)> {
    if let (Some(load_g), Some(reps)) = (record.max_load_g, record.max_load_reps) {
        let at = record.max_load_at.unwrap_or(record.last_performed_at);
        return Some((format!("{} × {}", format_kg(load_g), reps), at));
    }
    if let Some(hold_sec) = record.longest_hold_sec {
        let at = record.longest_hold_at.unwrap_or(record.last_performed_at);
        return Some((format!("{hold_sec}s hold"), at));
    }
    None
}

pub fn page(range: Range, dashboard: &Dashboard) -> Markup {
    layout_with_nav(
        "Frater",
        Some(signed_in_nav()),
        html! {
            main class="account-shell" {
                div class="page-head" {
                    h1 class="page-title" { "Dashboard" }
                    nav class="range-switch" aria-label="Time range" {
                        @for (option, label, href) in Range::options() {
                            a class="range-option" href=(href)
                                aria-current=[(option == range).then_some("page")] { (label) }
                        }
                    }
                }
                (tiles(dashboard))
                div class="account-grid account-grid-even" {
                    (exercise_records_card(&dashboard.exercise_records))
                    (run_records_card(&dashboard.run_records))
                }
                div class="account-grid account-grid-even" {
                    (workouts_card(&dashboard.workouts))
                    (runs_card(&dashboard.runs))
                }
            }
        },
    )
}

fn tiles(dashboard: &Dashboard) -> Markup {
    let stats = &dashboard.stats;
    let runs_word = if stats.run_count == 1 { "run" } else { "runs" };
    let workouts_note = format!(
        "{} strength · {} {}",
        stats.strength_count, stats.run_count, runs_word
    );
    let exercises_word = if stats.exercise_count == 1 {
        "exercise"
    } else {
        "exercises"
    };
    let volume_note =
        (stats.volume_g > 0).then(|| format!("across {} {}", stats.exercise_count, exercises_word));
    let distance_note = (stats.duration_sec > 0).then(|| format_duration(stats.duration_sec));
    html! {
        div class="tile-row" {
            (tile("Workouts", &(stats.strength_count + stats.run_count).to_string(), Some(&workouts_note)))
            (tile("Volume", &format_kg(stats.volume_g), volume_note.as_deref()))
            (tile("Distance", &format_km(stats.distance_m), distance_note.as_deref()))
        }
    }
}

fn tile(label: &str, value: &str, note: Option<&str>) -> Markup {
    html! {
        div class="tile" {
            div class="tile-label" { (label) }
            div class="tile-value" { (value) }
            div class="tile-note" { @if let Some(note) = note { (note) } }
        }
    }
}

fn exercise_records_card(records: &[PersonalRecord]) -> Markup {
    let rows = records
        .iter()
        .filter_map(|record| best_set(record).map(|(set, at)| (record, set, at)))
        .collect::<Vec<_>>();
    html! {
        article class="auth-card account-card" {
            h2 { "Exercise records" }
            p { "Your heaviest set for each exercise, all time." }
            div class="table-wrap" {
                table class="data-table" {
                    thead {
                        tr {
                            th scope="col" { "Exercise" }
                            th scope="col" { "Best set" }
                            th scope="col" { "Est. 1RM" }
                            th scope="col" { "Date (UTC)" }
                        }
                    }
                    tbody {
                        @if rows.is_empty() {
                            tr { td colspan="4" class="table-empty" { "No exercise records." } }
                        }
                        @for (record, set, at) in &rows {
                            tr {
                                th scope="row" class="cell-title" { (record.exercise_name) }
                                td class="col-numeric" { (set) }
                                td class="col-numeric" {
                                    @if let Some(estimate_g) = record.best_estimated_1rm_g {
                                        (format_kg(estimate_g))
                                    } @else {
                                        span class="cell-absent" { "—" }
                                    }
                                }
                                td class="cell-moment" { (moment(*at)) }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn run_records_card(records: &[RunRecord]) -> Markup {
    html! {
        article class="auth-card account-card" {
            h2 { "Run records" }
            p { "Your fastest time for each distance, all time." }
            div class="table-wrap" {
                table class="data-table" {
                    thead {
                        tr {
                            th scope="col" { "Distance" }
                            th scope="col" { "Time" }
                            th scope="col" { "Pace" }
                            th scope="col" { "Date (UTC)" }
                        }
                    }
                    tbody {
                        @if records.is_empty() {
                            tr { td colspan="4" class="table-empty" { "No run records." } }
                        }
                        @for record in records {
                            tr {
                                th scope="row" class="cell-title" {
                                    (record_distance_label(record.distance_m))
                                }
                                td class="col-numeric" { (format_duration(record.duration_sec)) }
                                td class="col-numeric" {
                                    (format_pace(record.duration_sec, record.distance_m))
                                }
                                td class="cell-moment" { (moment(record.started_at)) }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn workouts_card(workouts: &[WorkoutListEntry]) -> Markup {
    html! {
        article class="auth-card account-card" {
            h2 { "Workouts" }
            p { "Strength sessions in the selected range." }
            div class="table-wrap" {
                table class="data-table" {
                    thead {
                        tr {
                            th scope="col" { "Date (UTC)" }
                            th scope="col" { "Workout" }
                            th scope="col" { "Sets" }
                            th scope="col" { "Volume" }
                        }
                    }
                    tbody {
                        @if workouts.is_empty() {
                            tr { td colspan="4" class="table-empty" { "No workouts." } }
                        }
                        @for workout in workouts {
                            tr {
                                th scope="row" class="cell-moment" { (moment(workout.started_at)) }
                                td class="cell-title" {
                                    (workout.label.as_deref().unwrap_or("Workout"))
                                }
                                td class="col-numeric" { (workout.set_count) }
                                td class="col-numeric" { (format_kg(workout.volume_g)) }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn runs_card(runs: &[RunListEntry]) -> Markup {
    html! {
        article class="auth-card account-card" {
            h2 { "Runs" }
            p { "Runs in the selected range." }
            div class="table-wrap" {
                table class="data-table" {
                    thead {
                        tr {
                            th scope="col" { "Date (UTC)" }
                            th scope="col" { "Run" }
                            th scope="col" { "Distance" }
                            th scope="col" { "Time" }
                        }
                    }
                    tbody {
                        @if runs.is_empty() {
                            tr { td colspan="4" class="table-empty" { "No runs." } }
                        }
                        @for run in runs {
                            tr {
                                th scope="row" class="cell-moment" { (moment(run.started_at)) }
                                td class="cell-title" { (run.label.as_deref().unwrap_or("Run")) }
                                td class="col-numeric" { (format_km(run.distance_m)) }
                                td class="col-numeric" { (format_duration(run.duration_sec)) }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_rounds_to_whole_kilograms_with_grouping() {
        assert_eq!(format_kg(24_360_000), "24,360 kg");
        assert_eq!(format_kg(1_499), "1 kg");
        assert_eq!(format_kg(1_500), "2 kg");
        assert_eq!(format_kg(0), "0 kg");
    }

    #[test]
    fn distance_keeps_one_decimal() {
        assert_eq!(format_km(8_200), "8.2 km");
        assert_eq!(format_km(42_549), "42.5 km");
        assert_eq!(format_km(42_550), "42.6 km");
        assert_eq!(format_km(0), "0.0 km");
        assert_eq!(format_km(1_234_560), "1,234.6 km");
    }

    #[test]
    fn durations_switch_to_hours_past_sixty_minutes() {
        assert_eq!(format_duration(0), "0:00");
        assert_eq!(format_duration(65), "1:05");
        assert_eq!(format_duration(3_599), "59:59");
        assert_eq!(format_duration(3_600), "1:00:00");
        assert_eq!(format_duration(3_661), "1:01:01");
    }

    #[test]
    fn pace_divides_duration_by_kilometres() {
        assert_eq!(format_pace(1_500, 5_000), "5:00 /km");
        assert_eq!(format_pace(1_001, 3_000), "5:34 /km");
        assert_eq!(format_pace(240, 1_000), "4:00 /km");
    }

    #[test]
    fn record_distances_use_race_names() {
        assert_eq!(record_distance_label(1_000), "1 km");
        assert_eq!(record_distance_label(10_000), "10 km");
        assert_eq!(record_distance_label(21_098), "Half marathon");
        assert_eq!(record_distance_label(42_195), "Marathon");
    }

    #[test]
    fn unknown_ranges_fall_back_to_week() {
        assert_eq!(Range::parse(None), Range::Week);
        assert_eq!(Range::parse(Some("month")), Range::Month);
        assert_eq!(Range::parse(Some("decade")), Range::Week);
        assert_eq!(Range::parse(Some("all")).days(), None);
        assert_eq!(Range::parse(Some("year")).days(), Some(365));
    }
}
