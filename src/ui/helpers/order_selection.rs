//! Order-book selection helpers shared by Orders tab rendering and key handling.
//!
//! Selection is stored as an order UUID (`AppState.selected_order_id`) and always
//! resolved against the **currency-filtered** book projection, so highlight, ↑↓,
//! and Enter/take/cancel never target a row hidden by `currencies_filter`.

use std::collections::HashSet;

use mostro_core::prelude::SmallOrder;
use uuid::Uuid;

use crate::ui::AppState;

/// Whether `order` passes the active currency filter (empty filter = all pass).
pub fn order_passes_currency_filter(order: &SmallOrder, currencies_filter: &[String]) -> bool {
    if currencies_filter.is_empty() {
        return true;
    }
    let filter_set: HashSet<String> = currencies_filter.iter().map(|c| c.to_uppercase()).collect();
    filter_set.contains(&order.fiat_code.to_uppercase())
}

/// Currency-filtered book rows as `(original_index, order)` pairs.
pub fn get_filtered_book_orders(
    orders: &[SmallOrder],
    currencies_filter: &[String],
) -> Vec<(usize, SmallOrder)> {
    orders
        .iter()
        .enumerate()
        .filter(|(_, o)| order_passes_currency_filter(o, currencies_filter))
        .map(|(i, o)| (i, o.clone()))
        .collect()
}

/// Display row of the current selection inside `filtered`.
///
/// Falls back to the first visible row when nothing is selected or the selected
/// id is hidden by the currency filter. Returns `None` only when `filtered` is empty.
pub fn selected_book_display_idx(
    selected_order_id: Option<Uuid>,
    filtered: &[(usize, SmallOrder)],
) -> Option<usize> {
    if filtered.is_empty() {
        return None;
    }
    Some(
        selected_order_id
            .and_then(|id| filtered.iter().position(|(_, o)| o.id == Some(id)))
            .unwrap_or(0),
    )
}

/// The order the Orders table currently shows as selected.
///
/// Resolves `selected_order_id` against the currency-filtered book so Enter/take
/// always acts on the highlighted row — never on a row hidden by the filter.
pub fn selected_filtered_book_order(app: &AppState, orders: &[SmallOrder]) -> Option<SmallOrder> {
    let mut filtered = get_filtered_book_orders(orders, &app.currencies_filter);
    let idx = selected_book_display_idx(app.selected_order_id, &filtered)?;
    Some(filtered.swap_remove(idx).1)
}

/// Move Orders-tab selection `delta` rows within the filtered book, clamping at
/// both ends, and store the landing order's id (when present).
pub fn move_book_order_selection(app: &mut AppState, orders: &[SmallOrder], delta: isize) {
    let filtered = get_filtered_book_orders(orders, &app.currencies_filter);
    let Some(idx) = selected_book_display_idx(app.selected_order_id, &filtered) else {
        app.selected_order_id = None;
        return;
    };
    let new_idx = idx
        .saturating_add_signed(delta)
        .min(filtered.len().saturating_sub(1));
    app.selected_order_id = filtered[new_idx].1.id;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::UserRole;
    use mostro_core::prelude::Kind;

    fn order(id: Uuid, fiat: &str, payment: &str) -> SmallOrder {
        SmallOrder {
            id: Some(id),
            kind: Some(Kind::Buy),
            fiat_code: fiat.to_string(),
            fiat_amount: 100,
            amount: 50_000,
            payment_method: payment.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn empty_currency_filter_keeps_all_orders() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let orders = vec![order(a, "USD", "sepa"), order(b, "EUR", "sepa")];
        let filtered = get_filtered_book_orders(&orders, &[]);
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn currency_filter_hides_other_fiats() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let orders = vec![order(a, "USD", "sepa"), order(b, "EUR", "sepa")];
        let filtered = get_filtered_book_orders(&orders, &["EUR".to_string()]);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].1.id, Some(b));
    }

    /// Regression for the highlight/Enter mismatch: after a currency filter hides
    /// the previously selected order, resolution must fall back to the first
    /// *visible* row — the same one the table highlights — never the hidden id.
    #[test]
    fn hidden_selection_falls_back_to_first_visible_for_enter() {
        let usd_id = Uuid::new_v4();
        let eur_id = Uuid::new_v4();
        let orders = vec![
            order(usd_id, "USD", "PAY-USD"),
            order(eur_id, "EUR", "PAY-EUR"),
        ];

        let mut app = AppState::new(UserRole::User);
        app.selected_order_id = Some(usd_id);
        app.currencies_filter = vec!["EUR".to_string()];

        let filtered = get_filtered_book_orders(&orders, &app.currencies_filter);
        assert_eq!(
            selected_book_display_idx(app.selected_order_id, &filtered),
            Some(0),
            "table highlight falls back to first visible row"
        );

        let selected = selected_filtered_book_order(&app, &orders).expect("visible order");
        assert_eq!(selected.id, Some(eur_id));
        assert_eq!(selected.payment_method, "PAY-EUR");
    }

    #[test]
    fn selection_by_id_survives_list_reorder() {
        let keep = Uuid::new_v4();
        let other = Uuid::new_v4();
        let mut app = AppState::new(UserRole::User);
        app.selected_order_id = Some(keep);

        let reordered = vec![order(other, "USD", "first"), order(keep, "USD", "kept")];
        let selected = selected_filtered_book_order(&app, &reordered).expect("selection");
        assert_eq!(selected.id, Some(keep));
        assert_eq!(selected.payment_method, "kept");
    }

    #[test]
    fn move_selection_skips_hidden_rows_and_clamps() {
        let usd = Uuid::new_v4();
        let eur_a = Uuid::new_v4();
        let eur_b = Uuid::new_v4();
        let orders = vec![
            order(usd, "USD", "usd"),
            order(eur_a, "EUR", "eur-a"),
            order(eur_b, "EUR", "eur-b"),
        ];
        let mut app = AppState::new(UserRole::User);
        app.currencies_filter = vec!["EUR".to_string()];

        // No selection yet → resolve as first visible (eur_a), then move down.
        move_book_order_selection(&mut app, &orders, 1);
        assert_eq!(app.selected_order_id, Some(eur_b));

        move_book_order_selection(&mut app, &orders, 1);
        assert_eq!(
            app.selected_order_id,
            Some(eur_b),
            "clamped at bottom of visible list"
        );

        move_book_order_selection(&mut app, &orders, -1);
        assert_eq!(app.selected_order_id, Some(eur_a));

        move_book_order_selection(&mut app, &orders, -1);
        assert_eq!(
            app.selected_order_id,
            Some(eur_a),
            "clamped at top of visible list"
        );
    }

    #[test]
    fn empty_filtered_list_yields_no_selection() {
        let orders = vec![order(Uuid::new_v4(), "USD", "sepa")];
        let mut app = AppState::new(UserRole::User);
        app.currencies_filter = vec!["EUR".to_string()];
        app.selected_order_id = Some(Uuid::new_v4());

        assert!(selected_filtered_book_order(&app, &orders).is_none());
        move_book_order_selection(&mut app, &orders, 1);
        assert_eq!(app.selected_order_id, None);
    }
}
