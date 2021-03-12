use anyhow::*;
use chrono::prelude::*;
use chrono::Duration;
use derive_more::{Add, Sub};
use dotenv::dotenv;
use log::*;
use prettytable::{cell, color, format::Alignment, row, Attr, Cell, Row, Table};
use serde::Deserialize;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io::BufReader;
use structopt::StructOpt;
use toggl_rs::{TimeEntry, Toggl, TogglExt};

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(untagged)]
enum WorkDay {
    Weekday(Weekday),
    Offset(i64),
}

const DEFAULT_INPUT_FILE: &str = "toggl-ontrack.yaml";
const DEFAULT_WEEK_LENGTH: i64 = 7;
const DEFAULT_FIRST_WORK_DAY: WorkDay = WorkDay::Weekday(Weekday::Mon);
const DEFAULT_LAST_WORK_DAY: WorkDay = WorkDay::Weekday(Weekday::Fri);

/// Keep work hours on track using Toggl data
#[derive(Debug, StructOpt)]
#[structopt()]
struct Opt {
    /// The Toggl API token to use for authentication (from https://track.toggl.com/profile)
    #[structopt(long, env = "TOGGL_API_TOKEN")]
    api_token: String,
    /// File containing expected hours per week/client/project
    #[structopt(short = "i", long, env = "TOGGL_ONTRACK_FILE", default_value = DEFAULT_INPUT_FILE)]
    input_file: String,
    /// Fail with error if there are any warnings about time entries
    #[structopt(short = "s", long)]
    strict: bool,
    /// Show per-week hours table in addition to totals
    #[structopt(short = "w", long)]
    show_weekly: bool,
    /// Log verbosity level (off, error, warn, info, debug, trace)
    #[structopt(short = "v", long, default_value = "warn")]
    verbosity: LevelFilter,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct ProjectInput {
    expected_hours: f64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct ClientInput {
    expected_hours: f64,
    projects: Option<HashMap<String, ProjectInput>>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct WeekInput {
    start: NaiveDate,
    length: Option<i64>,
    first_work_day: Option<WorkDay>,
    last_work_day: Option<WorkDay>,
    clients: HashMap<String, ClientInput>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct DefaultsInput {
    first_work_day: Option<WorkDay>,
    last_work_day: Option<WorkDay>,
}

impl Default for DefaultsInput {
    fn default() -> Self {
        DefaultsInput {
            first_work_day: None,
            last_work_day: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct Input {
    #[serde(default)]
    defaults: DefaultsInput,
    weeks: Vec<WeekInput>,
}

#[derive(Add, Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Sub)]
struct Durations {
    expected: Duration,
    partial_expected: Duration,
    actual: Duration,
    today_actual: Duration,
    current_week_actual: Duration,
}

impl Durations {
    fn zero() -> Durations {
        Durations {
            expected: Duration::zero(),
            partial_expected: Duration::zero(),
            actual: Duration::zero(),
            today_actual: Duration::zero(),
            current_week_actual: Duration::zero(),
        }
    }

    fn expected(expected_hours: f64, partial_days: i64, num_work_days: i64) -> Durations {
        let mut result = Durations::zero();
        result.expected = Duration::seconds((expected_hours * 3600.0).round() as i64);
        result.partial_expected = Duration::seconds(i64::max(
            (expected_hours * 3600.0 * (partial_days as f64) / (num_work_days as f64)).round()
                as i64,
            0,
        ));
        result
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct Bucket {
    client: String,
    project: Option<String>,
}

impl Ord for Bucket {
    fn cmp(&self, other: &Self) -> Ordering {
        match (
            self.client.cmp(&other.client),
            &self.project,
            &other.project,
        ) {
            (Ordering::Equal, Some(_), None) => Ordering::Less,
            (Ordering::Equal, None, Some(_)) => Ordering::Greater,
            (Ordering::Equal, self_project, other_project) => self_project.cmp(&other_project),
            (ordering, _, _) => ordering,
        }
    }
}

impl PartialOrd for Bucket {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct WeekBucketDurations {
    week_start: DateTime<Local>,
    bucket: Bucket,
    week_length: i64,
    last_work_day_offset: i64,
    durations: Durations,
}

#[derive(Clone, Debug)]
struct TotalDurations {
    end_work_time: DateTime<Local>,
    durations: Durations,
}

fn work_day_offset(week_start: DateTime<Local>, work_day: WorkDay) -> i64 {
    match work_day {
        WorkDay::Weekday(weekday) => {
            let offset = weekday.num_days_from_sunday() as i64
                - week_start.weekday().num_days_from_sunday() as i64;
            if offset < 0 {
                offset + 7
            } else {
                offset
            }
        }
        WorkDay::Offset(offset) => offset,
    }
}

fn load_input(input_file: &str) -> Result<Input> {
    let file = File::open(input_file)
        .with_context(|| format!("could not opening input file: {}", input_file))?;
    let reader = BufReader::new(file);
    let input: Input = serde_yaml::from_reader(reader)
        .with_context(|| format!("could not parse input file: {}", input_file))?;
    debug!("input: {:#?}", input);
    Ok(input)
}

fn create_week_bucket_durations(
    now: DateTime<Local>,
    input: Input,
) -> Result<Vec<WeekBucketDurations>> {
    let mut results = Vec::new();
    for week_input in input.weeks {
        let week_start = Local
            .ymd(
                week_input.start.year(),
                week_input.start.month(),
                week_input.start.day(),
            )
            .and_hms(0, 0, 0);
        let first_work_day_offset = work_day_offset(
            week_start,
            week_input.first_work_day.unwrap_or(
                input
                    .defaults
                    .first_work_day
                    .unwrap_or(DEFAULT_FIRST_WORK_DAY),
            ),
        );
        let last_work_day_offset = work_day_offset(
            week_start,
            week_input.last_work_day.unwrap_or(
                input
                    .defaults
                    .last_work_day
                    .unwrap_or(DEFAULT_LAST_WORK_DAY),
            ),
        );
        if last_work_day_offset < first_work_day_offset {
            bail!("Last work day offset can not be before first work day offset");
        }
        let num_work_days = last_work_day_offset - first_work_day_offset + 1;
        let partial_days = i64::min(
            (now.date().and_hms(0, 0, 0) + Duration::days(1 - first_work_day_offset) - week_start)
                .num_days(),
            num_work_days,
        );
        for (client_name, client_input) in week_input.clients {
            if let Some(projects) = client_input.projects {
                for (project_name, project_input) in projects {
                    results.push(WeekBucketDurations {
                        week_start,
                        bucket: Bucket {
                            client: client_name.clone(),
                            project: Some(project_name),
                        },
                        week_length: week_input.length.unwrap_or(DEFAULT_WEEK_LENGTH),
                        last_work_day_offset,
                        durations: Durations::expected(
                            project_input.expected_hours,
                            partial_days,
                            num_work_days,
                        ),
                    });
                }
            }
            results.push(WeekBucketDurations {
                week_start,
                bucket: Bucket {
                    client: client_name,
                    project: None,
                },
                week_length: week_input.length.unwrap_or(DEFAULT_WEEK_LENGTH),
                last_work_day_offset,
                durations: Durations::expected(
                    client_input.expected_hours,
                    partial_days,
                    num_work_days,
                ),
            });
        }
    }
    results.sort();
    Ok(results)
}

fn get_time_entries(
    now: DateTime<Local>,
    toggl: &Toggl,
    week_bucket_durations: &Vec<WeekBucketDurations>,
) -> Result<Vec<TimeEntry>> {
    let min_week_start = week_bucket_durations
        .iter()
        .map(|v| v.week_start)
        .min()
        .unwrap_or(now);
    debug!("min_week_start: {:#?}", min_week_start);
    let time_entries = toggl
        .get_time_entries_range(
            Some(min_week_start.with_timezone(&Utc)),
            Some(now.with_timezone(&Utc)),
        )
        .context("Could not get time entries from Toggl")?;
    Ok(time_entries)
}

fn is_in_week(date: DateTime<Local>, week_start: DateTime<Local>, week_length: i64) -> bool {
    date >= week_start && date - week_start < Duration::days(week_length)
}

fn accumulate_week_bucket_durations(
    now: DateTime<Local>,
    mut week_bucket_durations: Vec<WeekBucketDurations>,
    time_entries: &Vec<TimeEntry>,
    strict: bool,
) -> Result<Vec<WeekBucketDurations>> {
    //TODO: optimize nested loops
    let now_time = now;
    let today_date = now_time.date();
    let mut found_warning = false;
    for time_entry in time_entries {
        if let (Some(client), Some(project)) = (&time_entry.client, &time_entry.project) {
            let mut found_match_client_only = false;
            let mut found_match_project = false;
            for WeekBucketDurations {
                week_start,
                bucket,
                week_length,
                last_work_day_offset: _,
                durations,
            } in &mut week_bucket_durations
            {
                let time_entry_start = time_entry.start.with_timezone(&Local);
                if client.name == bucket.client
                    && (bucket.project.is_none() || Some(&project.name) == bucket.project.as_ref())
                    && is_in_week(time_entry_start, *week_start, *week_length)
                {
                    if bucket.project.is_none() {
                        if found_match_client_only {
                            bail!("Multiple expected clients/weeks matched: {:?}", &time_entry);
                        } else {
                            found_match_client_only = true;
                        }
                    } else {
                        if found_match_project {
                            bail!(
                                "Multiple expected projects/weeks matched: {:?}",
                                &time_entry
                            );
                        } else {
                            found_match_project = true;
                        }
                    }
                    let time_entry_duration = time_entry
                        .stop
                        .map(|v| v.with_timezone(&Local))
                        .unwrap_or(now_time)
                        - time_entry_start;
                    durations.actual = durations.actual + time_entry_duration;
                    if is_in_week(now_time, *week_start, *week_length) {
                        durations.current_week_actual =
                            durations.current_week_actual + time_entry_duration;
                    }
                    if time_entry_start.date() == today_date {
                        durations.today_actual = durations.today_actual + time_entry_duration;
                    }
                }
            }
            if !found_match_client_only {
                warn!("no expected client/week matched: {:?}", &time_entry);
                found_warning = true;
            }
        } else {
            warn!(
                "time entry missing client and/or project: {:?}",
                &time_entry
            );
            found_warning = true;
        }
    }
    if strict && found_warning {
        bail!("Strict mode enabled (see warnings above)");
    }
    Ok(week_bucket_durations)
}

fn calculate_total_bucket_durations(
    week_bucket_durations: &Vec<WeekBucketDurations>,
) -> HashMap<Bucket, TotalDurations> {
    let mut result: HashMap<Bucket, TotalDurations> = HashMap::new();
    for source in week_bucket_durations {
        let source_end_work_date =
            source.week_start + Duration::days(source.last_work_day_offset + 1);
        if let Some(entry) = result.get_mut(&source.bucket) {
            if source_end_work_date > entry.end_work_time {
                entry.end_work_time = source_end_work_date;
            }
            entry.durations = entry.durations + source.durations;
        } else {
            result.insert(
                source.bucket.clone(),
                TotalDurations {
                    end_work_time: source_end_work_date,
                    durations: source.durations,
                },
            );
        }
    }
    result
}

fn duration_hours_cell_(duration: Duration, color: bool) -> Cell {
    let seconds = duration.num_seconds();
    let formatted = if seconds < 0 {
        format!(
            "-{}:{:02}",
            seconds.abs() / 3600,
            (seconds.abs() % 3600) / 60
        )
    } else {
        format!("{}:{:02}", seconds / 3600, (seconds % 3600) / 60)
    };
    let mut cell = Cell::new_align(&formatted, Alignment::RIGHT);
    if color {
        if seconds < 0 {
            cell.style(Attr::ForegroundColor(color::RED));
        } else if seconds > 0 {
            cell.style(Attr::ForegroundColor(color::GREEN));
        }
    }
    cell
}

fn duration_hours_cell(duration: Duration) -> Cell {
    duration_hours_cell_(duration, false)
}

fn color_duration_hours_cell(duration: Duration) -> Cell {
    duration_hours_cell_(duration, true)
}

fn print_week_bucket_durations_table(week_bucket_durations: &Vec<WeekBucketDurations>) {
    let mut table = Table::new();
    table.set_format(
        prettytable::format::FormatBuilder::new()
            .column_separator(' ')
            .build(),
    );
    table.add_row(row![
        b->"WEEK",
        " ",
        b->"CLIENT",
        b->"PROJECT",
        " ",
        br->"EXPECT",
        // br->"part",
        br->"ACTUAL",
        // br-> "week ",
        // br->"tod",
        br->"DIFFERENCE",
        // , br->"tod"
    ]);
    for WeekBucketDurations {
        week_start,
        bucket: Bucket { client, project },
        week_length: _,
        last_work_day_offset: _,
        durations,
    } in week_bucket_durations
    {
        table.add_row(Row::new(vec![
            Cell::new(&week_start.date().naive_local().to_string()),
            Cell::new(""),
            Cell::new(client),
            Cell::new(project.as_ref().map(String::as_str).unwrap_or("")),
            Cell::new(""),
            duration_hours_cell(durations.expected),
            // duration_hours_cell(durations.partial_expected),
            duration_hours_cell(durations.actual),
            // duration_hours_cell(durations.current_week_actual),
            // duration_hours_cell(durations.today_actual),
            color_duration_hours_cell(durations.actual - durations.expected),
            // color_duration_hours_cell(durations.actual - durations.partial_expected),
        ]));
    }
    table.printstd();
    println!();
}

fn print_total_bucket_durations_table(
    now: DateTime<Local>,
    total_bucket_durations: &HashMap<Bucket, TotalDurations>,
) {
    let mut table = Table::new();
    table.set_format(
        prettytable::format::FormatBuilder::new()
            .column_separator(' ')
            .build(),
    );
    table.add_row(Row::new(vec![
        Cell::new(""),
        Cell::new(""),
        Cell::new(""),
        Cell::new_align("THIS WEEK", Alignment::CENTER)
            .with_hspan(3)
            .with_style(Attr::Bold),
        Cell::new(""),
        Cell::new_align("TODAY", Alignment::CENTER)
            .with_hspan(3)
            .with_style(Attr::Bold),
        Cell::new(""),
        Cell::new(""),
    ]));
    table.add_row(row![
        b->"CLIENT",
        b->"PROJECT",
        " ",
        br->"expect",
        br->"actual",
        br->"remain",
        " ",
        br->"expect",
        br->"actual",
        br->"remain",
        " ",
        br->"AVG.R"
    ]);
    let mut total_durations = Durations::zero();
    let mut total_daily_average_remaining = Some(Duration::zero());
    let mut sorted_buckets: Vec<_> = total_bucket_durations.keys().collect();
    let tomorrow_start_time = now.date().and_hms(0, 0, 0) + Duration::days(1);
    sorted_buckets.sort();
    for bucket in sorted_buckets {
        let Bucket { client, project } = bucket;
        let TotalDurations {
            end_work_time,
            durations,
        } = total_bucket_durations.get(bucket).unwrap();
        let whole_days_until_end_work = (*end_work_time - tomorrow_start_time).num_days();
        let daily_average_remaining = if whole_days_until_end_work > 0 {
            Some(Duration::seconds(
                (durations.expected - durations.actual).num_seconds() / whole_days_until_end_work,
            ))
        } else {
            None
        };
        if project.is_none() {
            total_durations = total_durations + *durations;
            total_daily_average_remaining = daily_average_remaining
                .zip(total_daily_average_remaining)
                .map(|(v, u)| v + u);
        }
        table.add_row(Row::new(vec![
            Cell::new(client),                                             // CLIENT
            Cell::new(project.as_ref().map(String::as_str).unwrap_or("")), // PROJECT
            // duration_hours_cell(durations.expected), // EXPECTED (ALL TIME)
            // duration_hours_cell(durations.partial_expected), // EXPECTED (UP TO TODAY)
            // duration_hours_cell(durations.actual), // ACTUAL (ALL TIME)
            Cell::new(""),
            duration_hours_cell(
                durations.current_week_actual + durations.expected - durations.actual,
            ), // EXPECTED (THIS WEEK)
            duration_hours_cell(durations.current_week_actual), // ACTUAL (THIS WEEK)
            color_duration_hours_cell(durations.actual - durations.expected), // REMAINING (THIS WEEK)
            Cell::new(""),
            duration_hours_cell(
                durations.today_actual + durations.partial_expected - durations.actual,
            ), // EXPECTED (TODAY)
            duration_hours_cell(durations.today_actual), // ACTUAL (TODAY)
            color_duration_hours_cell(durations.actual - durations.partial_expected), // REMAINING (TODAY)
            Cell::new(""),
            daily_average_remaining
                .map(duration_hours_cell)
                .unwrap_or(Cell::new("(n/a)")), // AVERAGE REMAINING PER DAY
        ]));
    }
    table.add_row(Row::new(vec![
        Cell::new("TOTAL:").with_style(Attr::Bold),
        Cell::new(""),
        // duration_hours_cell(total_durations.expected).with_style(Attr::Bold), // EXPECTED (ALL TIME)
        // duration_hours_cell(total_durations.partial_expected).with_style(Attr::Bold), // EXPECTED (UP TO TODAY)
        // duration_hours_cell(total_durations.actual).with_style(Attr::Bold), // ACTUAL (ALL TIME)
        Cell::new(""),
        duration_hours_cell(
            total_durations.current_week_actual + total_durations.expected - total_durations.actual,
        )
        .with_style(Attr::Bold), // EXPECTED (THIS WEEK)
        duration_hours_cell(total_durations.current_week_actual).with_style(Attr::Bold), // ACTUAL (THIS WEEK)
        color_duration_hours_cell(total_durations.actual - total_durations.expected)
            .with_style(Attr::Bold), // REMAINING (THIS WEEK)
        Cell::new(""),
        duration_hours_cell(
            total_durations.today_actual + total_durations.partial_expected
                - total_durations.actual,
        )
        .with_style(Attr::Bold), // EXPECTED (TODAY)
        duration_hours_cell(total_durations.today_actual).with_style(Attr::Bold), // ACTUAL (TODAY)
        color_duration_hours_cell(total_durations.actual - total_durations.partial_expected)
            .with_style(Attr::Bold), // REMAINING (TODAY)
        Cell::new(""),
        total_daily_average_remaining
            .map(|v| duration_hours_cell(v).with_style(Attr::Bold))
            .unwrap_or(Cell::new("(n/a)")), // AVERAGE REMAINING PER DAY
    ]));
    table.printstd();
}

fn run() -> Result<()> {
    dotenv().ok();
    let opt = Opt::from_args();
    env_logger::builder()
        .filter_module(env!("CARGO_CRATE_NAME"), opt.verbosity)
        .format_module_path(false)
        .format_timestamp(None)
        .parse_default_env()
        .init();
    let input = load_input(&opt.input_file)?;
    let now = Local::now();
    let init_week_bucket_durations = create_week_bucket_durations(now, input)?;
    debug!(
        "init_week_bucket_durations: {:#?}",
        init_week_bucket_durations
    );
    let toggl = Toggl::init(&opt.api_token).context("Could not connect to Toggl")?;
    debug!("clients: {:#?}", toggl.clients);
    debug!("projects: {:#?}", toggl.projects);
    let time_entries = get_time_entries(now, &toggl, &init_week_bucket_durations)?;
    debug!("time_entries: {:#?}", time_entries);
    let accumulated_week_bucket_durations = accumulate_week_bucket_durations(
        now,
        init_week_bucket_durations,
        &time_entries,
        opt.strict,
    )?;
    debug!(
        "accumulated_week_bucket_durations: {:#?}",
        accumulated_week_bucket_durations
    );
    if opt.show_weekly {
        print_week_bucket_durations_table(&accumulated_week_bucket_durations);
    }
    let total_bucket_durations =
        calculate_total_bucket_durations(&accumulated_week_bucket_durations);
    debug!("total_bucket_durations: {:#?}", total_bucket_durations);
    print_total_bucket_durations_table(now, &total_bucket_durations);
    Ok(())
}

fn main() {
    if let Err(err) = run() {
        error!("{:#}", err);
        std::process::exit(1);
    }
}
