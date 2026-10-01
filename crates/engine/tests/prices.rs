//! Pricing in the engine against pricing.md, data.md and #76, on a simulated chain: token
//! metadata through the engine's calls, display prices per canonical block, and replay.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::Duration;

use alloy_primitives::{U256, address};
use alloy_sol_types::{SolEvent, SolValue};
use det::{
    InMemorySink, Recorder, RecordingClock, RecordingEventSource, RecordingRpc, Replay, SimClock,
    SimEventSource, SimRpc,
};
use engine::{Engine, EngineConfig, InMemoryOutbox, PoolUpdate, PriceUpdate, Summary};
use pricing::config::{BASE_USDC, BASE_WETH};
use pricing::metadata::encode;
use pricing::{PricingConfig, TokenMetadata};
use types::Timestamp;
use types::chain::{Address, B256, Block, Bytes, CallResult, EthCall, Log};
use venues::multicall;
use venues::v2::{self, BASE, Sync};

const START: Timestamp = Timestamp::from_unix_nanos(1_767_225_600_000_000_000);
const TOKEN: Address = address!("1000000000000000000000000000000000000001");
const OTHER: Address = address!("2000000000000000000000000000000000000002");
const E18: u128 = 1_000_000_000_000_000_000;
const E6: u128 = 1_000_000;

fn pair(a: Address, b: Address) -> Address {
    let (token0, token1) = if a < b { (a, b) } else { (b, a) };
    BASE.pair_address(token0, token1)
}

/// The v2 WETH/USDC pair: one of Base's reference pools.
fn weth_usdc() -> Address {
    pair(BASE_WETH, BASE_USDC)
}

/// A Sync giving `a` and `b` these reserves, whichever is token0.
fn sync(a: Address, b: Address, log_index: u64, reserve_a: u128, reserve_b: u128) -> Log {
    let (r0, r1) = if a < b {
        (reserve_a, reserve_b)
    } else {
        (reserve_b, reserve_a)
    };
    Log {
        address: pair(a, b),
        topics: vec![Sync::SIGNATURE_HASH],
        data: Bytes::from((U256::from(r0), U256::from(r1)).abi_encode()),
        log_index,
        transaction_hash: B256::ZERO,
    }
}

fn hash(number: u64) -> B256 {
    B256::left_padding_from(&number.to_be_bytes())
}

/// Blocks from 100, one every 2s.
fn chain(logs: Vec<Vec<Log>>) -> Vec<(Duration, Block)> {
    logs.into_iter()
        .enumerate()
        .map(|(i, logs)| {
            let number = 100 + i as u64;
            let block = Block {
                number,
                hash: hash(number),
                parent_hash: hash(number - 1),
                timestamp: 1_767_225_600 + 2 * i as u64,
                logs,
            };
            (Duration::from_secs(2 * (i as u64 + 1)), block)
        })
        .collect()
}

fn token(name: &str, symbol: &str, decimals: u8, supply: u128) -> TokenMetadata {
    TokenMetadata {
        name: Some(name.into()),
        symbol: Some(symbol.into()),
        decimals: Some(decimals),
        total_supply: Some(U256::from(supply)),
    }
}

/// What one `aggregate3` call asked about.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Asked {
    Pairs(Vec<Address>),
    Metadata { block: u64, tokens: Vec<Address> },
    Supply { block: u64, tokens: Vec<Address> },
}

/// A node that knows every pair's tokens and the tokens' metadata. A token without metadata
/// reverts every call.
#[derive(Default)]
struct Node {
    metadata: BTreeMap<Address, TokenMetadata>,
    pairs: BTreeMap<Address, (Address, Address)>,
    asked: Rc<RefCell<Vec<Asked>>>,
}

impl Node {
    fn new(pairs: &[(Address, Address)]) -> Self {
        let mut node = Self::default();
        for &(a, b) in pairs {
            let (token0, token1) = if a < b { (a, b) } else { (b, a) };
            node.pairs.insert(pair(a, b), (token0, token1));
        }
        node.metadata
            .insert(BASE_WETH, token("Wrapped Ether", "WETH", 18, 0));
        node.metadata
            .insert(BASE_USDC, token("USD Coin", "USDC", 6, 0));
        node
    }

    fn answer(&mut self, call: &EthCall) -> CallResult {
        let calls = multicall::decode_calls(&call.data).unwrap();
        let selector = |data: &Bytes| data[..4].to_vec();
        let first = selector(&calls[0].1);
        let targets: Vec<Address> = calls.iter().map(|(t, _)| *t).collect();
        let mut distinct = targets.clone();
        distinct.dedup();
        let name = pricing::metadata::calls(Address::ZERO)[0].1[..4].to_vec();
        let supply = pricing::metadata::total_supply_call(Address::ZERO).1[..4].to_vec();
        self.asked
            .borrow_mut()
            .push(if first == v2::token0_call()[..4] {
                Asked::Pairs(distinct)
            } else if first == name {
                Asked::Metadata {
                    block: call.block,
                    tokens: distinct,
                }
            } else if first == supply {
                Asked::Supply {
                    block: call.block,
                    tokens: distinct,
                }
            } else {
                panic!("an unexpected call")
            });
        let results: Vec<Option<Vec<u8>>> = calls
            .iter()
            .map(|(target, data)| {
                let s = selector(data);
                if s == v2::token0_call()[..4] {
                    return self.pairs.get(target).map(|p| v2::encode_address(p.0));
                }
                if s == v2::token1_call()[..4] {
                    return self.pairs.get(target).map(|p| v2::encode_address(p.1));
                }
                let m = self.metadata.get(target)?;
                let reads = pricing::metadata::calls(*target);
                if s == reads[0].1[..4] {
                    m.name.as_deref().map(encode::text)
                } else if s == reads[1].1[..4] {
                    m.symbol.as_deref().map(encode::text)
                } else if s == reads[2].1[..4] {
                    m.decimals.map(|d| encode::uint(U256::from(d)))
                } else {
                    m.total_supply.map(encode::uint)
                }
            })
            .collect();
        CallResult::Returned(Bytes::from(multicall::encode_results(&results)))
    }
}

struct Run {
    summary: Summary,
    updates: Vec<PoolUpdate>,
    prices: Vec<PriceUpdate>,
    recording: Vec<det::InputRecord>,
}

fn run(config: EngineConfig, blocks: Vec<(Duration, Block)>, mut node: Node) -> Run {
    let sink = InMemorySink::default();
    let outbox = InMemoryOutbox::default();
    let summary = det::run_simulated(async {
        let clock = SimClock::starting_at(START);
        let recorder = Recorder::new(Box::new(sink.clone()), Box::new(clock.clone()));
        recorder.record_config(config.encode());
        let rpc = SimRpc::new(move |call: EthCall| (Duration::from_millis(50), node.answer(&call)));
        let engine = Engine::new(
            config,
            Box::new(RecordingClock::new(Box::new(clock), recorder.clone())),
            Box::new(RecordingEventSource::new(
                Box::new(SimEventSource::new(blocks)),
                recorder.clone(),
            )),
            Box::new(RecordingRpc::new(Box::new(rpc), recorder)),
            Box::new(outbox.clone()),
        );
        engine.run().await.unwrap()
    });
    Run {
        summary,
        updates: outbox.updates(),
        prices: outbox.prices(),
        recording: sink.records(),
    }
}

fn close(actual: f64, expected: f64) -> bool {
    (actual - expected).abs() <= 1e-12 * expected.abs()
}

fn prices_of(run: &Run, token: Address) -> Vec<&PriceUpdate> {
    run.prices.iter().filter(|p| p.token == token).collect()
}

/// The pool update a price came from: the latest one of `pool` up to `block`.
fn latest_update(run: &Run, pool: Address, block: u64) -> &PoolUpdate {
    run.updates
        .iter()
        .rev()
        .find(|u| u.pool == pool && u.block_number <= block)
        .unwrap()
}

// #76: "The WETH display price tracks the reference pool". D19: WETH's USD price is the
// reference pools' weighted mid; here one reference pool, the v2 WETH/USDC pair.
#[test]
fn weth_is_priced_from_the_reference_pool_on_each_block() {
    let blocks = chain(vec![
        // 100 WETH against 270,000 USDC: $2,700.
        vec![sync(BASE_WETH, BASE_USDC, 0, 100 * E18, 270_000 * E6)],
        vec![],
        // A buy: 99 WETH against 272,727.27 USDC.
        vec![sync(BASE_WETH, BASE_USDC, 3, 99 * E18, 272_727_270_000)],
    ]);
    let run = run(
        EngineConfig::base(),
        blocks,
        Node::new(&[(BASE_WETH, BASE_USDC)]),
    );
    let weth = prices_of(&run, BASE_WETH);
    // Block 100 proves the pair; its metadata is read at block 101 and arrives after it, so
    // the first price goes out at 102's end, with 102's state.
    let at: Vec<u64> = weth.iter().map(|p| p.block_number).collect();
    assert_eq!(at, [102]);
    let expected = 272_727.27 / 99.0;
    assert!(close(weth[0].price_usd, expected), "{}", weth[0].price_usd);
    assert_eq!(weth[0].main_pool, weth_usdc());
    assert_eq!(weth[0].quote_token, BASE_USDC);
    assert!(close(weth[0].price_in_quote, expected));
    // $545,454.54 of USDC side: ±2% depth is about 2% of it (D24).
    assert!((0.019..0.021).contains(&(weth[0].depth_usd / 272_727.27)));
    assert_eq!(
        weth[0].caused_by,
        [latest_update(&run, weth_usdc(), 102).id]
    );
    assert_eq!(weth[0].metadata.symbol.as_deref(), Some("WETH"));
}

// #76: "A token with two pools of different depth gets the weighted mid", priced through
// WETH in one pool and USDC in the other (D19), with its lineage (D53) and FDV.
#[test]
fn a_token_in_two_pools_gets_the_depth_weighted_mid() {
    let blocks = chain(vec![
        vec![
            sync(BASE_WETH, BASE_USDC, 0, 1_000 * E18, 3_000_000 * E6),
            // TOKEN at 0.001 WETH = $3, with a $1.2M WETH side: ~$24k of ±2% depth.
            sync(TOKEN, BASE_WETH, 1, 400_000 * E18, 400 * E18),
            // TOKEN at about $2.90, with a $3M USDC side: ~$60k of ±2% depth.
            sync(TOKEN, BASE_USDC, 2, 1_034_480 * E18, 2_999_992_000_000),
        ],
        vec![],
        vec![],
    ]);
    let mut node = Node::new(&[
        (BASE_WETH, BASE_USDC),
        (TOKEN, BASE_WETH),
        (TOKEN, BASE_USDC),
    ]);
    node.metadata
        .insert(TOKEN, token("Token", "TKN", 18, 1_000_000 * E18));
    let run = run(EngineConfig::base(), blocks, node);
    // Proven at 100, metadata read at 101: priced at 102's end.

    let token_prices = prices_of(&run, TOKEN);
    assert_eq!(token_prices.len(), 1, "{token_prices:?}");
    let p = token_prices[0];
    assert_eq!(p.block_number, 102);
    assert_eq!(p.pools.len(), 2);
    let (weth_side, usdc_side): (Vec<&engine::PricedPool>, Vec<&engine::PricedPool>) = p
        .pools
        .iter()
        .partition(|pool| pool.quote.quote == BASE_WETH);
    let (shallow, deep) = (weth_side[0].quote, usdc_side[0].quote);
    assert!(close(shallow.price_usd, 3.0), "{}", shallow.price_usd);
    assert!(close(shallow.price_in_quote, 0.001));
    assert!(close(deep.price_usd, 2_999_992.0 / 1_034_480.0));
    assert!(deep.depth_usd > shallow.depth_usd);
    assert!(shallow.depth_usd >= 10_000.0, "both above the floor");

    let weighted = (shallow.price_usd * shallow.depth_usd + deep.price_usd * deep.depth_usd)
        / (shallow.depth_usd + deep.depth_usd);
    assert!(close(p.price_usd, weighted));
    assert!(p.price_usd > deep.price_usd && p.price_usd < shallow.price_usd);
    assert!(!p.thin);
    assert_eq!(p.main_pool, pair(TOKEN, BASE_USDC));
    assert!(close(p.price_in_quote, p.price_usd), "USDC is pinned at $1");
    // Market cap is total supply × price: fully diluted.
    assert!(close(p.fdv_usd.unwrap(), 1_000_000.0 * p.price_usd));

    // Caused by both pools' updates and by WETH's price, which converted the WETH pool.
    let weth_price = prices_of(&run, BASE_WETH)[0];
    assert_eq!(weth_price.block_number, 102);
    let mut expected = vec![
        latest_update(&run, pair(TOKEN, BASE_WETH), 102).id,
        latest_update(&run, pair(TOKEN, BASE_USDC), 102).id,
        weth_price.id,
    ];
    expected.sort();
    assert_eq!(p.caused_by, expected);
}

// D18, D20: a token with no pool above the floor is priced from its deepest pool, flagged thin.
#[test]
fn a_token_with_only_shallow_pools_is_thin() {
    let blocks = chain(vec![
        vec![
            sync(BASE_WETH, BASE_USDC, 0, 1_000 * E18, 3_000_000 * E6),
            // $500 a side: far below the $10,000 floor.
            sync(TOKEN, BASE_USDC, 1, 1_000 * E18, 500 * E6),
        ],
        vec![],
        vec![],
    ]);
    let mut node = Node::new(&[(BASE_WETH, BASE_USDC), (TOKEN, BASE_USDC)]);
    node.metadata.insert(TOKEN, token("Thin", "THIN", 18, E18));
    let run = run(EngineConfig::base(), blocks, node);
    let p = prices_of(&run, TOKEN)[0];
    assert!(p.thin);
    assert!(close(p.price_usd, 0.5));
    assert!(!p.pools[0].counted);
    assert_eq!(run.summary.coverage.as_ref().unwrap().tokens_thin, 1);
}

// #76: metadata is read in batched multicalls through the engine's calls (D82), once per
// token; tokens that revert get fallbacks and an unscalable token isn't priced.
#[test]
fn metadata_is_read_once_per_token_and_reverts_fall_back() {
    let blocks = chain(vec![
        vec![
            sync(BASE_WETH, BASE_USDC, 0, 1_000 * E18, 3_000_000 * E6),
            sync(TOKEN, BASE_USDC, 1, 1_000 * E18, 50_000 * E6),
            sync(OTHER, BASE_USDC, 2, 1_000 * E18, 50_000 * E6),
        ],
        vec![
            sync(TOKEN, BASE_USDC, 1, 1_001 * E18, 49_950 * E6),
            sync(OTHER, BASE_USDC, 2, 1_001 * E18, 49_950 * E6),
        ],
        vec![],
    ]);
    let mut node = Node::new(&[
        (BASE_WETH, BASE_USDC),
        (TOKEN, BASE_USDC),
        (OTHER, BASE_USDC),
    ]);
    // TOKEN answers only decimals; OTHER reverts everything.
    node.metadata.insert(
        TOKEN,
        TokenMetadata {
            decimals: Some(18),
            ..TokenMetadata::default()
        },
    );
    let asked = node.asked.clone();
    let run = run(EngineConfig::base(), blocks, node);

    let metadata: Vec<Asked> = asked
        .borrow()
        .iter()
        .filter(|a| matches!(a, Asked::Metadata { .. }))
        .cloned()
        .collect();
    assert_eq!(
        metadata,
        [Asked::Metadata {
            block: 101,
            tokens: vec![TOKEN, OTHER, BASE_WETH, BASE_USDC]
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect(),
        }]
    );
    let p = prices_of(&run, TOKEN);
    assert!(!p.is_empty());
    assert_eq!(p[0].metadata.name, None);
    assert_eq!(p[0].fdv_usd, None, "no supply, no market cap");
    assert!(prices_of(&run, OTHER).is_empty(), "no decimals, no price");
    let coverage = run.summary.coverage.unwrap();
    assert_eq!(coverage.tokens_without_decimals, 1);
    assert_eq!(coverage.tokens_quotable, 2);
}

// #76: supply is "re-read when supply can change": every `supply_refresh_blocks`.
#[test]
fn a_priced_tokens_supply_is_read_again_after_the_refresh_period() {
    let blocks = chain(vec![
        vec![
            sync(BASE_WETH, BASE_USDC, 0, 1_000 * E18, 3_000_000 * E6),
            sync(TOKEN, BASE_USDC, 1, 1_000 * E18, 50_000 * E6),
        ],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
    ]);
    let mut node = Node::new(&[(BASE_WETH, BASE_USDC), (TOKEN, BASE_USDC)]);
    node.metadata.insert(TOKEN, token("Token", "TKN", 18, E18));
    let asked = node.asked.clone();
    let mut config = EngineConfig::base();
    config.pricing.as_mut().unwrap().supply_refresh_blocks = 3;
    run(config, blocks, node);
    let supply: Vec<Asked> = asked
        .borrow()
        .iter()
        .filter(|a| matches!(a, Asked::Supply { .. }))
        .cloned()
        .collect();
    // Metadata read at 101; WETH and TOKEN priced at 102, USDC never (pinned): due at 104,
    // the oldest reads first, then by address.
    assert_eq!(
        supply,
        [Asked::Supply {
            block: 104,
            tokens: vec![TOKEN, BASE_WETH]
        }]
    );
}

// Pools between tokens that aren't quote assets don't price either token (D19); coverage
// counts what they trade against.
#[test]
fn a_token_without_a_quote_pool_is_unpriced_and_counted() {
    let blocks = chain(vec![vec![sync(TOKEN, OTHER, 0, E18, E18)], vec![]]);
    let node = Node::new(&[(TOKEN, OTHER)]);
    let asked = node.asked.clone();
    let run = run(EngineConfig::base(), blocks, node);
    assert!(run.prices.is_empty());
    assert!(
        asked.borrow().iter().all(|a| matches!(a, Asked::Pairs(_))),
        "no metadata read for an unpriceable token"
    );
    let coverage = run.summary.coverage.unwrap();
    assert_eq!(coverage.tokens_seen, 2);
    assert_eq!(coverage.tokens_quotable, 0);
    assert_eq!(coverage.unquoted_counterparts, [(TOKEN, 1), (OTHER, 1)]);
}

/// data.md: BLAKE3 over the kind (length-prefixed, u64 little-endian) and the key fields, cut
/// to 128 bits. Written out from the doc, not the code.
fn documented_id(kind: &str, fields: &[&[u8]]) -> [u8; 16] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&(kind.len() as u64).to_le_bytes());
    hasher.update(kind.as_bytes());
    for field in fields {
        hasher.update(field);
    }
    hasher.finalize().as_bytes()[..16].try_into().unwrap()
}

// data.md: a price update keys on (chain id, token, block hash).
#[test]
fn price_update_lineage_follows_data_md() {
    let blocks = chain(vec![
        vec![sync(BASE_WETH, BASE_USDC, 0, 1_000 * E18, 3_000_000 * E6)],
        vec![],
        vec![],
    ]);
    let run = run(
        EngineConfig::base(),
        blocks,
        Node::new(&[(BASE_WETH, BASE_USDC)]),
    );
    let proto = run.prices[0].to_proto();
    assert_eq!(
        proto.lineage.unwrap().id,
        documented_id(
            "price_update",
            &[
                &8453u64.to_le_bytes(),
                BASE_WETH.as_slice(),
                hash(102).as_slice()
            ]
        )
    );
    assert_eq!(proto.block_timestamp, 1_767_225_604);
}

// #76: "A recorded session reprices identically on replay (the det rules apply)".
#[test]
fn a_recorded_session_reprices_identically_on_replay() {
    let blocks = chain(vec![
        vec![
            sync(BASE_WETH, BASE_USDC, 0, 1_000 * E18, 3_000_000 * E6),
            sync(TOKEN, BASE_WETH, 1, 20_000 * E18, 20 * E18),
        ],
        vec![sync(TOKEN, BASE_USDC, 0, 103_448 * E18, 299_999_200_000)],
        vec![sync(BASE_WETH, BASE_USDC, 0, 999 * E18, 3_003_003 * E6)],
        vec![sync(
            TOKEN,
            BASE_WETH,
            4,
            19_990 * E18,
            20_010_000_000_000_000_000,
        )],
        vec![sync(BASE_WETH, BASE_USDC, 0, 998 * E18, 3_006_012 * E6)],
        vec![sync(TOKEN, BASE_USDC, 2, 103_400 * E18, 300_138_000_000)],
        vec![],
    ]);
    let mut node = Node::new(&[
        (BASE_WETH, BASE_USDC),
        (TOKEN, BASE_WETH),
        (TOKEN, BASE_USDC),
    ]);
    node.metadata
        .insert(TOKEN, token("Token", "TKN", 18, 1_000_000 * E18));
    let mut config = EngineConfig::base();
    config.pricing.as_mut().unwrap().supply_refresh_blocks = 2;
    let live = run(config.clone(), blocks, node);
    assert!(
        live.prices.len() >= 4,
        "{} price updates",
        live.prices.len()
    );
    assert!(live.summary.stats.supply_calls > 0);

    let replay = Replay::new(live.recording);
    let recorded = EngineConfig::decode(&replay.config().unwrap()).unwrap();
    assert_eq!(recorded, config, "the pricing config is recorded");
    let outbox = InMemoryOutbox::default();
    let engine = Engine::new(
        recorded,
        Box::new(replay.clock()),
        Box::new(replay.events()),
        Box::new(replay.rpc()),
        Box::new(outbox.clone()),
    );
    let summary = replay.run(engine.run()).unwrap();
    assert_eq!(summary, live.summary);
    let bits = |prices: &[PriceUpdate]| -> Vec<Vec<u8>> {
        prices
            .iter()
            .map(|p| prost::Message::encode_to_vec(&p.to_proto()))
            .collect()
    };
    assert_eq!(bits(&outbox.prices()), bits(&live.prices));
}

// A recording from before pricing has no pricing config: it replays without pricing, so it
// asks for no calls the recording doesn't hold.
#[test]
fn a_config_without_pricing_round_trips_and_prices_nothing() {
    let unpriced = EngineConfig {
        pricing: None,
        ..EngineConfig::base()
    };
    assert_eq!(
        EngineConfig::decode(&unpriced.encode()),
        Some(unpriced.clone())
    );
    let priced = EngineConfig::base();
    assert_eq!(EngineConfig::decode(&priced.encode()), Some(priced));
    assert_eq!(EngineConfig::base().pricing, Some(PricingConfig::base()));

    let blocks = chain(vec![
        vec![sync(BASE_WETH, BASE_USDC, 0, 1_000 * E18, 3_000_000 * E6)],
        vec![],
    ]);
    let node = Node::new(&[(BASE_WETH, BASE_USDC)]);
    let asked = node.asked.clone();
    let run = run(unpriced, blocks, node);
    assert!(run.prices.is_empty());
    assert_eq!(run.summary.coverage, None);
    assert_eq!(asked.borrow().len(), 1, "only the pair's proof");
}
