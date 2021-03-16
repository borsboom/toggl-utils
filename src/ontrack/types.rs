use chrono::prelude::*;
use chrono::Duration;
use derive_more::{Add, Sub};
use serde::Deserialize;
use std::cmp::Ordering;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq)]
#[serde(rename_all = "kebab-case", untagged, deny_unknown_fields)]
pub enum WorkDayInput {
    Weekday(Weekday),
    Offset(i64),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "kebab-case", untagged, deny_unknown_fields)]
pub enum WorkDaysInput {
    FromToWeekdays { from: Weekday, to: Weekday },
    FromToOffsets { from: i64, to: i64 },
    DayHours(HashMap<WorkDayInput, f64>),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ProjectInput {
    pub expected_hours: f64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ClientInput {
    pub expected_hours: f64,
    pub projects: Option<HashMap<String, ProjectInput>>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct PeriodInput {
    pub start: NaiveDate,
    pub length: Option<i64>,
    pub work_days: Option<WorkDaysInput>,
    pub clients: HashMap<String, ClientInput>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct DefaultsInput {
    pub period_length: Option<i64>,
    pub work_days: Option<WorkDaysInput>,
}

impl Default for DefaultsInput {
    fn default() -> Self {
        DefaultsInput {
            period_length: None,
            work_days: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Input {
    #[serde(default)]
    pub defaults: DefaultsInput,
    pub periods: Vec<PeriodInput>,
}

#[derive(Add, Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Sub)]
pub struct Durations {
    pub expected: Duration,
    pub partial_expected: Duration,
    pub actual: Duration,
    pub today_actual: Duration,
    pub current_period_actual: Duration,
}

impl Durations {
    pub fn zero() -> Durations {
        Durations {
            expected: Duration::zero(),
            partial_expected: Duration::zero(),
            actual: Duration::zero(),
            today_actual: Duration::zero(),
            current_period_actual: Duration::zero(),
        }
    }

    pub fn expected(expected_hours: f64, partial_percent: f64) -> Durations {
        let mut result = Durations::zero();
        result.expected = Duration::seconds((expected_hours * 3600.0).round() as i64);
        result.partial_expected = Duration::seconds(i64::max(
            (expected_hours * 3600.0 * partial_percent).round() as i64,
            0,
        ));
        result
    }

    pub fn current_period_expected(&self) -> Duration {
        self.current_period_actual + self.expected - self.actual
    }

    pub fn today_expected(&self) -> Duration {
        self.today_actual + self.partial_expected - self.actual
    }

    pub fn remaining(&self) -> Duration {
        self.actual - self.expected
    }

    pub fn partial_remaining(&self) -> Duration {
        self.actual - self.partial_expected
    }

    pub fn daily_average_remaining(&self, whole_days_until_end_work: i64) -> Option<Duration> {
        if whole_days_until_end_work > 0 {
            Some((self.expected - self.actual) / whole_days_until_end_work as i32)
        } else {
            None
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Bucket {
    pub client: String,
    pub project: Option<String>,
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
pub struct PeriodBucketDurations {
    pub period_start: Date<Local>,
    pub bucket: Bucket,
    pub period_length: i64,
    pub last_work_day_offset: i64,
    pub durations: Durations,
}

#[derive(Clone, Debug)]
pub struct TotalDurations {
    pub end_work_date: Date<Local>,
    pub durations: Durations,
}
