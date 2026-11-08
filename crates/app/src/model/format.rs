//! Time, size and source formatting.

use chidori_core::events::RecordedSources;
use chidori_core::session::model::{Continuation, SourceId};
use chrono::{DateTime, Datelike, FixedOffset, NaiveDate};

use crate::i18n::{is_ja, t, tf};

/// `HH:MM:SS` from a duration in ms.
pub fn hms(ms: u64) -> String {
    let s = ms / 1000;
    format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

fn parse(started_at: &str) -> Option<DateTime<FixedOffset>> {
    DateTime::parse_from_rfc3339(started_at).ok()
}

/// Wall-clock `HH:MM:SS` of a session offset, in the session's own UTC offset.
pub fn clock_at(started_at: &str, t_ms: u64) -> String {
    match parse(started_at) {
        Some(start) => (start + chrono::Duration::milliseconds(t_ms as i64)).format("%H:%M:%S").to_string(),
        None => hms(t_ms),
    }
}

/// Clock times of a session: from its start, or from the start of the continuation an offset
/// falls in (FR-08). A session from a file (FR-18) gives places in the file instead.
pub fn session_clock(started_at: &str, continued: &[Continuation], from_file: bool, t_ms: u64) -> String {
    if from_file {
        return hms(t_ms);
    }
    match continued.iter().rev().find(|c| c.at_ms <= t_ms) {
        Some(part) => clock_at(&part.started_at, t_ms - part.at_ms),
        None => clock_at(started_at, t_ms),
    }
}

/// `2026-10-02 15:13` in the session's offset.
pub fn date_time_of(started_at: &str) -> String {
    match (started_at.get(..10), started_at.get(11..16)) {
        (Some(d), Some(t)) => format!("{d} {t}"),
        _ => started_at.to_string(),
    }
}

/// `15:13` in the session's offset.
pub fn time_of(started_at: &str) -> String {
    started_at.get(11..16).unwrap_or_default().to_string()
}

pub fn bytes(n: u64) -> String {
    let n = n as f64;
    if n >= 1e9 {
        format!("{:.2} GB", n / 1e9)
    } else if n >= 1e8 {
        format!("{:.0} MB", n / 1e6)
    } else if n >= 1e6 {
        format!("{:.1} MB", n / 1e6)
    } else if n >= 1e3 {
        format!("{:.0} kB", n / 1e3)
    } else {
        format!("{n} B")
    }
}

/// Maps −60..0 dBFS onto 0..1.
pub fn level_fraction(dbfs: Option<f32>) -> f32 {
    match dbfs {
        Some(d) if !d.is_nan() => ((d + 60.0) / 60.0).clamp(0.0, 1.0),
        _ => 0.0,
    }
}

/// A short duration: `1時間12分`, `32分`, `45秒`.
pub fn duration_text(ms: u64) -> String {
    let s = (ms as f64 / 1000.0).round() as u64;
    if s < 60 {
        return tf("durS", &[("s", &s)]);
    }
    let m = (s as f64 / 60.0).round() as u64;
    if m < 60 { tf("durM", &[("m", &m)]) } else { tf("durHm", &[("h", &(m / 60)), ("m", &(m % 60))]) }
}

/// The recording time in the deck: `12:16` under an hour, `1:02:03` after.
pub fn timer_text(ms: u64) -> String {
    let s = ms / 1000;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{:02}:{:02}", s / 60, s % 60)
    }
}

/// The calendar day of a session start (`2026-10-03`), in the session's own offset.
pub fn day_of(started_at: &str) -> &str {
    started_at.get(..10).unwrap_or(started_at)
}

/// `10月3日（土）` / `Sat, Oct 3`; the year is added when it is not `this_year`.
pub fn day_text(day: &str, this_year: i32) -> String {
    let Ok(date) = NaiveDate::parse_from_str(day, "%Y-%m-%d") else { return day.to_string() };
    let other_year = date.year() != this_year;
    if is_ja() {
        const WEEK: [&str; 7] = ["月", "火", "水", "木", "金", "土", "日"];
        let w = WEEK[date.weekday().num_days_from_monday() as usize];
        let y = if other_year { format!("{}年", date.year()) } else { String::new() };
        format!("{y}{}月{}日（{w}）", date.month(), date.day())
    } else if other_year {
        date.format("%a, %b %-d, %Y").to_string()
    } else {
        date.format("%a, %b %-d").to_string()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DayName {
    Today,
    Yesterday,
}

/// 今日 or 昨日 for a day, seen from today (local time).
pub fn day_name(day: &str, today: NaiveDate) -> Option<DayName> {
    let date = NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()?;
    if date == today {
        Some(DayName::Today)
    } else if today.pred_opt() == Some(date) {
        Some(DayName::Yesterday)
    } else {
        None
    }
}

/// What is being recorded, in the UI language: `Zoom + マイク`, `システム全体`.
pub fn sources_text(sources: Option<&RecordedSources>) -> String {
    let Some(sources) = sources else { return String::new() };
    sources
        .ids
        .iter()
        .map(|id| match id {
            SourceId::App => sources.app_name.clone().unwrap_or_else(|| t("srcApps")),
            SourceId::File => sources.app_name.clone().unwrap_or_else(|| t("srcFile")),
            SourceId::System => t("srcSystem"),
            SourceId::Mic => t("srcMic"),
        })
        .collect::<Vec<_>>()
        .join(" + ")
}

/// Splits a catalog name like `標準 (large-v3-turbo q5_0)` into its everyday and technical parts.
pub fn model_name_parts(name: &str) -> (String, String) {
    let open = name.find(['(', '（']);
    let close = name.rfind([')', '）']);
    match (open, close) {
        (Some(o), Some(c)) if c > o => {
            let tech_start = o + name[o..].chars().next().map_or(1, char::len_utf8);
            (name[..o].trim().to_string(), name[tech_start..c].to_string())
        }
        _ => (name.to_string(), String::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times() {
        assert_eq!(hms(3_723_000), "01:02:03");
        assert_eq!(timer_text(736_000), "12:16");
        assert_eq!(timer_text(3_723_000), "1:02:03");
        assert_eq!(clock_at("2026-10-02T15:13:00+09:00", 61_000), "15:14:01");
        assert_eq!(date_time_of("2026-10-02T15:13:00+09:00"), "2026-10-02 15:13");
    }

    #[test]
    fn model_names() {
        assert_eq!(model_name_parts("標準 (large-v3-turbo q5_0)"), ("標準".into(), "large-v3-turbo q5_0".into()));
        assert_eq!(model_name_parts("軽量"), ("軽量".into(), String::new()));
    }

    #[test]
    fn days() {
        crate::i18n::set_locale(chidori_core::settings::Locale::Ja);
        assert_eq!(day_text("2026-10-03", 2026), "10月3日（土）");
        assert_eq!(day_text("2025-10-03", 2026), "2025年10月3日（金）");
    }
}
