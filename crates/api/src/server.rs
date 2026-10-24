//! The I/O around the pure core: a Kafka consumer task that feeds the engine's output into the feed,
//! and axum serving REST and the WebSocket stream from it (D45, D62).

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use axum::Router;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use det::{Clock, SystemClock};
use prost::Message as _;
use proto::api::v1::{DiscoveryList, ServerMessage};
use rdkafka::ClientConfig;
use rdkafka::consumer::{Consumer, StreamConsumer};
use rdkafka::error::KafkaError;
use rdkafka::message::Message as _;
use telemetry::metrics::{Metric, render};
use tokio::sync::broadcast;
use tower_http::cors::CorsLayer;
use tracing::Instrument;

use crate::discovery::{DiscoveryConfig, Filters};
use crate::feed::{Feed, Published};
use crate::stream::{Session, heartbeat, to_json};

/// How many ticks a connection may fall behind before it's dropped back to fresh snapshots,
/// rather than queueing without limit.
pub const CONNECTION_BACKLOG: usize = 256;

/// The engine's output topics the read models are built from.
pub const INPUT_TOPICS: [&str; 5] = [
    engine::POOL_UPDATES_TOPIC,
    engine::TRADES_TOPIC,
    engine::TRADE_HISTORY_TOPIC,
    engine::PRICES_TOPIC,
    engine::STATUS_TOPIC,
];

pub struct Config {
    pub listen: SocketAddr,
    /// Signs session tokens. Unset: `DEV_SESSION_SECRET`, with a warning.
    pub session_secret: Option<String>,
    /// A Privy app's ID and PEM verification key, for Privy logins.
    pub privy: Option<(String, String)>,
    /// Where demo accounts are kept between restarts. Unset: in memory only.
    pub postgres: Option<String>,
    /// Where a shadow trade's simulation `eth_call`s go. Unset, or unusable at start: trades
    /// simulate on the in-memory pool state.
    pub call_rpc: Option<String>,
    pub kafka: String,
    /// The Kafka consumer group. Offsets are never committed: every start rebuilds the read
    /// models from the start of the topic.
    pub group_id: String,
    /// The terminal's origin, for CORS.
    pub cors_origin: String,
    pub discovery: DiscoveryConfig,
}

#[derive(Debug)]
pub enum ServerError {
    Io(std::io::Error),
    Kafka(KafkaError),
    /// An input topic couldn't be created.
    Topic(det::kafka::InputLogError),
    BadCorsOrigin(String),
    /// A setting that doesn't work: a bad key, an unreachable store.
    Config(String),
}

/// What the consumer and the connections share.
pub struct Shared {
    // Only the consumer task writes the feed; connections lock it to read snapshots. A plain
    // mutex: it's held for one record or one snapshot, never across an await.
    feed: Mutex<Feed>,
    ticks: broadcast::Sender<Arc<Published>>,
    requests: AtomicU64,
    connections: AtomicU64,
    records: Mutex<BTreeMap<&'static str, Records>>,
    trades: Mutex<crate::Trades>,
    /// Signs guests' intents (D5): shadow only, on the placeholder domain.
    signer: execution::LocalSigner,
    /// The call worker a trade's simulation goes through, until it gives up on its endpoint.
    calls: Mutex<Option<tokio::sync::mpsc::UnboundedSender<chain_io::CallRequest>>>,
    /// Trades running at once.
    running: Arc<tokio::sync::Semaphore>,
    demo: crate::Demo,
}

/// Why a safety check didn't record a round trip.
#[derive(Debug)]
pub enum CheckError {
    /// No pool trades the token against WETH: nothing to check.
    NoPool,
    Failed(String),
}

/// A random address, from the OS's randomness: the I/O shell's, never a core's (rule 1).
fn random_address() -> types::chain::Address {
    use det::Rng as _;
    let mut rng = det::SeededRng::from_os();
    let mut bytes = [0u8; 20];
    for chunk in bytes.chunks_mut(8) {
        chunk.copy_from_slice(&rng.next_u64().to_le_bytes()[..chunk.len()]);
    }
    types::chain::Address::from(bytes)
}

/// What the safety probe spends on its round trip: 0.01 WETH.
const SAFETY_SPEND_WEI: u64 = 10_000_000_000_000_000;

/// The most trades running at once; another gets a 429.
const MAX_RUNNING_TRADES: usize = 8;
/// How long a trade may run. Past it the trade is abandoned, and the same intent can run again.
const TRADE_TIMEOUT: Duration = Duration::from_secs(20);

/// The guest signer's key: public, since a demo intent can't execute anywhere and the signer
/// holds nothing.
fn guest_key() -> alloy_primitives::B256 {
    alloy_primitives::keccak256(b"omnimarket demo guest signer")
}

/// What the consumer has done with one input topic's records.
#[derive(Default)]
struct Records {
    applied: u64,
    skipped: u64,
}

impl Shared {
    pub fn new() -> Arc<Self> {
        Self::with_discovery(DiscoveryConfig::default())
    }

    pub fn with_discovery(discovery: DiscoveryConfig) -> Arc<Self> {
        Self::with_calls(discovery, None)
    }

    /// With a call worker for trades' simulations.
    pub fn with_calls(
        discovery: DiscoveryConfig,
        calls: Option<tokio::sync::mpsc::UnboundedSender<chain_io::CallRequest>>,
    ) -> Arc<Self> {
        let sessions = crate::Sessions::new(DEV_SESSION_SECRET.as_bytes());
        Self::with_demo(discovery, calls, crate::Demo::new(sessions, None))
    }

    /// With demo accounts as `demo` keeps them.
    pub fn with_demo(
        discovery: DiscoveryConfig,
        calls: Option<tokio::sync::mpsc::UnboundedSender<chain_io::CallRequest>>,
        demo: crate::Demo,
    ) -> Arc<Self> {
        Arc::new(Self {
            feed: Mutex::new(Feed::new(discovery)),
            ticks: broadcast::channel(CONNECTION_BACKLOG).0,
            requests: AtomicU64::new(0),
            connections: AtomicU64::new(0),
            records: Mutex::new(BTreeMap::new()),
            trades: Mutex::new(crate::Trades::default()),
            signer: execution::LocalSigner::from_bytes(&guest_key())
                .expect("the guest key is a valid secp256k1 key"),
            calls: Mutex::new(calls),
            running: Arc::new(tokio::sync::Semaphore::new(MAX_RUNNING_TRADES)),
            demo,
        })
    }

    /// The account a request's bearer token is a session of.
    fn account_of(&self, headers: &axum::http::HeaderMap) -> Option<String> {
        self.demo.account_id(bearer(headers)?, now_ms())
    }

    /// The fill a shadow trade's receipt books: what it moved, valued by its quote-asset side
    /// at the display price.
    fn fill_of(&self, receipt: &proto::api::v1::TradeReceipt) -> crate::Fill {
        let token_in = receipt.token_in.clone().unwrap_or_default();
        let token_out = receipt.token_out.clone().unwrap_or_default();
        let amount_in = execution::intent::base_units(&receipt.amount_in, token_in.decimals)
            .unwrap_or_default();
        let amount_out = execution::intent::base_units(&receipt.amount_out, token_out.decimals)
            .unwrap_or_default();
        let address = |t: &proto::api::v1::TokenRef| {
            t.address
                .strip_prefix("0x")
                .and_then(|h| h.parse::<types::chain::Address>().ok())
        };
        let whole = |amount: &str| amount.parse::<f64>().unwrap_or(0.0);
        let quotes = crate::accounts::QUOTE_ASSETS;
        let usd = self.usd();
        let value_usd = match (address(&token_in), address(&token_out)) {
            (Some(a), _) if quotes.contains(&a) => {
                usd(a).map_or(0.0, |p| whole(&receipt.amount_in) * p)
            }
            (_, Some(b)) => usd(b).map_or(0.0, |p| whole(&receipt.amount_out) * p),
            _ => 0.0,
        };
        crate::Fill {
            trade_id: receipt.trade_id.clone(),
            token_in,
            amount_in,
            token_out,
            amount_out,
            value_usd,
            at_ms: receipt.steps.last().map_or(0, |s| s.finished_at_ms),
            receipt: Some(receipt.clone()),
        }
    }

    /// A token's display price in USD, for marking balances and positions.
    fn usd(&self) -> impl Fn(types::chain::Address) -> Option<f64> + '_ {
        move |token| self.feed().quotes().usd(token)
    }

    /// Simulations go in memory from now on: the call worker gave up on its endpoint.
    fn drop_calls(&self) {
        *self
            .calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    }

    fn trades(&self) -> MutexGuard<'_, crate::Trades> {
        self.trades
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Runs safety checks as they're queued, and every watched token's again once its report is
    /// `RECHECK_MS` old, against `url` (#89).
    async fn check_safety_forever(self: Arc<Self>, url: String) {
        let Ok(chain) = chain_io::HttpChain::new(&url) else {
            tracing::warn!(
                url,
                "the safety endpoint's URL doesn't parse: no safety checks"
            );
            return;
        };
        loop {
            self.feed().safety().sweep(now_ms());
            loop {
                let next = self.feed().safety().next_due();
                let Some(token) = next else { break };
                match self.check_safety(&chain, token).await {
                    Ok(()) => {}
                    Err(CheckError::NoPool) => {}
                    Err(CheckError::Failed(reason)) => {
                        tracing::warn!(reason, token = %token, "a safety check failed");
                        let report = crate::safety::failed(token, &reason, now_ms());
                        self.feed().safety().record(token, report);
                    }
                }
            }
            #[allow(clippy::disallowed_methods)] // the worker's pacing, outside any core
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }

    /// One round trip of `SAFETY_SPEND_WEI` of WETH through a pool trading `token` against WETH,
    /// at the latest block, recorded as the token's report. The probe sits at a fresh random
    /// address, called from another, so a token can't single the check out.
    pub async fn check_safety(
        &self,
        chain: &chain_io::HttpChain,
        token: types::chain::Address,
    ) -> Result<(), CheckError> {
        let (pool, v3, decimals) = {
            let mut feed = self.feed();
            let quotes = feed.quotes();
            let (pool, v3) = quotes.weth_pool(token).ok_or(CheckError::NoPool)?;
            (pool, v3, quotes.token_decimals(token).unwrap_or(18))
        };
        let failed = |e: String| CheckError::Failed(e);
        let block = chain_io::ChainReader::latest_number(chain)
            .await
            .map_err(|e| failed(e.to_string()))?;
        let (probe, from) = (random_address(), random_address());
        let spent = alloy_primitives::U256::from(SAFETY_SPEND_WEI);
        let call = types::chain::EthCall {
            to: probe,
            data: execution::safety::calldata(crate::accounts::WETH, token, pool, v3, spent),
            block,
        };
        let answer = chain
            .call_with_code(
                &call,
                from,
                probe,
                execution::safety::probe_code(),
                execution::safety::probe_balance(),
            )
            .await
            .map_err(|e| failed(e.to_string()))?;
        let types::chain::CallResult::Returned(bytes) = answer else {
            return Err(failed(format!("the probe reverted: {answer:?}")));
        };
        let trip = execution::safety::RoundTrip::decode(&bytes, spent)
            .ok_or_else(|| failed("the probe's answer doesn't decode".into()))?;
        let mut feed = self.feed();
        let weth = feed.quotes().token_ref(8453, crate::accounts::WETH);
        let report = crate::safety::report(token, &trip, pool, weth, decimals, block, now_ms());
        feed.safety().record(token, report);
        Ok(())
    }

    /// Runs a prepared trade to its end, on this thread: blocking, so call it off the server's
    /// runtime. The core of it is single-threaded (rule 6) on its own current-thread runtime.
    /// `None` if it didn't finish within `TRADE_TIMEOUT`: it's abandoned.
    fn run_trade(
        &self,
        run: Box<crate::trades::Run>,
        account_id: &str,
        wallet: Option<(types::chain::Address, Option<alloy_primitives::Signature>)>,
    ) -> Option<execution::Finished> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a current-thread runtime builds");
        let calls = self
            .calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let rpc = calls.map(det::ChannelRpc::new);
        let simulation = match &rpc {
            Some(rpc) => execution::Simulation::Node(rpc),
            None => execution::Simulation::InMemory,
        };
        let request = execution::Request {
            trade_id: run.trade_id.clone(),
            client_request_id: run.client_request_id.clone(),
            account_id: account_id.into(),
            quote: &run.quote,
        };
        let quote_request = run.quote_request.clone();
        let trade = execution::shadow::run(
            request,
            || {
                self.feed()
                    .quotes()
                    .price(&quote_request)
                    .map_err(|e| e.to_string())
            },
            match wallet {
                Some((address, signature)) => execution::Signer::Wallet { address, signature },
                None => execution::Signer::Guest(&self.signer),
            },
            simulation,
            &SystemClock,
        );
        // The I/O shell's bound on a trade, outside its core: a call endpoint that takes the
        // calls and never answers must not hold a trade, or its intent, for ever.
        #[allow(clippy::disallowed_methods)]
        let finished = runtime.block_on(tokio::time::timeout(TRADE_TIMEOUT, trade));
        match finished {
            Ok(finished) => {
                self.trades().finish(&run, finished.clone());
                Some(finished)
            }
            Err(_) => {
                self.trades().abandon(&run);
                None
            }
        }
    }

    fn feed(&self) -> MutexGuard<'_, Feed> {
        // A panic while holding the lock leaves the model as the last whole record left it.
        self.feed
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Applies one encoded `prices.base` record and publishes its deltas.
    pub fn apply_price(&self, payload: &[u8]) {
        self.apply(engine::PRICES_TOPIC, payload);
    }

    /// Applies one encoded record from an input topic and publishes its deltas. Publishing
    /// happens under the lock, so a connection's snapshot always includes every delta it was
    /// sent.
    pub fn apply(&self, topic: &str, payload: &[u8]) {
        let mut feed = self.feed();
        let published = match topic {
            engine::PRICES_TOPIC => match proto::price::v1::PriceUpdate::decode(payload) {
                Ok(update) => feed.apply_price(&update).map_err(|e| format!("{e:?}")),
                Err(e) => Err(e.to_string()),
            },
            engine::TRADES_TOPIC => proto::trade::v1::Trade::decode(payload)
                .map(|trade| feed.apply_trade(&trade))
                .map_err(|e| e.to_string()),
            engine::TRADE_HISTORY_TOPIC => match proto::trade::v1::Trade::decode(payload) {
                Ok(trade) => feed
                    .apply_history(&trade)
                    .map(|()| Vec::new())
                    .map_err(|e| format!("{e:?}")),
                Err(e) => Err(e.to_string()),
            },
            engine::POOL_UPDATES_TOPIC => proto::pool::v1::PoolUpdate::decode(payload)
                .map(|update| feed.apply_pool_update(&update))
                .map_err(|e| e.to_string()),
            engine::STATUS_TOPIC => match proto::status::v1::EngineStatus::decode(payload) {
                Ok(status) => feed.apply_status(&status).map_err(|e| format!("{e:?}")),
                Err(e) => Err(e.to_string()),
            },
            _ => Ok(Vec::new()),
        };
        let mut records = self
            .records
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let counts = records.entry(topic_name(topic)).or_default();
        match published {
            Ok(published) => {
                counts.applied += 1;
                for p in published {
                    // No receivers is fine: nobody is connected.
                    let _ = self.ticks.send(Arc::new(p));
                }
            }
            Err(error) => {
                counts.skipped += 1;
                tracing::warn!(topic, error, "skipped a record");
            }
        }
    }

    /// The text a scrape of `/metrics` returns.
    pub fn metrics(&self) -> String {
        let mut metrics = vec![
            Metric::counter("omnimarket_api_requests_total", "HTTP requests served.")
                .value(self.requests.load(Ordering::Relaxed)),
            Metric::gauge(
                "omnimarket_api_connections",
                "Open WebSocket stream connections.",
            )
            .value(self.connections.load(Ordering::Relaxed)),
        ];
        // Copied out before the records lock is taken: `apply` takes the feed's, then this one.
        let engine = self
            .feed()
            .status()
            .map(|s| (s.head_block_number, s.lag_ms));
        let records = self
            .records
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut applied = Metric::counter(
            "omnimarket_api_records_applied_total",
            "Records applied to the read models, by topic.",
        );
        let mut skipped = Metric::counter(
            "omnimarket_api_records_skipped_total",
            "Records that didn't decode or fit the contract, by topic.",
        );
        for (topic, counts) in records.iter() {
            applied = applied.labelled(&[("topic", topic)], counts.applied);
            skipped = skipped.labelled(&[("topic", topic)], counts.skipped);
        }
        metrics.extend([applied, skipped]);
        if let Some((head_block, lag_ms)) = engine {
            metrics.extend([
                Metric::gauge(
                    "omnimarket_api_engine_head_block",
                    "The engine's head block in the latest status the API holds.",
                )
                .value(head_block),
                Metric::gauge(
                    "omnimarket_api_engine_lag_ms",
                    "The engine's lag in the latest status the API holds.",
                )
                .value(lag_ms),
            ]);
        }
        render(&metrics)
    }
}

/// The input topic's static name, so a label can't grow with what a consumer is handed.
fn topic_name(topic: &str) -> &'static str {
    INPUT_TOPICS
        .into_iter()
        .find(|name| *name == topic)
        .unwrap_or("other")
}

/// The routes: `/health`, `/metrics`, `GET /v1/status`, `GET /v1/tokens/{chain_id}/{address}`,
/// `GET /v1/discovery`, `GET /v1/candles` and the stream at `/v1/stream`.
pub fn router(shared: Arc<Shared>, cors_origin: &str) -> Result<Router, ServerError> {
    let origin = HeaderValue::from_str(cors_origin)
        .map_err(|_| ServerError::BadCorsOrigin(cors_origin.to_string()))?;
    let cors = CorsLayer::new()
        .allow_origin(origin)
        .allow_methods([Method::GET])
        .allow_headers([header::CONTENT_TYPE]);
    Ok(Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/v1/status", get(status))
        .route("/v1/tokens/{chain_id}/{address}", get(token))
        .route("/v1/discovery", get(discovery))
        .route("/v1/candles", get(candles))
        .route("/v1/quotes", post(quote))
        .route("/v1/trades", post(trade))
        .route("/v1/session", post(open_session))
        .route("/v1/account", get(account))
        .route("/v1/positions", get(positions))
        .route("/v1/history", get(history))
        .route("/v1/trades/{trade_id}/receipt", get(receipt))
        .route("/v1/stream", get(stream))
        .layer(middleware::from_fn_with_state(
            shared.clone(),
            count_requests,
        ))
        // Added after the request counter, so Prometheus's scrapes don't count as requests.
        .route("/metrics", get(metrics))
        .layer(cors)
        .with_state(shared))
}

/// Runs the consumer and the server until either stops.
pub async fn serve(config: Config) -> Result<(), ServerError> {
    let calls = match config.call_rpc.clone() {
        Some(url) => {
            // The check before a run blocks on its own runtime (D88).
            let spawned = tokio::task::spawn_blocking(move || chain_io::spawn_call_worker(&url, 5))
                .await
                .expect("the call worker's spawn doesn't panic");
            match spawned {
                Ok(worker) => {
                    let requests = worker.requests;
                    let gave_up = worker.gave_up;
                    Some((requests, gave_up))
                }
                Err(error) => {
                    tracing::warn!(%error, "the call endpoint is unusable: trades simulate in memory");
                    None
                }
            }
        }
        None => None,
    };
    let (calls, gave_up) = match calls {
        Some((requests, gave_up)) => (Some(requests), Some(gave_up)),
        None => (None, None),
    };
    let secret = match (&config.session_secret, config.listen.ip().is_loopback()) {
        (Some(secret), _) => secret.clone(),
        (None, true) => {
            tracing::warn!("no session secret set: using the development one, on loopback only");
            DEV_SESSION_SECRET.to_string()
        }
        (None, false) => {
            return Err(ServerError::Config(
                "a server listening beyond loopback needs --session-secret".into(),
            ));
        }
    };
    let privy = match &config.privy {
        Some((app_id, key)) => Some(
            crate::Privy::new(app_id, key)
                .map_err(|e| ServerError::Config(format!("the Privy verification key: {e:?}")))?,
        ),
        None => None,
    };
    let demo = crate::Demo::new(crate::Sessions::new(secret.as_bytes()), privy);
    let demo = match &config.postgres {
        Some(url) => {
            let store = crate::Store::connect(url)
                .await
                .map_err(|e| ServerError::Config(format!("postgres: {e}")))?;
            demo.with_store(store)
                .await
                .map_err(|e| ServerError::Config(format!("loading the demo accounts: {e}")))?
        }
        None => demo,
    };
    let shared = Shared::with_demo(config.discovery, calls, demo);
    if let Some(gave_up) = gave_up {
        let watcher = shared.clone();
        // The worker holds every call it can't answer once it gives up (D88): stop sending it any.
        #[allow(clippy::disallowed_methods)]
        tokio::spawn(async move {
            if let Ok(error) = gave_up.await {
                tracing::warn!(%error, "the call endpoint gave up: trades simulate in memory");
                watcher.drop_calls();
            }
        });
    }
    let app = router(shared.clone(), &config.cors_origin)?;
    // The engine may not have started yet: create the topics so the consumer waits on them.
    for topic in INPUT_TOPICS {
        det::kafka::ensure_topic(&config.kafka, topic).map_err(ServerError::Topic)?;
    }
    let consumer: StreamConsumer = ClientConfig::new()
        .set("bootstrap.servers", &config.kafka)
        .set("group.id", &config.group_id)
        .set("enable.auto.commit", "false")
        .set("auto.offset.reset", "earliest")
        .create()
        .map_err(ServerError::Kafka)?;
    consumer
        .subscribe(&INPUT_TOPICS)
        .map_err(ServerError::Kafka)?;
    let listener = tokio::net::TcpListener::bind(config.listen)
        .await
        .map_err(ServerError::Io)?;
    tracing::info!(listen = %config.listen, "serving");
    match config.call_rpc.clone() {
        Some(url) => {
            // Chain I/O on its own thread and runtime, as chain-io's workers run (rule 6).
            let checker = shared.clone();
            #[allow(clippy::disallowed_methods)] // the safety worker: I/O around the read models
            std::thread::spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("a current-thread runtime builds");
                runtime.block_on(checker.check_safety_forever(url));
            });
        }
        None => tracing::warn!("no --call-rpc: no token safety checks"),
    }
    tokio::select! {
        biased;
        result = consume(&consumer, &shared) => result,
        result = axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()) => {
            result.map_err(ServerError::Io)
        }
    }
}

async fn consume(consumer: &StreamConsumer, shared: &Shared) -> Result<(), ServerError> {
    loop {
        let message = consumer.recv().await.map_err(ServerError::Kafka)?;
        if let Some(payload) = message.payload() {
            shared.apply(message.topic(), payload);
        }
    }
}

/// Counts requests, as a trace attribute on each request's span.
async fn count_requests(
    State(shared): State<Arc<Shared>>,
    request: Request,
    next: Next,
) -> Response {
    let requests = shared.requests.fetch_add(1, Ordering::Relaxed) + 1;
    let span = tracing::info_span!(
        "request",
        method = %request.method(),
        path = %request.uri().path(),
        requests,
    );
    next.run(request).instrument(span).await
}

async fn metrics(State(shared): State<Arc<Shared>>) -> Response {
    (
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        shared.metrics(),
    )
        .into_response()
}

/// The engine's latest status, or 503 before its first.
async fn status(State(shared): State<Arc<Shared>>) -> Response {
    match shared.feed().status() {
        Some(status) => (
            [(header::CONTENT_TYPE, "application/json")],
            serde_json::to_string(status).expect("generated messages always serialize"),
        )
            .into_response(),
        None => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

async fn token(
    State(shared): State<Arc<Shared>>,
    Path((chain_id, address)): Path<(u64, String)>,
) -> Response {
    let mut feed = shared.feed();
    let mut snapshot = feed
        .model()
        .token(&address.to_ascii_lowercase())
        .filter(|s| s.token.as_ref().is_some_and(|t| t.chain_id == chain_id))
        .cloned();
    // An open token page keeps its safety check fresh (#89).
    if let (Some(snapshot), Some(token)) = (
        snapshot.as_mut(),
        address
            .to_ascii_lowercase()
            .strip_prefix("0x")
            .and_then(|h| h.parse::<types::chain::Address>().ok()),
    ) {
        feed.safety().watch(token, now_ms());
        snapshot.safety = feed.safety().report(token).cloned();
    }
    match snapshot {
        Some(snapshot) => (
            [(header::CONTENT_TYPE, "application/json")],
            serde_json::to_string(&snapshot).expect("generated messages always serialize"),
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// `GET /v1/discovery?list=&min_depth_usd=&max_age_ms=`: the published feed, filtered. `list` is
/// `new` or `trending`; a parameter that doesn't parse is a 400.
async fn discovery(
    State(shared): State<Arc<Shared>>,
    Query(params): Query<BTreeMap<String, String>>,
) -> Response {
    let Some(filters) = parse_filters(&params) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let feed = {
        let mut model = shared.feed();
        let mut feed = model.discovery().feed(&filters);
        for row in &mut feed.rows {
            if let Some(token) = row.token.as_ref().and_then(|t| {
                t.address
                    .strip_prefix("0x")?
                    .parse::<types::chain::Address>()
                    .ok()
            }) {
                row.safety = model.safety().summary(token);
            }
        }
        feed
    };
    (
        [(header::CONTENT_TYPE, "application/json")],
        serde_json::to_string(&feed).expect("generated messages always serialize"),
    )
        .into_response()
}

/// The discovery filters from a query string; `None` when one doesn't parse.
pub fn parse_filters(params: &BTreeMap<String, String>) -> Option<Filters> {
    let mut filters = Filters::default();
    for (key, value) in params {
        if value.is_empty() {
            continue;
        }
        match key.as_str() {
            "list" => {
                filters.list = Some(match value.as_str() {
                    "new" => DiscoveryList::New,
                    "trending" => DiscoveryList::Trending,
                    _ => return None,
                })
            }
            "min_depth_usd" => filters.min_depth_usd = Some(value.parse().ok()?),
            "max_age_ms" => filters.max_age_ms = Some(value.parse().ok()?),
            _ => {}
        }
    }
    Some(filters)
}

/// `GET /v1/candles?chain_id=&token=&interval=&from_ms=&to_ms=`: the token's candles opening in
/// that range, oldest first; an empty series for a token without any. `interval` is a topic's
/// label (`1s` … `1d`); `from_ms` and `to_ms` are optional. A missing or unparsable parameter
/// is a 400.
async fn candles(
    State(shared): State<Arc<Shared>>,
    Query(params): Query<BTreeMap<String, String>>,
) -> Response {
    let Some(query) = parse_candles_query(&params) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let series = shared.feed().candles().series(
        Some(query.chain_id),
        query.token,
        query.interval,
        query.from_ms,
        query.to_ms,
    );
    (
        [(header::CONTENT_TYPE, "application/json")],
        serde_json::to_string(&series).expect("generated messages always serialize"),
    )
        .into_response()
}

/// What `GET /v1/candles` asks for.
#[derive(Debug, PartialEq)]
pub struct CandlesQuery {
    pub chain_id: u64,
    pub token: types::chain::Address,
    pub interval: proto::api::v1::CandleInterval,
    pub from_ms: u64,
    pub to_ms: u64,
}

/// The candles query from a query string; `None` when a parameter is missing or doesn't parse.
pub fn parse_candles_query(params: &BTreeMap<String, String>) -> Option<CandlesQuery> {
    let optional = |key: &str, default: u64| match params.get(key).filter(|v| !v.is_empty()) {
        Some(value) => value.parse().ok(),
        None => Some(default),
    };
    Some(CandlesQuery {
        chain_id: params.get("chain_id")?.parse().ok()?,
        // 0x and 40 hex digits; the parse refuses any other length.
        token: params.get("token")?.strip_prefix("0x")?.parse().ok()?,
        interval: crate::candles::interval_named(params.get("interval")?)?,
        from_ms: optional("from_ms", 0)?,
        to_ms: optional("to_ms", u64::MAX)?,
    })
}

/// The session secret when none is configured: fine on a laptop, never on a public server.
pub const DEV_SESSION_SECRET: &str = "omnimarket-dev-session-secret";

fn now_ms() -> u64 {
    SystemClock.now().unix_nanos / 1_000_000
}

/// A request's `Authorization: Bearer` token.
fn bearer(headers: &axum::http::HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}

fn json<T: serde::Serialize>(message: &T) -> Response {
    (
        [(header::CONTENT_TYPE, "application/json")],
        serde_json::to_string(message).expect("generated messages always serialize"),
    )
        .into_response()
}

/// `POST /v1/session`: a new guest account and its session, or with
/// `Authorization: Bearer <Privy identity token>`, the Privy user's account, created on their
/// first login and found again on the next. 401 for a Privy token that doesn't verify, 422
/// with no Privy app configured, 429 past `SESSIONS_PER_IP_HOUR` from one address.
async fn open_session(State(shared): State<Arc<Shared>>, request: Request) -> Response {
    let ip = request
        .extensions()
        .get::<axum::extract::ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip());
    let privy = bearer(request.headers()).map(str::to_string);
    let guest_id = format!("guest-{:016x}", det::SeededRng::from_os().seed());
    let opened = shared
        .demo
        .open(privy.as_deref(), guest_id, ip, now_ms())
        .await;
    match opened {
        Ok(opened) => {
            let account = shared.demo.account(&opened.account_id, shared.usd());
            json(&proto::api::v1::Session {
                lineage: account.as_ref().and_then(|a| a.lineage.clone()),
                session_token: opened.token,
                expires_at_ms: opened.expires_at_ms,
                account,
            })
        }
        Err(error) => {
            let code = match error {
                crate::DemoError::Privy(_) => StatusCode::UNAUTHORIZED,
                crate::DemoError::Limited(_) => StatusCode::TOO_MANY_REQUESTS,
                crate::DemoError::Store(_) => StatusCode::SERVICE_UNAVAILABLE,
                _ => StatusCode::UNPROCESSABLE_ENTITY,
            };
            (code, error.to_string()).into_response()
        }
    }
}

/// `GET /v1/account`: the session's account and its shadow balances. 401 without a session.
async fn account(State(shared): State<Arc<Shared>>, headers: axum::http::HeaderMap) -> Response {
    let Some(id) = shared.account_of(&headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    match shared.demo.account(&id, shared.usd()) {
        Some(account) => json(&account),
        None => StatusCode::UNAUTHORIZED.into_response(),
    }
}

/// `GET /v1/positions`: the session's positions, open first, marked to the display price.
async fn positions(State(shared): State<Arc<Shared>>, headers: axum::http::HeaderMap) -> Response {
    let Some(id) = shared.account_of(&headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let positions = shared.demo.positions(&id, shared.usd()).unwrap_or_default();
    json(&proto::api::v1::PositionList {
        lineage: None,
        positions,
    })
}

/// `GET /v1/history`: the session's finished trades' receipts, newest first.
async fn history(State(shared): State<Arc<Shared>>, headers: axum::http::HeaderMap) -> Response {
    let Some(id) = shared.account_of(&headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    json(&proto::api::v1::TradeHistory {
        lineage: None,
        receipts: shared.demo.history(&id).unwrap_or_default(),
    })
}

/// `POST /v1/quotes` with a `QuoteRequest`: a `Quote` on the latest pool state. A body or
/// field that doesn't parse is a 400; a pair that can't be quoted is a 422 with the reason.
/// With a Privy account's session, the quote carries the intent for its wallet to sign.
async fn quote(
    State(shared): State<Arc<Shared>>,
    headers: axum::http::HeaderMap,
    body: String,
) -> Response {
    let Ok(request) = serde_json::from_str::<proto::api::v1::QuoteRequest>(&body) else {
        return (StatusCode::BAD_REQUEST, "the body isn't a QuoteRequest").into_response();
    };
    let wallet = shared
        .account_of(&headers)
        .and_then(|id| shared.demo.wallet(&id));
    let quoted = shared.feed().quotes().quote(&request);
    match quoted {
        Ok(mut quote) => {
            if let Some(wallet) = wallet
                && let Some(intent) = execution::shadow::intent_for(&quote, wallet)
            {
                quote.intent_typed_data = execution::intent::typed_data(&intent, quote.chain_id);
            }
            json(&quote)
        }
        Err(error @ crate::QuoteError::Bad(_)) => {
            (StatusCode::BAD_REQUEST, error.to_string()).into_response()
        }
        Err(error) => (StatusCode::UNPROCESSABLE_ENTITY, error.to_string()).into_response(),
    }
}

/// `POST /v1/trades` with a `TradeRequest` and a session: runs the quote's trade in shadow
/// mode, books the fill to the account, and answers the trade's last `TradeStatus`. A guest's
/// intent is signed by the server; a Privy account's must carry its wallet's signature. The
/// same intent again gets the same status, never a second fill. 400 for a body that doesn't
/// parse, 401 without a session, 404 for an unknown or expired quote, 409 while the same trade
/// runs, 422 past the shadow balance or for a guest's request carrying a signature, 429 past
/// `TRADES_PER_ACCOUNT_HOUR` or `MAX_RUNNING_TRADES` at once, 504 past `TRADE_TIMEOUT`.
async fn trade(
    State(shared): State<Arc<Shared>>,
    headers: axum::http::HeaderMap,
    body: String,
) -> Response {
    let Ok(request) = serde_json::from_str::<proto::api::v1::TradeRequest>(&body) else {
        return (StatusCode::BAD_REQUEST, "the body isn't a TradeRequest").into_response();
    };
    let Some(account_id) = shared.account_of(&headers) else {
        return (StatusCode::UNAUTHORIZED, "open a session first").into_response();
    };
    let wallet = match shared.demo.wallet(&account_id) {
        Some(address) => Some((
            address,
            execution::intent::parse_signature(&request.intent_signature),
        )),
        None if !request.intent_signature.is_empty() => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                "a guest's intent is signed by the server: send no signature",
            )
                .into_response();
        }
        None => None,
    };
    let signer = wallet.map_or_else(|| shared.signer.address(), |(address, _)| address);
    let prepared = {
        let mut feed = shared.feed();
        shared
            .trades()
            .prepare(feed.quotes(), &request, &account_id, signer)
    };
    let run = match prepared {
        Ok(crate::trades::Prepared::Done(status)) => return json(&status),
        Ok(crate::trades::Prepared::Run(run)) => run,
        Err(error) => {
            let code = match error {
                crate::TradeError::Bad(_) => StatusCode::BAD_REQUEST,
                crate::TradeError::UnknownQuote => StatusCode::NOT_FOUND,
                crate::TradeError::Running => StatusCode::CONFLICT,
            };
            return (code, error.to_string()).into_response();
        }
    };
    let token_in = run.quote.token_in.clone().unwrap_or_default();
    let amount_in =
        execution::intent::base_units(&run.quote.amount_in, token_in.decimals).unwrap_or_default();
    if let Err(error) = shared
        .demo
        .admit(&account_id, &token_in, amount_in, now_ms())
    {
        shared.trades().abandon(&run);
        let code = match error {
            crate::DemoError::Limited(_) => StatusCode::TOO_MANY_REQUESTS,
            _ => StatusCode::UNPROCESSABLE_ENTITY,
        };
        return (code, error.to_string()).into_response();
    }
    let Ok(permit) = shared.running.clone().try_acquire_owned() else {
        shared.trades().abandon(&run);
        return (
            StatusCode::TOO_MANY_REQUESTS,
            "too many trades running: try again",
        )
            .into_response();
    };
    let runner = shared.clone();
    let id = account_id.clone();
    let ran = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        runner.run_trade(run, &id, wallet)
    })
    .await;
    let finished = match ran {
        Ok(Some(finished)) => finished,
        Ok(None) => {
            return (
                StatusCode::GATEWAY_TIMEOUT,
                "the trade timed out: try again",
            )
                .into_response();
        }
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    if let Some(receipt) = &finished.receipt {
        let fill = shared.fill_of(receipt);
        if let Err(error) = shared.demo.fill(&account_id, fill).await {
            tracing::warn!(%error, trade = %receipt.trade_id, "a simulated trade wasn't booked");
            return (StatusCode::UNPROCESSABLE_ENTITY, error.to_string()).into_response();
        }
    }
    json(&finished.status)
}

/// `GET /v1/trades/{trade_id}/receipt`: the session's trade that stopped before broadcast, or
/// 404, also for another account's. 401 without a session.
async fn receipt(
    State(shared): State<Arc<Shared>>,
    headers: axum::http::HeaderMap,
    Path(trade_id): Path<String>,
) -> Response {
    let Some(account_id) = shared.account_of(&headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let receipt = shared.trades().receipt(&trade_id, &account_id).cloned();
    match receipt {
        Some(receipt) => (
            [(header::CONTENT_TYPE, "application/json")],
            serde_json::to_string(&receipt).expect("generated messages always serialize"),
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn stream(ws: WebSocketUpgrade, State(shared): State<Arc<Shared>>) -> Response {
    ws.on_upgrade(move |socket| connection(socket, shared))
}

/// One connection: client frames, ticks and heartbeats, in that priority.
async fn connection(mut socket: WebSocket, shared: Arc<Shared>) {
    let connections = shared.connections.fetch_add(1, Ordering::Relaxed) + 1;
    let span = tracing::info_span!("connection", connections);
    async {
        tracing::info!("opened");
        // Subscribed before any snapshot is taken, so no tick between the two is missed.
        let mut ticks = shared.ticks.subscribe();
        let mut session = Session::default();
        let mut beat = heartbeat_every_second();
        loop {
            let out: Vec<ServerMessage> = tokio::select! {
                biased;
                frame = socket.recv() => match frame {
                    Some(Ok(Message::Text(text))) => session.on_text(&text, &shared.feed()),
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                    Some(Ok(_)) => Vec::new(),
                },
                tick = ticks.recv() => match tick {
                    Ok(published) => session
                        .on_published(&published, &shared.feed())
                        .into_iter()
                        .collect(),
                    // Too far behind: start again from fresh snapshots.
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        session.resync(&shared.feed())
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                },
                _ = beat.tick() => {
                    let now_ms = SystemClock.now().unix_nanos / 1_000_000;
                    vec![heartbeat(now_ms, shared.feed().model())]
                }
            };
            for message in out {
                if socket
                    .send(Message::Text(to_json(&message).into()))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        }
        tracing::info!("closed");
    }
    .instrument(span)
    .await;
    shared.connections.fetch_sub(1, Ordering::Relaxed);
}

/// The heartbeat's pacing. It's I/O at the edge, like the socket: its time never reaches a read
/// model, so it doesn't go through a recorded `det::Clock` wait.
#[allow(clippy::disallowed_methods)]
fn heartbeat_every_second() -> tokio::time::Interval {
    tokio::time::interval(Duration::from_secs(1))
}
