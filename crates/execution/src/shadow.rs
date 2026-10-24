//! A trade in shadow mode (D5): re-quote, build the intent, sign it, simulate it against live
//! state, and stop before broadcast. Time and the simulation's calls come through `det`, so a
//! recorded trade replays to the same receipt.

use alloy_primitives::{Address, B256, Bytes, Signature, U256, aliases::U24};
use alloy_sol_types::{SolCall, sol};
use det::{Clock, Rpc};
use proto::api::v1::trade_status::State;
use proto::api::v1::trade_step_timing::Outcome as StepOutcome;
use proto::api::v1::{
    ExecutionMode, Quote, RouteLeg, TradeReceipt, TradeStatus, TradeStep, TradeStepTiming,
};
use proto::lineage::v1::Lineage;
use types::LineageId;
use types::chain::{CallResult, EthCall};

use crate::intent::{self, DEMO_EXECUTOR, Intent, LocalSigner, base_units};

sol! {
    struct QuoteExactInputSingleParams {
        address tokenIn;
        address tokenOut;
        uint256 amountIn;
        uint24 fee;
        uint160 sqrtPriceLimitX96;
    }
    function quoteExactInputSingle(QuoteExactInputSingleParams params) external returns (uint256 amountOut, uint160 sqrtPriceX96After, uint32 initializedTicksCrossed, uint256 gasEstimate);
    function getAmountsOut(uint256 amountIn, address[] path) external view returns (uint256[] amounts);
}

/// Uniswap's QuoterV2 and UniswapV2Router02 on Base: a quote through them runs the swap
/// against the pools' state at the block asked for.
pub const QUOTER_V2: Address =
    alloy_primitives::address!("3d4e44Eb1374240CE5F1B871ab261CD16335B76a");
pub const V2_ROUTER: Address =
    alloy_primitives::address!("4752ba5DBc23f44D87826276BF6Fd6b1C372aD24");

/// How long an intent stays valid after its quote expires, s.
pub const DEADLINE_S: u64 = 60;

/// Who signs the intent (D57).
pub enum Signer<'a> {
    /// A guest: the server's local signer signs (D5).
    Guest(&'a LocalSigner),
    /// A Privy user: their signature, which must recover to their embedded wallet.
    Wallet {
        address: Address,
        signature: Option<Signature>,
    },
}

impl Signer<'_> {
    pub fn address(&self) -> Address {
        match self {
            Signer::Guest(local) => local.address(),
            Signer::Wallet { address, .. } => *address,
        }
    }
}

/// The hash of the intent `signer` signs for `quote`: the trade's natural key, known before
/// the trade runs, so a retry finds the trade it already made (build rule 7).
pub fn intent_hash(quote: &Quote, signer: Address) -> Option<B256> {
    let intent = Intent {
        signer,
        submitters: vec![DEMO_EXECUTOR],
        ..terms(quote)?
    };
    Some(intent::hash(&intent, quote.chain_id))
}

/// Where the simulation runs.
pub enum Simulation<'a> {
    /// `eth_call`s to Uniswap's quoter and v2 router at the fresh quote's block.
    Node(&'a dyn Rpc<Request = EthCall, Response = CallResult>),
    /// No node: the fresh quote itself, from the API's in-memory pool state.
    InMemory,
}

pub struct Request<'a> {
    pub trade_id: String,
    pub client_request_id: String,
    pub account_id: String,
    /// The quote the trade was asked for: its terms are what's signed.
    pub quote: &'a Quote,
}

/// A finished shadow trade: its last status, and the receipt if it stopped before broadcast.
#[derive(Clone, Debug, PartialEq)]
pub struct Finished {
    pub status: TradeStatus,
    pub receipt: Option<TradeReceipt>,
}

/// Runs one shadow trade. `requote` prices the quote's request again on the latest state.
pub async fn run(
    request: Request<'_>,
    requote: impl FnOnce() -> Result<Quote, String>,
    signer: Signer<'_>,
    simulation: Simulation<'_>,
    clock: &dyn Clock,
) -> Finished {
    let mut steps = Steps {
        clock,
        done: Vec::new(),
    };
    let quote = request.quote;

    // Quote: re-quote at send time (D27). Worse than the signed minimum: don't send.
    let started = steps.start();
    let fresh = match requote() {
        Ok(fresh) => fresh,
        Err(reason) => {
            steps.fail(TradeStep::Quote, started, &reason);
            return finish(&request, steps, None, None);
        }
    };
    let decimals_out = quote.token_out.as_ref().map_or(0, |t| t.decimals);
    let signed_min = base_units(&quote.min_amount_out, decimals_out).unwrap_or_default();
    let fresh_min = base_units(&fresh.min_amount_out, decimals_out).unwrap_or_default();
    if base_units(&fresh.amount_out, decimals_out).unwrap_or_default() < signed_min {
        let reason = format!(
            "the price moved: {} now, below the signed minimum {}",
            fresh.amount_out, quote.min_amount_out
        );
        steps.fail(TradeStep::Quote, started, &reason);
        return finish(&request, steps, None, None);
    }
    steps.succeed(TradeStep::Quote, started, "");

    // Build: D59's terms from the quote the user saw.
    let started = steps.start();
    let Some(terms) = terms(quote) else {
        let reason = "the quote's tokens or amounts don't parse".to_string();
        steps.fail(TradeStep::Build, started, &reason);
        return finish(&request, steps, None, None);
    };
    let intent = Intent {
        signer: signer.address(),
        submitters: vec![DEMO_EXECUTOR],
        ..terms
    };
    let hash = intent::hash(&intent, quote.chain_id);
    steps.succeed(TradeStep::Build, started, "");

    // Sign: the guest's by the local signer; a wallet's must recover to the wallet.
    let started = steps.start();
    match &signer {
        Signer::Guest(local) => {
            local.sign(hash);
        }
        Signer::Wallet { address, signature } => {
            let recovered = signature.as_ref().and_then(|s| intent::signer_of(hash, s));
            if recovered != Some(*address) {
                let reason = "the signature isn't the account wallet's".to_string();
                steps.fail(TradeStep::Sign, started, &reason);
                return finish(&request, steps, Some(hash), None);
            }
        }
    }
    steps.succeed(TradeStep::Sign, started, "");

    // Simulate: each leg in turn, the last leg's output against the tighter minimum (D59).
    let started = steps.start();
    let in_memory = matches!(simulation, Simulation::InMemory);
    let simulated = match simulation {
        Simulation::Node(rpc) => simulate(rpc, &fresh).await,
        Simulation::InMemory => base_units(&fresh.amount_out, decimals_out)
            .ok_or_else(|| "the fresh quote's output doesn't parse".to_string()),
    };
    let amount_out = match simulated {
        Ok(out) if in_memory => out,
        Ok(out) => net_of_fee(&fresh, out),
        Err(reason) => {
            steps.fail(TradeStep::Simulate, started, &reason);
            return finish(&request, steps, Some(hash), None);
        }
    };
    let min = signed_min.max(fresh_min);
    let detail = if in_memory { "simulated in memory" } else { "" };
    if amount_out < min {
        let reason = format!(
            "simulated {} below the minimum {}",
            scaled(amount_out, decimals_out),
            scaled(min, decimals_out)
        );
        steps.fail(TradeStep::Simulate, started, &reason);
        return finish(&request, steps, Some(hash), None);
    }
    steps.succeed(TradeStep::Simulate, started, detail);

    // Shadow mode stops here (D5).
    let started = steps.start();
    steps.succeed(TradeStep::StopBeforeBroadcast, started, "");

    let receipt = TradeReceipt {
        lineage: Some(Lineage {
            id: LineageId::from_natural_key("trade", hash.as_slice())
                .as_bytes()
                .to_vec(),
            caused_by: fresh
                .lineage
                .as_ref()
                .map(|l| l.id.clone())
                .into_iter()
                .collect(),
        }),
        trade_id: request.trade_id.clone(),
        account_id: request.account_id.clone(),
        mode: ExecutionMode::Shadow.into(),
        quote_id: quote.quote_id.clone(),
        intent_hash: hash.to_string(),
        demo_intent: true,
        chain_id: quote.chain_id,
        token_in: quote.token_in.clone(),
        token_out: quote.token_out.clone(),
        amount_in: quote.amount_in.clone(),
        amount_out: scaled(amount_out, decimals_out),
        signed_min_amount_out: quote.min_amount_out.clone(),
        min_amount_out: scaled(min, decimals_out),
        fee: fresh.fee.clone(),
        route: fresh.route.clone(),
        gas_estimate_native: fresh.gas_estimate_native.clone(),
        gas_estimate_usd: fresh.gas_estimate_usd.clone(),
        block_number: fresh.block_number,
        tx_hash: String::new(),
        simulated_in_memory: in_memory,
        steps: steps.done.clone(),
        total_ms: total_ms(&steps.done),
        firing_id: String::new(),
    };
    finish(&request, steps, Some(hash), Some(receipt))
}

/// The intent's terms from a quote, without signer or submitters.
fn terms(quote: &Quote) -> Option<Intent> {
    let token_in = quote.token_in.as_ref()?;
    let token_out = quote.token_out.as_ref()?;
    let address = |t: &str| t.strip_prefix("0x")?.parse::<Address>().ok();
    Some(Intent {
        signer: Address::ZERO,
        tokenIn: address(&token_in.address)?,
        maxAmountIn: base_units(&quote.amount_in, token_in.decimals)?,
        tokenOut: address(&token_out.address)?,
        minAmountOut: base_units(&quote.min_amount_out, token_out.decimals)?,
        deadline: U256::from(quote.expires_at_ms / 1000 + DEADLINE_S),
        submitters: Vec::new(),
        quoteId: quote
            .quote_id
            .strip_prefix("0x")
            .and_then(|h| h.parse::<alloy_primitives::FixedBytes<16>>().ok())
            .map_or(B256::ZERO, |id| B256::right_padding_from(id.as_slice())),
    })
}

/// Each leg through the quoter (v3) or the v2 router at the fresh quote's block, the output of
/// one leg the input of the next. Returns the last leg's output, before our fee on a sell.
async fn simulate(
    rpc: &dyn Rpc<Request = EthCall, Response = CallResult>,
    fresh: &Quote,
) -> Result<U256, String> {
    let Some(first) = fresh.route.first() else {
        return Err("the fresh quote has no route".into());
    };
    let decimals = first.token_in.as_ref().map_or(0, |t| t.decimals);
    let mut amount = base_units(&first.amount_in, decimals)
        .ok_or_else(|| "the route's input doesn't parse".to_string())?;
    for leg in &fresh.route {
        amount = simulate_leg(rpc, leg, amount, fresh.block_number).await?;
    }
    Ok(amount)
}

async fn simulate_leg(
    rpc: &dyn Rpc<Request = EthCall, Response = CallResult>,
    leg: &RouteLeg,
    amount: U256,
    block: u64,
) -> Result<U256, String> {
    let address = |t: Option<&proto::api::v1::TokenRef>| {
        t.and_then(|t| t.address.strip_prefix("0x")?.parse::<Address>().ok())
            .ok_or_else(|| format!("leg {} has a token that doesn't parse", leg.pool))
    };
    let (token_in, token_out) = (
        address(leg.token_in.as_ref())?,
        address(leg.token_out.as_ref())?,
    );
    let (to, data) = match leg.venue.as_str() {
        "uniswap-v3" => (
            QUOTER_V2,
            quoteExactInputSingleCall {
                params: QuoteExactInputSingleParams {
                    tokenIn: token_in,
                    tokenOut: token_out,
                    amountIn: amount,
                    fee: U24::from(leg.pool_fee_bps * 100),
                    sqrtPriceLimitX96: Default::default(),
                },
            }
            .abi_encode(),
        ),
        "uniswap-v2" => (
            V2_ROUTER,
            getAmountsOutCall {
                amountIn: amount,
                path: vec![token_in, token_out],
            }
            .abi_encode(),
        ),
        other => return Err(format!("no simulation for venue {other}")),
    };
    let answer = rpc
        .call(EthCall {
            to,
            data: Bytes::from(data),
            block,
        })
        .await;
    let returned = match answer {
        CallResult::Returned(bytes) => bytes,
        CallResult::Failed(error) => return Err(format!("the simulation reverted: {error}")),
    };
    let out = match leg.venue.as_str() {
        "uniswap-v3" => quoteExactInputSingleCall::abi_decode_returns(&returned)
            .map(|r| r.amountOut)
            .ok(),
        _ => getAmountsOutCall::abi_decode_returns(&returned)
            .ok()
            .and_then(|amounts| amounts.last().copied()),
    };
    out.ok_or_else(|| "the simulation's answer doesn't decode".into())
}

/// The route's simulated output after our fee (D28), when the fee comes from the proceeds. On
/// a buy it came from the input, before the route.
fn net_of_fee(fresh: &Quote, routed_out: U256) -> U256 {
    let fee = fresh.fee.as_ref();
    let fee_token = fee.and_then(|f| f.token.as_ref()).map(|t| &t.address);
    if fee_token.is_some() && fee_token == fresh.token_out.as_ref().map(|t| &t.address) {
        let bps = U256::from(fee.map_or(0, |f| f.fee_bps));
        routed_out - routed_out * bps / U256::from(10_000u64)
    } else {
        routed_out
    }
}

struct Steps<'a> {
    clock: &'a dyn Clock,
    done: Vec<TradeStepTiming>,
}

impl Steps<'_> {
    fn now_ms(&self) -> u64 {
        self.clock.now().unix_nanos / 1_000_000
    }

    fn start(&self) -> u64 {
        self.now_ms()
    }

    fn succeed(&mut self, step: TradeStep, started: u64, detail: &str) {
        self.record(step, started, StepOutcome::Succeeded, detail);
    }

    fn fail(&mut self, step: TradeStep, started: u64, detail: &str) {
        self.record(step, started, StepOutcome::Failed, detail);
    }

    fn record(&mut self, step: TradeStep, started: u64, outcome: StepOutcome, detail: &str) {
        let finished = self.now_ms();
        self.done.push(TradeStepTiming {
            step: step.into(),
            started_at_ms: started,
            finished_at_ms: finished,
            outcome: outcome.into(),
            detail: detail.to_string(),
        });
    }
}

fn total_ms(steps: &[TradeStepTiming]) -> u64 {
    match (steps.first(), steps.last()) {
        (Some(first), Some(last)) => last.finished_at_ms.saturating_sub(first.started_at_ms),
        _ => 0,
    }
}

fn finish(
    request: &Request<'_>,
    steps: Steps<'_>,
    hash: Option<B256>,
    receipt: Option<TradeReceipt>,
) -> Finished {
    let state = if receipt.is_some() {
        State::Simulated
    } else {
        State::Failed
    };
    let mut key = request.trade_id.as_bytes().to_vec();
    key.extend(hash.map(|h| h.0).unwrap_or_default());
    key.extend((steps.done.len() as u64).to_le_bytes());
    Finished {
        status: TradeStatus {
            lineage: Some(Lineage {
                id: LineageId::from_natural_key("trade_status", &key)
                    .as_bytes()
                    .to_vec(),
                caused_by: receipt
                    .as_ref()
                    .and_then(|r| r.lineage.as_ref())
                    .map(|l| l.id.clone())
                    .into_iter()
                    .collect(),
            }),
            trade_id: request.trade_id.clone(),
            client_request_id: request.client_request_id.clone(),
            account_id: request.account_id.clone(),
            mode: ExecutionMode::Shadow.into(),
            state: state.into(),
            steps: steps.done,
            firing_id: String::new(),
        },
        receipt,
    }
}

fn scaled(amount: U256, decimals: u32) -> String {
    let digits = amount.to_string();
    let decimals = decimals as usize;
    let padded = format!("{digits:0>width$}", width = decimals + 1);
    let (whole, fraction) = padded.split_at(padded.len() - decimals);
    let fraction = fraction.trim_end_matches('0');
    if fraction.is_empty() {
        whole.to_string()
    } else {
        format!("{whole}.{fraction}")
    }
}
