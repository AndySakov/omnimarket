//! The discovery read model (#81): New lists pools created during the session within a block of
//! their discovery; Trending ranks tokens by 5m volume and txns, and its order holds while ranks
//! don't change.

use std::collections::BTreeMap;

use api::{DISCOVERY_TOPIC, DiscoveryConfig, Feed, Filters, Published, Session, hex};
use proto::api::v1::{DiscoveryList, DiscoveryRow, delta, server_message, snapshot};
use proto::lineage::v1::Lineage;
use proto::pool::v1::{PoolState, PoolUpdate, V2Reserves, V3State, pool_state};
use proto::price::v1::{PoolPrice, PriceUpdate, TokenMetadata};
use proto::trade::v1::{Trade, trade};

const WETH: [u8; 20] = [0x42; 20];

fn lineage(tag: &[u8]) -> Option<Lineage> {
    let mut id = tag.to_vec();
    id.resize(16, 0);
    Some(Lineage {
        id,
        caused_by: vec![],
    })
}

fn time(block: u64) -> u64 {
    1_700_000_000 + 2 * block
}

fn pool_update(pool: u8, block: u64, reserves: &[u8]) -> PoolUpdate {
    PoolUpdate {
        lineage: lineage(&[1, pool, block as u8]),
        chain_id: 8453,
        pool: vec![pool; 20],
        block_number: block,
        block_hash: vec![block as u8; 32],
        log_index: 0,
        before: None,
        after: Some(PoolState {
            state: Some(pool_state::State::V2(V2Reserves {
                reserve0: reserves.to_vec(),
                reserve1: reserves.to_vec(),
            })),
        }),
    }
}

fn price(token: u8, pool: u8, block: u64, usd: f64, depth_usd: f64) -> PriceUpdate {
    PriceUpdate {
        lineage: lineage(&[2, token, block as u8]),
        chain_id: 8453,
        token: vec![token; 20],
        block_number: block,
        block_hash: vec![block as u8; 32],
        block_timestamp: time(block),
        metadata: Some(TokenMetadata {
            name: Some("Token".into()),
            symbol: Some("TKN".into()),
            decimals: Some(18),
            total_supply: None,
        }),
        price_usd: usd,
        depth_usd,
        thin: false,
        main_pool: vec![pool; 20],
        quote_token: WETH.to_vec(),
        price_in_quote: usd,
        fdv_usd: None,
        pools: vec![PoolPrice {
            pool: vec![pool; 20],
            quote_token: WETH.to_vec(),
            price_in_quote: usd,
            price_usd: usd,
            depth_usd,
            counted: true,
        }],
    }
}

/// A buy of `tokens` whole tokens (18 decimals).
fn buy(token: u8, pool: u8, block: u64, tokens: u64, log_index: u64) -> Trade {
    let amount = u128::from(tokens) * 1_000_000_000_000_000_000;
    Trade {
        lineage: lineage(&[3, token, block as u8, log_index as u8]),
        chain_id: 8453,
        pool: vec![pool; 20],
        venue: trade::Venue::UniswapV2.into(),
        token: vec![token; 20],
        quote: WETH.to_vec(),
        side: trade::Side::Buy.into(),
        token_amount: amount.to_be_bytes().to_vec(),
        quote_amount: vec![1],
        block_number: block,
        block_hash: vec![block as u8; 32],
        block_timestamp: time(block),
        log_index,
        ..Trade::default()
    }
}

fn sell(token: u8, pool: u8, block: u64, tokens: u64, log_index: u64) -> Trade {
    Trade {
        side: trade::Side::Sell.into(),
        ..buy(token, pool, block, tokens, log_index)
    }
}

fn rows(published: &[Published]) -> Vec<DiscoveryRow> {
    published
        .iter()
        .filter_map(|p| match &p.payload {
            delta::Payload::DiscoveryRow(row) => Some(row.clone()),
            _ => None,
        })
        .collect()
}

/// Moves all three topics to `block` with records that make no row (a pool first seen with
/// reserves, a trade in a token never priced, WETH's price), so every earlier block is whole and
/// the last one publishes.
fn tick(feed: &mut Feed, block: u64) -> Vec<Published> {
    let mut out = feed.apply_pool_update(&pool_update(0x77, block, &[0x01]));
    out.extend(feed.apply_trade(&buy(0x99, 0x98, block, 1, 99)));
    out.extend(
        feed.apply_price(&price(0x42, 0x01, block, 2000.0, 1e6))
            .unwrap(),
    );
    out
}

fn discovery_only(published: Vec<Published>) -> Vec<Published> {
    published
        .into_iter()
        .filter(|p| p.topic == DISCOVERY_TOPIC)
        .collect()
}

fn token_of(row: &DiscoveryRow) -> String {
    row.token.as_ref().unwrap().address.clone()
}

/// The published feed's rows in one list, in rank order.
fn list(feed: &Feed, list: DiscoveryList) -> Vec<(String, u32)> {
    let filters = Filters {
        list: Some(list),
        ..Filters::default()
    };
    let mut rows: Vec<(String, u32)> = feed
        .discovery()
        .feed(&filters)
        .rows
        .iter()
        .map(|r| (token_of(r), r.rank))
        .collect();
    rows.sort_by_key(|r| r.1);
    rows
}

#[test]
fn a_pool_created_during_the_session_is_new_within_one_block() {
    let mut feed = Feed::default();
    feed.apply_price(&price(0x42, 0x01, 9, 2000.0, 1e6))
        .unwrap();
    // Created empty at block 10, funded and priced in the same block.
    feed.apply_pool_update(&pool_update(0xc1, 10, &[]));
    feed.apply_pool_update(&pool_update(0xc1, 10, &[0x10]));
    feed.apply_price(&price(0xa1, 0xc1, 10, 0.5, 500.0))
        .unwrap();
    // Once every topic has reached the next block, block 10's rows publish.
    let published = tick(&mut feed, 11);
    assert!(
        discovery_only(published.clone())
            .iter()
            .all(|p| p.block_number == 10)
    );
    let new = rows(&published);
    assert_eq!(new.len(), 1, "{new:?}");
    let row = &new[0];
    assert_eq!(token_of(row), hex(&[0xa1; 20]));
    assert_eq!(row.pool, hex(&[0xc1; 20]));
    assert_eq!(row.venue, "uniswap-v2");
    assert_eq!(row.pool_created_block, 10);
    assert_eq!(row.pool_created_at_ms, time(10) * 1000);
    assert_eq!(row.lists, vec![i32::from(DiscoveryList::New)]);
    assert_eq!(row.rank, 1);
    assert_eq!(row.display_price_usd, "0.5");
    assert!(
        published
            .iter()
            .all(|p| p.topic == DISCOVERY_TOPIC || p.topic.starts_with("token:"))
    );
}

#[test]
fn a_new_pool_is_found_whichever_record_names_its_token_first() {
    let mut feed = Feed::default();
    // The trade naming the pool's token arrives before the pool's own discovery record.
    feed.apply_trade(&buy(0xa2, 0xc2, 20, 1, 0));
    feed.apply_pool_update(&pool_update(0xc2, 20, &[]));
    let published = tick(&mut feed, 21);
    let new = rows(&published);
    assert_eq!(new.len(), 1, "{new:?}");
    assert_eq!(new[0].pool_created_block, 20);
    assert_eq!(new[0].lists, vec![i32::from(DiscoveryList::New)]);
}

#[test]
fn a_pool_first_seen_with_reserves_is_not_new() {
    let mut feed = Feed::default();
    feed.apply_pool_update(&pool_update(0xc3, 30, &[0x10]));
    feed.apply_price(&price(0xa3, 0xc3, 30, 1.0, 500.0))
        .unwrap();
    let published = tick(&mut feed, 31);
    assert!(rows(&published).is_empty());
}

#[test]
fn trending_ranks_by_volume_and_holds_its_order_while_ranks_hold() {
    let mut feed = Feed::default();
    feed.apply_price(&price(0xa1, 0xc1, 1, 1.0, 50_000.0))
        .unwrap();
    feed.apply_price(&price(0xa2, 0xc2, 1, 1.0, 50_000.0))
        .unwrap();
    feed.apply_price(&price(0xa3, 0xc3, 1, 1.0, 50_000.0))
        .unwrap();
    // Too shallow to trend, whatever its volume.
    feed.apply_price(&price(0xa4, 0xc4, 1, 1.0, 100.0)).unwrap();
    feed.apply_trade(&buy(0xa1, 0xc1, 2, 100, 0));
    feed.apply_trade(&buy(0xa2, 0xc2, 2, 300, 1));
    feed.apply_trade(&buy(0xa3, 0xc3, 2, 200, 2));
    feed.apply_trade(&buy(0xa4, 0xc4, 2, 900, 3));
    tick(&mut feed, 3);
    let order = vec![
        (hex(&[0xa2; 20]), 1),
        (hex(&[0xa3; 20]), 2),
        (hex(&[0xa1; 20]), 3),
    ];
    assert_eq!(list(&feed, DiscoveryList::Trending), order);

    // More trades that leave the ranks as they were: the order holds, and no row moves.
    feed.apply_trade(&buy(0xa1, 0xc1, 4, 10, 0));
    feed.apply_trade(&buy(0xa2, 0xc2, 4, 10, 1));
    let published = tick(&mut feed, 5);
    assert_eq!(list(&feed, DiscoveryList::Trending), order);
    let changed: BTreeMap<String, u32> = rows(&published)
        .iter()
        .map(|r| (token_of(r), r.rank))
        .collect();
    assert_eq!(
        changed,
        BTreeMap::from([(hex(&[0xa1; 20]), 3), (hex(&[0xa2; 20]), 1)]),
        "only the rows that traded are sent, at their old ranks"
    );
    let row = &rows(&published)[1];
    assert_eq!(row.stats_5m.as_ref().unwrap().volume_usd, "310");
    assert_eq!(row.stats_5m.as_ref().unwrap().buys, 2);

    // A rank change moves the rows it touches.
    feed.apply_trade(&buy(0xa1, 0xc1, 6, 1000, 0));
    tick(&mut feed, 7);
    assert_eq!(list(&feed, DiscoveryList::Trending)[0].0, hex(&[0xa1; 20]));
}

#[test]
fn a_row_leaves_when_its_pool_ages_out_of_new() {
    let mut feed = Feed::new(DiscoveryConfig {
        new_pool_window_ms: 10_000,
        ..DiscoveryConfig::default()
    });
    feed.apply_pool_update(&pool_update(0xc1, 10, &[]));
    feed.apply_price(&price(0xa1, 0xc1, 10, 0.5, 500.0))
        .unwrap();
    assert_eq!(rows(&tick(&mut feed, 11)).len(), 1);
    // Five blocks (10s) later it's still new; at six it has aged out.
    tick(&mut feed, 15);
    let published = discovery_only(tick(&mut feed, 16));
    assert!(published.is_empty(), "{published:?}");
    let published = discovery_only(tick(&mut feed, 17));
    assert!(matches!(&published[..], [p] if matches!(&p.payload,
            delta::Payload::DiscoveryRowRemoved(r) if r.token == hex(&[0xa1; 20]))));
}

#[test]
fn filters_drop_shallow_and_old_rows_server_side() {
    let mut feed = Feed::default();
    feed.apply_price(&price(0xa1, 0xc1, 1, 1.0, 50_000.0))
        .unwrap();
    feed.apply_trade(&buy(0xa1, 0xc1, 1, 1, 0));
    feed.apply_pool_update(&pool_update(0xc2, 2, &[]));
    feed.apply_price(&price(0xa2, 0xc2, 2, 1.0, 500.0)).unwrap();
    tick(&mut feed, 12);
    tick(&mut feed, 13);
    let tokens = |filters: Filters| -> Vec<String> {
        feed.discovery()
            .feed(&filters)
            .rows
            .iter()
            .map(token_of)
            .collect()
    };
    assert_eq!(tokens(Filters::default()).len(), 2);
    assert_eq!(
        tokens(Filters {
            min_depth_usd: Some(1_000.0),
            ..Filters::default()
        }),
        vec![hex(&[0xa1; 20])]
    );
    // The new pool is 10 blocks (20s) old; the trending token's pool has no known age.
    assert_eq!(
        tokens(Filters {
            max_age_ms: Some(20_000),
            ..Filters::default()
        }),
        vec![hex(&[0xa2; 20])]
    );
    assert!(
        tokens(Filters {
            max_age_ms: Some(19_999),
            ..Filters::default()
        })
        .is_empty()
    );
}

#[test]
fn a_discovery_subscriber_gets_the_feed_then_row_deltas() {
    let mut feed = Feed::default();
    let mut session = Session::default();
    let out = session.on_text(r#"{"subscribe":{"topic":"discovery"}}"#, &feed);
    assert!(matches!(
        out[0].kind.as_ref().unwrap(),
        server_message::Kind::Snapshot(s)
            if matches!(&s.payload, Some(snapshot::Payload::Discovery(f)) if f.rows.is_empty())
    ));
    feed.apply_pool_update(&pool_update(0xc1, 10, &[]));
    feed.apply_price(&price(0xa1, 0xc1, 10, 0.5, 500.0))
        .unwrap();
    let published = tick(&mut feed, 11);
    let delta = published
        .iter()
        .find_map(|p| session.on_published(p, &feed))
        .expect("a delta");
    match delta.kind.unwrap() {
        server_message::Kind::Delta(d) => {
            assert_eq!(d.topic, "discovery");
            assert_eq!(d.seq, 1);
            assert!(matches!(d.payload, Some(delta::Payload::DiscoveryRow(_))));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_rest_filters_parse_or_refuse() {
    let params = |pairs: &[(&str, &str)]| -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    };
    assert_eq!(
        api::server::parse_filters(&params(&[
            ("list", "trending"),
            ("min_depth_usd", "2500.5"),
            ("max_age_ms", "60000"),
        ])),
        Some(Filters {
            list: Some(DiscoveryList::Trending),
            min_depth_usd: Some(2500.5),
            max_age_ms: Some(60_000),
        })
    );
    assert_eq!(
        api::server::parse_filters(&params(&[("list", "new"), ("max_age_ms", "")])),
        Some(Filters {
            list: Some(DiscoveryList::New),
            ..Filters::default()
        })
    );
    assert_eq!(
        api::server::parse_filters(&params(&[("list", "hot")])),
        None
    );
    assert_eq!(
        api::server::parse_filters(&params(&[("max_age_ms", "-1")])),
        None
    );
}

#[test]
fn window_stats_count_their_window_and_price_change_from_its_open() {
    let mut feed = Feed::default();
    // A 5m window is 150 blocks, an hour 1800; rows publish as of block 1802.
    feed.apply_price(&price(0xa1, 0xc1, 1, 1.0, 50_000.0))
        .unwrap();
    feed.apply_trade(&buy(0xa1, 0xc1, 1, 4, 0));
    // Exactly an hour before the rows' block: the hour's first moment.
    feed.apply_trade(&sell(0xa1, 0xc1, 2, 3, 0));
    feed.apply_price(&price(0xa1, 0xc1, 1000, 2.0, 50_000.0))
        .unwrap();
    feed.apply_trade(&buy(0xa1, 0xc1, 1000, 10, 0));
    feed.apply_price(&price(0xa1, 0xc1, 1700, 4.0, 50_000.0))
        .unwrap();
    feed.apply_trade(&buy(0xa1, 0xc1, 1700, 1, 0));
    feed.apply_price(&price(0xa1, 0xc1, 1802, 4.0, 50_000.0))
        .unwrap();
    tick(&mut feed, 1803);
    let row = feed.discovery().feed(&Filters::default()).rows[0].clone();
    let s5 = row.stats_5m.unwrap();
    // A trade is valued at the price before its block's prices apply. 5m: the trade at block
    // 1700 (1 token at $2); the price opened at $2.
    assert_eq!((s5.volume_usd.as_str(), s5.buys), ("2", 1));
    assert_eq!(s5.price_change_pct, "100");
    // 1h: blocks 2, 1000 and 1700; the block-1 trade aged out; the hour opened at $1.
    let s1h = row.stats_1h.unwrap();
    assert_eq!((s1h.volume_usd.as_str(), s1h.buys, s1h.sells), ("15", 2, 1));
    assert_eq!(s1h.price_change_pct, "300");
    // Since tracking began: everything (the block-1 trade came before the first price).
    let tracked = row.stats_tracked.unwrap();
    assert_eq!(
        (tracked.volume_usd.as_str(), tracked.buys, tracked.sells),
        ("15", 3, 1)
    );
    assert_eq!(tracked.price_change_pct, "300");
    // The row is caused by the latest record about its token, and names its quote asset.
    let lineage = row.lineage.unwrap();
    assert_eq!(
        lineage.caused_by,
        vec![{
            let mut id = vec![2, 0xa1, 1802u64 as u8];
            id.resize(16, 0);
            id
        }]
    );
    assert_eq!(lineage.id.len(), 16);
    let quote = row.quote_token.unwrap();
    assert_eq!((quote.chain_id, quote.address), (8453, hex(&WETH)));
    assert_eq!(row.pool, hex(&[0xc1; 20]));
}

fn v3_discovered(pool: u8, block: u64, initialized: bool) -> PoolUpdate {
    PoolUpdate {
        after: Some(PoolState {
            state: Some(pool_state::State::V3(V3State {
                initialized,
                ..V3State::default()
            })),
        }),
        ..pool_update(pool, block, &[])
    }
}

#[test]
fn a_pool_is_new_only_when_discovered_empty() {
    let mut feed = Feed::default();
    // New: an uninitialized v3 pool.
    feed.apply_pool_update(&v3_discovered(0xc1, 10, false));
    feed.apply_price(&price(0xa1, 0xc1, 10, 1.0, 500.0))
        .unwrap();
    // Not new: an initialized v3 pool, and a v2 pair with one reserve already in.
    feed.apply_pool_update(&v3_discovered(0xc2, 10, true));
    feed.apply_price(&price(0xa2, 0xc2, 10, 1.0, 500.0))
        .unwrap();
    let mut half = pool_update(0xc3, 10, &[]);
    half.after = Some(PoolState {
        state: Some(pool_state::State::V2(V2Reserves {
            reserve0: vec![],
            reserve1: vec![0x10],
        })),
    });
    feed.apply_pool_update(&half);
    feed.apply_price(&price(0xa3, 0xc3, 10, 1.0, 500.0))
        .unwrap();
    let new = rows(&tick(&mut feed, 11));
    let tokens: Vec<String> = new.iter().map(token_of).collect();
    assert_eq!(tokens, vec![hex(&[0xa1; 20])]);
    assert_eq!(new[0].venue, "uniswap-v3");
}

#[test]
fn a_token_with_two_new_pools_shows_the_newest_whichever_record_names_it() {
    let mut feed = Feed::default();
    feed.apply_pool_update(&pool_update(0xc1, 10, &[]));
    feed.apply_pool_update(&pool_update(0xc2, 12, &[]));
    // One price record lists both, older first.
    let mut both = price(0xa1, 0xc1, 12, 1.0, 500.0);
    let mut second = both.pools[0].clone();
    second.pool = vec![0xc2; 20];
    both.pools.push(second);
    feed.apply_price(&both).unwrap();
    // A trade names a token's older pool after its newer one; another names a newer pool.
    feed.apply_pool_update(&pool_update(0xc3, 12, &[]));
    feed.apply_pool_update(&pool_update(0xc4, 13, &[]));
    feed.apply_trade(&buy(0xa2, 0xc4, 13, 1, 0));
    feed.apply_trade(&buy(0xa2, 0xc3, 13, 1, 1));
    feed.apply_pool_update(&pool_update(0xc5, 13, &[]));
    feed.apply_trade(&buy(0xa3, 0xc3, 13, 1, 2));
    feed.apply_trade(&buy(0xa3, 0xc5, 13, 1, 3));
    tick(&mut feed, 14);
    let pools: BTreeMap<String, (String, u64)> = feed
        .discovery()
        .feed(&Filters::default())
        .rows
        .iter()
        .map(|r| (token_of(r), (r.pool.clone(), r.pool_created_block)))
        .collect();
    assert_eq!(pools[&hex(&[0xa1; 20])], (hex(&[0xc2; 20]), 12));
    assert_eq!(pools[&hex(&[0xa2; 20])], (hex(&[0xc4; 20]), 13));
    assert_eq!(pools[&hex(&[0xa3; 20])], (hex(&[0xc5; 20]), 13));
}

#[test]
fn trending_breaks_volume_ties_by_txns_and_lists_only_its_rows() {
    let mut feed = Feed::default();
    feed.apply_price(&price(0xa1, 0xc1, 1, 1.0, 50_000.0))
        .unwrap();
    feed.apply_price(&price(0xa2, 0xc2, 1, 1.0, 50_000.0))
        .unwrap();
    // The same $10 of volume: a2 in two trades, a1 in one. Txns put a2 first, ahead of the
    // address order.
    feed.apply_trade(&buy(0xa1, 0xc1, 2, 10, 0));
    feed.apply_trade(&buy(0xa2, 0xc2, 2, 5, 1));
    feed.apply_trade(&buy(0xa2, 0xc2, 2, 5, 2));
    // New, and not trending: never traded.
    feed.apply_pool_update(&pool_update(0xc3, 2, &[]));
    feed.apply_price(&price(0xa3, 0xc3, 2, 1.0, 50_000.0))
        .unwrap();
    tick(&mut feed, 3);
    assert_eq!(
        list(&feed, DiscoveryList::Trending),
        vec![(hex(&[0xa2; 20]), 1), (hex(&[0xa1; 20]), 2)]
    );
    let lists: BTreeMap<String, Vec<i32>> = feed
        .discovery()
        .feed(&Filters::default())
        .rows
        .iter()
        .map(|r| (token_of(r), r.lists.clone()))
        .collect();
    let trending = vec![i32::from(DiscoveryList::Trending)];
    assert_eq!(lists[&hex(&[0xa1; 20])], trending);
    assert_eq!(lists[&hex(&[0xa2; 20])], trending);
    assert_eq!(
        lists[&hex(&[0xa3; 20])],
        vec![i32::from(DiscoveryList::New)]
    );
}

#[test]
fn a_depth_at_the_minimum_passes_and_a_zero_opening_price_has_no_change() {
    let mut feed = Feed::default();
    feed.apply_pool_update(&pool_update(0xc1, 1, &[]));
    feed.apply_price(&price(0xa1, 0xc1, 1, 0.0, 1_000.0))
        .unwrap();
    feed.apply_price(&price(0xa1, 0xc1, 2, 1.0, 1_000.0))
        .unwrap();
    tick(&mut feed, 3);
    let filters = Filters {
        min_depth_usd: Some(1_000.0),
        ..Filters::default()
    };
    let rows = feed.discovery().feed(&filters).rows;
    assert_eq!(rows.len(), 1);
    let tracked = rows[0].stats_tracked.clone().unwrap();
    assert_eq!(tracked.price_change_pct, "");
}
