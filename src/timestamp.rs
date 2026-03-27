use time::format_description::FormatItem;
use time::macros::format_description;
use time::{OffsetDateTime, UtcOffset};

const COMPACT_FORMAT: &[FormatItem<'_>] = format_description!("[hour]:[minute]");
const FULL_FORMAT: &[FormatItem<'_>] = format_description!("[year]-[month]-[day] [hour]:[minute]");
const MONTH_DAY_COMPACT_FORMAT: &[FormatItem<'_>] =
    format_description!("[month]/[day]-[hour]:[minute]");

pub fn now_utc_timestamp() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

pub fn format_compact_local(timestamp: i64) -> String {
    format_with(timestamp, COMPACT_FORMAT)
}

pub fn format_full_local(timestamp: i64) -> String {
    format_with(timestamp, FULL_FORMAT)
}

pub fn format_month_day_local(timestamp: i64) -> String {
    format_with(timestamp, MONTH_DAY_COMPACT_FORMAT)
}

pub fn format_export_local(timestamp: i64) -> String {
    format_full_local(timestamp)
}

fn format_with(timestamp: i64, format: &[FormatItem<'_>]) -> String {
    let offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
    OffsetDateTime::from_unix_timestamp(timestamp)
        .unwrap_or(OffsetDateTime::UNIX_EPOCH)
        .to_offset(offset)
        .format(format)
        .unwrap_or_else(|_| String::from("1970-01-01 00:00"))
}

#[cfg(test)]
mod tests {
    use super::{
        format_compact_local, format_export_local, format_full_local, format_month_day_local,
    };

    #[test]
    fn formats_timestamps_without_panicking() {
        assert!(!format_compact_local(1_711_275_600).is_empty());
        assert!(!format_full_local(1_711_275_600).is_empty());
        let month_day = format_month_day_local(1_711_275_600);
        assert!(month_day.contains('/'));
        assert!(month_day.contains('-'));
        assert!(month_day.contains(':'));
        assert!(!format_export_local(1_711_275_600).is_empty());
    }
}
