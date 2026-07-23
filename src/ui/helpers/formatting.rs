use chrono::Utc;
use mostro_core::prelude::UserInfo;

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

/// Truncated order id for compact displays (sidebar rows, header cards); no `"Order: "` prefix.
/// Returns `"unknown"` when absent. Pairs with [`format_order_id`] (which keeps the prefix and
/// is used in full-sentence contexts like popups).
#[must_use]
pub fn short_order_id(order_id: Option<uuid::Uuid>) -> String {
    order_id
        .map(|id| id.to_string().chars().take(8).collect::<String>())
        .unwrap_or_else(|| "unknown".to_string())
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
