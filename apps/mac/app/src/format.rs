//! Small text helpers shared by every screen: relative times, hosts and sizes.
#![allow(dead_code)] // Callers land with the parity work; drop this once each helper is used.

use time::format_description::well_known::Rfc3339;
use time::{Date, OffsetDateTime, PrimitiveDateTime, macros::format_description};

/// Parses the timestamps the engine and server hand back: RFC 3339, or SQLite's
/// `2026-11-05 10:00:00` (taken as UTC).
pub fn parse_time(s: &str) -> Option<OffsetDateTime> {
    let s = s.trim();
    if let Ok(t) = OffsetDateTime::parse(s, &Rfc3339) {
        return Some(t);
    }
    let fmt = format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");
    let head = s.get(..19).unwrap_or(s);
    PrimitiveDateTime::parse(head, fmt).ok().map(|t| t.assume_utc())
}

/// "just now", "3 minutes ago", "yesterday", "4 days ago", or "Nov 5, 2026" past a week.
pub fn relative_time(iso: &str) -> String {
    relative_to(iso, OffsetDateTime::now_utc())
}

pub fn relative_to(iso: &str, now: OffsetDateTime) -> String {
    let Some(t) = parse_time(iso) else { return String::new() };
    let secs = (now - t).whole_seconds();
    let ahead = secs < 0;
    let abs = secs.unsigned_abs();
    let unit = |n: u64, one: &str, many: &str| {
        let word = if n == 1 { one } else { many };
        if ahead { format!("in {n} {word}") } else { format!("{n} {word} ago") }
    };
    if abs < 45 {
        "just now".into()
    } else if abs < 3600 {
        unit(((abs as f64) / 60.0).round().max(1.0) as u64, "minute", "minutes")
    } else if abs < 86_400 {
        unit(((abs as f64) / 3600.0).round().max(1.0) as u64, "hour", "hours")
    } else if abs < 7 * 86_400 {
        let days = ((abs as f64) / 86_400.0).round().max(1.0) as u64;
        match (days, ahead) {
            (1, false) => "yesterday".into(),
            (1, true) => "tomorrow".into(),
            _ => unit(days, "day", "days"),
        }
    } else {
        short_date(t.date())
    }
}

/// "Nov 5, 2026".
pub fn short_date(d: Date) -> String {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    format!("{} {}, {}", MONTHS[d.month() as usize - 1], d.day(), d.year())
}

/// The host of a URL ("en.wikipedia.org"), or the text itself when it isn't one.
pub fn host_of(url: &str) -> String {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let host = host.rsplit_once('@').map(|(_, h)| h).unwrap_or(host);
    if host.is_empty() { url.to_owned() } else { host.trim_start_matches("www.").to_owned() }
}

/// "840 KB", "12.4 MB".
pub fn file_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let b = bytes as f64;
    if b < KB {
        format!("{bytes} B")
    } else if b < KB * KB {
        format!("{:.0} KB", b / KB)
    } else if b < KB * KB * KB {
        format!("{:.1} MB", b / KB / KB)
    } else {
        format!("{:.1} GB", b / KB / KB / KB)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> OffsetDateTime {
        parse_time("2026-11-06T12:00:00Z").unwrap()
    }

    #[test]
    fn relative_times() {
        assert_eq!(relative_to("2026-11-06T11:59:40Z", now()), "just now");
        assert_eq!(relative_to("2026-11-06T11:57:00Z", now()), "3 minutes ago");
        assert_eq!(relative_to("2026-11-06 09:00:00", now()), "3 hours ago");
        assert_eq!(relative_to("2026-11-05T12:00:00Z", now()), "yesterday");
        assert_eq!(relative_to("2026-11-02T12:00:00Z", now()), "4 days ago");
        assert_eq!(relative_to("2026-10-01T12:00:00Z", now()), "Oct 1, 2026");
        assert_eq!(relative_to("nonsense", now()), "");
    }

    #[test]
    fn hosts_and_sizes() {
        assert_eq!(host_of("https://www.example.com/a?b"), "example.com");
        assert_eq!(host_of("http://user@host:8080/x"), "host:8080");
        assert_eq!(host_of("not a url"), "not a url");
        assert_eq!(file_size(512), "512 B");
        assert_eq!(file_size(13_000_000), "12.4 MB");
    }
}
