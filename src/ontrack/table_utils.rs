use chrono::Duration;
use prettytable::{color, format::Alignment, Attr, Cell};

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
    match (color, seconds) {
        (true, v) if v < 0 => cell.style(Attr::ForegroundColor(color::RED)),
        (true, v) if v > 0 => cell.style(Attr::ForegroundColor(color::GREEN)),
        _ => (),
    }
    cell
}

pub fn duration_hours_cell(duration: Duration) -> Cell {
    duration_hours_cell_(duration, false)
}

pub fn color_duration_hours_cell(duration: Duration) -> Cell {
    duration_hours_cell_(duration, true)
}
