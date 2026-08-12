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
