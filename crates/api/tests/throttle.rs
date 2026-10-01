//! The per-token throttle (D43): at most 20 ticks a second per token under a burst, leading
//! edge, and the last value always delivered.

use api::{MIN_GAP_MS, Throttle};
use proto::api::v1::TokenTick;

fn tick(token: &str, n: u64) -> TokenTick {
    TokenTick {
        token: token.to_string(),
        display_price_usd: n.to_string(),
        block_number: n,
        ..TokenTick::default()
    }
}

/// Offers one tick per token every millisecond for `burst_ms`, then lets time run on; returns
/// each delivery as (time, tick).
fn burst(tokens: &[&str], burst_ms: u64) -> Vec<(u64, TokenTick)> {
    let mut throttle = Throttle::default();
    let mut delivered = Vec::new();
    for now in 0..burst_ms + 1000 {
        if now < burst_ms {
            for token in tokens {
                if let Some(t) = throttle.offer(tick(token, now), now) {
                    delivered.push((now, t));
                }
            }
        }
        for t in throttle.due(now) {
            delivered.push((now, t));
        }
    }
    delivered
}

#[test]
fn a_burst_is_held_to_20_a_second_per_token_with_the_last_value_delivered() {
    let tokens = ["0xaa", "0xbb"];
    let delivered = burst(&tokens, 3000);
    for token in tokens {
        let times: Vec<u64> = delivered
            .iter()
            .filter(|(_, t)| t.token == token)
            .map(|(now, _)| *now)
            .collect();
        // No more than 20 in any one-second window.
        for (i, start) in times.iter().enumerate() {
            let in_window = times[i..].iter().take_while(|t| **t < start + 1000).count();
            assert!(
                in_window <= 20,
                "{token}: {in_window} ticks in the second from {start}"
            );
        }
        // The throttle doesn't starve the token: 20 a second through the burst.
        assert_eq!(
            times.len(),
            3000 / MIN_GAP_MS as usize + 1,
            "{token}: {times:?}"
        );
        // The last value offered is the last one delivered.
        let last = delivered
            .iter()
            .rev()
            .find(|(_, t)| t.token == token)
            .unwrap();
        assert_eq!(last.1.block_number, 2999);
    }
}

#[test]
fn the_first_tick_after_a_quiet_spell_goes_out_at_once() {
    let mut throttle = Throttle::default();
    assert!(throttle.offer(tick("0xaa", 1), 1000).is_some());
    assert!(
        throttle
            .offer(tick("0xaa", 2), 1000 + MIN_GAP_MS - 1)
            .is_none()
    );
    assert!(throttle.due(1000 + MIN_GAP_MS - 1).is_empty());
    // A tick at the end of the gap replaces the one held, and goes out.
    let sent = throttle.offer(tick("0xaa", 3), 1000 + MIN_GAP_MS).unwrap();
    assert_eq!(sent.block_number, 3);
    assert!(
        throttle.due(5000).is_empty(),
        "the held tick was merged into the one sent"
    );
}
