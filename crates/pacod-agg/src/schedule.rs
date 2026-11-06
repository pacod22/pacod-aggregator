//! Schedules: five-field cron expressions read as standard cron reads them, in a time zone.
//!
//! Copied from pacod-gw (commit 50fe4b7, `crates/pacod-gw-core/src/model/pricejob.rs`).

use chrono::{DateTime, Utc};

/// The days of the week as the `cron` crate names them, Sunday first.
const WEEKDAYS: [&str; 7] = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];

/// A five-field cron expression (minute, hour, day of month, month, day of week) as standard
/// cron reads it, turned into what the `cron` crate reads. That crate wants a seconds field
/// first, numbers the days of the week from 1 (Sunday) to 7, and runs only on days that match
/// both day fields. Standard cron numbers them from 0 (Sunday) to 7 (Sunday again) and runs on
/// days that match either field. The day of the week is therefore passed on as names, and an
/// expression that restricts both day fields is refused rather than read differently.
pub fn parse_cron(expr: &str) -> Result<cron::Schedule, String> {
    use std::str::FromStr;
    let fields: Vec<&str> = expr.split_whitespace().collect();
    let [minute, hour, day_of_month, month, day_of_week] = fields[..] else {
        return Err("schedule must have five fields: minute, hour, day of month, month, day of week".into());
    };
    let day_of_week = match weekdays(day_of_week).map_err(|e| format!("schedule: {e}"))? {
        None => "*".to_owned(),
        Some(_) if day_of_month != "*" => {
            return Err("schedule restricts both the day of the month and the day of the week, which standard cron \
                        reads as either of them: set one of the two to *"
                .into());
        }
        Some(days) => WEEKDAYS.iter().zip(days).filter(|(_, on)| *on).map(|(d, _)| *d).collect::<Vec<_>>().join(","),
    };
    cron::Schedule::from_str(&format!("0 {minute} {hour} {day_of_month} {month} {day_of_week}"))
        .map_err(|e| format!("schedule is not a cron expression: {e}"))
}

/// The days a standard cron day-of-week field names, Sunday first. `None` where it names every
/// day: such a field restricts nothing.
fn weekdays(field: &str) -> Result<Option<[bool; 7]>, String> {
    let number = |s: &str| -> Result<usize, String> {
        match s.parse::<usize>() {
            Ok(n) if n <= 7 => Ok(n),
            Ok(_) => Err(format!("{s} is not a day of the week (0 to 7, Sunday is 0 and 7)")),
            Err(_) => WEEKDAYS
                .iter()
                .position(|d| d.eq_ignore_ascii_case(s))
                .ok_or_else(|| format!("\"{s}\" is not a day of the week (0 to 7, or sun, mon, … sat)")),
        }
    };
    let mut days = [false; 7];
    for item in field.split(',') {
        let (range, step) = match item.split_once('/') {
            Some((range, step)) => {
                let step = step.parse::<usize>().ok().filter(|s| *s > 0);
                (range, step.ok_or_else(|| format!("\"{item}\" has no valid step"))?)
            }
            None => (item, 1),
        };
        let (from, to) = match range.split_once('-') {
            _ if range == "*" => (0, 6),
            Some((from, to)) => (number(from)?, number(to)?),
            // A single day with a step runs from that day to the end of the week.
            None if step > 1 => (number(range)?, 7),
            None => (number(range)?, number(range)?),
        };
        if from > to {
            return Err(format!("the range \"{range}\" goes backwards; write it as two ranges"));
        }
        for day in (from..=to).step_by(step) {
            days[day % 7] = true;
        }
    }
    Ok(days.iter().any(|on| !on).then_some(days))
}

/// Whether a run on `schedule`, read in `time_zone`, is due at `now`, given when the job last
/// started. A job that never ran is due; a schedule or time zone that cannot be read never is.
pub fn due_by(schedule: &str, time_zone: &str, last_started: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    let Some(last) = last_started else { return true };
    next_after(schedule, time_zone, last).is_some_and(|next| next <= now)
}

/// The first start of `schedule`, read in `time_zone`, after `from`.
pub fn next_after(schedule: &str, time_zone: &str, from: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let tz = time_zone.parse::<chrono_tz::Tz>().ok()?;
    parse_cron(schedule).ok()?.after(&from.with_timezone(&tz)).next().map(|t| t.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, TimeZone, Utc};

    fn days_of(schedule: &str) -> Vec<String> {
        let mut from = Utc.with_ymd_and_hms(2026, 11, 7, 12, 0, 0).unwrap();
        let mut days = Vec::new();
        while let Some(next) = next_after(schedule, "UTC", from).filter(|t| t.day() <= 14) {
            days.push(next.format("%a").to_string());
            from = next;
        }
        days
    }

    #[test]
    fn days_of_the_week_are_numbered_as_standard_cron_numbers_them() {
        assert_eq!(days_of("0 3 * * 1-5"), ["Mon", "Tue", "Wed", "Thu", "Fri"]);
        assert_eq!(days_of("0 3 * * mon-fri"), ["Mon", "Tue", "Wed", "Thu", "Fri"]);
        for sunday in ["0", "7", "sun", "SUN"] {
            assert_eq!(days_of(&format!("0 3 * * {sunday}")), ["Sun"], "{sunday}");
        }
        assert_eq!(days_of("0 3 * * 5-7"), ["Sun", "Fri", "Sat"]);
        assert_eq!(days_of("0 3 * * */2"), ["Sun", "Tue", "Thu", "Sat"]);
        assert_eq!(days_of("0 3 * * 1,3"), ["Mon", "Wed"]);
        assert_eq!(days_of("0 3 * * 1/2"), ["Sun", "Mon", "Wed", "Fri"]);
        assert_eq!(days_of("0 3 * * *").len(), 7);
        // A week from Monday 9 November: the weekly default runs on that Monday.
        let saturday = Utc.with_ymd_and_hms(2026, 11, 7, 12, 0, 0).unwrap();
        assert_eq!(
            next_after("41 4 * * 1", "UTC", saturday),
            Some(Utc.with_ymd_and_hms(2026, 11, 9, 4, 41, 0).unwrap())
        );
    }

    #[test]
    fn a_schedule_restricts_one_of_the_day_fields() {
        assert!(parse_cron("0 3 1 * 1").is_err(), "standard cron reads this as the 1st or a Monday");
        assert!(parse_cron("0 3 1 * *").is_ok());
        // Every day of the week restricts nothing.
        assert!(parse_cron("0 3 1 * 0-6").is_ok());
        for bad in ["0 3 * * 8", "0 3 * * fri-mon", "0 3 * * 1/0", "0 3 * * someday", "0 3 * * 1,,2"] {
            assert!(parse_cron(bad).is_err(), "{bad}");
        }
    }
}
