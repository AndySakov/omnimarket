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
use axum::routing::get;
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
pub const INPUT_TOPICS: [&str; 4] = [
    engine::POOL_UPDATES_TOPIC,
    engine::TRADES_TOPIC,
    engine::PRICES_TOPIC,
    engine::STATUS_TOPIC,
];

pub struct Config {
    pub listen: SocketAddr,
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
        Arc::new(Self {
            feed: Mutex::new(Feed::new(discovery)),
            ticks: broadcast::channel(CONNECTION_BACKLOG).0,
            requests: AtomicU64::new(0),
            connections: AtomicU64::new(0),
            records: Mutex::new(BTreeMap::new()),
        })
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
        if let Some(status) = self.feed().status() {
            metrics.extend([
                Metric::gauge(
                    "omnimarket_api_engine_head_block",
                    "The engine's head block in the latest status the API holds.",
                )
                .value(status.head_block_number),
                Metric::gauge(
                    "omnimarket_api_engine_lag_ms",
                    "The engine's lag in the latest status the API holds.",
                )
                .value(status.lag_ms),
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
/// `GET /v1/discovery` and the stream at `/v1/stream`.
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
    let shared = Shared::with_discovery(config.discovery);
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
