//! The token read model: each token's latest display price, as the `token:<address>` topic and
//! `GET /v1/tokens/{chain_id}/{address}` serve it. A pure function of the `prices.base` records
//! it has applied, in order, timed by their block time, never a wall clock (build rule 1), so a
//! recorded session gives the same API output every time.

use std::collections::BTreeMap;

use proto::api::v1::{PoolSummary, TokenRef, TokenSnapshot, TokenTick};
use proto::lineage::v1::Lineage;
use proto::price::v1::PriceUpdate;
use types::LineageId;

/// Why a record couldn't be applied.
#[derive(Debug, PartialEq)]
pub enum ModelError {
    /// The token field isn't a 20-byte address.
    BadToken { len: usize },
    /// The block hash isn't 32 bytes.
    BadHash { len: usize },
}

#[derive(Default)]
pub struct ReadModel {
    /// Keyed by lowercase 0x address.
    tokens: BTreeMap<String, TokenSnapshot>,
    head_block_number: u64,
}

impl ReadModel {
    /// Applies one price record and returns the tick it makes on `token:<address>`.
    pub fn apply_price(&mut self, update: &PriceUpdate) -> Result<TokenTick, ModelError> {
        if update.token.len() != 20 {
            return Err(ModelError::BadToken {
                len: update.token.len(),
            });
        }
        let address = hex(&update.token);
        let price_id = update
            .lineage
            .as_ref()
            .map(|l| l.id.clone())
            .unwrap_or_default();
        let metadata = update.metadata.clone().unwrap_or_default();
        let total_supply = match (&metadata.total_supply, metadata.decimals) {
            (Some(raw), Some(decimals)) => scaled_decimal(raw, decimals),
            _ => String::new(),
        };
        let main_pool_price_usd = update
            .pools
            .iter()
            .find(|p| p.pool == update.main_pool)
            .map(|p| decimal(p.price_usd))
            .unwrap_or_default();
        let pools = update
            .pools
            .iter()
            .map(|p| PoolSummary {
                address: hex(&p.pool),
                // The venue, fee tier and creation block aren't in the price record yet; the
                // discovery read model (#81) adds them.
                venue: String::new(),
                pool_fee_bps: 0,
                quote_token: Some(self.token_ref(update.chain_id, &p.quote_token)),
                price_usd: decimal(p.price_usd),
                depth_usd: decimal(p.depth_usd),
                // Every pool in a price record was priced (D11).
                active: true,
                created_block: 0,
            })
            .collect();
        let market_cap_usd = update.fdv_usd.map(decimal).unwrap_or_default();
        let block_time_ms = update.block_timestamp.saturating_mul(1000);
        let snapshot = TokenSnapshot {
            lineage: Some(derived_lineage("token_snapshot", &price_id)),
            token: Some(TokenRef {
                chain_id: update.chain_id,
                address: address.clone(),
                symbol: metadata.symbol.clone().unwrap_or_default(),
                name: metadata.name.clone().unwrap_or_default(),
                decimals: metadata.decimals.unwrap_or_default(),
            }),
            display_price_usd: decimal(update.price_usd),
            display_price_quote: decimal(update.price_in_quote),
            quote_token: Some(self.token_ref(update.chain_id, &update.quote_token)),
            main_pool_price_usd,
            total_supply,
            market_cap_usd: market_cap_usd.clone(),
            // The price record's market cap is total supply × price: fully diluted.
            market_cap_is_fdv: update.fdv_usd.is_some(),
            depth_usd: decimal(update.depth_usd),
            thin: update.thin,
            pools,
            stats_5m: None,
            stats_1h: None,
            stats_tracked: None,
            safety: None,
            block_number: update.block_number,
            block_time_ms,
        };
        let tick = TokenTick {
            lineage: Some(derived_lineage("token_tick", &price_id)),
            chain_id: update.chain_id,
            token: address.clone(),
            display_price_usd: snapshot.display_price_usd.clone(),
            display_price_quote: snapshot.display_price_quote.clone(),
            market_cap_usd,
            depth_usd: snapshot.depth_usd.clone(),
            thin: update.thin,
            block_number: update.block_number,
            block_time_ms,
        };
        self.tokens.insert(address, snapshot);
        self.head_block_number = self.head_block_number.max(update.block_number);
        Ok(tick)
    }

    /// The token's snapshot, by lowercase 0x address.
    pub fn token(&self, address: &str) -> Option<&TokenSnapshot> {
        self.tokens.get(address)
    }

    /// Every priced token's address, in order.
    pub fn token_addresses(&self) -> impl Iterator<Item = &String> {
        self.tokens.keys()
    }

    /// The highest block a record has been applied from.
    pub fn head_block_number(&self) -> u64 {
        self.head_block_number
    }

    /// A token reference, named from its own snapshot when it has one.
    fn token_ref(&self, chain_id: u64, address: &[u8]) -> TokenRef {
        let address = hex(address);
        match self.tokens.get(&address).and_then(|s| s.token.clone()) {
            Some(known) => known,
            None => TokenRef {
                chain_id,
                address,
                ..TokenRef::default()
            },
        }
    }
}

/// An API record's lineage: derived from the price record that caused it (D53).
fn derived_lineage(kind: &str, price_id: &[u8]) -> Lineage {
    Lineage {
        id: LineageId::from_natural_key(kind, price_id)
            .as_bytes()
            .to_vec(),
        caused_by: vec![price_id.to_vec()],
    }
}

/// Lowercase 0x-prefixed hex (D91).
pub fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(2 + 2 * bytes.len());
    out.push_str("0x");
    for b in bytes {
        out.push(char::from(b"0123456789abcdef"[usize::from(b >> 4)]));
        out.push(char::from(b"0123456789abcdef"[usize::from(b & 0xf)]));
    }
    out
}

/// A decimal string with no exponent (D91). Rust prints an `f64` as the shortest decimal that
/// reads back to the same value, never in scientific notation. Not-a-number and infinities,
/// which a price record shouldn't carry, become empty (unknown).
pub fn decimal(value: f64) -> String {
    if value.is_finite() {
        format!("{value}")
    } else {
        String::new()
    }
}

/// A raw big-endian integer divided by 10^decimals, exactly, as a decimal string.
pub fn scaled_decimal(big_endian: &[u8], decimals: u32) -> String {
    // Base-10 digits by repeated division of the big-endian bytes by 10.
    let mut number = big_endian.to_vec();
    let mut digits = Vec::new();
    while number.iter().any(|&b| b != 0) {
        let mut remainder = 0u32;
        for byte in number.iter_mut() {
            let value = (remainder << 8) | u32::from(*byte);
            *byte = (value / 10) as u8;
            remainder = value % 10;
        }
        digits.push(b'0' + remainder as u8);
    }
    let decimals = decimals as usize;
    while digits.len() <= decimals {
        digits.push(b'0');
    }
    digits.reverse();
    let (whole, fraction) = digits.split_at(digits.len() - decimals);
    let fraction = std::str::from_utf8(fraction)
        .expect("digits are ASCII")
        .trim_end_matches('0');
    let whole = std::str::from_utf8(whole).expect("digits are ASCII");
    if fraction.is_empty() {
        whole.to_string()
    } else {
        format!("{whole}.{fraction}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimals_have_no_exponent() {
        assert_eq!(decimal(0.000_000_123), "0.000000123");
        assert_eq!(decimal(1e21), "1000000000000000000000");
        assert_eq!(decimal(f64::NAN), "");
    }

    #[test]
    fn scaled_decimals_are_exact() {
        assert_eq!(scaled_decimal(&[], 18), "0");
        assert_eq!(scaled_decimal(&[0x01, 0x00], 2), "2.56");
        assert_eq!(scaled_decimal(&[0x0a], 0), "10");
        assert_eq!(scaled_decimal(&[0x0a], 1), "1");
        assert_eq!(scaled_decimal(&[0x05], 3), "0.005");
        // 10^18 + 1 wei: one token and a wei.
        let raw = 1_000_000_000_000_000_001u128.to_be_bytes();
        assert_eq!(scaled_decimal(&raw, 18), "1.000000000000000001");
    }

    #[test]
    fn hex_is_lowercase_and_prefixed() {
        assert_eq!(hex(&[0xab, 0x01]), "0xab01");
    }
}
