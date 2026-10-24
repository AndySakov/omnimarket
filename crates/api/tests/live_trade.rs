//! A buy and a sell against a live Base pool (#84): the WETH/USDC 0.05% pool's state read at
//! the latest block, quoted from it, and each trade simulated through Uniswap's quoter at that
//! block. Needs the network: `cargo test -p api --test live_trade -- --ignored --nocapture`.

use alloy_primitives::{B256, U256};
use api::{Quotes, hex};
use det::{ChannelRpc, Rpc, SystemClock};
use execution::LocalSigner;
use execution::shadow::{self, Request, Signer, Simulation};
use proto::api::v1::QuoteRequest;
use proto::api::v1::trade_status::State;
use proto::pool::v1::{PoolKey, PoolState, PoolUpdate, V3State, V3Tick, pool_key, pool_state};
use proto::price::v1::{PriceUpdate, TokenMetadata};
use types::chain::{Address, Bytes, CallResult, EthCall};
use venues::v3::{self, BASE};

const POOL: Address = alloy_primitives::address!("d0b53D9277642d899DF5C87A3966A349A798F224");
const WETH: Address = alloy_primitives::address!("4200000000000000000000000000000000000006");
const USDC: Address = alloy_primitives::address!("833589fCD6eDb6E08f4c7C32D4f71b54bdA02913");

fn call(
    rpc: &ChannelRpc<EthCall, CallResult>,
    runtime: &tokio::runtime::Runtime,
    to: Address,
    data: Vec<u8>,
    block: u64,
) -> Vec<u8> {
    match runtime.block_on(rpc.call(EthCall {
        to,
        data: Bytes::from(data),
        block,
    })) {
        CallResult::Returned(bytes) => bytes.to_vec(),
        CallResult::Failed(error) => panic!("eth_call failed: {error}"),
    }
}

#[test]
#[ignore = "needs the network: cargo test -p api --test live_trade -- --ignored --nocapture"]
fn a_live_buy_and_sell_simulate_within_their_quotes_minimums() {
    let url = std::env::var("BASE_RPC").unwrap_or_else(|_| "https://mainnet.base.org".into());
    let worker = chain_io::spawn_call_worker(&url, 5).expect("the endpoint answers");
    let rpc = ChannelRpc::new(worker.requests.clone());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let latest = {
        let chain = chain_io::HttpChain::new(&url).unwrap();
        runtime
            .block_on(chain_io::ChainReader::latest_number(&chain))
            .unwrap()
    };
    // A few blocks back, so every node behind the endpoint has it.
    let block = latest - 3;
    let price = v3::decode_slot0(&call(&rpc, &runtime, POOL, v3::slot0_call(), block)).unwrap();
    let liquidity =
        v3::decode_liquidity(&call(&rpc, &runtime, POOL, v3::liquidity_call(), block)).unwrap();
    // The ticks in the bitmap words within ±2 of the price's: ±5,120 ticks, ±67% in price.
    let word = (price.tick.div_euclid(10) >> 8) as i16;
    let mut ticks = Vec::new();
    for w in word - 2..=word + 2 {
        let answer = call(
            &rpc,
            &runtime,
            BASE.tick_lens,
            v3::populated_ticks_call(POOL, w),
            block,
        );
        ticks.extend(v3::decode_populated_ticks(&answer).unwrap());
    }
    let mut quotes = Quotes::default();
    quotes.apply_pool_update(&PoolUpdate {
        chain_id: 8453,
        pool: POOL.to_vec(),
        block_number: block,
        after: Some(PoolState {
            state: Some(pool_state::State::V3(V3State {
                initialized: true,
                sqrt_price_x96: price.sqrt_price_x96.to_be_bytes_trimmed_vec(),
                tick: price.tick,
                liquidity: U256::from(liquidity).to_be_bytes_trimmed_vec(),
                ticks: ticks
                    .iter()
                    .map(|(tick, l)| V3Tick {
                        tick: *tick,
                        liquidity_gross: U256::from(l.gross).to_be_bytes_trimmed_vec(),
                        liquidity_net: l.net.to_be_bytes().to_vec(),
                    })
                    .collect(),
            })),
        }),
        key: Some(PoolKey {
            venue: pool_key::Venue::UniswapV3.into(),
            token0: WETH.to_vec(),
            token1: USDC.to_vec(),
            fee_pips: 500,
            tick_spacing: 10,
        }),
        ..PoolUpdate::default()
    });
    quotes.apply_price(&PriceUpdate {
        token: WETH.to_vec(),
        block_number: block,
        block_timestamp: 1,
        price_usd: 2_500.0,
        metadata: Some(TokenMetadata {
            decimals: Some(18),
            ..TokenMetadata::default()
        }),
        ..PriceUpdate::default()
    });

    let signer = LocalSigner::from_bytes(&B256::repeat_byte(0x11)).unwrap();
    for (id, token_in, token_out, amount) in [
        ("live-buy", USDC, WETH, "100"),
        ("live-sell", WETH, USDC, "0.05"),
    ] {
        let quote = quotes
            .quote(&QuoteRequest {
                client_request_id: id.into(),
                chain_id: 8453,
                token_in: hex(token_in.as_slice()),
                token_out: hex(token_out.as_slice()),
                amount_in: amount.into(),
                slippage_bps: None,
            })
            .unwrap();
        let finished = runtime.block_on(shadow::run(
            Request {
                trade_id: id.into(),
                client_request_id: id.into(),
                account_id: "guest".into(),
                quote: &quote,
            },
            || Ok(quote.clone()),
            Signer::Guest(&signer),
            Simulation::Node(&rpc),
            &SystemClock,
        ));
        let receipt = finished.receipt.expect("the trade simulated");
        println!(
            "{id} at block {block}: quoted {} {}, simulated {}, minimum {}, steps {:?}",
            quote.amount_out,
            quote.token_out.as_ref().unwrap().symbol,
            receipt.amount_out,
            receipt.min_amount_out,
            receipt
                .steps
                .iter()
                .map(|s| s.finished_at_ms - s.started_at_ms)
                .collect::<Vec<_>>(),
        );
        assert_eq!(finished.status.state(), State::Simulated);
        assert_eq!(receipt.amount_out, quote.amount_out, "the quote is exact");
    }
    drop(rpc);
    drop(worker.requests);
    let _ = worker.thread.join();
}
