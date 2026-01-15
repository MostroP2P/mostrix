// Integration tests for parsing functions
use mostrix::util::order_utils::order_from_tags;
use nostr_sdk::prelude::{Tag, Tags};

#[test]
fn test_order_from_tags_basic() {
    let mut tags = Tags::new();
    tags.push(Tag::parse(["d", "123e4567-e89b-12d3-a456-426614174000"]).unwrap());
    tags.push(Tag::parse(["k", "buy"]).unwrap());
    tags.push(Tag::parse(["f", "USD"]).unwrap());
    tags.push(Tag::parse(["amt", "100000"]).unwrap());
    tags.push(Tag::parse(["fa", "100"]).unwrap());
    tags.push(Tag::parse(["pm", "bank_transfer"]).unwrap());
    tags.push(Tag::parse(["premium", "5"]).unwrap());

    let order = order_from_tags(tags).unwrap();
    assert_eq!(order.fiat_code, "USD");
    assert_eq!(order.amount, 100000);
    assert_eq!(order.fiat_amount, 100);
    assert_eq!(order.payment_method, "bank_transfer");
    assert_eq!(order.premium, 5);
    assert!(order.kind.is_some());
}

#[test]
fn test_order_from_tags_with_range() {
    let mut tags = Tags::new();
    tags.push(Tag::parse(["d", "123e4567-e89b-12d3-a456-426614174001"]).unwrap());
    tags.push(Tag::parse(["k", "sell"]).unwrap());
    tags.push(Tag::parse(["f", "EUR"]).unwrap());
    tags.push(Tag::parse(["amt", "50000"]).unwrap());
    tags.push(Tag::parse(["fa", "50", "200"]).unwrap()); // min_amount, max_amount
    tags.push(Tag::parse(["pm", "paypal"]).unwrap());

    let order = order_from_tags(tags).unwrap();
    assert_eq!(order.fiat_code, "EUR");
    assert_eq!(order.amount, 50000);
    assert_eq!(order.min_amount, Some(50));
    assert_eq!(order.max_amount, Some(200));
    assert!(order.kind.is_some());
}
