//! Shadow trades (#84): re-quote, build, sign, simulate against the quoter, stop before
//! broadcast. Run on det's simulated clock and node, so the step timings are the simulated
//! node's latency, and a recording replays to the same outcome.

use std::time::Duration;

use alloy_primitives::{Address, B256, U256, address};
use alloy_sol_types::SolValue;
use det::{InMemorySink, Recorder, RecordingClock, RecordingRpc, Replay, SimClock, SimRpc};
use execution::shadow::{self, Finished, Request, Signer, Simulation};
use execution::{Intent, LocalSigner, intent};
use proto::api::v1::trade_status::State;
use proto::api::v1::{Fee, Quote, RouteLeg, Slippage, TokenRef, TradeStep};
use types::Timestamp;
use types::chain::{Bytes, CallResult, EthCall};

const START: Timestamp = Timestamp::from_unix_nanos(1_790_000_000_000_000_000);
const WETH: &str = "0x4200000000000000000000000000000000000006";
const USDC: &str = "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913";
const POOL: &str = "0xd0b53d9277642d899df5c87a3966a349a798f224";
const QUOTER: Address = address!("3d4e44Eb1374240CE5F1B871ab261CD16335B76a");

fn token(address: &str, symbol: &str, decimals: u32) -> TokenRef {
    TokenRef {
        chain_id: 8453,
        address: address.into(),
        symbol: symbol.into(),
        decimals,
        ..TokenRef::default()
    }
}

/// A buy of WETH with 1,000 USDC through the 0.05% pool: 10 USDC fee, 990 routed, 0.4 WETH
/// out, at least 0.388 (3%).
fn buy() -> Quote {
    Quote {
        quote_id: format!("0x{}", "ab".repeat(16)),
        chain_id: 8453,
        block_number: 100,
        expires_at_ms: 1_790_000_004_000,
        token_in: Some(token(USDC, "USDC", 6)),
        token_out: Some(token(WETH, "WETH", 18)),
        amount_in: "1000".into(),
        amount_out: "0.4".into(),
        min_amount_out: "0.388".into(),
        fee: Some(Fee {
            fee_bps: 100,
            amount: "10".into(),
            token: Some(token(USDC, "USDC", 6)),
            ..Fee::default()
        }),
        slippage: Some(Slippage {
            bps: 300,
            ..Slippage::default()
        }),
        route: vec![RouteLeg {
            pool: POOL.into(),
            venue: "uniswap-v3".into(),
            pool_fee_bps: 5,
            token_in: Some(token(USDC, "USDC", 6)),
            token_out: Some(token(WETH, "WETH", 18)),
            amount_in: "990".into(),
            amount_out: "0.4".into(),
        }],
        ..Quote::default()
    }
}

/// A sell of 1 WETH for USDC: the route pays 2,500 USDC before our fee, 2,475 after.
fn sell() -> Quote {
    Quote {
        token_in: Some(token(WETH, "WETH", 18)),
        token_out: Some(token(USDC, "USDC", 6)),
        amount_in: "1".into(),
        amount_out: "2475".into(),
        min_amount_out: "2400.75".into(),
        fee: Some(Fee {
            fee_bps: 100,
            amount: "25".into(),
            token: Some(token(USDC, "USDC", 6)),
            ..Fee::default()
        }),
        route: vec![RouteLeg {
            token_in: Some(token(WETH, "WETH", 18)),
            token_out: Some(token(USDC, "USDC", 6)),
            amount_in: "1".into(),
            amount_out: "2500".into(),
            ..buy().route[0].clone()
        }],
        ..buy()
    }
}

/// A quoter that answers every swap with `out`, 40ms later, and records what it was asked.
fn quoter(out: U256) -> SimRpc<EthCall, CallResult> {
    SimRpc::new(move |call: EthCall| {
        assert_eq!(call.to, QUOTER);
        assert_eq!(call.block, 100);
        // quoteExactInputSingle((tokenIn, tokenOut, amountIn, fee, limit)): the fourth word
        // after the selector is the pool's fee in pips, 5 bps being 500.
        assert_eq!(
            U256::from_be_slice(&call.data[4 + 3 * 32..4 + 4 * 32]),
            U256::from(500u64)
        );
        let answer = (out, U256::ZERO, 0u32, U256::ZERO).abi_encode_params();
        (
            Duration::from_millis(40),
            CallResult::Returned(Bytes::from(answer)),
        )
    })
}

fn signer() -> LocalSigner {
    LocalSigner::from_bytes(&B256::repeat_byte(0x11)).unwrap()
}

fn request(quote: &Quote) -> Request<'_> {
    Request {
        trade_id: "t-1".into(),
        client_request_id: "c-1".into(),
        account_id: "guest-1".into(),
        quote,
    }
}

fn trade(quote: Quote, fresh: Quote, out: U256) -> Finished {
    let local = signer();
    let rpc = quoter(out);
    det::run_simulated(async {
        // Made inside the simulated runtime, so it reads its paused time from the start.
        let clock = SimClock::starting_at(START);
        shadow::run(
            request(&quote),
            || Ok(fresh),
            Signer::Guest(&local),
            Simulation::Node(&rpc),
            &clock,
        )
        .await
    })
}

fn steps(finished: &Finished) -> Vec<(TradeStep, u64, u64)> {
    finished
        .status
        .steps
        .iter()
        .map(|s| (s.step(), s.started_at_ms, s.finished_at_ms))
        .collect()
}

#[test]
fn a_buy_simulates_within_its_minimum_and_stops_before_broadcast() {
    let finished = trade(buy(), buy(), U256::from(395_000_000_000_000_000u64));
    assert_eq!(finished.status.state(), State::Simulated);
    let t0 = 1_790_000_000_000;
    // The simulation's call takes the node's 40ms; every other step is instant.
    assert_eq!(
        steps(&finished),
        vec![
            (TradeStep::Quote, t0, t0),
            (TradeStep::Build, t0, t0),
            (TradeStep::Sign, t0, t0),
            (TradeStep::Simulate, t0, t0 + 40),
            (TradeStep::StopBeforeBroadcast, t0 + 40, t0 + 40),
        ]
    );
    let receipt = finished.receipt.unwrap();
    assert_eq!(
        (
            receipt.amount_out.as_str(),
            receipt.signed_min_amount_out.as_str(),
            receipt.min_amount_out.as_str()
        ),
        ("0.395", "0.388", "0.388")
    );
    assert_eq!((receipt.total_ms, receipt.block_number), (40, 100));
    assert!(receipt.demo_intent && !receipt.simulated_in_memory && receipt.tx_hash.is_empty());
    let expected = Intent {
        signer: signer().address(),
        tokenIn: USDC.parse().unwrap(),
        maxAmountIn: U256::from(1_000_000_000u64),
        tokenOut: WETH.parse().unwrap(),
        minAmountOut: U256::from(388_000_000_000_000_000u64),
        deadline: U256::from(1_790_000_064u64),
        submitters: vec![intent::DEMO_EXECUTOR],
        quoteId: B256::right_padding_from(&[0xab; 16]),
    };
    assert_eq!(
        receipt.intent_hash,
        intent::hash(&expected, 8453).to_string()
    );
}

#[test]
fn a_sell_takes_our_fee_from_the_simulated_proceeds() {
    // The quoter pays 2,490 USDC: 2,465.1 after the 1%.
    let finished = trade(sell(), sell(), U256::from(2_490_000_000u64));
    let receipt = finished.receipt.unwrap();
    assert_eq!(receipt.amount_out, "2465.1");
    assert_eq!(receipt.min_amount_out, "2400.75");
}

#[test]
fn the_same_quote_gives_the_same_intent_and_receipt() {
    let out = U256::from(395_000_000_000_000_000u64);
    assert_eq!(trade(buy(), buy(), out), trade(buy(), buy(), out));
}

#[test]
fn a_simulation_below_the_minimum_fails_the_trade() {
    let finished = trade(buy(), buy(), U256::from(380_000_000_000_000_000u64));
    assert_eq!(finished.status.state(), State::Failed);
    assert_eq!(finished.receipt, None);
    let last = finished.status.steps.last().unwrap();
    assert_eq!(
        (last.step(), last.detail.as_str()),
        (
            TradeStep::Simulate,
            "simulated 0.38 below the minimum 0.388"
        )
    );
}

#[test]
fn a_tighter_fresh_minimum_is_the_one_simulated_against() {
    // The re-quote's minimum, 0.396, is above what the quoter now pays.
    let fresh = Quote {
        amount_out: "0.41".into(),
        min_amount_out: "0.396".into(),
        ..buy()
    };
    let finished = trade(buy(), fresh, U256::from(395_000_000_000_000_000u64));
    assert_eq!(
        finished.status.steps.last().unwrap().detail,
        "simulated 0.395 below the minimum 0.396"
    );
}

#[test]
fn a_price_that_moved_below_the_signed_minimum_stops_at_the_quote() {
    let fresh = Quote {
        amount_out: "0.38".into(),
        ..buy()
    };
    let finished = trade(buy(), fresh, U256::ZERO);
    assert_eq!(steps(&finished).len(), 1);
    assert_eq!(
        finished.status.steps[0].detail,
        "the price moved: 0.38 now, below the signed minimum 0.388"
    );
}

#[test]
fn a_signature_from_another_wallet_is_rejected() {
    let quote = buy();
    let wallet = signer();
    let other = LocalSigner::from_bytes(&B256::repeat_byte(0x22)).unwrap();
    let run = |signature| {
        let rpc = quoter(U256::from(395_000_000_000_000_000u64));
        det::run_simulated(async {
            let clock = SimClock::starting_at(START);
            shadow::run(
                request(&quote),
                || Ok(buy()),
                Signer::Wallet {
                    address: wallet.address(),
                    signature,
                },
                Simulation::Node(&rpc),
                &clock,
            )
            .await
        })
    };
    // The intent's terms, signed by another key.
    let rejected = run(Some(other.sign(signed_hash(wallet.address()))));
    assert_eq!(rejected.status.state(), State::Failed);
    assert_eq!(
        rejected.status.steps.last().unwrap().detail,
        "the signature isn't the account wallet's"
    );
    let missing = run(None);
    assert_eq!(missing.status.state(), State::Failed);
    let accepted = run(Some(wallet.sign(signed_hash(wallet.address()))));
    assert_eq!(accepted.status.state(), State::Simulated);
}

/// The hash a wallet at `signer` signs for `buy()`.
fn signed_hash(signer: Address) -> B256 {
    intent::hash(
        &Intent {
            signer,
            tokenIn: USDC.parse().unwrap(),
            maxAmountIn: U256::from(1_000_000_000u64),
            tokenOut: WETH.parse().unwrap(),
            minAmountOut: U256::from(388_000_000_000_000_000u64),
            deadline: U256::from(1_790_000_064u64),
            submitters: vec![intent::DEMO_EXECUTOR],
            quoteId: B256::right_padding_from(&[0xab; 16]),
        },
        8453,
    )
}

#[test]
fn a_recorded_trade_replays_to_the_same_outcome() {
    let sink = InMemorySink::default();
    let recorder = Recorder::new(
        Box::new(sink.clone()),
        Box::new(SimClock::starting_at(START)),
    );
    let local = signer();
    let quote = buy();
    let recorded = det::run_simulated(async {
        let clock = RecordingClock::new(Box::new(SimClock::starting_at(START)), recorder.clone());
        let rpc = RecordingRpc::new(
            Box::new(quoter(U256::from(395_000_000_000_000_000u64))),
            recorder.clone(),
        );
        shadow::run(
            request(&quote),
            || Ok(buy()),
            Signer::Guest(&local),
            Simulation::Node(&rpc),
            &clock,
        )
        .await
    });
    let replay = Replay::new(sink.records());
    let clock = replay.clock();
    let rpc = replay.rpc::<EthCall, CallResult>();
    let replayed = replay.run(shadow::run(
        request(&quote),
        || Ok(buy()),
        Signer::Guest(&local),
        Simulation::Node(&rpc),
        &clock,
    ));
    assert_eq!(replayed, recorded);
}

#[test]
fn the_intent_hash_is_the_quotes_terms_signed_by_its_signer() {
    let signer = signer().address();
    let expected = Intent {
        signer,
        tokenIn: USDC.parse().unwrap(),
        maxAmountIn: U256::from(1_000_000_000u64),
        tokenOut: WETH.parse().unwrap(),
        minAmountOut: U256::from(388_000_000_000_000_000u64),
        deadline: U256::from(1_790_000_064u64),
        submitters: vec![intent::DEMO_EXECUTOR],
        quoteId: B256::right_padding_from(&[0xab; 16]),
    };
    assert_eq!(
        shadow::intent_hash(&buy(), signer),
        Some(intent::hash(&expected, 8453))
    );
    // Another signer, or another quote, is another intent.
    assert_ne!(
        shadow::intent_hash(&buy(), Address::repeat_byte(9)),
        shadow::intent_hash(&buy(), signer)
    );
    assert_ne!(
        shadow::intent_hash(&sell(), signer),
        shadow::intent_hash(&buy(), signer)
    );
}

#[test]
fn an_in_memory_sell_takes_no_second_fee() {
    // In memory the fresh quote stands in for the simulation, and its output is already net
    // of our fee.
    let local = signer();
    let finished = det::run_simulated(async {
        let clock = SimClock::starting_at(START);
        shadow::run(
            request(&sell()),
            || Ok(sell()),
            Signer::Guest(&local),
            Simulation::InMemory,
            &clock,
        )
        .await
    });
    let receipt = finished.receipt.unwrap();
    assert_eq!(receipt.amount_out, "2475");
    assert!(receipt.simulated_in_memory);
    assert_eq!(finished.status.steps[3].detail, "simulated in memory");
}

#[test]
fn a_simulation_exactly_at_the_minimum_fills() {
    let finished = trade(buy(), buy(), U256::from(388_000_000_000_000_000u64));
    assert_eq!(finished.status.state(), State::Simulated);
}

#[test]
fn a_fresh_quote_exactly_at_the_signed_minimum_goes_on() {
    let fresh = Quote {
        amount_out: "0.388".into(),
        ..buy()
    };
    let finished = trade(buy(), fresh, U256::from(390_000_000_000_000_000u64));
    assert_eq!(finished.status.steps[0].detail, "");
    assert_eq!(finished.status.state(), State::Simulated);
}
