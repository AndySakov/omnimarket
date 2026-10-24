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
    /// The call worker a trade's simulation goes through, if there is one.
    calls: Option<tokio::sync::mpsc::UnboundedSender<chain_io::CallRequest>>,
}

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
        Arc::new(Self {
            feed: Mutex::new(Feed::new(discovery)),
            ticks: broadcast::channel(CONNECTION_BACKLOG).0,
            requests: AtomicU64::new(0),
            connections: AtomicU64::new(0),
            records: Mutex::new(BTreeMap::new()),
            trades: Mutex::new(crate::Trades::default()),
            signer: execution::LocalSigner::from_bytes(&guest_key())
                .expect("the guest key is a valid secp256k1 key"),
            calls,
        })
    }

    fn trades(&self) -> MutexGuard<'_, crate::Trades> {
        self.trades
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Runs a prepared trade to its end, on this thread: blocking, so call it off the server's
    /// runtime. The core of it is single-threaded (rule 6) on its own current-thread runtime.
    fn run_trade(&self, run: Box<crate::trades::Run>) -> execution::Finished {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a current-thread runtime builds");
        let rpc = self.calls.clone().map(det::ChannelRpc::new);
        let simulation = match &rpc {
            Some(rpc) => execution::Simulation::Node(rpc),
            None => execution::Simulation::InMemory,
        };
        let request = execution::Request {
            trade_id: run.trade_id.clone(),
            client_request_id: run.client_request_id.clone(),
            account_id: crate::GUEST_ACCOUNT.into(),
            quote: &run.quote,
        };
        let quote_request = run.quote_request.clone();
        let finished = runtime.block_on(execution::shadow::run(
            request,
            || {
                self.feed()
                    .quotes()
                    .price(&quote_request)
                    .map_err(|e| e.to_string())
            },
            execution::Signer::Guest(&self.signer),
            simulation,
            &SystemClock,
        ));
        self.trades().finish(run.hash, finished.clone());
        finished
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
                Ok(worker) => Some(worker.requests),
                Err(error) => {
                    tracing::warn!(%error, "the call endpoint is unusable: trades simulate in memory");
                    None
                }
            }
        }
        None => None,
    };
    let shared = Shared::with_calls(config.discovery, calls);
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
    tokio::select! {
        biased;
        result = consume(&consumer, &shared) => result,
        result = axum::serve(listener, app) => result.map_err(ServerError::Io),
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
    let feed = shared.feed();
    let snapshot = feed
        .model()
        .token(&address.to_ascii_lowercase())
        .filter(|s| s.token.as_ref().is_some_and(|t| t.chain_id == chain_id));
    match snapshot {
        Some(snapshot) => (
            [(header::CONTENT_TYPE, "application/json")],
            serde_json::to_string(snapshot).expect("generated messages always serialize"),
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
    let feed = shared.feed().discovery().feed(&filters);
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

/// `POST /v1/quotes` with a `QuoteRequest`: a `Quote` on the latest pool state. A body or
/// field that doesn't parse is a 400; a pair that can't be quoted is a 422 with the reason.
async fn quote(State(shared): State<Arc<Shared>>, body: String) -> Response {
    let Ok(request) = serde_json::from_str::<proto::api::v1::QuoteRequest>(&body) else {
        return (StatusCode::BAD_REQUEST, "the body isn't a QuoteRequest").into_response();
    };
    let quoted = shared.feed().quotes().quote(&request);
    match quoted {
        Ok(quote) => (
            [(header::CONTENT_TYPE, "application/json")],
            serde_json::to_string(&quote).expect("generated messages always serialize"),
        )
            .into_response(),
        Err(error @ crate::QuoteError::Bad(_)) => {
            (StatusCode::BAD_REQUEST, error.to_string()).into_response()
        }
        Err(error) => (StatusCode::UNPROCESSABLE_ENTITY, error.to_string()).into_response(),
    }
}

/// `POST /v1/trades` with a `TradeRequest`: runs the quote's trade in shadow mode and answers
/// its last `TradeStatus`. The same intent again gets the same status, never a second fill.
/// 400 for a body that doesn't parse, 404 for an unknown or expired quote, 409 while the same
/// trade runs, 422 for a wallet's signature (guests only until #85).
async fn trade(State(shared): State<Arc<Shared>>, body: String) -> Response {
    let Ok(request) = serde_json::from_str::<proto::api::v1::TradeRequest>(&body) else {
        return (StatusCode::BAD_REQUEST, "the body isn't a TradeRequest").into_response();
    };
    let prepared = {
        let mut feed = shared.feed();
        shared
            .trades()
            .prepare(feed.quotes(), &request, shared.signer.address())
    };
    let status = match prepared {
        Ok(crate::trades::Prepared::Done(status)) => status,
        Ok(crate::trades::Prepared::Run(run)) => {
            let runner = shared.clone();
            match tokio::task::spawn_blocking(move || runner.run_trade(run)).await {
                Ok(finished) => finished.status,
                Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            }
        }
        Err(error) => {
            let code = match error {
                crate::TradeError::Bad(_) => StatusCode::BAD_REQUEST,
                crate::TradeError::UnknownQuote => StatusCode::NOT_FOUND,
                crate::TradeError::Running => StatusCode::CONFLICT,
                crate::TradeError::NoWallets => StatusCode::UNPROCESSABLE_ENTITY,
            };
            return (code, error.to_string()).into_response();
        }
    };
    (
        [(header::CONTENT_TYPE, "application/json")],
        serde_json::to_string(&status).expect("generated messages always serialize"),
    )
        .into_response()
}

/// `GET /v1/trades/{trade_id}/receipt`: a trade that stopped before broadcast, or 404.
async fn receipt(State(shared): State<Arc<Shared>>, Path(trade_id): Path<String>) -> Response {
    let receipt = shared.trades().receipt(&trade_id).cloned();
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
