//! Venue state against real Base blocks (`tests/fixtures/base-blocks.txt`, from `capture.py`):
//! each case starts from a pool's state at block N-1, applies block N's logs, and must reach the
//! state the chain answered at N.

use std::collections::BTreeMap;

use alloy_primitives::{Address, B256, Bytes, U256};
use types::chain::Log;
use venues::v2::{self, Reserves};
use venues::v3::{self, Event, Pool, Price, TickLiquidity};

#[derive(Debug, Default)]
struct V3State {
    price: Option<Price>,
    liquidity: u128,
    ticks: BTreeMap<i32, TickLiquidity>,
}

#[derive(Debug)]
struct V3Case {
    block: u64,
    pool: Address,
    spacing: i32,
    before: V3State,
    logs: Vec<Log>,
    after: V3State,
}

#[derive(Debug)]
struct V2Case {
    block: u64,
    pair: Address,
    before: Reserves,
    logs: Vec<Log>,
    after: Reserves,
}

fn parse() -> (Vec<V3Case>, Vec<V2Case>) {
    let text = include_str!("fixtures/base-blocks.txt");
    let (mut v3s, mut v2s) = (Vec::new(), Vec::new());
    let mut lines = text.lines().filter(|l| !l.starts_with('#')).peekable();
    while let Some(line) = lines.next() {
        let head: Vec<&str> = line.split(' ').collect();
        let block = head[1].parse().unwrap();
        let address: Address = head[2].parse().unwrap();
        let mut body = Vec::new();
        while let Some(next) = lines.next_if(|l| !l.starts_with("v3 ") && !l.starts_with("v2 ")) {
            body.push(next);
        }
        let logs: Vec<Log> = body
            .iter()
            .filter_map(|l| l.strip_prefix("log "))
            .map(|l| log(address, l))
            .collect();
        match head[0] {
            "v3" => {
                let mut states = [V3State::default(), V3State::default()];
                let mut at = None;
                for l in &body {
                    let f: Vec<&str> = l.split(' ').collect();
                    match f[0] {
                        "state" => {
                            at = Some(at.map_or(0, |_| 1));
                            let s = &mut states[at.unwrap()];
                            s.price = Some(Price {
                                sqrt_price_x96: f[1].parse::<U256>().unwrap(),
                                tick: f[2].parse().unwrap(),
                            });
                            s.liquidity = f[3].parse().unwrap();
                        }
                        "tick" => {
                            states[at.unwrap()].ticks.insert(
                                f[1].parse().unwrap(),
                                TickLiquidity {
                                    gross: f[2].parse().unwrap(),
                                    net: f[3].parse().unwrap(),
                                },
                            );
                        }
                        _ => {}
                    }
                }
                let [before, after] = states;
                v3s.push(V3Case {
                    block,
                    pool: address,
                    spacing: head[3].parse().unwrap(),
                    before,
                    logs,
                    after,
                });
            }
            "v2" => {
                let reserves: Vec<Reserves> = body
                    .iter()
                    .filter_map(|l| l.strip_prefix("reserves "))
                    .map(|l| {
                        let (r0, r1) = l.split_once(' ').unwrap();
                        Reserves {
                            reserve0: r0.parse().unwrap(),
                            reserve1: r1.parse().unwrap(),
                        }
                    })
                    .collect();
                v2s.push(V2Case {
                    block,
                    pair: address,
                    before: reserves[0],
                    logs,
                    after: reserves[1],
                });
            }
            other => panic!("unknown case {other}"),
        }
    }
    (v3s, v2s)
}

fn log(address: Address, line: &str) -> Log {
    let (topics, data) = line.split_once(' ').unwrap();
    Log {
        address,
        topics: topics
            .split(',')
            .map(|t| t.parse::<B256>().unwrap())
            .collect(),
        data: data.parse::<Bytes>().unwrap(),
        log_index: 0,
        transaction_hash: B256::ZERO,
    }
}

#[test]
fn v3_pools_reach_the_state_base_answered_after_each_block() {
    let (cases, _) = parse();
    for case in &cases {
        let mut pool = Pool::new(case.pool, Address::ZERO, Address::ZERO, 0, case.spacing);
        pool.price = case.before.price;
        pool.liquidity = case.before.liquidity;
        pool.ticks = case
            .before
            .ticks
            .iter()
            .filter(|(_, t)| t.gross > 0)
            .map(|(k, t)| (*k, *t))
            .collect();
        for log in &case.logs {
            let event = v3::decode(log)
                .unwrap_or_else(|| panic!("block {}: a log that doesn't decode", case.block));
            pool.apply(event);
        }
        let at = format!("block {} pool {}", case.block, case.pool);
        assert_eq!(pool.price, case.after.price, "{at}");
        assert_eq!(pool.liquidity, case.after.liquidity, "{at}");
        for (tick, expected) in &case.after.ticks {
            assert_eq!(
                pool.ticks.get(tick).copied().unwrap_or_default(),
                *expected,
                "{at} tick {tick}"
            );
            assert_eq!(
                pool.ticks.contains_key(tick),
                expected.gross > 0,
                "{at} tick {tick}"
            );
        }
    }
}

#[test]
fn v2_pairs_reach_the_reserves_base_answered_after_each_block() {
    let (_, cases) = parse();
    for case in &cases {
        let mut reserves = case.before;
        for log in &case.logs {
            assert!(v2::is_sync(log), "block {}", case.block);
            reserves = v2::decode_sync(log).unwrap();
        }
        assert_eq!(
            reserves, case.after,
            "block {} pair {}",
            case.block, case.pair
        );
    }
}

/// The capture keeps blocks until it has these. A recapture that loses one fails here rather
/// than silently testing less.
#[test]
fn the_fixtures_cover_each_tick_spacing_and_both_range_edges() {
    let (v3s, v2s) = parse();
    assert!(v3s.len() >= 20, "{} v3 blocks", v3s.len());
    for spacing in [1, 10, 60] {
        let changing = v3s
            .iter()
            .filter(|c| c.spacing == spacing)
            .filter(|c| c.logs.iter().any(changes_liquidity))
            .count();
        assert!(
            changing >= 5,
            "{changing} liquidity changes at spacing {spacing}"
        );
    }
    let empties = v3s.iter().any(|c| {
        c.logs.iter().any(|l| match v3::decode(l) {
            Some(Event::Burn {
                lower,
                upper,
                amount,
            }) if amount > 0 => [lower, upper]
                .iter()
                .any(|t| c.before.ticks[t].gross > 0 && c.after.ticks[t].gross == 0),
            _ => false,
        })
    });
    assert!(empties, "no burn empties a tick");
    assert!(
        v3s.iter().any(starts_at_the_price),
        "no mint or burn starts at the price"
    );
    let syncs: usize = v2s.iter().map(|c| c.logs.len()).sum();
    assert!(syncs >= 10, "{syncs} syncs");
}

fn changes_liquidity(log: &Log) -> bool {
    matches!(
        v3::decode(log),
        Some(Event::Mint { amount, .. } | Event::Burn { amount, .. }) if amount > 0
    )
}

/// Mints and burns whose lower tick is the price when they land, changing the pool's liquidity
/// on net (a mint and an equal burn cancel), with no swap after them to reset it. A range rule
/// that leaves the lower tick out gets these blocks wrong.
fn starts_at_the_price(case: &V3Case) -> bool {
    let mut tick = case.before.price.unwrap().tick;
    let mut net: i128 = 0;
    for log in &case.logs {
        match v3::decode(log) {
            Some(Event::Swap { price, .. }) => {
                tick = price.tick;
                net = 0;
            }
            Some(Event::Mint { lower, amount, .. }) if lower == tick => net += amount as i128,
            Some(Event::Burn { lower, amount, .. }) if lower == tick => net -= amount as i128,
            _ => {}
        }
    }
    net != 0
}
