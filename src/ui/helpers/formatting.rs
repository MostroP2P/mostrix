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
