//! Order-book selection helpers shared by Orders tab rendering and key handling.
//!
//! Selection is stored as an order UUID (`AppState.selected_order_id`) and always
//! resolved against the **currency-filtered** book projection, so highlight, ↑↓,
//! and Enter/take/cancel never target a row hidden by `currencies_filter`.

use std::collections::HashSet;

use mostro_core::prelude::{Kind, SmallOrder};
use uuid::Uuid;

use crate::ui::orders::{
    OrderBookFilters, OrderBookKindFilter, OrderBookSort, OrderBookSortDirection, OrderBookSortKey,
};
use crate::ui::AppState;

/// Whether `order` passes the active currency filter (empty filter = all pass).
pub fn order_passes_currency_filter(order: &SmallOrder, currencies_filter: &[String]) -> bool {
    if currencies_filter.is_empty() {
        return true;
    }
    let filter_set: HashSet<String> = currencies_filter.iter().map(|c| c.to_uppercase()).collect();
    filter_set.contains(&order.fiat_code.to_uppercase())
}

/// Whether `order` passes local Orders-tab filters (kind, fiat, premium band).
pub fn order_passes_order_filters(order: &SmallOrder, filters: &OrderBookFilters) -> bool {
    match filters.kind {
        OrderBookKindFilter::Any => {}
        OrderBookKindFilter::Buy if !matches!(order.kind, Some(Kind::Buy)) => return false,
        OrderBookKindFilter::Sell if !matches!(order.kind, Some(Kind::Sell)) => return false,
        _ => {}
    }

    let fiat = filters.fiat_code.trim();
    if !fiat.is_empty() && !order.fiat_code.eq_ignore_ascii_case(fiat) {
        return false;
    }

    filters.premium.is_none_or(|p| order.premium == p)
}

/// Alias kept for call sites that previously passed a wall-clock timestamp.
pub fn order_passes_order_filters_at(
    order: &SmallOrder,
    filters: &OrderBookFilters,
    _now: i64,
) -> bool {
    order_passes_order_filters(order, filters)
}

/// Filtered book rows as `(original_index, order)` pairs.
pub fn get_filtered_book_orders(
    orders: &[SmallOrder],
    currencies_filter: &[String],
    order_filters: &OrderBookFilters,
) -> Vec<(usize, SmallOrder)> {
    get_sorted_filtered_book_orders(
        orders,
        currencies_filter,
        order_filters,
        &OrderBookSort::default(),
    )
}

/// Filtered and sorted book rows as `(original_index, order)` pairs.
pub fn get_sorted_filtered_book_orders(
    orders: &[SmallOrder],
    currencies_filter: &[String],
    order_filters: &OrderBookFilters,
    order_sort: &OrderBookSort,
) -> Vec<(usize, SmallOrder)> {
    let mut filtered = orders
        .iter()
        .enumerate()
        .filter(|(_, o)| order_passes_currency_filter(o, currencies_filter))
        .filter(|(_, o)| order_passes_order_filters(o, order_filters))
        .map(|(i, o)| (i, o.clone()))
        .collect::<Vec<_>>();
    sort_book_orders(&mut filtered, order_sort);
    filtered
}

fn fiat_sort_amount(order: &SmallOrder) -> i64 {
    if order.fiat_amount != 0 {
        order.fiat_amount
    } else {
        order.min_amount.unwrap_or(0)
    }
}

fn kind_sort_label(order: &SmallOrder) -> &'static str {
    match order.kind {
        Some(Kind::Buy) => "buy",
        Some(Kind::Sell) => "sell",
        _ => "",
    }
}

fn sort_book_orders(orders: &mut [(usize, SmallOrder)], order_sort: &OrderBookSort) {
    orders.sort_by(|(a_idx, a), (b_idx, b)| {
        let ordering = match order_sort.key {
            OrderBookSortKey::Created => a.created_at.unwrap_or(0).cmp(&b.created_at.unwrap_or(0)),
            OrderBookSortKey::FiatAmount => fiat_sort_amount(a).cmp(&fiat_sort_amount(b)),
            OrderBookSortKey::Premium => a.premium.cmp(&b.premium),
            OrderBookSortKey::FiatCurrency => a
                .fiat_code
                .to_ascii_uppercase()
                .cmp(&b.fiat_code.to_ascii_uppercase()),
            OrderBookSortKey::PaymentMethod => a
                .payment_method
                .to_ascii_lowercase()
                .cmp(&b.payment_method.to_ascii_lowercase()),
            OrderBookSortKey::Kind => kind_sort_label(a).cmp(kind_sort_label(b)),
        };
        let directed = match order_sort.direction {
            OrderBookSortDirection::Ascending => ordering,
            OrderBookSortDirection::Descending => ordering.reverse(),
        };
        directed
            .then_with(|| b.created_at.unwrap_or(0).cmp(&a.created_at.unwrap_or(0)))
            .then_with(|| a_idx.cmp(b_idx))
    });
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
    let mut filtered = get_sorted_filtered_book_orders(
        orders,
        &app.currencies_filter,
        &app.order_filters,
        &app.order_sort,
    );
    let idx = selected_book_display_idx(app.selected_order_id, &filtered)?;
    Some(filtered.swap_remove(idx).1)
}

/// Move Orders-tab selection `delta` rows within the filtered book, clamping at
/// both ends, and store the landing order's id (when present).
pub fn move_book_order_selection(app: &mut AppState, orders: &[SmallOrder], delta: isize) {
    let filtered = get_sorted_filtered_book_orders(
        orders,
        &app.currencies_filter,
        &app.order_filters,
        &app.order_sort,
    );
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
    use crate::ui::orders::{
        OrderBookFilters, OrderBookKindFilter, OrderBookSort, OrderBookSortDirection,
        OrderBookSortKey,
    };
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

    fn sell_order(id: Uuid, fiat: &str, payment: &str) -> SmallOrder {
        SmallOrder {
            kind: Some(Kind::Sell),
            ..order(id, fiat, payment)
        }
    }

    fn sorted_ids(
        orders: &[SmallOrder],
        key: OrderBookSortKey,
        direction: OrderBookSortDirection,
    ) -> Vec<Uuid> {
        get_sorted_filtered_book_orders(
            orders,
            &[],
            &OrderBookFilters::default(),
            &OrderBookSort { key, direction },
        )
        .into_iter()
        .filter_map(|(_, order)| order.id)
        .collect()
    }

    #[test]
    fn empty_currency_filter_keeps_all_orders() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let orders = vec![order(a, "USD", "sepa"), order(b, "EUR", "sepa")];
        let filtered = get_filtered_book_orders(&orders, &[], &OrderBookFilters::default());
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn currency_filter_hides_other_fiats() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let orders = vec![order(a, "USD", "sepa"), order(b, "EUR", "sepa")];
        let filtered =
            get_filtered_book_orders(&orders, &["EUR".to_string()], &OrderBookFilters::default());
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].1.id, Some(b));
    }

    #[test]
    fn local_filters_match_kind_fiat_and_exact_premium() {
        let buy = Uuid::new_v4();
        let sell = Uuid::new_v4();
        let orders = vec![
            SmallOrder {
                premium: 5,
                ..order(buy, "USD", "Cash App")
            },
            SmallOrder {
                premium: -2,
                ..sell_order(sell, "EUR", "SEPA")
            },
        ];
        let filters = OrderBookFilters {
            kind: OrderBookKindFilter::Sell,
            fiat_code: "eur".to_string(),
            premium: Some(-2),
        };

        let filtered = get_filtered_book_orders(&orders, &[], &filters);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].1.id, Some(sell));
    }

    #[test]
    fn exact_premium_filter_keeps_matching_percent_only() {
        let zero = Uuid::new_v4();
        let discount = Uuid::new_v4();
        let markup = Uuid::new_v4();
        let orders = vec![
            SmallOrder {
                premium: 0,
                ..order(zero, "USD", "cash")
            },
            SmallOrder {
                premium: -3,
                ..order(discount, "USD", "cash")
            },
            SmallOrder {
                premium: 4,
                ..order(markup, "USD", "cash")
            },
        ];

        let at_zero = OrderBookFilters {
            premium: Some(0),
            ..Default::default()
        };
        assert_eq!(get_filtered_book_orders(&orders, &[], &at_zero).len(), 1);
        assert_eq!(
            get_filtered_book_orders(&orders, &[], &at_zero)[0].1.id,
            Some(zero)
        );

        let at_minus_three = OrderBookFilters {
            premium: Some(-3),
            ..Default::default()
        };
        assert_eq!(
            get_filtered_book_orders(&orders, &[], &at_minus_three)[0]
                .1
                .id,
            Some(discount)
        );

        let at_four = OrderBookFilters {
            premium: Some(4),
            ..Default::default()
        };
        assert_eq!(
            get_filtered_book_orders(&orders, &[], &at_four)[0].1.id,
            Some(markup)
        );
    }

    #[test]
    fn default_sort_keeps_newest_created_orders_first() {
        let old = Uuid::new_v4();
        let new = Uuid::new_v4();
        let orders = vec![
            SmallOrder {
                created_at: Some(10),
                ..order(old, "USD", "cash")
            },
            SmallOrder {
                created_at: Some(20),
                ..order(new, "USD", "cash")
            },
        ];

        let filtered = get_filtered_book_orders(&orders, &[], &OrderBookFilters::default());

        assert_eq!(
            filtered
                .into_iter()
                .filter_map(|(_, order)| order.id)
                .collect::<Vec<_>>(),
            vec![new, old]
        );
    }

    #[test]
    fn sorting_orders_by_numeric_fields_respects_direction() {
        let low = Uuid::new_v4();
        let high = Uuid::new_v4();
        let range = Uuid::new_v4();
        let orders = vec![
            SmallOrder {
                fiat_amount: 50,
                premium: -3,
                ..order(low, "USD", "cash")
            },
            SmallOrder {
                fiat_amount: 850,
                premium: 5,
                ..order(high, "USD", "cash")
            },
            SmallOrder {
                fiat_amount: 0,
                min_amount: Some(100),
                max_amount: Some(1_000),
                premium: 0,
                ..order(range, "USD", "cash")
            },
        ];

        assert_eq!(
            sorted_ids(
                &orders,
                OrderBookSortKey::FiatAmount,
                OrderBookSortDirection::Ascending
            ),
            vec![low, range, high]
        );
        assert_eq!(
            sorted_ids(
                &orders,
                OrderBookSortKey::FiatAmount,
                OrderBookSortDirection::Descending
            ),
            vec![high, range, low]
        );
        assert_eq!(
            sorted_ids(
                &orders,
                OrderBookSortKey::Premium,
                OrderBookSortDirection::Ascending
            ),
            vec![low, range, high]
        );
    }

    #[test]
    fn sorting_orders_by_text_fields_is_case_insensitive() {
        let mxn = Uuid::new_v4();
        let eur = Uuid::new_v4();
        let usd = Uuid::new_v4();
        let orders = vec![
            SmallOrder {
                payment_method: "zelle".to_string(),
                ..order(usd, "usd", "zelle")
            },
            SmallOrder {
                payment_method: "Cash".to_string(),
                ..order(mxn, "MXN", "Cash")
            },
            SmallOrder {
                payment_method: "bank".to_string(),
                ..order(eur, "eur", "bank")
            },
        ];

        assert_eq!(
            sorted_ids(
                &orders,
                OrderBookSortKey::FiatCurrency,
                OrderBookSortDirection::Ascending
            ),
            vec![eur, mxn, usd]
        );
        assert_eq!(
            sorted_ids(
                &orders,
                OrderBookSortKey::PaymentMethod,
                OrderBookSortDirection::Ascending
            ),
            vec![eur, mxn, usd]
        );
    }

    #[test]
    fn sorting_orders_by_kind_groups_buy_and_sell() {
        let buy = Uuid::new_v4();
        let sell = Uuid::new_v4();
        let unknown = Uuid::new_v4();
        let orders = vec![
            sell_order(sell, "USD", "cash"),
            SmallOrder {
                kind: None,
                ..order(unknown, "USD", "cash")
            },
            order(buy, "USD", "cash"),
        ];

        assert_eq!(
            sorted_ids(
                &orders,
                OrderBookSortKey::Kind,
                OrderBookSortDirection::Ascending
            ),
            vec![unknown, buy, sell]
        );
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

        let filtered =
            get_filtered_book_orders(&orders, &app.currencies_filter, &app.order_filters);
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
    fn move_selection_follows_sorted_projection() {
        let low = Uuid::new_v4();
        let mid = Uuid::new_v4();
        let high = Uuid::new_v4();
        let orders = vec![
            SmallOrder {
                fiat_amount: 300,
                ..order(high, "USD", "high")
            },
            SmallOrder {
                fiat_amount: 100,
                ..order(low, "USD", "low")
            },
            SmallOrder {
                fiat_amount: 200,
                ..order(mid, "USD", "mid")
            },
        ];
        let mut app = AppState::new(UserRole::User);
        app.order_sort = OrderBookSort {
            key: OrderBookSortKey::FiatAmount,
            direction: OrderBookSortDirection::Ascending,
        };

        move_book_order_selection(&mut app, &orders, 1);
        assert_eq!(
            app.selected_order_id,
            Some(mid),
            "selection starts from first sorted row, then moves down"
        );

        move_book_order_selection(&mut app, &orders, 1);
        assert_eq!(app.selected_order_id, Some(high));

        move_book_order_selection(&mut app, &orders, -1);
        assert_eq!(app.selected_order_id, Some(mid));
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
