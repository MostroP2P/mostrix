//! Admin dispute selection helpers shared by rendering and key handling.

use std::str::FromStr;

use mostro_core::prelude::DisputeStatus;

use crate::models::AdminDispute;
use crate::ui::{AppState, DisputeFilter};

/// Filter disputes based on the current filter state.
/// Returns owned data so the caller can mutate app (e.g. scroll state) in the same block.
pub fn get_filtered_disputes(app: &AppState) -> Vec<(usize, AdminDispute)> {
    app.admin_disputes_in_progress
        .iter()
        .enumerate()
        .filter(|(_, d)| {
            let status = d
                .status
                .as_deref()
                .and_then(|s| DisputeStatus::from_str(s).ok());
            match app.dispute_filter {
                DisputeFilter::InProgress => status == Some(DisputeStatus::InProgress),
                DisputeFilter::Finalized => matches!(
                    status,
                    Some(DisputeStatus::Settled)
                        | Some(DisputeStatus::SellerRefunded)
                        | Some(DisputeStatus::Released)
                ),
            }
        })
        .map(|(i, d)| (i, d.clone()))
        .collect()
}

/// Display row of the current selection inside `filtered`.
///
/// Falls back to the first row when nothing is selected or the selected
/// dispute is not visible under the current filter. Returns `None` only when
/// the filtered list is empty.
pub fn selected_display_idx(app: &AppState, filtered: &[(usize, AdminDispute)]) -> Option<usize> {
    if filtered.is_empty() {
        return None;
    }
    Some(
        app.selected_dispute_id
            .as_deref()
            .and_then(|id| filtered.iter().position(|(_, d)| d.dispute_id == id))
            .unwrap_or(0),
    )
}

/// The dispute the sidebar currently shows as selected.
///
/// Resolves the stored dispute id against the filtered (visible) list, so key
/// handlers always act on the dispute the UI highlights — never on rows hidden
/// by the current filter.
pub fn selected_filtered_dispute(app: &AppState) -> Option<AdminDispute> {
    let mut filtered = get_filtered_disputes(app);
    let idx = selected_display_idx(app, &filtered)?;
    Some(filtered.swap_remove(idx).1)
}

/// Move the sidebar selection `delta` rows within the filtered list, clamping
/// at both ends, and store the landing dispute's id as the new selection.
pub fn move_dispute_selection(app: &mut AppState, delta: isize) {
    let filtered = get_filtered_disputes(app);
    let Some(idx) = selected_display_idx(app, &filtered) else {
        return;
    };
    let new_idx = idx
        .saturating_add_signed(delta)
        .min(filtered.len().saturating_sub(1));
    app.selected_dispute_id = Some(filtered[new_idx].1.dispute_id.clone());
}
