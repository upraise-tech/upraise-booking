//! Instance-wide defaults for new users and new event types.
//!
//! Two optional environment variables let an operator change what a new
//! account starts with:
//!
//! - `CALRS_DEFAULT_TIMEZONE`: an IANA name (`Africa/Cairo`). Given to every new
//!   user row and used wherever a host or event type has no valid timezone of
//!   its own. Unset or empty means `UTC`; an invalid name also means `UTC`, with
//!   one warning in the log.
//! - `CALRS_DEFAULT_AVAILABILITY`: the per-day schedule string the event-type
//!   form submits (`day:HH:MM-HH:MM[,HH:MM-HH:MM];...`, day 0 = Sunday), for
//!   example `0:10:00-17:00;1:10:00-17:00;2:10:00-17:00;3:10:00-17:00;4:10:00-17:00`.
//!   Seeds a user's default availability and is the last-resort schedule for a
//!   new event type. Unset, empty or invalid means Monday to Friday
//!   09:00-17:00; an invalid value warns once.
//!
//! Neither variable touches existing rows. Both are read on every call (env
//! lookups are cheap), so a test can change them under `ENV_LOCK`.

use chrono::NaiveTime;
use chrono_tz::Tz;
use std::collections::BTreeMap;

/// Per-day availability: day of week (0 = Sunday) to `(start, end)` windows.
pub type Schedule = BTreeMap<i32, Vec<(String, String)>>;

pub const TIMEZONE_VAR: &str = "CALRS_DEFAULT_TIMEZONE";
pub const AVAILABILITY_VAR: &str = "CALRS_DEFAULT_AVAILABILITY";

/// The timezone used when nothing is configured.
pub const FALLBACK_TIMEZONE: &str = "UTC";

/// Monday to Friday, 09:00-17:00: the schedule used when nothing is configured.
pub const FALLBACK_AVAILABILITY: &str =
    "1:09:00-17:00;2:09:00-17:00;3:09:00-17:00;4:09:00-17:00;5:09:00-17:00";

/// Parse a timezone value. `None` for an empty or unknown name.
pub fn parse_timezone(raw: &str) -> Option<Tz> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    raw.parse::<Tz>().ok()
}

/// Parse a schedule string strictly. Every segment must be a day 0-6 followed
/// by one or more `HH:MM-HH:MM` windows whose start is before its end; any
/// malformed part rejects the whole value, so a typo falls back to the default
/// instead of silently dropping a day. `None` for an empty or invalid value.
pub fn parse_availability(raw: &str) -> Option<Schedule> {
    let mut out = Schedule::new();
    for segment in raw.split(';') {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }
        let (day, windows) = segment.split_once(':')?;
        let day: i32 = day.trim().parse().ok().filter(|d| (0..=6).contains(d))?;
        for window in windows.split(',') {
            let (start, end) = window.trim().split_once('-')?;
            let start = NaiveTime::parse_from_str(start.trim(), "%H:%M").ok()?;
            let end = NaiveTime::parse_from_str(end.trim(), "%H:%M").ok()?;
            if start >= end {
                return None;
            }
            out.entry(day).or_default().push((
                start.format("%H:%M").to_string(),
                end.format("%H:%M").to_string(),
            ));
        }
    }
    for windows in out.values_mut() {
        windows.sort();
        windows.dedup();
    }
    (!out.is_empty()).then_some(out)
}

/// Serialise a schedule back to the `day:start-end,start-end;...` form.
pub fn schedule_string(schedule: &Schedule) -> String {
    schedule
        .iter()
        .map(|(day, windows)| {
            let ws: Vec<String> = windows.iter().map(|(s, e)| format!("{s}-{e}")).collect();
            format!("{day}:{}", ws.join(","))
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// Days of a schedule as the legacy comma-separated form field (`"1,2,3,4,5"`).
pub fn days_csv(schedule: &Schedule) -> String {
    schedule
        .keys()
        .map(|d| d.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// The first window of a schedule, for the legacy single start/end form fields.
pub fn first_window(schedule: &Schedule) -> (String, String) {
    schedule
        .values()
        .next()
        .and_then(|w| w.first().cloned())
        .unwrap_or_else(|| ("09:00".to_string(), "17:00".to_string()))
}

/// A one-line English summary for CLI output: `Sun 10:00-17:00, Mon 10:00-17:00`.
pub fn describe(schedule: &Schedule) -> String {
    const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    schedule
        .iter()
        .map(|(day, windows)| {
            let ws: Vec<String> = windows.iter().map(|(s, e)| format!("{s}-{e}")).collect();
            format!("{} {}", DAYS[*day as usize], ws.join(","))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn fallback_timezone() -> Tz {
    Tz::UTC
}

fn fallback_availability() -> Schedule {
    parse_availability(FALLBACK_AVAILABILITY).expect("fallback schedule parses")
}

/// Resolve a raw environment value to a timezone, reporting whether it was
/// rejected (set but not a valid IANA name).
fn resolve_timezone(raw: Option<&str>) -> (Tz, bool) {
    match raw.map(str::trim).filter(|s| !s.is_empty()) {
        None => (fallback_timezone(), false),
        Some(s) => match parse_timezone(s) {
            Some(tz) => (tz, false),
            None => (fallback_timezone(), true),
        },
    }
}

/// Resolve a raw environment value to a schedule, reporting whether it was
/// rejected (set but malformed).
fn resolve_availability(raw: Option<&str>) -> (Schedule, bool) {
    match raw.map(str::trim).filter(|s| !s.is_empty()) {
        None => (fallback_availability(), false),
        Some(s) => match parse_availability(s) {
            Some(schedule) => (schedule, false),
            None => (fallback_availability(), true),
        },
    }
}

/// The instance default timezone (`CALRS_DEFAULT_TIMEZONE`, else UTC).
pub fn timezone() -> Tz {
    let raw = std::env::var(TIMEZONE_VAR).ok();
    let (tz, rejected) = resolve_timezone(raw.as_deref());
    if rejected {
        static WARNED: std::sync::Once = std::sync::Once::new();
        WARNED.call_once(|| {
            tracing::warn!(
                var = TIMEZONE_VAR,
                value = raw.as_deref().unwrap_or(""),
                "not a valid IANA timezone, using UTC"
            );
        });
    }
    tz
}

/// The instance default timezone as the string stored in `users.timezone`.
pub fn timezone_name() -> String {
    timezone().name().to_string()
}

/// The instance default availability (`CALRS_DEFAULT_AVAILABILITY`, else
/// Monday to Friday 09:00-17:00).
pub fn availability() -> Schedule {
    let raw = std::env::var(AVAILABILITY_VAR).ok();
    let (schedule, rejected) = resolve_availability(raw.as_deref());
    if rejected {
        static WARNED: std::sync::Once = std::sync::Once::new();
        WARNED.call_once(|| {
            tracing::warn!(
                var = AVAILABILITY_VAR,
                value = raw.as_deref().unwrap_or(""),
                "not a valid availability schedule, using Mon-Fri 09:00-17:00"
            );
        });
    }
    schedule
}

/// The instance default availability as a schedule string.
pub fn availability_string() -> String {
    schedule_string(&availability())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(pairs: &[(i32, &[(&str, &str)])]) -> Schedule {
        pairs
            .iter()
            .map(|(d, ws)| {
                (
                    *d,
                    ws.iter()
                        .map(|(a, b)| (a.to_string(), b.to_string()))
                        .collect(),
                )
            })
            .collect()
    }

    #[test]
    fn fallback_is_weekdays_nine_to_five() {
        let (schedule, rejected) = resolve_availability(None);
        assert!(!rejected);
        assert_eq!(days_csv(&schedule), "1,2,3,4,5");
        assert_eq!(first_window(&schedule), ("09:00".into(), "17:00".into()));
        assert_eq!(schedule_string(&schedule), FALLBACK_AVAILABILITY);
    }

    #[test]
    fn sunday_to_thursday_parses() {
        let raw = "0:10:00-17:00;1:10:00-17:00;2:10:00-17:00;3:10:00-17:00;4:10:00-17:00";
        let (schedule, rejected) = resolve_availability(Some(raw));
        assert!(!rejected);
        assert_eq!(days_csv(&schedule), "0,1,2,3,4");
        assert_eq!(first_window(&schedule), ("10:00".into(), "17:00".into()));
        assert_eq!(schedule_string(&schedule), raw);
        assert_eq!(
            describe(&schedule),
            "Sun 10:00-17:00, Mon 10:00-17:00, Tue 10:00-17:00, Wed 10:00-17:00, Thu 10:00-17:00"
        );
    }

    #[test]
    fn split_windows_and_whitespace_are_accepted() {
        let parsed = parse_availability(" 1: 13:00-17:00 , 09:00-12:00 ; ;6:9:30-11:00; ").unwrap();
        assert_eq!(
            parsed,
            s(&[
                (1, &[("09:00", "12:00"), ("13:00", "17:00")]),
                (6, &[("09:30", "11:00")]),
            ])
        );
    }

    #[test]
    fn empty_value_falls_back_without_warning() {
        for raw in ["", "   ", ";;"] {
            let (schedule, rejected) = resolve_availability(Some(raw));
            assert_eq!(schedule_string(&schedule), FALLBACK_AVAILABILITY, "{raw:?}");
            // ";;" is non-empty text with no segments: treated as invalid.
            assert_eq!(rejected, raw == ";;", "{raw:?}");
        }
    }

    #[test]
    fn malformed_values_fall_back_with_warning() {
        for raw in [
            "7:09:00-17:00",
            "x:09:00-17:00",
            "1:0900-1700",
            "1:09:00",
            "1:17:00-09:00",
            "1:09:00-09:00",
            "1:25:00-26:00",
            "1:09:00-17:00;2",
            "1:09:00-17:00;2:",
            "1:09:00-12:00,",
        ] {
            let (schedule, rejected) = resolve_availability(Some(raw));
            assert!(rejected, "{raw:?} should be rejected");
            assert_eq!(schedule_string(&schedule), FALLBACK_AVAILABILITY, "{raw:?}");
        }
    }

    #[test]
    fn timezone_resolution() {
        assert_eq!(resolve_timezone(None), (Tz::UTC, false));
        assert_eq!(resolve_timezone(Some("")), (Tz::UTC, false));
        assert_eq!(resolve_timezone(Some("  ")), (Tz::UTC, false));
        assert_eq!(
            resolve_timezone(Some(" Africa/Cairo ")),
            (chrono_tz::Africa::Cairo, false)
        );
        assert_eq!(resolve_timezone(Some("Mars/Olympus")), (Tz::UTC, true));
        assert_eq!(resolve_timezone(Some("Cairo")), (Tz::UTC, true));
    }

    /// The env-reading wrappers, under the shared lock so no other test sees
    /// the override.
    #[tokio::test]
    async fn env_overrides_and_falls_back() {
        let _lock = crate::test_support::ENV_LOCK.lock().await;
        let _guard = crate::test_support::EnvGuard::new();

        assert_eq!(timezone_name(), "UTC");
        assert_eq!(availability_string(), FALLBACK_AVAILABILITY);

        std::env::set_var(TIMEZONE_VAR, "Africa/Cairo");
        std::env::set_var(AVAILABILITY_VAR, "0:10:00-17:00;4:10:00-17:00");
        assert_eq!(timezone_name(), "Africa/Cairo");
        assert_eq!(availability_string(), "0:10:00-17:00;4:10:00-17:00");

        std::env::set_var(TIMEZONE_VAR, "Not/AZone");
        std::env::set_var(AVAILABILITY_VAR, "garbage");
        assert_eq!(timezone_name(), "UTC");
        assert_eq!(availability_string(), FALLBACK_AVAILABILITY);
    }
}
