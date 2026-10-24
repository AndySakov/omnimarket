//! Demo accounts over the API (#85): a guest session to a funded balance in one call, shadow
//! trades booked to it, a Privy login that links to the same account on return, the demo's
//! guards, and accounts surviving a restart.

use std::sync::Arc;

use alloy_primitives::{B256, U256};
use api::server::{Shared, router};
use api::{Demo, DemoError, Privy, SESSIONS_PER_IP_HOUR, Sessions, Store, hex};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use p256::ecdsa::signature::Signer as _;
use p256::pkcs8::EncodePublicKey;
use prost::Message as _;
use proto::api::v1::trade_status::State;
use proto::api::v1::{
    Account, PositionList, Quote, QuoteRequest, Session, TradeHistory, TradeRequest, TradeStatus,
};
use proto::pool::v1::{PoolKey, PoolState, PoolUpdate, V2Reserves, pool_key, pool_state};
use proto::price::v1::{PriceUpdate, TokenMetadata};

const MEME: [u8; 20] = [0x30; 20];
const WETH: [u8; 20] = [
    0x42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x06,
];
const USDC: [u8; 20] = [
    0x83, 0x35, 0x89, 0xfc, 0xd6, 0xed, 0xb6, 0xe0, 0x8f, 0x4c, 0x7c, 0x32, 0xd4, 0xf7, 0x1b, 0x54,
    0xbd, 0xa0, 0x29, 0x13,
];
const APP: &str = "test-app";

fn be(n: u128) -> Vec<u8> {
    U256::from(n).to_be_bytes_trimmed_vec()
}

fn e(n: u32) -> u128 {
    10u128.pow(n)
}

fn pair(pool: [u8; 20], a: ([u8; 20], u128), b: ([u8; 20], u128)) -> PoolUpdate {
    let (first, second) = if a.0 < b.0 { (a, b) } else { (b, a) };
    PoolUpdate {
        chain_id: 8453,
        pool: pool.to_vec(),
        block_number: 100,
        after: Some(PoolState {
            state: Some(pool_state::State::V2(V2Reserves {
                reserve0: be(first.1),
                reserve1: be(second.1),
            })),
        }),
        key: Some(PoolKey {
            venue: pool_key::Venue::UniswapV2.into(),
            token0: first.0.to_vec(),
            token1: second.0.to_vec(),
            fee_pips: 3_000,
            tick_spacing: 0,
        }),
        ..PoolUpdate::default()
    }
}

fn price(token: [u8; 20], symbol: &str, usd: f64, timestamp: u64) -> PriceUpdate {
    PriceUpdate {
        chain_id: 8453,
        token: token.to_vec(),
        block_number: 100,
        // Within a quote's 4s of the wall clock, as live prices are.
        block_timestamp: timestamp,
        price_usd: usd,
        metadata: Some(TokenMetadata {
            symbol: Some(symbol.into()),
            decimals: Some(18),
            ..TokenMetadata::default()
        }),
        ..PriceUpdate::default()
    }
}

#[allow(clippy::disallowed_methods)] // the test's own wall clock, as the server's
fn now_s() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// A server with MEME at $0.01 in a deep MEME/WETH pair and WETH at $2,500.
async fn server(demo: Demo) -> (Arc<Shared>, String) {
    let shared = Shared::with_demo(api::DiscoveryConfig::default(), None, demo);
    let app = router(shared.clone(), "http://localhost:5173").unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap().to_string();
    #[allow(clippy::disallowed_methods)] // the test's own server task
    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap()
    });
    for update in [
        pair([0xb1; 20], (MEME, 100_000_000 * e(18)), (WETH, 400 * e(18))),
        pair(
            [0xc1; 20],
            (WETH, 40_000 * e(18)),
            (USDC, 100_000_000 * e(6)),
        ),
    ] {
        shared.apply(engine::POOL_UPDATES_TOPIC, &update.encode_to_vec());
    }
    let now = now_s();
    shared.apply(
        engine::PRICES_TOPIC,
        &price(WETH, "WETH", 2_500.0, now).encode_to_vec(),
    );
    shared.apply(
        engine::PRICES_TOPIC,
        &price(MEME, "MEME", 0.01, now).encode_to_vec(),
    );
    (shared, address)
}

async fn http(
    address: &str,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: &str,
) -> (u16, String) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    let auth = token.map_or(String::new(), |t| format!("Authorization: Bearer {t}\r\n"));
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\n{auth}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    let code = response[9..12].parse().unwrap();
    (
        code,
        response
            .split("\r\n\r\n")
            .nth(1)
            .unwrap_or_default()
            .to_string(),
    )
}

fn demo() -> Demo {
    Demo::new(Sessions::new(b"test-secret"), None)
}

/// Quotes `amount` of `from` into `to` and trades it, returning the trade's status.
async fn trade(
    address: &str,
    token: &str,
    id: &str,
    from: [u8; 20],
    to: [u8; 20],
    amount: &str,
) -> TradeStatus {
    let request = QuoteRequest {
        client_request_id: id.into(),
        chain_id: 8453,
        token_in: hex(&from),
        token_out: hex(&to),
        amount_in: amount.into(),
        slippage_bps: None,
    };
    let (code, body) = http(
        address,
        "POST",
        "/v1/quotes",
        Some(token),
        &serde_json::to_string(&request).unwrap(),
    )
    .await;
    assert_eq!(code, 200, "{body}");
    let quote: Quote = serde_json::from_str(&body).unwrap();
    let request = TradeRequest {
        client_request_id: id.into(),
        quote_id: quote.quote_id,
        intent_signature: String::new(),
    };
    let (code, body) = http(
        address,
        "POST",
        "/v1/trades",
        Some(token),
        &serde_json::to_string(&request).unwrap(),
    )
    .await;
    assert_eq!(code, 200, "{body}");
    serde_json::from_str(&body).unwrap()
}

fn balances(account: &Account) -> Vec<(String, String)> {
    account
        .balances
        .iter()
        .map(|b| (b.token.as_ref().unwrap().symbol.clone(), b.amount.clone()))
        .collect()
}

#[tokio::test]
async fn a_guest_session_funds_an_account_that_books_its_shadow_trades() {
    let (_shared, address) = server(demo()).await;
    let (code, body) = http(&address, "POST", "/v1/session", None, "").await;
    assert_eq!(code, 200, "{body}");
    let session: Session = serde_json::from_str(&body).unwrap();
    let account = session.account.unwrap();
    assert!(account.account_id.starts_with("guest-"));
    assert_eq!(
        balances(&account),
        vec![("WETH".into(), "1".into()), ("USDC".into(), "1000".into())]
    );
    let token = session.session_token;

    // 500 USDC into MEME, then half of it back, then the rest.
    let bought = trade(&address, &token, "buy", USDC, MEME, "500").await;
    assert_eq!(bought.state(), State::Simulated);
    let (_, body) = http(&address, "GET", "/v1/positions", Some(&token), "").await;
    let positions: PositionList = serde_json::from_str(&body).unwrap();
    let meme = &positions.positions[0];
    // 495 USDC routed after the fee: 49,179.01 MEME through WETH; the cost is the $500 spent.
    assert_eq!(
        (
            meme.amount.as_str(),
            meme.cost_basis_usd.as_str(),
            meme.closed
        ),
        ("49179.005029296297437105", "500", false)
    );
    let sold = trade(
        &address,
        &token,
        "half",
        MEME,
        USDC,
        "24589.502514648148718552",
    )
    .await;
    assert_eq!(sold.state(), State::Simulated);
    let rest = trade(
        &address,
        &token,
        "rest",
        MEME,
        USDC,
        "24589.502514648148718553",
    )
    .await;
    assert_eq!(rest.state(), State::Simulated);

    let (_, body) = http(&address, "GET", "/v1/positions", Some(&token), "").await;
    let positions: PositionList = serde_json::from_str(&body).unwrap();
    let meme = &positions.positions[0];
    assert_eq!(
        (
            meme.amount.as_str(),
            meme.cost_basis_usd.as_str(),
            meme.closed
        ),
        ("0", "0", true)
    );
    let realized: f64 = meme.realized_pnl_usd.parse().unwrap();
    assert!(
        (-25.0..-15.0).contains(&realized),
        "fees and price impact both ways: {realized}"
    );

    let (_, body) = http(&address, "GET", "/v1/account", Some(&token), "").await;
    let account: Account = serde_json::from_str(&body).unwrap();
    assert_eq!(balances(&account)[0], ("WETH".into(), "1".into()));
    let (_, body) = http(&address, "GET", "/v1/history", Some(&token), "").await;
    let history: TradeHistory = serde_json::from_str(&body).unwrap();
    assert_eq!(history.receipts.len(), 3);
    assert_eq!(history.receipts[2].trade_id, bought.trade_id);

    // Past the shadow balance: refused, and nothing booked.
    let request = QuoteRequest {
        client_request_id: "big".into(),
        chain_id: 8453,
        token_in: hex(&USDC),
        token_out: hex(&MEME),
        amount_in: "5000".into(),
        slippage_bps: None,
    };
    let (_, body) = http(
        &address,
        "POST",
        "/v1/quotes",
        Some(&token),
        &serde_json::to_string(&request).unwrap(),
    )
    .await;
    let quote: Quote = serde_json::from_str(&body).unwrap();
    let request = TradeRequest {
        client_request_id: "big".into(),
        quote_id: quote.quote_id,
        intent_signature: String::new(),
    };
    let (code, body) = http(
        &address,
        "POST",
        "/v1/trades",
        Some(&token),
        &serde_json::to_string(&request).unwrap(),
    )
    .await;
    assert_eq!(code, 422, "{body}");
    assert!(body.starts_with("the shadow balance holds"), "{body}");

    // No session, or a forged one: 401.
    assert_eq!(http(&address, "GET", "/v1/account", None, "").await.0, 401);
    assert_eq!(
        http(
            &address,
            "GET",
            "/v1/account",
            Some("v1.guest-x.9999999999999.0x00"),
            ""
        )
        .await
        .0,
        401
    );
}

/// A Privy identity token for `user`, with an embedded wallet, signed by `key`.
fn identity_token(key: &p256::ecdsa::SigningKey, user: &str, wallet: &str, exp: u64) -> String {
    let header = URL_SAFE_NO_PAD.encode(r#"{"alg":"ES256","typ":"JWT"}"#);
    let linked = format!(
        r#"[{{"type":"wallet","address":"{wallet}","chain_type":"ethereum","wallet_client_type":"privy"}}]"#
    );
    let claims = serde_json::json!({
        "iss": "privy.io",
        "aud": APP,
        "sub": user,
        "exp": exp,
        "linked_accounts": linked,
    });
    let claims = URL_SAFE_NO_PAD.encode(claims.to_string());
    let signing = format!("{header}.{claims}");
    let signature: p256::ecdsa::Signature = key.sign(signing.as_bytes());
    format!("{signing}.{}", URL_SAFE_NO_PAD.encode(signature.to_bytes()))
}

#[tokio::test]
async fn a_privy_login_links_to_the_same_account_and_its_wallet_signs_its_trades() {
    let key = p256::ecdsa::SigningKey::from_slice(&[0x42; 32]).unwrap();
    let pem = key
        .verifying_key()
        .to_public_key_pem(Default::default())
        .unwrap();
    let wallet = execution::LocalSigner::from_bytes(&B256::repeat_byte(0x77)).unwrap();
    let wallet_hex = hex(wallet.address().as_slice());
    let privy = Privy::new(APP, &pem).unwrap();
    let (_shared, address) = server(Demo::new(Sessions::new(b"s"), Some(privy))).await;

    let login = identity_token(&key, "did:privy:abc", &wallet_hex, now_s() + 600);
    let (code, body) = http(&address, "POST", "/v1/session", Some(&login), "").await;
    assert_eq!(code, 200, "{body}");
    let first: Session = serde_json::from_str(&body).unwrap();
    let account = first.account.unwrap();
    assert_eq!(
        (account.account_id.as_str(), account.wallet_address.as_str()),
        ("privy-abc", wallet_hex.as_str())
    );
    // A return login finds the same account.
    let (_, body) = http(&address, "POST", "/v1/session", Some(&login), "").await;
    let second: Session = serde_json::from_str(&body).unwrap();
    assert_eq!(second.account.unwrap().account_id, "privy-abc");

    // The quote carries the intent for the wallet; its signature trades, another's doesn't.
    let token = first.session_token;
    let request = QuoteRequest {
        client_request_id: "p".into(),
        chain_id: 8453,
        token_in: hex(&USDC),
        token_out: hex(&MEME),
        amount_in: "100".into(),
        slippage_bps: None,
    };
    let (_, body) = http(
        &address,
        "POST",
        "/v1/quotes",
        Some(&token),
        &serde_json::to_string(&request).unwrap(),
    )
    .await;
    let quote: Quote = serde_json::from_str(&body).unwrap();
    let typed: serde_json::Value = serde_json::from_str(&quote.intent_typed_data).unwrap();
    assert_eq!(
        typed["message"]["signer"].as_str().unwrap().to_lowercase(),
        wallet_hex
    );
    let hash = execution::intent::hash(
        &execution::shadow::intent_for(&quote, wallet.address()).unwrap(),
        8453,
    );
    let sign = |signer: &execution::LocalSigner| {
        format!(
            "0x{}",
            alloy_primitives::hex::encode(signer.sign(hash).as_bytes())
        )
    };
    let other = execution::LocalSigner::from_bytes(&B256::repeat_byte(0x78)).unwrap();
    let forged = TradeRequest {
        client_request_id: "f".into(),
        quote_id: quote.quote_id.clone(),
        intent_signature: sign(&other),
    };
    let (_, body) = http(
        &address,
        "POST",
        "/v1/trades",
        Some(&token),
        &serde_json::to_string(&forged).unwrap(),
    )
    .await;
    let status: TradeStatus = serde_json::from_str(&body).unwrap();
    assert_eq!(status.state(), State::Failed);
    assert_eq!(
        status.steps.last().unwrap().detail,
        "the signature isn't the account wallet's"
    );
    // The wallet's own signature on the same intent then trades.
    let signed = TradeRequest {
        client_request_id: "s".into(),
        quote_id: quote.quote_id.clone(),
        intent_signature: sign(&wallet),
    };
    let (code, body) = http(
        &address,
        "POST",
        "/v1/trades",
        Some(&token),
        &serde_json::to_string(&signed).unwrap(),
    )
    .await;
    assert_eq!(code, 200, "{body}");
    let status: TradeStatus = serde_json::from_str(&body).unwrap();
    assert_eq!(status.state(), State::Simulated);
    assert_eq!(status.account_id, "privy-abc");

    // A forged identity token: 401.
    let mut bad = login.clone();
    bad.replace_range(bad.len() - 4.., "AAAA");
    assert_eq!(
        http(&address, "POST", "/v1/session", Some(&bad), "")
            .await
            .0,
        401
    );
}

#[tokio::test]
async fn one_address_opens_a_limited_number_of_sessions_an_hour() {
    let demo = demo();
    let ip = "203.0.113.7".parse().unwrap();
    for i in 0..SESSIONS_PER_IP_HOUR {
        demo.open(None, format!("guest-{i}"), Some(ip), 1_000)
            .await
            .unwrap();
    }
    assert!(matches!(
        demo.open(None, "guest-x".into(), Some(ip), 2_000).await,
        Err(DemoError::Limited(_))
    ));
    // Another address, or an hour later, is fine.
    demo.open(
        None,
        "guest-y".into(),
        Some("203.0.113.8".parse().unwrap()),
        2_000,
    )
    .await
    .unwrap();
    demo.open(None, "guest-z".into(), Some(ip), 1_000 + 3_600_000)
        .await
        .unwrap();
}

/// Needs Postgres: OMNIMARKET_TEST_POSTGRES=postgres://… (CI's service container).
#[tokio::test]
#[ignore = "needs Postgres: set OMNIMARKET_TEST_POSTGRES, then --ignored"]
async fn accounts_and_their_positions_survive_a_restart() {
    let url = std::env::var("OMNIMARKET_TEST_POSTGRES").expect("OMNIMARKET_TEST_POSTGRES");
    let store = Store::connect(&url).await.unwrap();
    let (shared, address) = server(demo().with_store(store).await.unwrap()).await;
    let (_, body) = http(&address, "POST", "/v1/session", None, "").await;
    let session: Session = serde_json::from_str(&body).unwrap();
    let token = session.session_token;
    let id = session.account.unwrap().account_id;
    trade(&address, &token, &format!("{id}-buy"), USDC, MEME, "100").await;
    let (_, before) = http(&address, "GET", "/v1/positions", Some(&token), "").await;
    drop(shared);

    // A new server on the same store, as after a restart: the session and the positions hold.
    let store = Store::connect(&url).await.unwrap();
    let (_shared, address) = server(demo().with_store(store).await.unwrap()).await;
    let (code, after) = http(&address, "GET", "/v1/positions", Some(&token), "").await;
    assert_eq!(code, 200, "{after}");
    let (before, after): (PositionList, PositionList) = (
        serde_json::from_str(&before).unwrap(),
        serde_json::from_str(&after).unwrap(),
    );
    assert_eq!(after.positions[0].amount, before.positions[0].amount);
    assert_eq!(after.positions[0].cost_basis_usd, "100");
}

/// A signed Privy-style token with `claims`, by `key`.
fn signed(key: &p256::ecdsa::SigningKey, claims: serde_json::Value) -> String {
    let header = URL_SAFE_NO_PAD.encode(r#"{"alg":"ES256","typ":"JWT"}"#);
    let signing = format!("{header}.{}", URL_SAFE_NO_PAD.encode(claims.to_string()));
    let signature: p256::ecdsa::Signature = key.sign(signing.as_bytes());
    format!("{signing}.{}", URL_SAFE_NO_PAD.encode(signature.to_bytes()))
}

#[test]
fn a_privy_token_must_name_privy_this_app_and_a_time_still_to_come() {
    use api::PrivyError;
    let key = p256::ecdsa::SigningKey::from_slice(&[0x42; 32]).unwrap();
    let pem = key
        .verifying_key()
        .to_public_key_pem(Default::default())
        .unwrap();
    let privy = Privy::new(APP, &pem).unwrap();
    let now = 1_790_000_000u64;
    let claims = |iss: &str, aud: serde_json::Value, exp: u64| serde_json::json!({ "iss": iss, "aud": aud, "sub": "did:privy:abc", "exp": exp });
    let verify = |c| privy.verify(&signed(&key, c), now);
    assert!(verify(claims("privy.io", APP.into(), now + 1)).is_ok());
    assert!(
        verify(claims(
            "privy.io",
            serde_json::json!(["other", APP]),
            now + 1
        ))
        .is_ok()
    );
    // Signed by the app's key, but from another issuer, for another app, or expired.
    assert_eq!(
        verify(claims("evil.io", APP.into(), now + 1)),
        Err(PrivyError::Rejected)
    );
    assert_eq!(
        verify(claims("privy.io", "other-app".into(), now + 1)),
        Err(PrivyError::Rejected)
    );
    assert_eq!(
        verify(claims(
            "privy.io",
            serde_json::json!(["other-app"]),
            now + 1
        )),
        Err(PrivyError::Rejected)
    );
    assert_eq!(
        verify(claims("privy.io", APP.into(), now)),
        Err(PrivyError::Rejected)
    );
    assert_eq!(
        verify(claims("privy.io", APP.into(), now - 1)),
        Err(PrivyError::Rejected)
    );
}

#[test]
fn only_a_privy_embedded_wallet_becomes_the_accounts_wallet() {
    let key = p256::ecdsa::SigningKey::from_slice(&[0x42; 32]).unwrap();
    let pem = key
        .verifying_key()
        .to_public_key_pem(Default::default())
        .unwrap();
    let privy = Privy::new(APP, &pem).unwrap();
    let with = |linked: &str| {
        let claims = serde_json::json!({
            "iss": "privy.io", "aud": APP, "sub": "did:privy:abc", "exp": 2_000_000_000u64,
            "linked_accounts": linked,
        });
        privy
            .verify(&signed(&key, claims), 1_790_000_000)
            .unwrap()
            .wallet
    };
    let external = r#"[{"type":"wallet","address":"0x7777777777777777777777777777777777777777","wallet_client_type":"metamask"}]"#;
    assert_eq!(with(external), None);
    let email = r#"[{"type":"email","address":"a@b.c"}]"#;
    assert_eq!(with(email), None);
    let embedded = r#"[{"type":"wallet","address":"0x7777777777777777777777777777777777777777","wallet_client_type":"privy"}]"#;
    assert_eq!(
        with(embedded),
        Some(types::chain::Address::repeat_byte(0x77))
    );
}

#[tokio::test]
async fn an_account_makes_a_limited_number_of_trades_an_hour() {
    let demo = demo();
    let opened = demo
        .open(None, "guest-cap".into(), None, 1_000)
        .await
        .unwrap();
    let usdc = proto::api::v1::TokenRef {
        chain_id: 8453,
        address: hex(&USDC),
        symbol: "USDC".into(),
        decimals: 6,
        ..Default::default()
    };
    for _ in 0..api::TRADES_PER_ACCOUNT_HOUR {
        demo.admit(&opened.account_id, &usdc, U256::from(1u64), 2_000)
            .unwrap();
    }
    assert!(matches!(
        demo.admit(&opened.account_id, &usdc, U256::from(1u64), 3_000),
        Err(DemoError::Limited(_))
    ));
    // An hour on, there's room again.
    demo.admit(
        &opened.account_id,
        &usdc,
        U256::from(1u64),
        2_000 + 3_600_000,
    )
    .unwrap();
}

#[tokio::test]
async fn a_session_lasts_a_week_from_now_and_one_address_is_limited() {
    let (_shared, address) = server(demo()).await;
    let (_, body) = http(&address, "POST", "/v1/session", None, "").await;
    let session: Session = serde_json::from_str(&body).unwrap();
    let week_ms = 7 * 24 * 3_600_000;
    let now_ms = now_s() * 1000;
    assert!(
        (now_ms + week_ms - 60_000..=now_ms + week_ms + 60_000).contains(&session.expires_at_ms),
        "{}",
        session.expires_at_ms
    );
    // The test client is one address: past the hourly limit, a 429.
    for _ in 1..SESSIONS_PER_IP_HOUR {
        assert_eq!(http(&address, "POST", "/v1/session", None, "").await.0, 200);
    }
    assert_eq!(http(&address, "POST", "/v1/session", None, "").await.0, 429);
}

#[tokio::test]
async fn trades_past_the_hourly_cap_get_a_429() {
    let (_shared, address) = server(demo()).await;
    let (_, body) = http(&address, "POST", "/v1/session", None, "").await;
    let token = serde_json::from_str::<Session>(&body)
        .unwrap()
        .session_token;
    // Each a different amount, so a different intent, not a repeat.
    let trade_of = |i: usize| async move {
        let request = QuoteRequest {
            client_request_id: format!("c{i}"),
            chain_id: 8453,
            token_in: hex(&USDC),
            token_out: hex(&MEME),
            amount_in: format!("1.{i:04}"),
            slippage_bps: None,
        };
        (i, request)
    };
    for i in 0..api::TRADES_PER_ACCOUNT_HOUR {
        let (_, request) = trade_of(i).await;
        let (_, body) = http(
            &address,
            "POST",
            "/v1/quotes",
            Some(&token),
            &serde_json::to_string(&request).unwrap(),
        )
        .await;
        let quote: Quote = serde_json::from_str(&body).unwrap();
        let trade = TradeRequest {
            client_request_id: format!("c{i}"),
            quote_id: quote.quote_id,
            intent_signature: String::new(),
        };
        let (code, body) = http(
            &address,
            "POST",
            "/v1/trades",
            Some(&token),
            &serde_json::to_string(&trade).unwrap(),
        )
        .await;
        assert_eq!(code, 200, "{body}");
    }
    let (_, request) = trade_of(9_999).await;
    let (_, body) = http(
        &address,
        "POST",
        "/v1/quotes",
        Some(&token),
        &serde_json::to_string(&request).unwrap(),
    )
    .await;
    let quote: Quote = serde_json::from_str(&body).unwrap();
    let trade = TradeRequest {
        client_request_id: "over".into(),
        quote_id: quote.quote_id,
        intent_signature: String::new(),
    };
    let (code, _) = http(
        &address,
        "POST",
        "/v1/trades",
        Some(&token),
        &serde_json::to_string(&trade).unwrap(),
    )
    .await;
    assert_eq!(code, 429);
}

#[tokio::test]
async fn a_fill_paid_or_paid_out_in_weth_is_valued_at_weths_price() {
    let (_shared, address) = server(demo()).await;
    let (_, body) = http(&address, "POST", "/v1/session", None, "").await;
    let token = serde_json::from_str::<Session>(&body)
        .unwrap()
        .session_token;
    // 0.1 WETH into MEME: a cost basis of $250 at WETH's $2,500.
    let bought = trade(&address, &token, "w", WETH, MEME, "0.1").await;
    assert_eq!(bought.state(), State::Simulated);
    let (_, body) = http(&address, "GET", "/v1/positions", Some(&token), "").await;
    let position = serde_json::from_str::<PositionList>(&body)
        .unwrap()
        .positions
        .remove(0);
    assert_eq!(position.cost_basis_usd, "250");
    // All of it back into WETH: what it realised is the WETH it fetched at $2,500, less $250.
    let sold = trade(&address, &token, "s", MEME, WETH, &position.amount).await;
    assert_eq!(sold.state(), State::Simulated);
    let (_, body) = http(&address, "GET", "/v1/history", Some(&token), "").await;
    let proceeds: f64 = serde_json::from_str::<TradeHistory>(&body)
        .unwrap()
        .receipts[0]
        .amount_out
        .parse()
        .unwrap();
    let (_, body) = http(&address, "GET", "/v1/positions", Some(&token), "").await;
    let realized: f64 = serde_json::from_str::<PositionList>(&body)
        .unwrap()
        .positions[0]
        .realized_pnl_usd
        .parse()
        .unwrap();
    assert!(
        (realized - (proceeds * 2_500.0 - 250.0)).abs() < 1e-6,
        "{realized} from {proceeds}"
    );
}

#[tokio::test]
async fn an_expired_privy_token_is_refused_at_the_time_of_the_login() {
    let key = p256::ecdsa::SigningKey::from_slice(&[0x42; 32]).unwrap();
    let pem = key
        .verifying_key()
        .to_public_key_pem(Default::default())
        .unwrap();
    let demo = Demo::new(Sessions::new(b"s"), Some(Privy::new(APP, &pem).unwrap()));
    let wallet = hex(&[0x77; 20]);
    let token = identity_token(&key, "did:privy:x", &wallet, 1_790_000_000);
    // Logging in a second after it expired: refused, as Unix seconds of the login's time.
    assert!(matches!(
        demo.open(Some(&token), "g".into(), None, 1_790_000_001_000)
            .await,
        Err(DemoError::Privy(_))
    ));
    assert!(
        demo.open(Some(&token), "g".into(), None, 1_789_999_999_000)
            .await
            .is_ok()
    );
}
