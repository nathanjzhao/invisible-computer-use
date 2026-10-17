use crate::{Result, invalid};
use chrono::{
    DateTime, Datelike, Duration, LocalResult, NaiveDate, NaiveDateTime, Offset, TimeZone,
    Timelike, Utc,
};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

pub const APPLE_EPOCH_SECONDS: i64 = 978_307_200;
pub const BILLION: i64 = 1_000_000_000;
pub fn parse(value: &str) -> Result<i64> {
    let date = DateTime::parse_from_rfc3339(value)
        .map_err(|_| invalid("Dates must be ISO-8601 date-times with a timezone"))?;
    date.timestamp_nanos_opt()
        .and_then(|n| n.checked_sub(APPLE_EPOCH_SECONDS * BILLION))
        .ok_or_else(|| invalid("Date is outside the supported timestamp range"))
}
pub fn now() -> i64 {
    Utc::now().timestamp_nanos_opt().unwrap() - APPLE_EPOCH_SECONDS * BILLION
}
pub fn datetime(value: i64) -> Result<DateTime<Utc>> {
    let unix = value
        .checked_add(APPLE_EPOCH_SECONDS * BILLION)
        .ok_or_else(|| invalid("Timestamp overflow"))?;
    Ok(DateTime::from_timestamp_nanos(unix))
}
pub fn iso(value: i64) -> Result<String> {
    Ok(datetime(value)?.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true))
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Interval {
    #[default]
    Total,
    Hour,
    Day,
    Week,
    Month,
    Year,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Bucket {
    #[serde(rename = "from")]
    pub from: String,
    #[serde(rename = "to")]
    pub to: String,
    #[serde(skip)]
    pub lower: i64,
    #[serde(skip)]
    pub upper: i64,
}
fn local(tz: Tz, naive: NaiveDateTime, preferred_offset: Option<i32>) -> Result<DateTime<Tz>> {
    let mut value = naive;
    // Some zones skip midnight or even an entire calendar day.
    for _ in 0..=1440 {
        match tz.from_local_datetime(&value) {
            LocalResult::Single(d) => return Ok(d),
            LocalResult::Ambiguous(a, b) => {
                return Ok(
                    if preferred_offset == Some(b.offset().fix().local_minus_utc()) {
                        b
                    } else {
                        a
                    },
                );
            }
            LocalResult::None => {
                value = value
                    .checked_add_signed(Duration::minutes(1))
                    .ok_or_else(|| invalid("Calendar boundary overflow"))?
            }
        }
    }
    Err(invalid("Could not resolve the calendar boundary"))
}
fn apple(d: DateTime<Tz>) -> Result<i64> {
    d.timestamp_nanos_opt()
        .and_then(|n| n.checked_sub(APPLE_EPOCH_SECONDS * BILLION))
        .ok_or_else(|| invalid("Calendar boundary overflow"))
}
pub fn buckets(from: i64, to: i64, interval: Interval, zone: &str) -> Result<Vec<Bucket>> {
    if from > to {
        return Err(invalid("from must not be later than to"));
    }
    let tz: Tz = zone
        .parse()
        .map_err(|_| invalid("Unknown IANA time zone"))?;
    if from == to {
        return Ok(vec![]);
    }
    if interval == Interval::Total {
        return Ok(vec![Bucket {
            from: iso(from)?,
            to: iso(to)?,
            lower: from,
            upper: to,
        }]);
    }
    let date = datetime(from)?.with_timezone(&tz);
    let day = date.date_naive();
    let floor = match interval {
        Interval::Hour => day.and_hms_opt(date.hour(), 0, 0).unwrap(),
        Interval::Day => day.and_hms_opt(0, 0, 0).unwrap(),
        Interval::Week => (day - Duration::days(day.weekday().num_days_from_monday() as i64))
            .and_hms_opt(0, 0, 0)
            .unwrap(),
        Interval::Month => NaiveDate::from_ymd_opt(day.year(), day.month(), 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap(),
        Interval::Year => NaiveDate::from_ymd_opt(day.year(), 1, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap(),
        Interval::Total => unreachable!(),
    };
    let mut start = local(tz, floor, Some(date.offset().fix().local_minus_utc()))?;
    let mut out = Vec::new();
    while apple(start)? < to {
        if out.len() >= 10_000 {
            return Err(invalid(
                "The requested range has too many buckets; use a larger interval",
            ));
        }
        let next = if interval == Interval::Hour {
            start + Duration::hours(1)
        } else {
            let d = start.date_naive();
            let d = match interval {
                Interval::Day => d.checked_add_signed(Duration::days(1)),
                Interval::Week => d.checked_add_signed(Duration::days(7)),
                Interval::Month => d.checked_add_months(chrono::Months::new(1)),
                Interval::Year => d.checked_add_months(chrono::Months::new(12)),
                _ => unreachable!(),
            }
            .ok_or_else(|| invalid("Calendar range overflow"))?;
            local(tz, d.and_hms_opt(0, 0, 0).unwrap(), None)?
        };
        let lower = apple(start)?.max(from);
        let upper = apple(next)?.min(to);
        if upper <= lower {
            return Err(invalid("Calendar buckets did not advance"));
        }
        out.push(Bucket {
            from: iso(lower)?,
            to: iso(upper)?,
            lower,
            upper,
        });
        start = next;
    }
    Ok(out)
}
