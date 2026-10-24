//! Quotes against Uniswap's own answers on Base. The pool state is pricing's reference-pools
//! fixture, block 52,036,164 (each v3 pool's initialized ticks within ±3% of its price). The
//! expected outputs are QuoterV2's `quoteExactInputSingle` (0x3d4e44Eb1374240CE5F1B871ab261CD16335B76a)
//! and UniswapV2Router02's `getAmountsOut` (0x4752ba5DBc23f44D87826276BF6Fd6b1C372aD24), called
//! at that block on 2026-10-24. Each swap ends within the captured ticks.

use std::collections::BTreeMap;

use alloy_primitives::{Address, U256, address};
use routing::{Pool, PoolKind};
use venues::v3::{self, Price, TickLiquidity};

const WETH: Address = address!("4200000000000000000000000000000000000006");
const USDC: Address = address!("833589fCD6eDb6E08f4c7C32D4f71b54bdA02913");
const USDT: Address = address!("fde4C96c8593536E31F229EA8f37b2ADa2699bb2");
const WETH_USDC_500: Address = address!("d0b53D9277642d899DF5C87A3966A349A798F224");
const WETH_USDC_3000: Address = address!("6c561B446416E1A00E8E93E221854d6eA4171372");
const WETH_USDT_500: Address = address!("d92E0767473D1E3FF11Ac036f2b1DB90aD0aE55F");
const WETH_USDC_V2: Address = address!("88A43bbDF9D098eEC7bCEda4e2494615dfD9bB9C");

fn tokens(pool: Address) -> (Address, Address) {
    if pool == WETH_USDT_500 {
        (WETH, USDT)
    } else {
        (WETH, USDC)
    }
}

fn pools() -> BTreeMap<Address, Pool> {
    let text = include_str!("../../pricing/tests/fixtures/reference-pools.txt");
    let mut pools = BTreeMap::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split(' ').collect();
        match f[0] {
            "v3" => {
                let address: Address = f[1].parse().unwrap();
                let (token0, token1) = tokens(address);
                let mut pool = v3::Pool::new(
                    address,
                    token0,
                    token1,
                    f[2].parse().unwrap(),
                    f[3].parse().unwrap(),
                );
                pool.price = Some(Price {
                    sqrt_price_x96: f[4].parse().unwrap(),
                    tick: f[5].parse().unwrap(),
                });
                pool.liquidity = f[6].parse().unwrap();
                pools.insert(
                    address,
                    Pool {
                        address,
                        token0,
                        token1,
                        kind: PoolKind::V3(pool),
                    },
                );
            }
            "tick" => {
                let address: Address = f[1].parse().unwrap();
                let PoolKind::V3(pool) = &mut pools.get_mut(&address).unwrap().kind else {
                    panic!("a tick of a v2 pair");
                };
                pool.ticks.insert(
                    f[2].parse().unwrap(),
                    TickLiquidity {
                        gross: f[3].parse().unwrap(),
                        net: f[4].parse().unwrap(),
                    },
                );
            }
            "v2" => {
                let address: Address = f[1].parse().unwrap();
                pools.insert(
                    address,
                    Pool {
                        address,
                        token0: WETH,
                        token1: USDC,
                        kind: PoolKind::V2 {
                            reserve0: f[2].parse().unwrap(),
                            reserve1: f[3].parse().unwrap(),
                        },
                    },
                );
            }
            _ => {}
        }
    }
    pools
}

fn out(pools: &BTreeMap<Address, Pool>, pool: Address, token_in: Address, amount: u128) -> U256 {
    pools[&pool]
        .amount_out(token_in, U256::from(amount))
        .unwrap()
}

#[test]
fn v3_quotes_match_the_quoter_to_the_unit() {
    let pools = pools();
    // (pool, token in, amount in, QuoterV2's amount out)
    let cases: [(Address, Address, u128, u128); 11] = [
        (WETH_USDC_500, WETH, 10u128.pow(16), 26_886_970),
        (WETH_USDC_3000, WETH, 10u128.pow(16), 26_821_908),
        (WETH_USDC_500, WETH, 10u128.pow(18), 2_688_600_760),
        (WETH_USDC_3000, WETH, 10u128.pow(18), 2_682_187_811),
        // Crosses 3 initialized ticks.
        (WETH_USDC_500, WETH, 20 * 10u128.pow(18), 53_735_133_121),
        (WETH_USDC_3000, WETH, 20 * 10u128.pow(18), 53_642_576_441),
        (WETH_USDC_500, USDC, 100_000_000, 37_155_489_817_407_891),
        (WETH_USDC_3000, USDC, 100_000_000, 37_059_590_024_055_202),
        // $50,000 the other way: crosses 2.
        (
            WETH_USDC_500,
            USDC,
            50_000_000_000,
            18_565_285_321_963_785_578,
        ),
        (
            WETH_USDC_3000,
            USDC,
            50_000_000_000,
            18_529_397_171_878_995_104,
        ),
        // A thin pool: 1,000 USDT crosses 4.
        (WETH_USDT_500, USDT, 1_000_000_000, 366_888_524_196_632_934),
    ];
    for (pool, token_in, amount, quoter) in cases {
        assert_eq!(
            out(&pools, pool, token_in, amount),
            U256::from(quoter),
            "{amount} of {token_in} through {pool}"
        );
    }
}

#[test]
fn v2_quotes_match_the_router() {
    let pools = pools();
    assert_eq!(
        out(&pools, WETH_USDC_V2, WETH, 10u128.pow(18)),
        U256::from(2_670_852_554u64)
    );
    assert_eq!(
        out(&pools, WETH_USDC_V2, USDC, 1_000_000_000),
        U256::from(370_127_776_640_096_144u128)
    );
}

/// Quote latency on the reference pools (four WETH/stablecoin pools, two of them v3 walking
/// ticks), routing 1 WETH to USDC and 3,000 USDC to WETH, direct and through hubs. Prints p50
/// and p99 for verification.md: `cargo test --release -p routing --test quoter -- --ignored
/// --nocapture`.
#[test]
#[ignore = "a measurement: run in release with --nocapture"]
#[allow(clippy::disallowed_methods)] // the measurement's own wall clock
fn quote_latency() {
    let pools = pools();
    let mut times = Vec::new();
    for i in 0..20_000u64 {
        let (token_in, token_out, amount) = if i % 2 == 0 {
            (WETH, USDC, U256::from(10u128.pow(18)))
        } else {
            (USDC, WETH, U256::from(3_000_000_000u64))
        };
        let start = std::time::Instant::now();
        let best = routing::routes(&pools, token_in, token_out, amount, &[WETH, USDC])
            .into_iter()
            .max_by_key(|r| r.amount_out());
        times.push(start.elapsed());
        assert!(best.is_some());
    }
    times.sort();
    let at = |q: f64| times[((times.len() as f64) * q) as usize - 1];
    println!("quote latency: p50 {:?}, p99 {:?}", at(0.50), at(0.99));
}
