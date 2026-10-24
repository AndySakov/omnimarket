//! Trigger orders (#86) over recorded price paths: each crossing fires once (D35), and a book
//! rebuilt from its stored orders mid-path, as after a restart, neither fires again nor skips.

use alloy_primitives::{Address, keccak256};
use triggers::{Book, Kind, Order, State, firing_id};

const MEME: Address = Address::repeat_byte(0x30);

fn order(id: &str, kind: Kind, level: f64) -> Order {
    Order {
        order_id: id.into(),
        account_id: "guest-1".into(),
        token: MEME,
        kind,
        level_usd: level,
        expires_at_ms: 0,
        state: State::Open,
        firing_count: 0,
    }
}

/// MEME's display price, one per canonical block from 100, falling through $0.008 and back.
const PATH: [f64; 6] = [0.010, 0.009, 0.0081, 0.0079, 0.0075, 0.0085];

/// The orders that fire on each price of `path`, from block `first`.
fn play(book: &mut Book, path: &[f64], first: u64) -> Vec<(u64, String)> {
    let mut fired = Vec::new();
    for (i, price) in path.iter().enumerate() {
        let block = first + i as u64;
        for firing in book.on_price(MEME, *price, block, block * 2_000) {
            fired.push((block, firing.order_id));
        }
    }
    fired
}

#[test]
fn a_price_path_crossing_a_stop_fires_it_once() {
    let mut book = Book::default();
    book.insert(order("stop", Kind::StopLoss, 0.008));
    // It crosses at block 103 ($0.0079) and stays below at 104: one firing.
    assert_eq!(play(&mut book, &PATH, 100), vec![(103, "stop".to_string())]);
    let stop = book.get("stop").unwrap();
    assert_eq!((stop.state, stop.firing_count), (State::Firing, 1));
}

#[test]
fn a_restart_mid_path_neither_fires_again_nor_skips() {
    let mut book = Book::default();
    book.insert(order("stop", Kind::StopLoss, 0.008));
    book.insert(order("deeper", Kind::StopLoss, 0.0076));
    let before = play(&mut book, &PATH[..4], 100);
    assert_eq!(before, vec![(103, "stop".to_string())]);

    // The store keeps each order with its state and firing count; a restart rebuilds the book
    // from them and carries on along the path.
    let stored: Vec<Order> = book.orders().cloned().collect();
    let mut rebuilt = Book::default();
    for order in stored {
        rebuilt.insert(order);
    }
    let after = play(&mut rebuilt, &PATH[4..], 104);
    // The stop that already fired stays fired; the deeper one fires when its level comes.
    assert_eq!(after, vec![(104, "deeper".to_string())]);
    assert_eq!(rebuilt.get("stop").unwrap().firing_count, 1);
}

#[test]
fn each_kind_fires_on_its_side_of_the_level_nearest_first() {
    let mut book = Book::default();
    book.insert(order("tp", Kind::TakeProfit, 0.012));
    book.insert(order("tp-low", Kind::TakeProfit, 0.011));
    book.insert(order("buy", Kind::LimitBuy, 0.009));
    // Below every level but the buy's: the buy fires, the take-profits don't.
    assert_eq!(
        book.on_price(MEME, 0.0085, 1, 0)
            .iter()
            .map(|f| f.order_id.as_str())
            .collect::<Vec<_>>(),
        vec!["buy"]
    );
    // Through both take-profits: the level nearest the price first.
    assert_eq!(
        book.on_price(MEME, 0.013, 2, 0)
            .iter()
            .map(|f| f.order_id.as_str())
            .collect::<Vec<_>>(),
        vec!["tp", "tp-low"]
    );
    // Exactly at a level fires it.
    let mut book = Book::default();
    book.insert(order("at", Kind::StopLoss, 0.008));
    assert_eq!(book.on_price(MEME, 0.008, 1, 0).len(), 1);
}

#[test]
fn a_cancelled_or_expired_order_never_fires() {
    let mut book = Book::default();
    book.insert(order("cancelled", Kind::StopLoss, 0.008));
    book.set_state("cancelled", State::Cancelled);
    book.insert(Order {
        expires_at_ms: 10_000,
        ..order("expiring", Kind::StopLoss, 0.008)
    });
    assert_eq!(book.on_price(MEME, 0.007, 6, 12_000), vec![]);
    assert_eq!(book.get("expiring").unwrap().state, State::Expired);
    // Another token's price touches none of them.
    let mut book = Book::default();
    book.insert(order("stop", Kind::StopLoss, 0.008));
    assert_eq!(
        book.on_price(Address::repeat_byte(0x31), 0.001, 1, 0),
        vec![]
    );
}

#[test]
fn a_firing_id_hashes_the_order_id_and_its_count() {
    let mut input = b"stop".to_vec();
    input.extend(1u64.to_le_bytes());
    assert_eq!(firing_id("stop", 1), keccak256(input).0);
    assert_ne!(firing_id("stop", 1), firing_id("stop", 2));
}

#[test]
fn a_price_that_isnt_a_positive_number_fires_nothing() {
    let mut book = Book::default();
    book.insert(order("stop", Kind::StopLoss, 0.008));
    book.insert(order("tp", Kind::TakeProfit, 0.012));
    for price in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert_eq!(book.on_price(MEME, price, 1, 0), vec![], "{price}");
    }
    assert_eq!(book.get("stop").unwrap().state, State::Open);
}
