//! Trade records (#77, D97): one per Swap on a tracked pool, published to `trades.<chain>`.
//! A trade is a fact read from its log. It carries no USD: the API adds that from the
//! display price at the trade's block.

use alloy_primitives::{I256, U256};
use types::LineageId;
use types::chain::{Address, B256, Block, Log};

/// Quote base units per token base unit are scaled by this in `price_e36`, so a memecoin
/// worth a billionth of a cent per whole token still keeps its significant digits.
const PRICE_SCALE: U256 = U256::from_limbs([0xb34b_9f10_0000_0000, 0x00c0_97ce_7bc9_0715, 0, 0]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Venue {
    UniswapV2,
    UniswapV3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// The pool paid out the token: someone bought it with the quote asset.
    Buy,
    /// The pool took the token in: someone sold it for the quote asset.
    Sell,
}

/// A Swap log in the terms both venues share: where it is, who it names, and how much of
/// token0 and token1 the pool gained (negative: paid out).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SwapSeen {
    pub block_number: u64,
    pub block_hash: B256,
    pub block_timestamp: u64,
    pub log_index: u64,
    pub tx_hash: B256,
    pub sender: Address,
    pub recipient: Address,
    pub amount0: I256,
    pub amount1: I256,
}

impl SwapSeen {
    pub fn new(
        block: &Block,
        log: &Log,
        (sender, recipient): (Address, Address),
        (amount0, amount1): (I256, I256),
    ) -> Self {
        Self {
            block_number: block.number,
            block_hash: block.hash,
            block_timestamp: block.timestamp,
            log_index: log.log_index,
            tx_hash: log.transaction_hash,
            sender,
            recipient,
            amount0,
            amount1,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trade {
    /// From the natural key (chain, block hash, log index), D71.
    pub id: LineageId,
    /// The chain event at the same key: the Swap log.
    pub caused_by: LineageId,
    pub chain_id: u64,
    pub pool: Address,
    pub venue: Venue,
    pub token: Address,
    pub quote: Address,
    pub side: Side,
    /// Base units.
    pub token_amount: U256,
    pub quote_amount: U256,
    /// Quote base units per token base unit, times 10^36, rounded down. `None` when the token
    /// amount is zero, or the scaled price doesn't fit 256 bits.
    pub price_e36: Option<U256>,
    pub sender: Address,
    pub recipient: Address,
    pub tx_hash: B256,
    pub block_number: u64,
    pub block_hash: B256,
    pub block_timestamp: u64,
    pub log_index: u64,
}

impl Trade {
    /// The trade a pool's swap records. `tokens` are the pool's (token0, token1);
    /// `quote_assets` is the engine's list, most preferred first.
    pub(crate) fn new(
        chain_id: u64,
        venue: Venue,
        pool: Address,
        tokens: (Address, Address),
        quote_assets: &[Address],
        swap: &SwapSeen,
    ) -> Self {
        let (token0, token1) = tokens;
        let (token, quote, token_delta, quote_delta) = if quote_is_token0(tokens, quote_assets) {
            (token1, token0, swap.amount1, swap.amount0)
        } else {
            (token0, token1, swap.amount0, swap.amount1)
        };
        // A swap that moves none of the token (dust rounding to zero) is told by the quote.
        let side =
            if token_delta.is_negative() || (token_delta.is_zero() && quote_delta.is_positive()) {
                Side::Buy
            } else {
                Side::Sell
            };
        let token_amount = token_delta.unsigned_abs();
        let quote_amount = quote_delta.unsigned_abs();
        let price_e36 = (!token_amount.is_zero())
            .then(|| quote_amount.checked_mul(PRICE_SCALE))
            .flatten()
            .map(|scaled| scaled / token_amount);

        let event_key = [
            &chain_id.to_le_bytes()[..],
            swap.block_hash.as_slice(),
            &swap.log_index.to_le_bytes(),
        ]
        .concat();
        Self {
            id: LineageId::from_natural_key("trade", &event_key),
            caused_by: LineageId::from_natural_key("chain_event", &event_key),
            chain_id,
            pool,
            venue,
            token,
            quote,
            side,
            token_amount,
            quote_amount,
            price_e36,
            sender: swap.sender,
            recipient: swap.recipient,
            tx_hash: swap.tx_hash,
            block_number: swap.block_number,
            block_hash: swap.block_hash,
            block_timestamp: swap.block_timestamp,
            log_index: swap.log_index,
        }
    }

    pub fn to_proto(&self) -> proto::trade::v1::Trade {
        use proto::trade::v1::trade;
        proto::trade::v1::Trade {
            lineage: Some(proto::lineage::v1::Lineage {
                id: self.id.as_bytes().to_vec(),
                caused_by: vec![self.caused_by.as_bytes().to_vec()],
            }),
            chain_id: self.chain_id,
            pool: self.pool.to_vec(),
            venue: match self.venue {
                Venue::UniswapV2 => trade::Venue::UniswapV2,
                Venue::UniswapV3 => trade::Venue::UniswapV3,
            } as i32,
            token: self.token.to_vec(),
            quote: self.quote.to_vec(),
            side: match self.side {
                Side::Buy => trade::Side::Buy,
                Side::Sell => trade::Side::Sell,
            } as i32,
            token_amount: self.token_amount.to_be_bytes_trimmed_vec(),
            quote_amount: self.quote_amount.to_be_bytes_trimmed_vec(),
            price_e36: self.price_e36.map(|p| p.to_be_bytes_trimmed_vec()),
            sender: self.sender.to_vec(),
            recipient: self.recipient.to_vec(),
            tx_hash: self.tx_hash.to_vec(),
            block_number: self.block_number,
            block_hash: self.block_hash.to_vec(),
            block_timestamp: self.block_timestamp,
            log_index: self.log_index,
        }
    }
}

/// Whether token0 is the quote asset: the pool's token that comes first in `quote_assets`.
/// When neither is listed, token1 is the quote.
fn quote_is_token0((token0, token1): (Address, Address), quote_assets: &[Address]) -> bool {
    let rank = |token: Address| quote_assets.iter().position(|&q| q == token);
    match (rank(token0), rank(token1)) {
        (Some(r0), Some(r1)) => r0 < r1,
        (Some(_), None) => true,
        (None, _) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_price_scale_is_ten_to_the_36() {
        assert_eq!(PRICE_SCALE, U256::from(10u64).pow(U256::from(36u64)));
    }

    #[test]
    fn the_quote_is_the_more_preferred_listed_token() {
        let (a, b, c) = (
            Address::repeat_byte(1),
            Address::repeat_byte(2),
            Address::repeat_byte(3),
        );
        let quotes = [b, c];
        assert!(!quote_is_token0((a, b), &quotes));
        assert!(quote_is_token0((b, a), &quotes));
        assert!(quote_is_token0((b, c), &quotes));
        assert!(!quote_is_token0((c, b), &quotes));
        // Neither listed: token1 quotes.
        assert!(!quote_is_token0((a, Address::repeat_byte(4)), &quotes));
        assert!(!quote_is_token0((a, b), &[]));
    }
}
