use crate::ontrack::table_utils::*;
use crate::ontrack::types::*;
use anyhow::*;
use chrono::prelude::*;
use chrono::Duration;
use log::*;
use prettytable::{cell, row, Cell, Row, Table};
use std::collections::{HashMap, HashSet};
use toggl_rs::{TimeEntry, Toggl, TogglExt};

const DEFAULT_PERIOD_LENGTH: i64 = 7;
const DEFAULT_WORK_DAYS: WorkDaysInput = WorkDaysInput::FromToWeekdays {
    from: Weekday::Mon,
    to: Weekday::Fri,
};

fn work_day_offsets(
    period_start: Date<Local>,
    period_length: i64,
    work_day: WorkDayInput,
) -> Vec<i64> {
    match work_day {
        WorkDayInput::Weekday(weekday) => {
            let mut offset = weekday.num_days_from_sunday() as i64
                - period_start.weekday().num_days_from_sunday() as i64;
            let mut results = Vec::new();
            while offset < period_length {
                if offset >= 0 {
                    results.push(offset);
                }
                offset += 7
            }
            results
        }
        WorkDayInput::Offset(offset) => vec![offset],
    }
}

#[test]
fn test_work_day_offsets() {
    assert_eq!(
        work_day_offsets(
            Local.ymd(2021, 3, 7),
            15,
            WorkDayInput::Weekday(Weekday::Sun)
        ),
        vec![0, 7, 14]
    );
    assert_eq!(
        work_day_offsets(
            Local.ymd(2021, 3, 7),
            15,
            WorkDayInput::Weekday(Weekday::Wed)
        ),
        vec![3, 10]
    );
    assert_eq!(
        work_day_offsets(
            Local.ymd(2021, 3, 7),
            15,
            WorkDayInput::Weekday(Weekday::Sat)
        ),
        vec![6, 13]
    );
    assert_eq!(
        work_day_offsets(
            Local.ymd(2021, 3, 9),
            7,
            WorkDayInput::Weekday(Weekday::Sun)
        ),
        vec![5]
    );
}

fn is_date_in_period(date: Date<Local>, period_start: Date<Local>, period_length: i64) -> bool {
    date >= period_start && date < period_start + Duration::days(period_length)
}

#[test]
fn test_is_date_in_period() {
    assert!(!is_date_in_period(
        Local.ymd(2021, 3, 14),
        Local.ymd(2021, 3, 14),
        0
    ));
    assert!(is_date_in_period(
        Local.ymd(2021, 3, 14),
        Local.ymd(2021, 3, 14),
        1
    ));
    assert!(!is_date_in_period(
        Local.ymd(2021, 3, 13),
        Local.ymd(2021, 3, 14),
        1
    ));
    assert!(!is_date_in_period(
        Local.ymd(2021, 3, 15),
        Local.ymd(2021, 3, 14),
        1
    ));
    assert!(is_date_in_period(
        Local.ymd(2021, 11, 1),
        Local.ymd(2021, 11, 1),
        1
    ));
    assert!(!is_date_in_period(
        Local.ymd(2021, 10, 31),
        Local.ymd(2021, 11, 1),
        1
    ));
    assert!(!is_date_in_period(
        Local.ymd(2021, 11, 2),
        Local.ymd(2021, 11, 1),
        1
    ));
    assert!(is_date_in_period(
        Local.ymd(2021, 3, 14),
        Local.ymd(2021, 3, 7),
        14
    ));
    assert!(is_date_in_period(
        Local.ymd(2021, 3, 7),
        Local.ymd(2021, 3, 7),
        14
    ));
    assert!(!is_date_in_period(
        Local.ymd(2021, 3, 6),
        Local.ymd(2021, 3, 7),
        14
    ));
    assert!(is_date_in_period(
        Local.ymd(2021, 3, 20),
        Local.ymd(2021, 3, 7),
        14
    ));
    assert!(!is_date_in_period(
        Local.ymd(2021, 2, 21),
        Local.ymd(2021, 3, 7),
        14
    ));
}

fn preallocate_hours(
    period_start: Date<Local>,
    period_length: i64,
    work_days_input: &WorkDaysInput,
) -> Result<(HashMap<i64, f64>, f64)> {
    let mut offsets = HashMap::new();
    let mut total = 0.0;
    match work_days_input {
        WorkDaysInput::FromToWeekdays { from, to } => {
            let mut day = *from;
            let mut unallocated_offsets = HashSet::new();
            loop {
                for offset in
                    work_day_offsets(period_start, period_length, WorkDayInput::Weekday(day))
                {
                    unallocated_offsets.insert(offset);
                }
                if day == *to {
                    break;
                }
                day = day.succ();
            }
            for offset in 0..period_length {
                if !unallocated_offsets.contains(&offset) {
                    offsets.insert(offset, 0.0);
                }
            }
        }
        WorkDaysInput::FromToOffsets { from, to } => {
            if to < from {
                bail!("'To' work day offset can not be before 'from' work day offset");
            }
            for before_first_day_offset in 0..*from {
                offsets.insert(before_first_day_offset, 0.0);
            }
            for after_last_day_offset in (to + 1)..period_length {
                offsets.insert(after_last_day_offset, 0.0);
            }
        }
        WorkDaysInput::DayHours(work_day_hours) => {
            for (work_day, hours) in work_day_hours {
                for offset in work_day_offsets(period_start, period_length, *work_day) {
                    *offsets.entry(offset).or_insert(0.0) += hours;
                    total += hours;
                }
            }
        }
    }
    Ok((offsets, total))
}

#[test]
fn test_preallocate_hours() -> Result<()> {
    assert_eq!(
        preallocate_hours(
            Local.ymd(2021, 3, 14),
            13,
            &WorkDaysInput::FromToWeekdays {
                from: Weekday::Mon,
                to: Weekday::Fri
            }
        )?,
        (
            vec![(0, 0.0), (6, 0.0), (7, 0.0)].into_iter().collect(),
            0.0
        )
    );
    assert_eq!(
        preallocate_hours(
            Local.ymd(2021, 3, 14),
            7,
            &WorkDaysInput::FromToOffsets { from: 1, to: 5 }
        )?,
        (vec![(0, 0.0), (6, 0.0)].into_iter().collect(), 0.0)
    );
    assert!(preallocate_hours(
        Local.ymd(2021, 3, 14),
        7,
        &WorkDaysInput::FromToOffsets { from: 5, to: 1 }
    )
    .is_err());
    assert_eq!(
        preallocate_hours(
            Local.ymd(2021, 3, 14),
            13,
            &WorkDaysInput::DayHours(
                vec![
                    (WorkDayInput::Weekday(Weekday::Sun), 1.0),
                    (WorkDayInput::Weekday(Weekday::Sat), 2.5)
                ]
                .into_iter()
                .collect()
            )
        )?,
        (
            vec![(0, 1.0), (6, 2.5), (7, 1.0)].into_iter().collect(),
            4.5
        )
    );
    Ok(())
}

fn calculate_partial_period_hours_percent(
    now: DateTime<Local>,
    period_start: Date<Local>,
    period_length: i64,
    clients: &HashMap<String, ClientInput>,
    work_days_input: &WorkDaysInput,
) -> Result<(f64, i64)> {
    let (offset_preallocated_hours, total_preallocated_hours) =
        preallocate_hours(period_start, period_length, work_days_input)?;
    let today_offset = (now.date() - period_start).num_days();
    let total_expected_hours: f64 = clients.values().map(|v| v.expected_hours).sum();
    let mut partial_percent = 0.0;
    let mut last_work_day_offset = 0;
    info!("Daily hours for period starting: {}", period_start);
    for offset in 0..period_length {
        let hours = match offset_preallocated_hours.get(&offset) {
            Some(preallocated_hours) => *preallocated_hours,
            None => {
                (total_expected_hours - total_preallocated_hours)
                    / (period_length - offset_preallocated_hours.len() as i64) as f64
            }
        };
        info!(
            "  {}: {} ({:.1}%)",
            offset,
            hours,
            hours * 100.0 / total_expected_hours
        );
        if offset <= today_offset {
            partial_percent += hours / total_expected_hours;
        }
        if hours > 0.0 {
            last_work_day_offset = offset;
        }
    }
    debug!(
        "  partial_percent={} last_work_day_offset={} total_expected_hours={} today_offset={}",
        partial_percent, last_work_day_offset, total_expected_hours, today_offset
    );
    Ok((partial_percent, last_work_day_offset))
}

#[test]
fn test_calculate_partial_period_hours_percent() -> Result<()> {
    assert_eq!(
        calculate_partial_period_hours_percent(
            Local.ymd(2021, 3, 12).and_hms(12, 0, 0),
            Local.ymd(2021, 3, 7),
            10,
            &vec![
                (
                    "Client 1".to_string(),
                    ClientInput {
                        expected_hours: 30.0,
                        projects: None
                    }
                ),
                (
                    "Client 2".to_string(),
                    ClientInput {
                        expected_hours: 10.0,
                        projects: None
                    }
                )
            ]
            .into_iter()
            .collect(),
            &WorkDaysInput::DayHours(
                vec![
                    (WorkDayInput::Weekday(Weekday::Sun), 2.0),
                    (WorkDayInput::Weekday(Weekday::Mon), 3.0),
                    (WorkDayInput::Weekday(Weekday::Tue), 0.0)
                ]
                .into_iter()
                .collect()
            )
        )?,
        (0.6875, 8)
    );
    assert_eq!(
        calculate_partial_period_hours_percent(
            Local.ymd(2021, 3, 14).and_hms(12, 0, 0),
            Local.ymd(2021, 3, 7),
            7,
            &vec![(
                "Client".to_string(),
                ClientInput {
                    expected_hours: 40.0,
                    projects: None
                }
            )]
            .into_iter()
            .collect(),
            &WorkDaysInput::FromToWeekdays {
                from: Weekday::Mon,
                to: Weekday::Fri
            },
        )?,
        (1.0, 5)
    );
    Ok(())
}

#[derive(Clone, Debug)]
pub struct Processor {
    now: DateTime<Local>,
    period_bucket_durations: Vec<PeriodBucketDurations>,
    found_warning: bool,
}

impl Processor {
    pub fn new(now: DateTime<Local>) -> Processor {
        Processor {
            now,
            period_bucket_durations: Vec::new(),
            found_warning: false,
        }
    }

    fn initialize_period_client(
        &mut self,
        period_start: Date<Local>,
        period_length: i64,
        last_work_day_offset: i64,
        partial_percent: f64,
        client_name: String,
        client_input: ClientInput,
    ) {
        if let Some(projects) = client_input.projects {
            for (project_name, project_input) in projects {
                self.period_bucket_durations.push(PeriodBucketDurations {
                    period_start,
                    bucket: Bucket {
                        client: client_name.clone(),
                        project: Some(project_name),
                    },
                    period_length,
                    last_work_day_offset,
                    durations: Durations::expected(project_input.expected_hours, partial_percent),
                });
            }
        }
        self.period_bucket_durations.push(PeriodBucketDurations {
            period_start,
            bucket: Bucket {
                client: client_name,
                project: None,
            },
            period_length,
            last_work_day_offset,
            durations: Durations::expected(client_input.expected_hours, partial_percent),
        });
    }

    pub fn initialize(&mut self, input: Input) -> Result<()> {
        for period_input in input.periods {
            let period_start = Local.from_local_date(&period_input.start).unwrap();
            let defaults_input = &input.defaults;
            let period_length = period_input.length.unwrap_or_else(|| {
                defaults_input
                    .period_length
                    .unwrap_or(DEFAULT_PERIOD_LENGTH)
            });
            let work_days_input = period_input.work_days.as_ref().unwrap_or_else(|| {
                defaults_input
                    .work_days
                    .as_ref()
                    .unwrap_or(&DEFAULT_WORK_DAYS)
            });
            let (partial_percent, last_work_day_offset) = calculate_partial_period_hours_percent(
                self.now,
                period_start,
                period_length,
                &period_input.clients,
                work_days_input,
            )?;
            for (client_name, client_input) in period_input.clients {
                self.initialize_period_client(
                    period_start,
                    period_length,
                    last_work_day_offset,
                    partial_percent,
                    client_name,
                    client_input,
                );
            }
        }
        self.period_bucket_durations.sort();
        debug!(
            "initial period_bucket_durations: {:#?}",
            self.period_bucket_durations
        );
        Ok(())
    }

    fn get_time_entries(&self, toggl: &Toggl) -> Result<Vec<TimeEntry>> {
        let min_period_start = self
            .period_bucket_durations
            .iter()
            .map(|v| v.period_start)
            .min()
            .unwrap_or_else(|| self.now.date());
        debug!("min_period_start: {:#?}", min_period_start);
        let time_entries = toggl
            .get_time_entries_range(
                Some(min_period_start.and_hms(0, 0, 0).with_timezone(&Utc)),
                Some(self.now.with_timezone(&Utc)),
            )
            .context("Could not get time entries from Toggl")?;
        Ok(time_entries)
    }

    fn accumulate_time_entry(
        &mut self,
        start: DateTime<Local>,
        stop: DateTime<Local>,
        time_entry: &TimeEntry,
    ) -> Result<()> {
        let duration = stop - start;
        if let (Some(client), Some(project)) = (&time_entry.client, &time_entry.project) {
            let mut found_match_client_only = false;
            let mut found_match_project = false;
            for PeriodBucketDurations {
                period_start,
                bucket,
                period_length,
                last_work_day_offset: _,
                durations,
            } in &mut self.period_bucket_durations
            {
                if client.name == bucket.client
                    && (bucket.project.is_none() || Some(&project.name) == bucket.project.as_ref())
                    && is_date_in_period(start.date(), *period_start, *period_length)
                {
                    if bucket.project.is_none() {
                        if found_match_client_only {
                            bail!(
                                "Multiple expected clients/periods matched: {:?}",
                                &time_entry
                            );
                        } else {
                            found_match_client_only = true;
                        }
                    } else if found_match_project {
                        bail!(
                            "Multiple expected projects/periods matched: {:?}",
                            &time_entry
                        );
                    } else {
                        found_match_project = true;
                    }
                    durations.actual = durations.actual + duration;
                    if is_date_in_period(self.now.date(), *period_start, *period_length) {
                        durations.current_period_actual =
                            durations.current_period_actual + duration;
                    }
                    if start.date() == self.now.date() {
                        durations.today_actual = durations.today_actual + duration;
                    }
                }
            }
            if !found_match_client_only {
                warn!("no expected client/period matched: {:?}", &time_entry);
                self.found_warning = true;
            }
        } else {
            warn!(
                "time entry missing client and/or project: {:?}",
                &time_entry
            );
            self.found_warning = true;
        }
        Ok(())
    }

    fn accumulate_time_entries(&mut self, time_entries: &[TimeEntry]) -> Result<()> {
        //TODO: optimize nested loops
        for time_entry in time_entries {
            let time_entry_start = time_entry.start.with_timezone(&Local);
            let time_entry_stop = time_entry
                .stop
                .map(|v| v.with_timezone(&Local))
                .unwrap_or(self.now);
            let mut start = time_entry_start;
            while start.date() != time_entry_stop.date() {
                let stop = start.date().and_hms(0, 0, 0) + Duration::days(1);
                self.accumulate_time_entry(start, stop, &time_entry)?;
                start = stop;
            }
            self.accumulate_time_entry(start, time_entry_stop, &time_entry)?;
        }
        Ok(())
    }

    pub fn process(&mut self, strict: bool, toggl: &Toggl) -> Result<()> {
        let time_entries = self.get_time_entries(toggl)?;
        debug!("time_entries: {:#?}", time_entries);
        self.accumulate_time_entries(&time_entries)?;
        if strict && self.found_warning {
            bail!("Strict mode enabled (see warning(s) above)");
        }
        debug!(
            "processed period_bucket_durations: {:#?}",
            self.period_bucket_durations
        );
        Ok(())
    }

    pub fn print_table(&self) {
        let mut table = Table::new();
        table.set_format(
            prettytable::format::FormatBuilder::new()
                .column_separator(' ')
                .build(),
        );
        table.add_row(row![
            b->"PERIOD",
            " ",
            b->"CLIENT",
            b->"PROJECT",
            " ",
            br->"EXPECT",
            // br->"part",
            br->"ACTUAL",
            // br-> "period ",
            // br->"tod",
            br->"DIFFERENCE",
            // , br->"tod"
        ]);
        for PeriodBucketDurations {
            period_start,
            bucket: Bucket { client, project },
            period_length: _,
            last_work_day_offset: _,
            durations,
        } in &self.period_bucket_durations
        {
            table.add_row(Row::new(vec![
                Cell::new(&period_start.naive_local().to_string()),
                Cell::new(""),
                Cell::new(&client),
                Cell::new(project.as_ref().map(String::as_str).unwrap_or("")),
                Cell::new(""),
                duration_hours_cell(durations.expected),
                // duration_hours_cell(durations.partial_expected),
                duration_hours_cell(durations.actual),
                // duration_hours_cell(durations.current_period_actual),
                // duration_hours_cell(durations.today_actual),
                color_duration_hours_cell(durations.remaining()),
                // color_duration_hours_cell(durations.today_remaining()),
            ]));
        }
        table.printstd();
        println!();
    }

    pub fn calculate_totals(&self) -> HashMap<Bucket, TotalDurations> {
        let mut result: HashMap<Bucket, TotalDurations> = HashMap::new();
        for source in &self.period_bucket_durations {
            let source_end_work_date =
                source.period_start + Duration::days(source.last_work_day_offset + 1);
            if let Some(entry) = result.get_mut(&source.bucket) {
                if source_end_work_date > entry.end_work_date {
                    entry.end_work_date = source_end_work_date;
                }
                entry.durations = entry.durations + source.durations;
            } else {
                result.insert(
                    source.bucket.clone(),
                    TotalDurations {
                        end_work_date: source_end_work_date,
                        durations: source.durations,
                    },
                );
            }
        }
        debug!("total_bucket_durations: {:#?}", result);
        result
    }
}
