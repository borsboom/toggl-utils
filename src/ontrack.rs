mod processor;
mod table_utils;
mod types;

use crate::ontrack::processor::*;
use crate::ontrack::table_utils::*;
use crate::ontrack::types::*;
use anyhow::*;
use chrono::prelude::*;
use chrono::Duration;
use log::*;
use prettytable::{cell, format::Alignment, row, Attr, Cell, Row, Table};
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use structopt::StructOpt;
use toggl_rs::Toggl;

const DEFAULT_INPUT_FILE: &str = "toggl-ontrack.yaml";

/// Keep work hours on track using Toggl data
#[derive(Debug, StructOpt)]
#[structopt()]
pub struct Options {
    /// The Toggl API token to use for authentication (from https://track.toggl.com/profile)
    #[structopt(long, env = "TOGGL_API_TOKEN")]
    pub api_token: String,
    /// File containing expected hours per period/client/project
    #[structopt(short = "i", long, env = "TOGGL_ONTRACK_FILE", default_value = DEFAULT_INPUT_FILE)]
    pub input_file: String,
    /// Fail with error if there are any warnings about time entries
    #[structopt(short = "s", long)]
    pub strict: bool,
    /// Show per-period hours table in addition to totals
    #[structopt(short = "p", long)]
    pub show_periods: bool,
    /// Log verbosity level (off, error, warn, info, debug, trace)
    #[structopt(short = "v", long, default_value = "warn")]
    pub verbosity: LevelFilter,
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
        Cell::new_align("CURRENT PERIOD", Alignment::CENTER)
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
    let mut max_whole_days_until_end_work = 0;
    let mut sorted_buckets: Vec<_> = total_bucket_durations.keys().collect();
    let tomorrow_date = now.date() + Duration::days(1);
    sorted_buckets.sort();
    for bucket in sorted_buckets {
        let Bucket { client, project } = bucket;
        let TotalDurations {
            end_work_date,
            durations,
        } = total_bucket_durations.get(bucket).unwrap();
        let whole_days_until_end_work = (*end_work_date - tomorrow_date).num_days();
        if project.is_none() {
            total_durations = total_durations + *durations;
            max_whole_days_until_end_work =
                i64::max(max_whole_days_until_end_work, whole_days_until_end_work);
        }
        table.add_row(Row::new(vec![
            Cell::new(client),                                             // CLIENT
            Cell::new(project.as_ref().map(String::as_str).unwrap_or("")), // PROJECT
            // duration_hours_cell(durations.expected), // EXPECTED (ALL TIME)
            // duration_hours_cell(durations.partial_expected), // EXPECTED (UP TO TODAY)
            // duration_hours_cell(durations.actual), // ACTUAL (ALL TIME)
            Cell::new(""),
            duration_hours_cell(durations.current_period_expected()), // EXPECTED (CURRENT PERIOD)
            duration_hours_cell(durations.current_period_actual),     // ACTUAL (CURRENT PERIOD)
            color_duration_hours_cell(durations.remaining()),         // REMAINING (CURRENT PERIOD)
            Cell::new(""),
            duration_hours_cell(durations.today_expected()), // EXPECTED (TODAY)
            duration_hours_cell(durations.today_actual),     // ACTUAL (TODAY)
            color_duration_hours_cell(durations.partial_remaining()), // REMAINING (TODAY)
            Cell::new(""),
            durations
                .daily_average_remaining(whole_days_until_end_work)
                .map(duration_hours_cell)
                .unwrap_or_else(|| Cell::new("(n/a)")), // AVERAGE REMAINING PER DAY
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
            total_durations.current_period_actual + total_durations.expected
                - total_durations.actual,
        )
        .with_style(Attr::Bold), // EXPECTED (CURRENT PERIOD)
        duration_hours_cell(total_durations.current_period_actual).with_style(Attr::Bold), // ACTUAL (CURRENT PERIOD)
        color_duration_hours_cell(total_durations.actual - total_durations.expected)
            .with_style(Attr::Bold), // REMAINING (CURRENT PERIOD)
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
        total_durations
            .daily_average_remaining(max_whole_days_until_end_work)
            .map(|v| duration_hours_cell(v).with_style(Attr::Bold))
            .unwrap_or_else(|| Cell::new("(n/a)")), // AVERAGE REMAINING PER DAY
    ]));
    table.printstd();
}

pub fn run(options: Options) -> Result<()> {
    let input = load_input(&options.input_file)?;
    let now = Local::now();
    let mut processor = Processor::new(now);
    processor.initialize(input)?;
    let toggl = Toggl::init(&options.api_token).context("Could not connect to Toggl")?;
    debug!("toggl.clients: {:#?}", toggl.clients);
    debug!("toggl.projects: {:#?}", toggl.projects);
    processor.process(options.strict, &toggl)?;
    if options.show_periods {
        processor.print_table();
    }
    let total_bucket_durations = processor.calculate_totals();
    print_total_bucket_durations_table(now, &total_bucket_durations);
    Ok(())
}
