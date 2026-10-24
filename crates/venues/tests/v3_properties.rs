//! Properties of `venues::v3::Pool` over random event sequences. The expected state comes from
//! the positions the events opened, not from `Pool`'s own arithmetic.

use std::collections::BTreeMap;

use alloy_primitives::{Address, I256, U256};
use proptest::prelude::*;
use venues::v3::{Event, MAX_TICK, MIN_TICK, Pool, Price, TickLiquidity};

fn price(tick: i32) -> Price {
    Price {
        sqrt_price_x96: U256::from(1u64) << 96,
        tick,
    }
}

/// `Tick.tickSpacingToMaxLiquidityPerTick`: a pool rejects a mint that takes a tick's gross
/// liquidity past this.
fn max_liquidity_per_tick(spacing: i32) -> u128 {
    let min = MIN_TICK / spacing * spacing;
    let max = MAX_TICK / spacing * spacing;
    let ticks = ((max - min) / spacing + 1) as u128;
    u128::MAX / ticks
}

#[derive(Clone, Debug)]
enum Op {
    Mint {
        a: i32,
        b: i32,
        amount: u128,
    },
    /// Burns part of the position at `position` (modulo the open positions).
    Burn {
        position: usize,
        amount: u128,
    },
    Swap {
        tick: i32,
    },
}

/// The open positions, and what the pool's state should be given them.
#[derive(Default)]
struct Positions {
    open: Vec<(i32, i32, u128)>,
}

impl Positions {
    fn ticks(&self) -> BTreeMap<i32, TickLiquidity> {
        let mut ticks = BTreeMap::<i32, TickLiquidity>::new();
        for &(lower, upper, amount) in &self.open {
            let l = ticks.entry(lower).or_default();
            l.gross += amount;
            l.net += amount as i128;
            let u = ticks.entry(upper).or_default();
            u.gross += amount;
            u.net -= amount as i128;
        }
        ticks.retain(|_, t| t.gross > 0);
        ticks
    }

    /// The liquidity active at `tick`: positions whose lower tick the price has crossed, less
    /// those whose upper tick it has crossed too.
    fn active(&self, tick: i32) -> I256 {
        let mut sum = I256::ZERO;
        for &(lower, upper, amount) in &self.open {
            let amount = I256::try_from(amount).unwrap();
            if lower <= tick {
                sum += amount;
            }
            if upper <= tick {
                sum -= amount;
            }
        }
        sum
    }
}

/// Plays `ops` as a pool contract would accept them: a mint the contract would revert (equal
/// ticks, a tick past its maximum liquidity, or active liquidity past `u128`) is skipped, and a
/// burn takes no more than its position holds. Checks `check` after every event.
fn play(
    spacing: i32,
    start: i32,
    ops: &[Op],
    mut check: impl FnMut(&Pool, &Positions) -> Result<(), TestCaseError>,
) -> Result<(Pool, Positions), TestCaseError> {
    let mut pool = Pool::new(Address::ZERO, Address::ZERO, Address::ZERO, 500, spacing);
    pool.apply(Event::Initialize(price(start)));
    let mut positions = Positions::default();
    let mut tick = start;
    let limit = max_liquidity_per_tick(spacing);
    for op in ops {
        let event = match *op {
            Op::Mint { a, b, amount } => {
                let (lower, upper) = (a.min(b), a.max(b));
                let amount = amount.clamp(1, limit);
                let ticks = positions.ticks();
                let gross = |t: i32| ticks.get(&t).map_or(0, |l| l.gross);
                if lower == upper || gross(lower) > limit - amount || gross(upper) > limit - amount
                {
                    continue;
                }
                positions.open.push((lower, upper, amount));
                if positions.active(tick) > I256::try_from(u128::MAX).unwrap() {
                    positions.open.pop();
                    continue;
                }
                Event::Mint {
                    lower,
                    upper,
                    amount,
                }
            }
            Op::Burn { position, amount } => {
                if positions.open.is_empty() {
                    continue;
                }
                let i = position % positions.open.len();
                let (lower, upper, held) = positions.open[i];
                let amount = 1 + amount % held;
                positions.open[i].2 -= amount;
                if positions.open[i].2 == 0 {
                    positions.open.swap_remove(i);
                }
                Event::Burn {
                    lower,
                    upper,
                    amount,
                }
            }
            Op::Swap { tick: to } => {
                tick = to;
                Event::Swap {
                    price: price(to),
                    liquidity: u128::try_from(positions.active(to)).unwrap(),
                }
            }
        };
        pool.apply(event);
        check(&pool, &positions)?;
    }
    Ok((pool, positions))
}

/// A tick for a position: often within a few spacings of `near`, so a range often starts or
/// ends at the price, and sometimes anywhere up to the pool's extreme ticks.
fn position_tick(spacing: i32, near: i32) -> impl Strategy<Value = i32> {
    let (lo, hi) = (-(MAX_TICK / spacing), MAX_TICK / spacing);
    let k = near.div_euclid(spacing);
    prop_oneof![
        4 => (-4i32..=4).prop_map(move |d| (k + d).clamp(lo, hi) * spacing),
        1 => (lo..=hi).prop_map(move |k| k * spacing),
        1 => prop_oneof![Just(lo * spacing), Just(hi * spacing)],
    ]
}

/// A price tick: often on or next to a tick a position can use, sometimes anywhere.
fn price_tick(spacing: i32, near: i32) -> impl Strategy<Value = i32> {
    let k = near.div_euclid(spacing);
    prop_oneof![
        3 => (-4i32..=4).prop_map(move |d| ((k + d) * spacing).clamp(MIN_TICK, MAX_TICK - 1)),
        1 => (-4i32..=4, 0..spacing)
            .prop_map(move |(d, o)| ((k + d) * spacing + o).clamp(MIN_TICK, MAX_TICK - 1)),
        1 => MIN_TICK..MAX_TICK,
    ]
}

fn amount() -> impl Strategy<Value = u128> {
    prop_oneof![
        1u128..=1_000,
        any::<u128>(),
        Just(u128::MAX),
        Just(i128::MAX as u128),
        Just(1u128 << 127),
    ]
}

fn op(spacing: i32, near: i32) -> impl Strategy<Value = Op> {
    prop_oneof![
        3 => (position_tick(spacing, near), position_tick(spacing, near), amount())
            .prop_map(|(a, b, amount)| Op::Mint { a, b, amount }),
        2 => (any::<usize>(), any::<u128>()).prop_map(|(position, amount)| Op::Burn { position, amount }),
        2 => price_tick(spacing, near).prop_map(|tick| Op::Swap { tick }),
    ]
}

fn case() -> impl Strategy<Value = (i32, i32, Vec<Op>)> {
    (
        prop_oneof![Just(1), Just(10), Just(60), Just(200)],
        MIN_TICK..MAX_TICK,
    )
        .prop_flat_map(|(spacing, start)| {
            (
                Just(spacing),
                Just(start),
                proptest::collection::vec(op(spacing, start), 1..40),
            )
        })
}

proptest! {
    #[test]
    fn active_liquidity_is_the_net_of_the_ticks_at_or_below_the_price((spacing, start, ops) in case()) {
        play(spacing, start, &ops, |pool, positions| {
            let tick = pool.price.unwrap().tick;
            let net: I256 = pool
                .ticks
                .range(..=tick)
                .map(|(_, t)| I256::try_from(t.net).unwrap())
                .fold(I256::ZERO, |sum, net| sum + net);
            prop_assert_eq!(I256::try_from(pool.liquidity).unwrap(), net);
            prop_assert_eq!(I256::try_from(pool.liquidity).unwrap(), positions.active(tick));
            prop_assert_eq!(&pool.ticks, &positions.ticks());
            Ok(())
        })?;
    }

    #[test]
    fn a_mint_then_a_burn_of_the_same_range_and_amount_restores_the_pool(
        (spacing, start, ops) in case(),
        a in any::<i32>(),
        b in any::<i32>(),
        amount in amount(),
    ) {
        let (mut pool, positions) = play(spacing, start, &ops, |_, _| Ok(()))?;
        let tick = pool.price.unwrap().tick;
        let (lo, hi) = (-(MAX_TICK / spacing), MAX_TICK / spacing);
        let near = |r: i32| (tick.div_euclid(spacing) + r % 5).clamp(lo, hi) * spacing;
        let (lower, upper) = (near(a).min(near(b)), near(a).max(near(b)));
        let ticks = positions.ticks();
        let gross = |t: i32| ticks.get(&t).map_or(0, |l| l.gross);
        let room = (max_liquidity_per_tick(spacing) - gross(lower).max(gross(upper)))
            .min(u128::MAX - pool.liquidity);
        prop_assume!(lower < upper && room > 0);
        let amount = amount.clamp(1, room);
        let before = pool.clone();
        pool.apply(Event::Mint { lower, upper, amount });
        pool.apply(Event::Burn { lower, upper, amount });
        prop_assert_eq!(pool, before);
    }

    /// Events no pool emits (a burn of liquidity never minted, amounts past `int128`, ticks
    /// off the spacing) still don't panic.
    #[test]
    fn any_event_at_any_tick_and_amount_applies_without_panicking(
        events in proptest::collection::vec(
            prop_oneof![
                (MIN_TICK..=MAX_TICK, MIN_TICK..=MAX_TICK, amount())
                    .prop_map(|(lower, upper, amount)| Event::Mint { lower, upper, amount }),
                (MIN_TICK..=MAX_TICK, MIN_TICK..=MAX_TICK, amount())
                    .prop_map(|(lower, upper, amount)| Event::Burn { lower, upper, amount }),
                (MIN_TICK..=MAX_TICK, amount())
                    .prop_map(|(tick, liquidity)| Event::Swap { price: price(tick), liquidity }),
            ],
            1..40,
        ),
        start in MIN_TICK..=MAX_TICK,
    ) {
        let mut pool = Pool::new(Address::ZERO, Address::ZERO, Address::ZERO, 500, 1);
        pool.apply(Event::Initialize(price(start)));
        for event in events {
            let (_, after) = pool.apply(event);
            if let Event::Mint { lower, upper, .. } | Event::Burn { lower, upper, .. } = event {
                prop_assert_eq!(after.ticks.iter().map(|(t, _)| *t).collect::<Vec<_>>(), vec![lower, upper]);
            }
        }
    }
}
