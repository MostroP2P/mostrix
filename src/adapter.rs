// Adapter to convert SmallOrder to Order struct used in mostrix
use mostro_core::prelude::*;
use crate::models::Order;
use crate::util::fetch_orders_list;
use nostr_sdk::prelude::*;

/// Converts SmallOrder from mostro-core to Order struct used in mostrix
pub fn small_order_to_order(small_order: &SmallOrder) -> Order {
    Order {
        id: small_order.id.map(|id| id.to_string()),
        kind: small_order.kind.map(|k| format!("{:?}", k).to_lowercase()),
        status: small_order.status.map(|s| format!("{:?}", s).to_lowercase()),
        amount: small_order.amount,
        fiat_code: small_order.fiat_code.clone(),
        min_amount: small_order.min_amount,
        max_amount: small_order.max_amount,
        fiat_amount: small_order.fiat_amount,
        payment_method: small_order.payment_method.clone(),
        premium: small_order.premium,
        buyer_trade_pubkey: small_order.buyer_trade_pubkey.clone(),
        seller_trade_pubkey: small_order.seller_trade_pubkey.clone(),
        created_at: small_order.created_at,
        expires_at: small_order.expires_at,
        is_mine: false, // This would need to be determined based on pubkey comparison
    }
}

/// Fetches orders using the reused logic from mostro-cli
pub async fn fetch_orders(
    client: &Client,
    mostro_pubkey: PublicKey,
    status: Option<Status>,
    currency: Option<String>,
    kind: Option<mostro_core::order::Kind>,
) -> anyhow::Result<Vec<Order>> {
    let small_orders = fetch_orders_list(client, mostro_pubkey, status, currency, kind).await?;
    
    // Convert SmallOrder to Order
    let orders: Vec<Order> = small_orders
        .into_iter()
        .map(|small_order| small_order_to_order(&small_order))
        .collect();
    
    Ok(orders)
}


