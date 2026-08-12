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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::UserRole;

    fn dispute(id: &str, status: &str) -> AdminDispute {
        AdminDispute {
            dispute_id: id.to_string(),
            status: Some(status.to_string()),
            ..Default::default()
        }
    }

    fn admin_app(disputes: Vec<AdminDispute>) -> AppState {
        let mut app = AppState::new(UserRole::Admin);
        app.admin_disputes_in_progress = disputes;
        app
    }

    /// Regression for the wrong-dispute chat/finalization bug: with finalized
    /// disputes sorted above in-progress ones (taken_at DESC), the resolved
    /// selection must be the first *visible* dispute, never the hidden row at
    /// raw index 0.
    #[test]
    fn resolves_first_visible_dispute_not_raw_index_zero() {
        let app = admin_app(vec![
            dispute("finalized-a", "seller-refunded"),
            dispute("finalized-b", "seller-refunded"),
            dispute("in-progress-c", "in-progress"),
            dispute("in-progress-d", "in-progress"),
        ]);

        let filtered = get_filtered_disputes(&app);
        let original_indices: Vec<usize> = filtered.iter().map(|(i, _)| *i).collect();
        assert_eq!(
            original_indices,
            vec![2, 3],
            "only in-progress rows visible"
        );

        let selected = selected_filtered_dispute(&app).expect("a dispute is selectable");
        assert_eq!(selected.dispute_id, "in-progress-c");
    }

    /// Selection stored by id must keep resolving the same dispute after the
    /// list is refreshed and re-ordered (e.g. a newly taken dispute lands at
    /// the top).
    #[test]
    fn selection_by_id_survives_list_reorder() {
        let mut app = admin_app(vec![
            dispute("in-progress-c", "in-progress"),
            dispute("in-progress-d", "in-progress"),
        ]);
        app.selected_dispute_id = Some("in-progress-d".to_string());

        app.admin_disputes_in_progress = vec![
            dispute("finalized-a", "seller-refunded"),
            dispute("in-progress-new", "in-progress"),
            dispute("in-progress-d", "in-progress"),
            dispute("in-progress-c", "in-progress"),
        ];

        let selected = selected_filtered_dispute(&app).expect("selection resolves");
        assert_eq!(selected.dispute_id, "in-progress-d");
    }

    /// When the selected dispute stops being visible under the current filter
    /// (e.g. it was just finalized), resolution falls back to the first
    /// visible row instead of a hidden one.
    #[test]
    fn hidden_selection_falls_back_to_first_visible() {
        let mut app = admin_app(vec![
            dispute("in-progress-c", "in-progress"),
            dispute("in-progress-d", "in-progress"),
        ]);
        app.selected_dispute_id = Some("in-progress-c".to_string());

        app.admin_disputes_in_progress[0] = dispute("in-progress-c", "seller-refunded");

        let filtered = get_filtered_disputes(&app);
        assert_eq!(selected_display_idx(&app, &filtered), Some(0));
        let selected = selected_filtered_dispute(&app).expect("fallback resolves");
        assert_eq!(selected.dispute_id, "in-progress-d");
    }

    /// The Finalized filter shows only settled/refunded/released disputes and
    /// resolves the selection among them.
    #[test]
    fn finalized_filter_resolves_finalized_rows() {
        let mut app = admin_app(vec![
            dispute("finalized-a", "seller-refunded"),
            dispute("in-progress-c", "in-progress"),
            dispute("finalized-b", "settled"),
        ]);
        app.dispute_filter = DisputeFilter::Finalized;

        let filtered = get_filtered_disputes(&app);
        let ids: Vec<&str> = filtered
            .iter()
            .map(|(_, d)| d.dispute_id.as_str())
            .collect();
        assert_eq!(ids, vec!["finalized-a", "finalized-b"]);

        let selected = selected_filtered_dispute(&app).expect("selection resolves");
        assert_eq!(selected.dispute_id, "finalized-a");
    }

    /// Up/Down moves within the visible list only — hidden rows are skipped —
    /// and clamps at both ends.
    #[test]
    fn move_selection_skips_hidden_rows_and_clamps() {
        let mut app = admin_app(vec![
            dispute("finalized-a", "seller-refunded"),
            dispute("in-progress-c", "in-progress"),
            dispute("finalized-b", "settled"),
            dispute("in-progress-d", "in-progress"),
        ]);

        move_dispute_selection(&mut app, 1);
        assert_eq!(app.selected_dispute_id.as_deref(), Some("in-progress-d"));

        move_dispute_selection(&mut app, 1);
        assert_eq!(
            app.selected_dispute_id.as_deref(),
            Some("in-progress-d"),
            "clamped at the bottom of the visible list"
        );

        move_dispute_selection(&mut app, -1);
        assert_eq!(app.selected_dispute_id.as_deref(), Some("in-progress-c"));

        move_dispute_selection(&mut app, -1);
        assert_eq!(
            app.selected_dispute_id.as_deref(),
            Some("in-progress-c"),
            "clamped at the top of the visible list"
        );
    }

    /// With nothing visible under the current filter there is no selection to
    /// resolve, and navigation is a no-op.
    #[test]
    fn empty_filtered_list_yields_no_selection() {
        let mut app = admin_app(vec![
            dispute("finalized-a", "seller-refunded"),
            dispute("finalized-b", "settled"),
        ]);

        assert!(selected_filtered_dispute(&app).is_none());
        assert_eq!(
            selected_display_idx(&app, &get_filtered_disputes(&app)),
            None
        );

        move_dispute_selection(&mut app, 1);
        assert_eq!(app.selected_dispute_id, None, "navigation stays a no-op");
    }
}
