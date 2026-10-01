//! Pricing against pricing.md and D18, D19, D24, on Base's reference pools as they were at
//! block 52,036,164 (`tests/fixtures/reference-pools.txt`, from `capture.py`).

use std::collections::BTreeMap;

use alloy_primitives::{Address, U256, address};
use pricing::config::{BASE_USDC, BASE_WETH};
use pricing::{PoolQuote, PricingConfig, display_price, pool};
use venues::v2::Reserves;
use venues::v3::{Price, TickLiquidity};

const WETH_DECIMALS: u8 = 18;
const USDC_DECIMALS: u8 = 6;
const FLOOR_USD: f64 = 10_000.0;

enum Recorded {
    V3 {
        price: Price,
        liquidity: u128,
        ticks: BTreeMap<i32, TickLiquidity>,
    },
    V2(Reserves),
}

fn recorded_pools() -> BTreeMap<Address, Recorded> {
    let text = include_str!("fixtures/reference-pools.txt");
    let mut pools = BTreeMap::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        let address = |s: &str| s.parse::<Address>().unwrap();
        match f[0] {
            "v3" => {
                pools.insert(
                    address(f[1]),
                    Recorded::V3 {
                        price: Price {
                            sqrt_price_x96: f[4].parse::<U256>().unwrap(),
                            tick: f[5].parse().unwrap(),
                        },
                        liquidity: f[6].parse().unwrap(),
                        ticks: BTreeMap::new(),
                    },
                );
            }
            "tick" => {
                let Some(Recorded::V3 { ticks, .. }) = pools.get_mut(&address(f[1])) else {
                    panic!("a tick before its pool");
                };
                ticks.insert(
                    f[2].parse().unwrap(),
                    TickLiquidity {
                        gross: f[3].parse().unwrap(),
                        net: f[4].parse().unwrap(),
                    },
                );
            }
            "v2" => {
                pools.insert(
                    address(f[1]),
                    Recorded::V2(Reserves {
                        reserve0: f[2].parse().unwrap(),
                        reserve1: f[3].parse().unwrap(),
                    }),
                );
            }
            _ => {}
        }
    }
    pools
}

/// WETH (token0) priced in a WETH/stablecoin pool: dollars per WETH, depth in dollars.
fn weth_quote(address: Address, recorded: &Recorded) -> PoolQuote {
    let priced = match recorded {
        Recorded::V3 {
            price,
            liquidity,
            ticks,
        } => pool::v3(
            Some(*price),
            *liquidity,
            ticks,
            WETH_DECIMALS,
            USDC_DECIMALS,
        ),
        Recorded::V2(reserves) => pool::v2(*reserves, WETH_DECIMALS, USDC_DECIMALS),
    }
    .unwrap();
    let (price, depth) = priced.for_token(true);
    PoolQuote {
        pool: address,
        quote: BASE_USDC,
        price_in_quote: price,
        price_usd: price,
        depth_usd: depth,
    }
}

fn assert_close(actual: f64, expected: f64, what: &str) {
    let error = (actual - expected).abs() / expected;
    assert!(
        error < 1e-9,
        "{what}: {actual} vs {expected} (relative error {error:e})"
    );
}

const POOL_500: Address = address!("d0b53D9277642d899DF5C87A3966A349A798F224");
const POOL_3000: Address = address!("6c561B446416E1A00E8E93E221854d6eA4171372");
const USDT_500: Address = address!("d92E0767473D1E3FF11Ac036f2b1DB90aD0aE55F");
const V2_PAIR: Address = address!("88A43bbDF9D098eEC7bCEda4e2494615dfD9bB9C");

// The expected values are worked out independently of this crate, in 60-digit decimals from
// the same recorded state: price = (sqrtPriceX96 / 2^96)² · 10^(18−6); depth = USDC in to move
// the price up 2% plus WETH in to move it down 2% at the mid, walking the recorded ticks with
// sqrt(1.0001^tick) boundaries (D24).
const EXPECTED: [(Address, f64, f64); 4] = [
    (POOL_500, 2690.043009161581, 1_414_034.680748),
    (POOL_3000, 2690.261701368983, 45_664_456.972589),
    (USDT_500, 2691.93651260199, 1_679.152362),
    (V2_PAIR, 2689.648163531152, 13_422.627973),
];

// pricing.md: each pool's mid and ±2% depth, from v3 ticks or v2 reserves.
#[test]
fn reference_pools_price_and_depth_match_the_formula() {
    let pools = recorded_pools();
    for (address, price, depth) in EXPECTED {
        let quote = weth_quote(address, &pools[&address]);
        assert_close(quote.price_usd, price, &format!("{address} price"));
        assert_close(quote.depth_usd, depth, &format!("{address} depth"));
    }
}

// #76: "The WETH display price tracks the reference pool". D19: the native token's USD price is
// the liquidity-weighted mid of the reference pools, pools below the floor left out (D18).
#[test]
fn the_weth_display_price_is_the_weighted_mid_of_the_reference_pools() {
    let pools = recorded_pools();
    let config = PricingConfig::base();
    assert_eq!(
        config.reference_pools,
        [POOL_500, POOL_3000, USDT_500, V2_PAIR],
        "the fixture holds the configured reference pools"
    );
    let quotes: Vec<PoolQuote> = config
        .reference_pools
        .iter()
        .map(|address| weth_quote(*address, &pools[address]))
        .collect();
    let weth = display_price(&quotes, FLOOR_USD).unwrap();

    // The USDT pool's $1,679 of depth is below the $10,000 floor, so it doesn't count.
    let counted = [EXPECTED[0], EXPECTED[1], EXPECTED[3]];
    let expected = counted.iter().map(|(_, p, d)| p * d).sum::<f64>()
        / counted.iter().map(|(_, _, d)| d).sum::<f64>();
    assert_close(weth.price_usd, expected, "WETH display price");
    assert_close(
        weth.price_usd,
        2690.254959794327,
        "WETH display price, pinned",
    );
    assert!(!weth.thin);
    assert_eq!(weth.main.pool, POOL_3000, "the deepest reference pool");
    // Within a basis point of the deepest pool it follows.
    assert!((weth.price_usd / weth.main.price_usd - 1.0).abs() < 1e-4);
    assert_eq!(
        BASE_WETH,
        address!("4200000000000000000000000000000000000006")
    );
}

// #76: "A token with two pools of different depth gets the weighted mid". D18:
// price = Σ mid·depth / Σ depth over the pools above the floor.
#[test]
fn a_token_with_two_pools_gets_the_depth_weighted_mid() {
    let shallow = PoolQuote {
        pool: address!("1000000000000000000000000000000000000001"),
        quote: BASE_WETH,
        price_in_quote: 0.0004,
        price_usd: 1.10,
        depth_usd: 20_000.0,
    };
    let deep = PoolQuote {
        pool: address!("2000000000000000000000000000000000000002"),
        price_usd: 1.00,
        depth_usd: 80_000.0,
        ..shallow
    };
    let price = display_price(&[shallow, deep], FLOOR_USD).unwrap();
    // (1.10 · 20,000 + 1.00 · 80,000) / 100,000
    assert_close(price.price_usd, 1.02, "weighted mid");
    assert_eq!(price.depth_usd, 100_000.0);
    assert_eq!(price.main.pool, deep.pool);
    assert!(!price.thin);

    // Order doesn't matter.
    let reversed = display_price(&[deep, shallow], FLOOR_USD).unwrap();
    assert_eq!(reversed.price_usd.to_bits(), price.price_usd.to_bits());
}

// D18: "Pools below the liquidity floor don't count toward the weighted mid."
#[test]
fn a_pool_below_the_floor_does_not_move_the_price() {
    let deep = PoolQuote {
        pool: address!("1000000000000000000000000000000000000001"),
        quote: BASE_USDC,
        price_in_quote: 1.0,
        price_usd: 1.0,
        depth_usd: 50_000.0,
    };
    let manipulated = PoolQuote {
        pool: address!("2000000000000000000000000000000000000002"),
        price_usd: 1_000.0,
        depth_usd: 9_999.0,
        ..deep
    };
    let price = display_price(&[deep, manipulated], FLOOR_USD).unwrap();
    assert_eq!(price.price_usd, 1.0);
    assert!(!price.thin);
}

// D18, D20: no pool above the floor → the deepest pool's price, flagged thin.
#[test]
fn a_token_with_no_pool_above_the_floor_is_thin_and_priced_from_its_deepest_pool() {
    let a = PoolQuote {
        pool: address!("1000000000000000000000000000000000000001"),
        quote: BASE_WETH,
        price_in_quote: 0.001,
        price_usd: 3.0,
        depth_usd: 900.0,
    };
    let b = PoolQuote {
        pool: address!("2000000000000000000000000000000000000002"),
        price_usd: 2.0,
        depth_usd: 4_000.0,
        ..a
    };
    let price = display_price(&[a, b], FLOOR_USD).unwrap();
    assert!(price.thin);
    assert_eq!(price.price_usd, 2.0);
    assert_eq!(price.main.pool, b.pool);
    assert_eq!(price.depth_usd, 4_900.0);
}

#[test]
fn no_usable_pool_gives_no_price() {
    assert_eq!(display_price(&[], FLOOR_USD), None);
    let broken = PoolQuote {
        pool: Address::ZERO,
        quote: BASE_WETH,
        price_in_quote: f64::NAN,
        price_usd: f64::INFINITY,
        depth_usd: 1e9,
    };
    assert_eq!(display_price(&[broken], FLOOR_USD), None);
}
