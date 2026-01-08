// Integration tests for database operations
mod common;

use common::{create_test_db, test_mnemonic};
use mostrix::models::{Order, User};
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;

#[tokio::test]
async fn test_user_new() {
    let pool = create_test_db().await.unwrap();
    let mnemonic = test_mnemonic();

    let user = User::new(mnemonic.clone(), &pool).await.unwrap();

    assert!(!user.i0_pubkey.is_empty());
    assert_eq!(user.mnemonic, mnemonic);
    assert!(user.created_at > 0);
}

#[tokio::test]
async fn test_user_get() {
    let pool = create_test_db().await.unwrap();
    let mnemonic = test_mnemonic();

    let user1 = User::new(mnemonic, &pool).await.unwrap();
    let user2 = User::get(&pool).await.unwrap();

    assert_eq!(user1.i0_pubkey, user2.i0_pubkey);
    assert_eq!(user1.mnemonic, user2.mnemonic);
}

#[tokio::test]
async fn test_user_update_last_trade_index() {
    let pool = create_test_db().await.unwrap();
    let mnemonic = test_mnemonic();

    User::new(mnemonic, &pool).await.unwrap();
    User::update_last_trade_index(&pool, 5).await.unwrap();

    let user = User::get(&pool).await.unwrap();
    assert_eq!(user.last_trade_index, Some(5));
}

#[tokio::test]
async fn test_user_get_identity_keys() {
    let pool = create_test_db().await.unwrap();
    let mnemonic = test_mnemonic();

    User::new(mnemonic, &pool).await.unwrap();
    let keys = User::get_identity_keys(&pool).await.unwrap();

    assert!(!keys.public_key().to_string().is_empty());
}

#[tokio::test]
async fn test_user_derive_trade_keys() {
    let pool = create_test_db().await.unwrap();
    let mnemonic = test_mnemonic();

    let user = User::new(mnemonic, &pool).await.unwrap();

    // Derive same index twice - should produce same keys
    let keys1 = user.derive_trade_keys(1).unwrap();
    let keys2 = user.derive_trade_keys(1).unwrap();

    assert_eq!(keys1.secret_key(), keys2.secret_key());
    assert_eq!(keys1.public_key(), keys2.public_key());

    // Different indices should produce different keys
    let keys3 = user.derive_trade_keys(2).unwrap();
    assert_ne!(keys1.secret_key(), keys3.secret_key());
    assert_ne!(keys1.public_key(), keys3.public_key());
}

#[tokio::test]
async fn test_user_save() {
    let pool = create_test_db().await.unwrap();
    let mnemonic = test_mnemonic();

    let mut user = User::new(mnemonic, &pool).await.unwrap();
    user.last_trade_index = Some(10);
    user.save(&pool).await.unwrap();

    let saved_user = User::get(&pool).await.unwrap();
    assert_eq!(saved_user.last_trade_index, Some(10));
}

#[tokio::test]
async fn test_order_new() {
    let pool = create_test_db().await.unwrap();
    let trade_keys = Keys::generate();

    let mut small_order = SmallOrder::default();
    small_order.id = Some(uuid::Uuid::new_v4());
    small_order.kind = Some(mostro_core::order::Kind::Buy);
    small_order.fiat_code = "USD".to_string();
    small_order.amount = 100000;
    small_order.fiat_amount = 100;
    small_order.payment_method = "bank_transfer".to_string();
    small_order.premium = 5;

    let order = Order::new(&pool, small_order.clone(), &trade_keys, Some(123))
        .await
        .unwrap();

    assert!(order.id.is_some());
    assert_eq!(order.fiat_code, "USD");
    assert_eq!(order.amount, 100000);
    assert_eq!(order.fiat_amount, 100);
    assert!(order.trade_keys.is_some());
}

#[tokio::test]
async fn test_order_get_by_id() {
    let pool = create_test_db().await.unwrap();
    let trade_keys = Keys::generate();

    let mut small_order = SmallOrder::default();
    let order_id = uuid::Uuid::new_v4();
    small_order.id = Some(order_id);
    small_order.kind = Some(mostro_core::order::Kind::Sell);
    small_order.fiat_code = "EUR".to_string();
    small_order.amount = 50000;
    small_order.fiat_amount = 50;
    small_order.payment_method = "paypal".to_string();
    small_order.premium = 3;

    let created_order = Order::new(&pool, small_order, &trade_keys, None)
        .await
        .unwrap();
    let order_id_str = created_order.id.as_ref().unwrap();

    let retrieved_order = Order::get_by_id(&pool, order_id_str).await.unwrap();

    assert_eq!(retrieved_order.id, created_order.id);
    assert_eq!(retrieved_order.fiat_code, "EUR");
    assert_eq!(retrieved_order.amount, 50000);
}

#[tokio::test]
async fn test_order_get_by_id_not_found() {
    let pool = create_test_db().await.unwrap();
    let result = Order::get_by_id(&pool, "nonexistent-id").await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_order_update_existing() {
    let pool = create_test_db().await.unwrap();
    let trade_keys = Keys::generate();

    let mut small_order = SmallOrder::default();
    let order_id = uuid::Uuid::new_v4();
    small_order.id = Some(order_id);
    small_order.kind = Some(mostro_core::order::Kind::Buy);
    small_order.fiat_code = "USD".to_string();
    small_order.amount = 100000;
    small_order.fiat_amount = 100;
    small_order.payment_method = "bank".to_string();
    small_order.premium = 5;

    // Create order
    let order1 = Order::new(&pool, small_order.clone(), &trade_keys, None)
        .await
        .unwrap();

    // Update with same ID but different data
    small_order.amount = 200000;
    small_order.fiat_amount = 200;
    let order2 = Order::new(&pool, small_order, &trade_keys, None)
        .await
        .unwrap();

    // Should have same ID but updated values
    assert_eq!(order1.id, order2.id);
    assert_eq!(order2.amount, 200000);
    assert_eq!(order2.fiat_amount, 200);
}
