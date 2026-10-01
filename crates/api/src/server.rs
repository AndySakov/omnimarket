//! The I/O around the pure core: a Kafka consumer task that feeds `prices.base` into the feed,
//! and axum serving REST and the WebSocket stream from it (D45, D62).

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use axum::Router;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Request, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use det::{Clock, SystemClock};
use prost::Message as _;
use proto::api::v1::ServerMessage;
use rdkafka::ClientConfig;
use rdkafka::consumer::{Consumer, StreamConsumer};
use rdkafka::error::KafkaError;
use rdkafka::message::Message as _;
use tokio::sync::broadcast;
use tower_http::cors::CorsLayer;
use tracing::Instrument;

use crate::feed::{Feed, Published};
use crate::stream::{Session, heartbeat, to_json};

/// How many ticks a connection may fall behind before it's dropped back to fresh snapshots,
/// rather than queueing without limit.
pub const CONNECTION_BACKLOG: usize = 256;

pub struct Config {
    pub listen: SocketAddr,
    pub kafka: String,
    /// The Kafka consumer group. Offsets are never committed: every start rebuilds the read
    /// models from the start of the topic.
    pub group_id: String,
    /// The terminal's origin, for CORS.
    pub cors_origin: String,
}

#[derive(Debug)]
pub enum ServerError {
    Io(std::io::Error),
    Kafka(KafkaError),
    /// `prices.base` couldn't be created.
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
}

impl Shared {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            feed: Mutex::new(Feed::default()),
            ticks: broadcast::channel(CONNECTION_BACKLOG).0,
            requests: AtomicU64::new(0),
            connections: AtomicU64::new(0),
        })
    }

    fn feed(&self) -> MutexGuard<'_, Feed> {
        // A panic while holding the lock leaves the model as the last whole record left it.
        self.feed
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Applies one encoded `prices.base` record and publishes its ticks. Publishing happens
    /// under the lock, so a connection's snapshot always includes every tick it was sent.
    pub fn apply_price(&self, payload: &[u8]) {
        let update = match proto::price::v1::PriceUpdate::decode(payload) {
            Ok(update) => update,
            Err(e) => {
                tracing::warn!(error = %e, "skipped a price record that doesn't decode");
                return;
            }
        };
        let mut feed = self.feed();
        match feed.apply_price(&update) {
            Ok(published) => {
                for p in published {
                    // No receivers is fine: nobody is connected.
                    let _ = self.ticks.send(Arc::new(p));
                }
            }
            Err(e) => tracing::warn!(error = ?e, "skipped a price record"),
        }
    }
}

/// The routes: `/health`, `GET /v1/tokens/{chain_id}/{address}` and the stream at `/v1/stream`.
pub fn router(shared: Arc<Shared>, cors_origin: &str) -> Result<Router, ServerError> {
    let origin = HeaderValue::from_str(cors_origin)
        .map_err(|_| ServerError::BadCorsOrigin(cors_origin.to_string()))?;
    let cors = CorsLayer::new()
        .allow_origin(origin)
        .allow_methods([Method::GET])
        .allow_headers([header::CONTENT_TYPE]);
    Ok(Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/v1/tokens/{chain_id}/{address}", get(token))
        .route("/v1/stream", get(stream))
        .layer(middleware::from_fn_with_state(
            shared.clone(),
            count_requests,
        ))
        .layer(cors)
        .with_state(shared))
}

/// Runs the consumer and the server until either stops.
pub async fn serve(config: Config) -> Result<(), ServerError> {
    let shared = Shared::new();
    let app = router(shared.clone(), &config.cors_origin)?;
    // The engine may not have started yet: create the topic so the consumer waits on it.
    det::kafka::ensure_topic(&config.kafka, engine::PRICES_TOPIC).map_err(ServerError::Topic)?;
    let consumer: StreamConsumer = ClientConfig::new()
        .set("bootstrap.servers", &config.kafka)
        .set("group.id", &config.group_id)
        .set("enable.auto.commit", "false")
        .set("auto.offset.reset", "earliest")
        .create()
        .map_err(ServerError::Kafka)?;
    consumer
        .subscribe(&[engine::PRICES_TOPIC])
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
            shared.apply_price(payload);
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
                    Some(Ok(Message::Text(text))) => session.on_text(&text, shared.feed().model()),
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                    Some(Ok(_)) => Vec::new(),
                },
                tick = ticks.recv() => match tick {
                    Ok(published) => session
                        .on_published(&published, shared.feed().model())
                        .into_iter()
                        .collect(),
                    // Too far behind: start again from fresh snapshots.
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        session.resync(shared.feed().model())
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
