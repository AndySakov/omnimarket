//! What the engine publishes: pool updates, with the state before and after (D12), and price
//! updates (D18, D77).

use std::cell::RefCell;
use std::rc::Rc;

use det::kafka::KafkaPublisher;
use pricing::{DisplayPrice, PoolQuote, TokenMetadata};
use prost::Message as _;
use types::LineageId;
use types::chain::{Address, B256, Block};
use venues::v2::Reserves;
use venues::v3::{Price, TickLiquidity};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PoolState {
    V2(Reserves),
    V3(V3State),
}

/// A v3 pool's price and active liquidity, and the ticks an update touched: every
/// initialized tick when the pool was just discovered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct V3State {
    /// `None` until the pool is initialized.
    pub price: Option<Price>,
    pub liquidity: u128,
    pub ticks: Vec<(i32, TickLiquidity)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PoolUpdate {
    /// From the natural key (chain, pool, block hash, log index), D71.
    pub id: LineageId,
    /// The chain event (chain, block hash, log index) that caused it.
    pub caused_by: LineageId,
    pub chain_id: u64,
    pub pool: Address,
    pub block_number: u64,
    pub block_hash: B256,
    pub log_index: u64,
    /// `None` when the pool was just discovered.
    pub before: Option<PoolState>,
    pub after: PoolState,
}

impl PoolUpdate {
    pub fn new(
        chain_id: u64,
        pool: Address,
        event: (u64, B256, u64),
        before: Option<PoolState>,
        after: PoolState,
    ) -> Self {
        let (block_number, block_hash, log_index) = event;
        let event_key = [
            &chain_id.to_le_bytes()[..],
            block_hash.as_slice(),
            &log_index.to_le_bytes(),
        ]
        .concat();
        let update_key = [
            &chain_id.to_le_bytes()[..],
            pool.as_slice(),
            block_hash.as_slice(),
            &log_index.to_le_bytes(),
        ]
        .concat();
        Self {
            id: LineageId::from_natural_key("pool_update", &update_key),
            caused_by: LineageId::from_natural_key("chain_event", &event_key),
            chain_id,
            pool,
            block_number,
            block_hash,
            log_index,
            before,
            after,
        }
    }

    pub fn to_proto(&self) -> proto::pool::v1::PoolUpdate {
        proto::pool::v1::PoolUpdate {
            lineage: Some(proto::lineage::v1::Lineage {
                id: self.id.as_bytes().to_vec(),
                caused_by: vec![self.caused_by.as_bytes().to_vec()],
            }),
            chain_id: self.chain_id,
            pool: self.pool.to_vec(),
            block_number: self.block_number,
            block_hash: self.block_hash.to_vec(),
            log_index: self.log_index,
            before: self.before.as_ref().map(state_to_proto),
            after: Some(state_to_proto(&self.after)),
        }
    }
}

fn state_to_proto(state: &PoolState) -> proto::pool::v1::PoolState {
    use proto::pool::v1::pool_state::State;
    let state = match state {
        PoolState::V2(reserves) => State::V2(proto::pool::v1::V2Reserves {
            reserve0: big_endian(reserves.reserve0),
            reserve1: big_endian(reserves.reserve1),
        }),
        PoolState::V3(v3) => State::V3(proto::pool::v1::V3State {
            initialized: v3.price.is_some(),
            sqrt_price_x96: v3
                .price
                .map(|p| p.sqrt_price_x96.to_be_bytes_trimmed_vec())
                .unwrap_or_default(),
            tick: v3.price.map_or(0, |p| p.tick),
            liquidity: big_endian(v3.liquidity),
            ticks: v3
                .ticks
                .iter()
                .map(|(tick, l)| proto::pool::v1::V3Tick {
                    tick: *tick,
                    liquidity_gross: big_endian(l.gross),
                    liquidity_net: l.net.to_be_bytes().to_vec(),
                })
                .collect(),
        }),
    };
    proto::pool::v1::PoolState { state: Some(state) }
}

/// Big-endian without leading zeros; zero is empty.
fn big_endian(value: u128) -> Vec<u8> {
    let bytes = value.to_be_bytes();
    let first = bytes.iter().position(|&b| b != 0).unwrap_or(bytes.len());
    bytes[first..].to_vec()
}

/// One of the pools a price update was priced from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PricedPool {
    pub quote: PoolQuote,
    /// At or above the liquidity floor, so it counts toward the display price (D18).
    pub counted: bool,
}

/// A token's display price after a canonical block (D18, D77).
#[derive(Clone, Debug, PartialEq)]
pub struct PriceUpdate {
    /// From the natural key (chain, token, block hash).
    pub id: LineageId,
    /// The latest update of each pool priced from, and the native token's price update when a
    /// pool quotes in it.
    pub caused_by: Vec<LineageId>,
    pub chain_id: u64,
    pub token: Address,
    pub block_number: u64,
    pub block_hash: B256,
    pub block_timestamp: u64,
    pub metadata: TokenMetadata,
    pub price_usd: f64,
    pub depth_usd: f64,
    pub thin: bool,
    pub main_pool: Address,
    pub quote_token: Address,
    /// The display price in the main pool's quote asset.
    pub price_in_quote: f64,
    /// Total supply × price: fully diluted.
    pub fdv_usd: Option<f64>,
    pub pools: Vec<PricedPool>,
}

impl PriceUpdate {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        chain_id: u64,
        block: &Block,
        token: Address,
        caused_by: Vec<LineageId>,
        metadata: TokenMetadata,
        display: DisplayPrice,
        price_in_quote: f64,
        fdv_usd: Option<f64>,
        pools: Vec<PricedPool>,
    ) -> Self {
        let key = [
            &chain_id.to_le_bytes()[..],
            token.as_slice(),
            block.hash.as_slice(),
        ]
        .concat();
        Self {
            id: LineageId::from_natural_key("price_update", &key),
            caused_by,
            chain_id,
            token,
            block_number: block.number,
            block_hash: block.hash,
            block_timestamp: block.timestamp,
            metadata,
            price_usd: display.price_usd,
            depth_usd: display.depth_usd,
            thin: display.thin,
            main_pool: display.main.pool,
            quote_token: display.main.quote,
            price_in_quote,
            fdv_usd,
            pools,
        }
    }

    pub fn to_proto(&self) -> proto::price::v1::PriceUpdate {
        use proto::price::v1;
        v1::PriceUpdate {
            lineage: Some(proto::lineage::v1::Lineage {
                id: self.id.as_bytes().to_vec(),
                caused_by: self
                    .caused_by
                    .iter()
                    .map(|id| id.as_bytes().to_vec())
                    .collect(),
            }),
            chain_id: self.chain_id,
            token: self.token.to_vec(),
            block_number: self.block_number,
            block_hash: self.block_hash.to_vec(),
            block_timestamp: self.block_timestamp,
            metadata: Some(v1::TokenMetadata {
                name: self.metadata.name.clone(),
                symbol: self.metadata.symbol.clone(),
                decimals: self.metadata.decimals.map(u32::from),
                total_supply: self
                    .metadata
                    .total_supply
                    .map(|s| s.to_be_bytes_trimmed_vec()),
            }),
            price_usd: self.price_usd,
            depth_usd: self.depth_usd,
            thin: self.thin,
            main_pool: self.main_pool.to_vec(),
            quote_token: self.quote_token.to_vec(),
            price_in_quote: self.price_in_quote,
            fdv_usd: self.fdv_usd,
            pools: self
                .pools
                .iter()
                .map(|p| v1::PoolPrice {
                    pool: p.quote.pool.to_vec(),
                    quote_token: p.quote.quote.to_vec(),
                    price_in_quote: p.quote.price_in_quote,
                    price_usd: p.quote.price_usd,
                    depth_usd: p.quote.depth_usd,
                    counted: p.counted,
                })
                .collect(),
        }
    }
}

/// Where pool and price updates go. Publishing can't change the engine's decisions, so it
/// isn't a recorded input.
pub trait Outbox {
    fn publish(&self, update: &PoolUpdate);
    fn publish_price(&self, update: &PriceUpdate);
}

/// Keeps updates in memory, for tests and replays. Clones share one list.
#[derive(Clone, Default)]
pub struct InMemoryOutbox {
    updates: Rc<RefCell<Vec<PoolUpdate>>>,
    prices: Rc<RefCell<Vec<PriceUpdate>>>,
}

impl InMemoryOutbox {
    pub fn updates(&self) -> Vec<PoolUpdate> {
        self.updates.borrow().clone()
    }

    pub fn prices(&self) -> Vec<PriceUpdate> {
        self.prices.borrow().clone()
    }
}

impl Outbox for InMemoryOutbox {
    fn publish(&self, update: &PoolUpdate) {
        self.updates.borrow_mut().push(update.clone());
    }

    fn publish_price(&self, update: &PriceUpdate) {
        self.prices.borrow_mut().push(update.clone());
    }
}

/// Publishes to `pool-updates.<chain>`, keyed by pool, and `prices.<chain>`, keyed by token.
pub struct KafkaOutbox {
    pool_updates: KafkaPublisher,
    prices: KafkaPublisher,
}

impl KafkaOutbox {
    pub fn new(pool_updates: KafkaPublisher, prices: KafkaPublisher) -> Self {
        Self {
            pool_updates,
            prices,
        }
    }
}

impl Outbox for KafkaOutbox {
    fn publish(&self, update: &PoolUpdate) {
        self.pool_updates
            .publish(update.pool.as_slice(), &update.to_proto().encode_to_vec());
    }

    fn publish_price(&self, update: &PriceUpdate) {
        self.prices
            .publish(update.token.as_slice(), &update.to_proto().encode_to_vec());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn big_endian_drops_leading_zeros() {
        assert_eq!(big_endian(0), Vec::<u8>::new());
        assert_eq!(big_endian(0x01ff), vec![0x01, 0xff]);
    }

    // pool_update.proto: unsigned integers are big-endian without leading zeros, and a tick's
    // liquidity_net is 16 bytes, two's complement, big-endian. A pool not yet initialized has
    // `initialized` false and its price fields unset. The expected bytes are written out here,
    // not computed with this module's helpers.
    #[test]
    fn updates_go_on_the_wire_as_pool_update_proto_says() {
        use alloy_primitives::U256;
        use proto::pool::v1::pool_state::State;
        use proto::pool::v1::{V2Reserves, V3State as WireV3, V3Tick};

        let wire_after = |after: PoolState| {
            PoolUpdate::new(
                8453,
                Address::repeat_byte(9),
                (5, B256::ZERO, 3),
                None,
                after,
            )
            .to_proto()
            .after
            .and_then(|state| state.state)
        };

        assert_eq!(
            wire_after(PoolState::V2(Reserves {
                reserve0: 0,
                reserve1: 0x01_0000,
            })),
            Some(State::V2(V2Reserves {
                reserve0: vec![],
                reserve1: vec![0x01, 0x00, 0x00],
            }))
        );

        assert_eq!(
            wire_after(PoolState::V3(V3State {
                price: Some(Price {
                    sqrt_price_x96: U256::from(1u64) << 96,
                    tick: -7,
                }),
                liquidity: 1_000,
                ticks: vec![(-60, TickLiquidity { gross: 5, net: -5 })],
            })),
            Some(State::V3(WireV3 {
                initialized: true,
                // 2^96: a one and twelve zero bytes.
                sqrt_price_x96: vec![1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                tick: -7,
                liquidity: vec![0x03, 0xe8],
                ticks: vec![V3Tick {
                    tick: -60,
                    liquidity_gross: vec![5],
                    liquidity_net: vec![
                        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                        0xff, 0xff, 0xff, 0xfb,
                    ],
                }],
            }))
        );

        assert_eq!(
            wire_after(PoolState::V3(V3State {
                price: None,
                liquidity: 0,
                ticks: vec![],
            })),
            Some(State::V3(WireV3 {
                initialized: false,
                sqrt_price_x96: vec![],
                tick: 0,
                liquidity: vec![],
                ticks: vec![],
            }))
        );
    }

    #[test]
    fn the_same_event_gives_the_same_ids() {
        let event = (5, B256::repeat_byte(1), 3);
        let after = PoolState::V2(Reserves {
            reserve0: 1,
            reserve1: 2,
        });
        let a = PoolUpdate::new(8453, Address::repeat_byte(9), event, None, after.clone());
        let b = PoolUpdate::new(8453, Address::repeat_byte(9), event, None, after.clone());
        let other_pool = PoolUpdate::new(8453, Address::repeat_byte(8), event, None, after);
        assert_eq!((a.id, a.caused_by), (b.id, b.caused_by));
        assert_ne!(a.id, other_pool.id);
        assert_eq!(a.caused_by, other_pool.caused_by);
    }
}
