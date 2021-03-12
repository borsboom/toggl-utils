# toggl-ontrack

This lightweight command-line tool helps you keep your work hours on track.
It's helpful if you have many clients/projects and an unpredictable work
schedule. It requires you to track your actual work hours using [Toggl
Track](https://toggl.com/track/) (the free plan works fine).

In a nutshell, you tell the tool how many hours of work are required for each
client (and, optionally, project) every week, and it helps keep you on-track
throughout the week by dividing those hours between work days, using your Toggl
time entries to tell you how many hours you are over/under every day and week.
It also rolls over any remaining hours to the next day/week, to compensate
automatically for any over/under hours.

## Setup

### Installation

1. Install Rust toolchain: [follow instructions](https://www.rust-lang.org/tools/install)
2. Clone this repo: `git clone https://github.com/borsboom/toggl-utils.git`
3. Change to the cloned directory: `cd toggl-utils`
4. Build and install the binary: `cargo install --path . --bin toggl-ontrack`

### Configuration

Set the `TOGGL_API_TOKEN` environment variable to your Toggl API token, which
is needed in order to read your time entries from Toggl.  It can also be passed
in using the `--api-token` argument.  You can create or retrieve the API token
from [your Toggl Track profile](https://track.toggl.com/profile) after signing
in (toward the bottom of the page).

Note that environment variables will also be read from a `.env` file.

## Input

The input is specified as a YAML file named, by default, `toggl-ontrack.yaml`
(you can override this name with the `--input-file` argument or the
`TOGGL_ONTRACK_FILE` environment variable).

### Weeks

This file must contain a `weeks` list with entries for each week.  Within each
week, the number of expected work hours for each client and, optionally,
project is provided.

```yaml
weeks:
- start: 2021-02-21
  clients:
    "Client 1":
      expected-hours: 15
    "Client 2":
      expected-hours: 20
- start: 2021-02-28
  clients:
    "Client 1":
      expected-hours: 15
    "Client 2":
      expected-hours: 20
      projects:
        "Project 1":
          expected-hours: 5
        "Project 2":
          expected-hours: 8
```

Projects are optional.  All client hours, reglardless of project, are tracked
for the client, but specifying projects lets you also keep track of specific
projects within the client.

Weeks may start on any day of the week.  By default, work days are Monday
through Friday, but you may specify which days of the week are work days using
`first-work-day` and `last-work-day`.  For example:

```yaml
weeks:
- start: 2021-02-28
  first-work-day: sun
  last-work-day: thu
  clients:
    "Client 1":
      expected-hours: 15
    "Client 2":
      expected-hours: 20
- …
```

It's also to use the `length` field to change the length of a week to something
other than the default of seven days.  For example, if you want to move to
starting weeks on Monday instead of Sunday, you would make the current week
eight days long and then have the next week start on Monday).  In this case,
set use the `length` field.  For example:

```yaml
weeks:
- start: 2021-02-28
  length: 8
  clients:
    …
```

Weeks that are longer than seven days may have more than one occurrence of a
weekday.  Since that makes the `first-work-day` and `last-work-day` potentially
ambiguous, you may also specify these as _offsets_ from the first day of the
week.  For example, a week that starts on Sunday and has a length of eight days
will have two Sundays: both the first and the last day of the week.  In this
case `sun` would refer to the _first_ sunday only.  You could instead use `0`,
which means zero days from the start of the week, or the first Sunday.  `1`
would be Monday.  To refer to the _last_ sunday of the week, you would use `7`.
For example:

```yaml
weeks:
- start: 2021-02-28
  first-work-day: 1 # this is Monday, since Feb 28th is a Sunday
  last-work-day: 5  # this is Friday
  clients:
    …
```

### Defaults

You can set the default work days, which apply to all weeks that don't override
them, using an optional `defaults` section.  For example:

```yaml
defaults:
  first-work-day: sun
  last-work-day: thu
weeks:
- …
```

## Run

Run the tool using `toggl-ontrack`.

Run `toggl-ontrack --help` for information about additional command-line
options.

## Output

This prints a table with expected, actual and remaining hours, for both the
current week and the current day.  For example:

```
                          THIS WEEK                TODAY
CLIENT    PROJECT  expect actual remain   expect actual remain   AVG.R
FirstCli            21:36  13:38  -7:57     0:35   2:37   2:02    3:58
SecondCli ProjA     10:00   1:01  -8:58     4:25   0:27  -3:58    4:29
SecondCli ProjB      5:00   2:14  -2:45     0:15   0:00  -0:15    1:22
SecondCli           14:58   4:45 -10:13     4:07   1:23  -2:43    5:06
TOTAL:              36:35  18:24 -18:10     4:42   4:01  -0:40    9:05
```

Some notes about the fields.

* `expect`: the number of hours expected to work this week or today.  Note that
  the "this week" value may be different than the expected hours specified for
  the week in the input file because previous weeks are "roller over."  In
  other words, if you worked too few hours last week, then this week's expected
  hours will be higher.  If you worked extra last week, this week's expected
  hours will be lower.

* `actual`: how many hours you have actually worked for a client/project this
  week/today.

* `remain`: the difference between the actual and expected hours.  If negative
  (which will display red if your console supports it color) you have more
  hours to work.  If positive (which will display green if your console
  supports color) you have attained or exceeded the expected hours.

* `AVG.R`: the average number of hours you would have to work every day for the
  rest of the week if you stopped working right now.

If there are project specified for a client, the client hours _include_ the
project hours (that is, they are the _total_ hours for that client, for all
projects).
