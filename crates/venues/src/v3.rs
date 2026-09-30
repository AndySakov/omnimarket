//! Uniswap v3 pools: price, active liquidity and the tick table, from events (D21).

use std::collections::BTreeMap;

use alloy_primitives::{Address, B256, U256, keccak256};
use alloy_sol_types::{SolCall, SolEvent, SolValue, sol};
use types::chain::Log;

sol! {
    event PoolCreated(address indexed token0, address indexed token1, uint24 indexed fee, int24 tickSpacing, address pool);
    event Initialize(uint160 sqrtPriceX96, int24 tick);
    event Swap(address indexed sender, address indexed recipient, int256 amount0, int256 amount1, uint160 sqrtPriceX96, uint128 liquidity, int24 tick);
    event Mint(address sender, address indexed owner, int24 indexed tickLower, int24 indexed tickUpper, uint128 amount, uint256 amount0, uint256 amount1);
    event Burn(address indexed owner, int24 indexed tickLower, int24 indexed tickUpper, uint128 amount, uint256 amount0, uint256 amount1);

    function token0() external view returns (address);
    function token1() external view returns (address);
    function fee() external view returns (uint24);
    function tickSpacing() external view returns (int24);
    function slot0() external view returns (uint160 sqrtPriceX96, int24 tick, uint16 observationIndex, uint16 observationCardinality, uint16 observationCardinalityNext, uint8 feeProtocol, bool unlocked);
    function liquidity() external view returns (uint128);
    function tickBitmap(int16 wordPosition) external view returns (uint256);
    function ticks(int24 tick) external view returns (uint128 liquidityGross, int128 liquidityNet, uint256 feeGrowthOutside0X128, uint256 feeGrowthOutside1X128, int56 tickCumulativeOutside, uint160 secondsPerLiquidityOutsideX128, uint32 secondsOutside, bool initialized);

    struct PopulatedTick {
        int24 tick;
        int128 liquidityNet;
        uint128 liquidityGross;
    }
    function getPopulatedTicksInWord(address pool, int16 tickBitmapIndex) external view returns (PopulatedTick[] populatedTicks);
}

pub const MIN_TICK: i32 = -887_272;
pub const MAX_TICK: i32 = 887_272;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Deployment {
    pub factory: Address,
    pub init_code_hash: B256,
    /// Uniswap's TickLens: returns a bitmap word's initialized ticks in one call.
    pub tick_lens: Address,
}

/// Uniswap v3 on Base (D80: checked against a live pool in the tests).
pub const BASE: Deployment = Deployment {
    factory: alloy_primitives::address!("33128a8fC17869897dcE68Ed026d694621f6FDfD"),
    init_code_hash: alloy_primitives::b256!(
        "e34f199b19b2b4f47f68442619d555527d244f78a3297ea89325f843f87b8b54"
    ),
    tick_lens: alloy_primitives::address!("0CdeE061c75D43c82520eD998C23ac2991c9ac6d"),
};

impl Deployment {
    /// The address the factory creates the pool for two tokens and a fee at (CREATE2).
    pub fn pool_address(&self, token0: Address, token1: Address, fee: u32) -> Address {
        let fee = alloy_primitives::aliases::U24::from(fee);
        let salt = keccak256((token0, token1, fee).abi_encode());
        self.factory.create2(salt, self.init_code_hash)
    }
}

/// One initialized tick: the liquidity referencing it, and the change in active liquidity
/// when the price crosses it upwards.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TickLiquidity {
    pub gross: u128,
    pub net: i128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Price {
    pub sqrt_price_x96: U256,
    pub tick: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pool {
    pub address: Address,
    pub token0: Address,
    pub token1: Address,
    pub fee: u32,
    pub tick_spacing: i32,
    /// `None` until the pool is initialized.
    pub price: Option<Price>,
    pub liquidity: u128,
    pub ticks: BTreeMap<i32, TickLiquidity>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Initialize(Price),
    Swap {
        price: Price,
        liquidity: u128,
    },
    Mint {
        lower: i32,
        upper: i32,
        amount: u128,
    },
    Burn {
        lower: i32,
        upper: i32,
        amount: u128,
    },
}

/// What one event changed: enough to publish before and after, and to undo it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub price: Option<Price>,
    pub liquidity: u128,
    /// Only the ticks the event touched. An absent tick has zero liquidity.
    pub ticks: Vec<(i32, TickLiquidity)>,
}

impl Pool {
    /// The pool as its factory creates it: uninitialized, no liquidity.
    pub fn new(
        address: Address,
        token0: Address,
        token1: Address,
        fee: u32,
        tick_spacing: i32,
    ) -> Self {
        Self {
            address,
            token0,
            token1,
            fee,
            tick_spacing,
            price: None,
            liquidity: 0,
            ticks: BTreeMap::new(),
        }
    }

    fn snapshot(&self, touched: &[i32]) -> Snapshot {
        Snapshot {
            price: self.price,
            liquidity: self.liquidity,
            ticks: touched
                .iter()
                .map(|t| (*t, self.ticks.get(t).copied().unwrap_or_default()))
                .collect(),
        }
    }

    /// Applies one event as the pool contract does, and returns the state before and after.
    pub fn apply(&mut self, event: Event) -> (Snapshot, Snapshot) {
        let touched: Vec<i32> = match event {
            Event::Mint { lower, upper, .. } | Event::Burn { lower, upper, .. } => {
                vec![lower, upper]
            }
            _ => Vec::new(),
        };
        let before = self.snapshot(&touched);
        match event {
            Event::Initialize(price) => self.price = Some(price),
            Event::Swap { price, liquidity } => {
                self.price = Some(price);
                self.liquidity = liquidity;
            }
            Event::Mint {
                lower,
                upper,
                amount,
            } => self.change_position(lower, upper, amount as i128),
            Event::Burn {
                lower,
                upper,
                amount,
            } => self.change_position(lower, upper, -(amount as i128)),
        }
        let after = self.snapshot(&touched);
        (before, after)
    }

    /// `Pool.modifyPosition`: the range's ticks, and the active liquidity if the current tick
    /// is inside the range (`tickLower <= tick < tickUpper`).
    fn change_position(&mut self, lower: i32, upper: i32, delta: i128) {
        if delta == 0 {
            return;
        }
        self.change_tick(lower, delta, delta);
        self.change_tick(upper, delta, -delta);
        if let Some(price) = self.price
            && lower <= price.tick
            && price.tick < upper
        {
            self.liquidity = self.liquidity.wrapping_add_signed(delta);
        }
    }

    fn change_tick(&mut self, tick: i32, gross_delta: i128, net_delta: i128) {
        let entry = self.ticks.entry(tick).or_default();
        entry.gross = entry.gross.wrapping_add_signed(gross_delta);
        entry.net = entry.net.wrapping_add(net_delta);
        if entry.gross == 0 {
            self.ticks.remove(&tick);
        }
    }
}

/// The bitmap words that can hold a pool's ticks, lowest first.
pub fn words(tick_spacing: i32) -> std::ops::RangeInclusive<i16> {
    let word = |tick: i32| (tick.div_euclid(tick_spacing) >> 8) as i16;
    word(MIN_TICK)..=word(MAX_TICK)
}

pub fn decode(log: &Log) -> Option<Event> {
    let topics = log.topics.iter().copied();
    match *log.topics.first()? {
        Initialize::SIGNATURE_HASH => {
            let e = Initialize::decode_raw_log(topics, &log.data).ok()?;
            Some(Event::Initialize(Price {
                sqrt_price_x96: U256::from(e.sqrtPriceX96),
                tick: e.tick.as_i32(),
            }))
        }
        Swap::SIGNATURE_HASH => {
            let e = Swap::decode_raw_log(topics, &log.data).ok()?;
            Some(Event::Swap {
                price: Price {
                    sqrt_price_x96: U256::from(e.sqrtPriceX96),
                    tick: e.tick.as_i32(),
                },
                liquidity: e.liquidity,
            })
        }
        Mint::SIGNATURE_HASH => {
            let e = Mint::decode_raw_log(topics, &log.data).ok()?;
            Some(Event::Mint {
                lower: e.tickLower.as_i32(),
                upper: e.tickUpper.as_i32(),
                amount: e.amount,
            })
        }
        Burn::SIGNATURE_HASH => {
            let e = Burn::decode_raw_log(topics, &log.data).ok()?;
            Some(Event::Burn {
                lower: e.tickLower.as_i32(),
                upper: e.tickUpper.as_i32(),
                amount: e.amount,
            })
        }
        _ => None,
    }
}

/// A new pool, from its factory's `PoolCreated`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Created {
    pub pool: Address,
    pub token0: Address,
    pub token1: Address,
    pub fee: u32,
    pub tick_spacing: i32,
}

pub fn is_pool_created(log: &Log) -> bool {
    log.topics.first() == Some(&PoolCreated::SIGNATURE_HASH)
}

pub fn decode_pool_created(log: &Log) -> Option<Created> {
    let e = PoolCreated::decode_raw_log(log.topics.iter().copied(), &log.data).ok()?;
    Some(Created {
        pool: e.pool,
        token0: e.token0,
        token1: e.token1,
        fee: e.fee.to(),
        tick_spacing: e.tickSpacing.as_i32(),
    })
}

// Calls and their answers.

pub fn token0_call() -> Vec<u8> {
    token0Call {}.abi_encode()
}

pub fn token1_call() -> Vec<u8> {
    token1Call {}.abi_encode()
}

pub fn fee_call() -> Vec<u8> {
    feeCall {}.abi_encode()
}

pub fn tick_spacing_call() -> Vec<u8> {
    tickSpacingCall {}.abi_encode()
}

pub fn slot0_call() -> Vec<u8> {
    slot0Call {}.abi_encode()
}

pub fn liquidity_call() -> Vec<u8> {
    liquidityCall {}.abi_encode()
}

pub fn tick_bitmap_call(word: i16) -> Vec<u8> {
    tickBitmapCall { wordPosition: word }.abi_encode()
}

pub fn ticks_call(tick: i32) -> Vec<u8> {
    ticksCall {
        tick: alloy_primitives::aliases::I24::try_from(tick).expect("ticks fit in int24"),
    }
    .abi_encode()
}

pub fn populated_ticks_call(pool: Address, word: i16) -> Vec<u8> {
    getPopulatedTicksInWordCall {
        pool,
        tickBitmapIndex: word,
    }
    .abi_encode()
}

pub fn decode_address(returned: &[u8]) -> Option<Address> {
    token0Call::abi_decode_returns(returned).ok()
}

pub fn decode_fee(returned: &[u8]) -> Option<u32> {
    feeCall::abi_decode_returns(returned).ok().map(|f| f.to())
}

pub fn decode_tick_spacing(returned: &[u8]) -> Option<i32> {
    tickSpacingCall::abi_decode_returns(returned)
        .ok()
        .map(|s| s.as_i32())
}

pub fn decode_slot0(returned: &[u8]) -> Option<Price> {
    let slot0 = slot0Call::abi_decode_returns(returned).ok()?;
    Some(Price {
        sqrt_price_x96: U256::from(slot0.sqrtPriceX96),
        tick: slot0.tick.as_i32(),
    })
}

pub fn decode_liquidity(returned: &[u8]) -> Option<u128> {
    liquidityCall::abi_decode_returns(returned).ok()
}

pub fn decode_tick_bitmap(returned: &[u8]) -> Option<U256> {
    tickBitmapCall::abi_decode_returns(returned).ok()
}

pub fn decode_tick(returned: &[u8]) -> Option<TickLiquidity> {
    let tick = ticksCall::abi_decode_returns(returned).ok()?;
    Some(TickLiquidity {
        gross: tick.liquidityGross,
        net: tick.liquidityNet,
    })
}

pub fn decode_populated_ticks(returned: &[u8]) -> Option<Vec<(i32, TickLiquidity)>> {
    let ticks = getPopulatedTicksInWordCall::abi_decode_returns(returned).ok()?;
    Some(
        ticks
            .into_iter()
            .map(|t| {
                (
                    t.tick.as_i32(),
                    TickLiquidity {
                        gross: t.liquidityGross,
                        net: t.liquidityNet,
                    },
                )
            })
            .collect(),
    )
}

/// Answers for simulated nodes and tests.
pub mod answers {
    use super::*;

    pub fn address(a: Address) -> Vec<u8> {
        token0Call::abi_encode_returns(&a)
    }

    pub fn fee(fee: u32) -> Vec<u8> {
        feeCall::abi_encode_returns(&alloy_primitives::aliases::U24::from(fee))
    }

    pub fn tick_spacing(spacing: i32) -> Vec<u8> {
        tickSpacingCall::abi_encode_returns(
            &alloy_primitives::aliases::I24::try_from(spacing).expect("fits int24"),
        )
    }

    pub fn slot0(price: Price) -> Vec<u8> {
        slot0Call::abi_encode_returns(&slot0Return {
            sqrtPriceX96: alloy_primitives::aliases::U160::from(price.sqrt_price_x96),
            tick: alloy_primitives::aliases::I24::try_from(price.tick).expect("fits int24"),
            observationIndex: 0,
            observationCardinality: 0,
            observationCardinalityNext: 0,
            feeProtocol: 0,
            unlocked: true,
        })
    }

    pub fn liquidity(liquidity: u128) -> Vec<u8> {
        liquidityCall::abi_encode_returns(&liquidity)
    }

    pub fn tick_bitmap(bits: U256) -> Vec<u8> {
        tickBitmapCall::abi_encode_returns(&bits)
    }

    pub fn populated_ticks(ticks: &[(i32, TickLiquidity)]) -> Vec<u8> {
        let ticks: Vec<PopulatedTick> = ticks
            .iter()
            .map(|(tick, l)| PopulatedTick {
                tick: alloy_primitives::aliases::I24::try_from(*tick).expect("fits int24"),
                liquidityNet: l.net,
                liquidityGross: l.gross,
            })
            .collect();
        getPopulatedTicksInWordCall::abi_encode_returns(&ticks)
    }

    pub fn tick(l: TickLiquidity) -> Vec<u8> {
        ticksCall::abi_encode_returns(&ticksReturn {
            liquidityGross: l.gross,
            liquidityNet: l.net,
            feeGrowthOutside0X128: U256::ZERO,
            feeGrowthOutside1X128: U256::ZERO,
            tickCumulativeOutside: alloy_primitives::aliases::I56::ZERO,
            secondsPerLiquidityOutsideX128: alloy_primitives::aliases::U160::ZERO,
            secondsOutside: 0,
            initialized: l.gross > 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use alloy_primitives::address;

    use super::*;

    const WETH: Address = address!("4200000000000000000000000000000000000006");
    const USDC: Address = address!("833589fCD6eDb6E08f4c7C32D4f71b54bdA02913");

    // Read from the factory's getPool(WETH, USDC, 500) on Base, 2026-09-30.
    #[test]
    fn computes_the_live_weth_usdc_pool() {
        assert_eq!(
            BASE.pool_address(WETH, USDC, 500),
            address!("d0b53d9277642d899df5c87a3966a349a798f224")
        );
    }

    fn price(tick: i32) -> Price {
        Price {
            sqrt_price_x96: U256::from(1u64) << 96,
            tick,
        }
    }

    #[test]
    fn a_mint_in_range_adds_active_liquidity_and_ticks() {
        let mut pool = Pool::new(Address::ZERO, WETH, USDC, 500, 10);
        pool.apply(Event::Initialize(price(0)));
        let (before, after) = pool.apply(Event::Mint {
            lower: -10,
            upper: 10,
            amount: 100,
        });
        assert_eq!(pool.liquidity, 100);
        assert_eq!(
            before.ticks,
            vec![
                (-10, TickLiquidity::default()),
                (10, TickLiquidity::default())
            ]
        );
        assert_eq!(
            after.ticks,
            vec![
                (
                    -10,
                    TickLiquidity {
                        gross: 100,
                        net: 100
                    }
                ),
                (
                    10,
                    TickLiquidity {
                        gross: 100,
                        net: -100
                    }
                )
            ]
        );
    }

    #[test]
    fn the_upper_tick_is_outside_the_range() {
        let mut pool = Pool::new(Address::ZERO, WETH, USDC, 500, 10);
        pool.apply(Event::Initialize(price(10)));
        pool.apply(Event::Mint {
            lower: -10,
            upper: 10,
            amount: 100,
        });
        assert_eq!(pool.liquidity, 0);
    }

    #[test]
    fn burning_everything_removes_the_ticks() {
        let mut pool = Pool::new(Address::ZERO, WETH, USDC, 500, 10);
        pool.apply(Event::Initialize(price(0)));
        pool.apply(Event::Mint {
            lower: -10,
            upper: 10,
            amount: 100,
        });
        pool.apply(Event::Burn {
            lower: -10,
            upper: 10,
            amount: 100,
        });
        assert_eq!(pool.liquidity, 0);
        assert!(pool.ticks.is_empty());
    }

    #[test]
    fn word_ranges_cover_every_tick() {
        assert_eq!(words(1), -3466..=3465);
        assert_eq!(words(10), -347..=346);
        assert_eq!(words(60), -58..=57);
        assert_eq!(words(200), -18..=17);
    }

    #[test]
    fn answers_round_trip() {
        let p = price(-7);
        assert_eq!(decode_slot0(&answers::slot0(p)), Some(p));
        assert_eq!(decode_fee(&answers::fee(3000)), Some(3000));
        assert_eq!(decode_tick_spacing(&answers::tick_spacing(60)), Some(60));
        let ticks = vec![(-60, TickLiquidity { gross: 5, net: -5 })];
        assert_eq!(
            decode_populated_ticks(&answers::populated_ticks(&ticks)),
            Some(ticks)
        );
        let t = TickLiquidity { gross: 9, net: 3 };
        assert_eq!(decode_tick(&answers::tick(t)), Some(t));
    }
}
