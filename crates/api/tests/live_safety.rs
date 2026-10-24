//! The safety probe against live Base pools (#89): DEGEN and TOSHI, through their Uniswap v3 and
//! v2 WETH pools, in one `eth_call` each with the probe injected. Needs the network:
//! `cargo test -p api --test live_safety -- --ignored --nocapture`.

use alloy_primitives::{U256, address};
use execution::safety::{self, RoundTrip, Verdict};
use types::chain::{Address, CallResult, EthCall};

const WETH: Address = address!("4200000000000000000000000000000000000006");

#[tokio::test]
#[ignore = "needs the network: cargo test -p api --test live_safety -- --ignored --nocapture"]
async fn live_tokens_round_trip_through_their_pools() {
    let url = std::env::var("BASE_RPC").unwrap_or_else(|_| "https://mainnet.base.org".into());
    let chain = chain_io::HttpChain::new(&url).unwrap();
    let block = chain_io::ChainReader::latest_number(&chain).await.unwrap() - 3;
    let degen = address!("4ed4E862860beD51a9570b96d89aF5E1B0Efefed");
    let toshi = address!("AC1Bd2486aAf3B5C0fc3Fd868558b082a531B2B4");
    let amount = U256::from(10u64).pow(U256::from(16u64));
    for (name, token, pool, v3) in [
        (
            "DEGEN v3",
            degen,
            address!("c9034c3E7F58003E6ae0C8438e7c8f4598d5ACAA"),
            true,
        ),
        (
            "DEGEN v2",
            degen,
            address!("7C327d692B72f60b28AecEDbcC1BA784712fE7b2"),
            false,
        ),
        (
            "TOSHI v3",
            toshi,
            address!("5aa4AD647580bfE86258d300Bc9852F4434E2c61"),
            true,
        ),
        (
            "TOSHI v2",
            toshi,
            address!("F641ff99d18bAbd6fFa38FA5cBc09dD7DaB385C4"),
            false,
        ),
    ] {
        let call = EthCall {
            to: safety::PROBE,
            data: safety::calldata(WETH, token, pool, v3, amount),
            block,
        };
        let answer = chain
            .call_with_code(
                &call,
                safety::PROBE,
                safety::probe_code(),
                safety::probe_balance(),
            )
            .await
            .unwrap();
        let CallResult::Returned(bytes) = answer else {
            panic!("{name}: the probe reverted: {answer:?}");
        };
        let trip = RoundTrip::decode(&bytes, amount).unwrap();
        println!(
            "{name} at block {block}: bought {}, sold {}, buy tax {} ppm, sell tax {} ppm, returned {} of {} wei, {:?} {}",
            trip.bought,
            trip.sold,
            trip.buy_tax_ppm(),
            trip.sell_tax_ppm(),
            trip.returned,
            trip.spent,
            trip.verdict(),
            trip.reason
        );
        assert_eq!(trip.verdict(), Verdict::Passed, "{name}");
    }
}

/// DEGEN's check, run as the API's worker runs it, shows on its token snapshot.
#[tokio::test]
#[ignore = "needs the network: cargo test -p api --test live_safety -- --ignored --nocapture"]
async fn a_live_tokens_check_appears_on_its_token_snapshot() {
    use prost::Message as _;
    use proto::pool::v1::{PoolKey, PoolState, PoolUpdate, V2Reserves, pool_key, pool_state};
    use proto::price::v1::{PriceUpdate, TokenMetadata};

    let url = std::env::var("BASE_RPC").unwrap_or_else(|_| "https://mainnet.base.org".into());
    let chain = chain_io::HttpChain::new(&url).unwrap();
    let degen = address!("4ed4E862860beD51a9570b96d89aF5E1B0Efefed");
    let pair = address!("7C327d692B72f60b28AecEDbcC1BA784712fE7b2");
    let shared = api::server::Shared::new();
    // The engine's records of the pair (WETH is token0) and DEGEN's price; the reserves only
    // need to be there, since the probe trades against the chain's own state.
    shared.apply(
        engine::POOL_UPDATES_TOPIC,
        &PoolUpdate {
            chain_id: 8453,
            pool: pair.to_vec(),
            block_number: 1,
            after: Some(PoolState {
                state: Some(pool_state::State::V2(V2Reserves {
                    reserve0: vec![1],
                    reserve1: vec![1],
                })),
            }),
            key: Some(PoolKey {
                venue: pool_key::Venue::UniswapV2.into(),
                token0: WETH.to_vec(),
                token1: degen.to_vec(),
                fee_pips: 3_000,
                tick_spacing: 0,
            }),
            ..PoolUpdate::default()
        }
        .encode_to_vec(),
    );
    shared.apply(
        engine::PRICES_TOPIC,
        &PriceUpdate {
            chain_id: 8453,
            token: degen.to_vec(),
            block_number: 1,
            block_timestamp: 1,
            price_usd: 0.001,
            main_pool: pair.to_vec(),
            metadata: Some(TokenMetadata {
                symbol: Some("DEGEN".into()),
                decimals: Some(18),
                ..TokenMetadata::default()
            }),
            ..PriceUpdate::default()
        }
        .encode_to_vec(),
    );
    shared.check_safety(&chain, degen).await.unwrap();

    let app = api::server::router(shared.clone(), "http://localhost:5173").unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap().to_string();
    #[allow(clippy::disallowed_methods)] // the test's own server task
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let body = {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut stream = tokio::net::TcpStream::connect(&address).await.unwrap();
        let path = format!("/v1/tokens/8453/{}", api::hex(degen.as_slice()));
        stream
            .write_all(
                format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n")
                    .as_bytes(),
            )
            .await
            .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        response.split("\r\n\r\n").nth(1).unwrap().to_string()
    };
    let snapshot: proto::api::v1::TokenSnapshot = serde_json::from_str(&body).unwrap();
    let safety = snapshot.safety.unwrap();
    let trip = safety.round_trip.clone().unwrap();
    println!(
        "DEGEN at block {}: {:?}, spent {} WETH, back {} WETH, buy tax {}%, sell tax {}%, via {}",
        safety.checked_block,
        safety.verdict(),
        trip.amount_spent,
        trip.amount_returned,
        trip.buy_tax_pct,
        trip.sell_tax_pct,
        trip.pool
    );
    assert_eq!(safety.verdict(), proto::api::v1::SafetyVerdict::Passed);
    assert!(trip.sell_succeeded);
}
