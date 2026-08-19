use chrono::{DateTime, Local, Utc};
use mostro_core::prelude::{DisputeStatus, UserInfo};
use ratatui::style::Color;
use std::str::FromStr;

use crate::models::AdminDispute;

/// Formats user rating with star visualization.
/// Rating must be in 0-5 range. Returns formatted string with stars and stats.
pub fn format_user_rating(info: Option<&UserInfo>) -> String {
    if let Some(info) = info {
        let rating = info.rating.clamp(0.0, 5.0);
        let star_count = rating.round() as usize;
        let stars = "⭐".repeat(star_count);
        format!(
            "{} {:.1}/5 ({} trades completed, {} days)",
            stars, rating, info.reviews, info.operating_days
        )
    } else {
        "No rating available".to_string()
    }
}

/// Check if a dispute is finalized (Settled, SellerRefunded, or Released).
pub fn is_dispute_finalized(selected_dispute: &AdminDispute) -> Option<bool> {
    Some(selected_dispute.is_finalized())
}

/// Color for a kebab-case dispute status in the admin header/sidebar.
pub fn dispute_status_color(status: Option<&str>) -> Color {
    match status.and_then(|s| DisputeStatus::from_str(s).ok()) {
        Some(DisputeStatus::Initiated) => Color::Yellow,
        Some(DisputeStatus::InProgress) => Color::Green,
        Some(DisputeStatus::Settled) | Some(DisputeStatus::Released) => Color::Green,
        Some(DisputeStatus::SellerRefunded) => Color::Red,
        None => Color::White,
    }
}

/// Formats an order ID for display (truncates to 8 chars).
pub fn format_order_id(order_id: Option<uuid::Uuid>) -> String {
    if let Some(id) = order_id {
        format!(
            "Order: {}",
            id.to_string().chars().take(8).collect::<String>()
        )
    } else {
        "Order: Unknown".to_string()
    }
}

/// Formats an order premium with an explicit sign and its semantic UI color.
#[must_use]
pub fn format_premium(premium: i64) -> (String, Color) {
    match premium {
        0 => ("0%".to_string(), Color::Gray),
        value if value > 0 => (format!("+{value}%"), Color::Green),
        value => (format!("{value}%"), Color::Red),
    }
}

/// Truncated order id for compact displays (sidebar rows, header cards); no `"Order: "` prefix.
/// Returns `"unknown"` when absent. Pairs with [`format_order_id`] (which keeps the prefix and
/// is used in full-sentence contexts like popups).
#[must_use]
pub fn short_order_id(order_id: Option<uuid::Uuid>) -> String {
    order_id
        .map(|id| id.to_string().chars().take(8).collect::<String>())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Formats a Unix timestamp for display in the user's local timezone.
#[must_use]
pub fn format_local_timestamp(timestamp: i64, format: &str) -> Option<String> {
    DateTime::from_timestamp(timestamp, 0)
        .map(|dt| dt.with_timezone(&Local).format(format).to_string())
}

/// Compact relative-time label for list rows (e.g. `"3m ago"`, `"2d ago"`), as opposed to the
/// more verbose `format_instance_info_age` (e.g. `"2 hours ago"`) used for instance info banners.
#[must_use]
pub fn relative_time_compact(timestamp: i64) -> String {
    relative_time_compact_from(timestamp, Utc::now().timestamp())
}

/// Testable core of [`relative_time_compact`] with an explicit `now` reference point.
fn relative_time_compact_from(timestamp: i64, now: i64) -> String {
    let delta = now.saturating_sub(timestamp).max(0);
    const MINUTE: i64 = 60;
    const HOUR: i64 = 60 * MINUTE;
    const DAY: i64 = 24 * HOUR;
    const MONTH: i64 = 30 * DAY;

    if delta < MINUTE {
        "just now".to_string()
    } else if delta < HOUR {
        format!("{}m ago", delta / MINUTE)
    } else if delta < DAY {
        format!("{}h ago", delta / HOUR)
    } else if delta < MONTH {
        format!("{}d ago", delta / DAY)
    } else {
        format!("{}mo ago", delta / MONTH)
    }
}

#[cfg(test)]
mod short_order_id_tests {
    use super::*;

    #[test]
    fn truncates_to_eight_chars_without_prefix() {
        let id = uuid::Uuid::parse_str("6c162b3f-0000-0000-0000-000000000000").unwrap();
        assert_eq!(short_order_id(Some(id)), "6c162b3f");
    }

    #[test]
    fn none_renders_unknown() {
        assert_eq!(short_order_id(None), "unknown");
    }
}

#[cfg(test)]
mod relative_time_tests {
    use super::*;

    #[test]
    fn under_a_minute_is_just_now() {
        assert_eq!(relative_time_compact_from(1_000, 1_030), "just now");
    }

    #[test]
    fn minutes_ago() {
        assert_eq!(relative_time_compact_from(1_000, 1_000 + 90), "1m ago");
    }

    #[test]
    fn hours_ago() {
        assert_eq!(relative_time_compact_from(1_000, 1_000 + 3_661), "1h ago");
    }

    #[test]
    fn days_ago() {
        assert_eq!(
            relative_time_compact_from(1_000, 1_000 + 2 * 86_400 + 10),
            "2d ago"
        );
    }

    #[test]
    fn months_ago() {
        assert_eq!(
            relative_time_compact_from(1_000, 1_000 + 65 * 86_400),
            "2mo ago"
        );
    }

    #[test]
    fn future_timestamp_clamps_to_just_now() {
        assert_eq!(relative_time_compact_from(2_000, 1_000), "just now");
    }
}

#[cfg(test)]
mod local_timestamp_tests {
    use super::*;

    #[test]
    fn formats_valid_timestamp_with_requested_pattern() {
        let timestamp = 1_700_000_000;
        let expected = DateTime::from_timestamp(timestamp, 0)
            .unwrap()
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M")
            .to_string();

        assert_eq!(
            format_local_timestamp(timestamp, "%Y-%m-%d %H:%M"),
            Some(expected)
        );
    }

    #[test]
    fn invalid_timestamp_returns_none() {
        assert_eq!(format_local_timestamp(i64::MAX, "%Y-%m-%d"), None);
    }
}

#[cfg(test)]
mod premium_tests {
    use super::*;

    #[test]
    fn preserves_sign_and_uses_semantic_color() {
        assert_eq!(format_premium(2), ("+2%".to_string(), Color::Green));
        assert_eq!(format_premium(-3), ("-3%".to_string(), Color::Red));
        assert_eq!(format_premium(0), ("0%".to_string(), Color::Gray));
    }

    #[test]
    fn dispute_status_color_matches_lifecycle() {
        assert_eq!(dispute_status_color(Some("in-progress")), Color::Green);
        assert_eq!(dispute_status_color(Some("seller-refunded")), Color::Red);
        assert_eq!(dispute_status_color(Some("settled")), Color::Green);
        assert_eq!(dispute_status_color(Some("initiated")), Color::Yellow);
        assert_eq!(dispute_status_color(None), Color::White);
        assert_eq!(dispute_status_color(Some("unknown")), Color::White);
    }
}
