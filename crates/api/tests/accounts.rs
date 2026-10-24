//! Demo accounts (#85): a buy, a partial sell and a full sell, with the balances, average entry
//! and PnL they leave.

use alloy_primitives::U256;
use api::{AccountState, Fill, FillError, hex};
use proto::api::v1::TokenRef;
use proto::api::v1::account::Kind;
use types::chain::Address;

const MEME: Address = Address::repeat_byte(0x30);
const USDC: Address = alloy_primitives::address!("833589fCD6eDb6E08f4c7C32D4f71b54bdA02913");

fn token(address: Address, symbol: &str, decimals: u32) -> TokenRef {
    TokenRef {
        chain_id: 8453,
        address: hex(address.as_slice()),
        symbol: symbol.into(),
        decimals,
        ..TokenRef::default()
    }
}

fn units(whole: u64, decimals: u32) -> U256 {
    U256::from(whole) * U256::from(10u64).pow(U256::from(decimals))
}

/// `usdc` USDC for `meme` MEME, or back.
fn fill(id: &str, buy: bool, meme: u64, usdc: u64, at_ms: u64) -> Fill {
    let (m, u) = (token(MEME, "MEME", 18), token(USDC, "USDC", 6));
    let (token_in, amount_in, token_out, amount_out) = if buy {
        (u, units(usdc, 6), m, units(meme, 18))
    } else {
        (m, units(meme, 18), u, units(usdc, 6))
    };
    Fill {
        trade_id: id.into(),
        token_in,
        amount_in,
        token_out,
        amount_out,
        value_usd: usdc as f64,
        at_ms,
        receipt: None,
    }
}

/// (amount, average entry, cost basis, value, unrealised, unrealised %, realised, closed)
fn meme_position(
    account: &AccountState,
    mark: f64,
) -> (String, String, String, String, String, String, String, bool) {
    let p = account
        .positions(|t| (t == MEME).then_some(mark))
        .into_iter()
        .find(|p| p.token.as_ref().unwrap().symbol == "MEME")
        .unwrap();
    (
        p.amount,
        p.average_entry_price_usd,
        p.cost_basis_usd,
        p.value_usd,
        p.unrealized_pnl_usd,
        p.unrealized_pnl_pct,
        p.realized_pnl_usd,
        p.closed,
    )
}

fn usdc_balance(account: &AccountState) -> U256 {
    account.balance(USDC)
}

#[test]
fn a_buy_a_partial_sell_and_a_full_sell_leave_the_right_balances_entry_and_pnl() {
    let mut account = AccountState::new("guest-1".into(), Kind::Guest, None, 1_000);
    assert_eq!(usdc_balance(&account), units(1_000, 6));

    // 500 USDC for 50,000 MEME: $0.01 each.
    account
        .apply(fill("buy", true, 50_000, 500, 2_000))
        .unwrap();
    assert_eq!(usdc_balance(&account), units(500, 6));
    assert_eq!(
        meme_position(&account, 0.01),
        (
            "50000".into(),
            "0.01".into(),
            "500".into(),
            "500".into(),
            "0".into(),
            "0".into(),
            "0".into(),
            false
        )
    );

    // 20,000 of them for 300 USDC: 40% of the cost basis, $200, goes; $100 is realised.
    account
        .apply(fill("part", false, 20_000, 300, 3_000))
        .unwrap();
    assert_eq!(usdc_balance(&account), units(800, 6));
    // Marked at $0.02: the 30,000 left are worth $600 against $300 of cost.
    assert_eq!(
        meme_position(&account, 0.02),
        (
            "30000".into(),
            "0.01".into(),
            "300".into(),
            "600".into(),
            "300".into(),
            "100".into(),
            "100".into(),
            false
        )
    );

    // The rest for 450 USDC: $150 more realised, and the position closes.
    account
        .apply(fill("rest", false, 30_000, 450, 4_000))
        .unwrap();
    assert_eq!(usdc_balance(&account), units(1_250, 6));
    assert_eq!(account.balance(MEME), U256::ZERO);
    let closed = meme_position(&account, 0.02);
    assert_eq!(
        (
            closed.0.as_str(),
            closed.2.as_str(),
            closed.6.as_str(),
            closed.7
        ),
        ("0", "0", "250", true)
    );
    // The account lists what it holds: 1 WETH and 1,250 USDC.
    let balances: Vec<(String, String)> = account
        .account(|_| None)
        .balances
        .into_iter()
        .map(|b| (b.token.unwrap().symbol, b.amount))
        .collect();
    assert_eq!(
        balances,
        vec![("WETH".into(), "1".into()), ("USDC".into(), "1250".into())]
    );
}

#[test]
fn a_trade_past_the_shadow_balance_is_refused_and_changes_nothing() {
    let mut account = AccountState::new("guest-2".into(), Kind::Guest, None, 1_000);
    let before = account.clone();
    assert_eq!(
        account.apply(fill("big", true, 1, 2_000, 2_000)),
        Err(FillError::Insufficient {
            have: "1000 USDC".into(),
            need: "2000 USDC".into()
        })
    );
    assert_eq!(account, before);
}

#[test]
fn an_account_values_its_balances_and_dates_its_positions() {
    let mut account = AccountState::new("guest-3".into(), Kind::Guest, None, 1_000);
    // 1 WETH at $2,500 and 1,000 USDC at $1.
    let weth = alloy_primitives::address!("4200000000000000000000000000000000000006");
    let values: Vec<String> = account
        .account(|t| Some(if t == weth { 2_500.0 } else { 1.0 }))
        .balances
        .into_iter()
        .map(|b| b.value_usd)
        .collect();
    assert_eq!(values, vec!["2500".to_string(), "1000".to_string()]);
    let lineage = account.account(|_| None).lineage.unwrap();
    assert_eq!(
        lineage.id,
        types::LineageId::from_natural_key("account", b"guest-3")
            .as_bytes()
            .to_vec()
    );
    // A position opens at its first buy, and again at the first buy after it closed.
    account.apply(fill("b1", true, 1_000, 10, 2_000)).unwrap();
    account.apply(fill("b2", true, 1_000, 10, 3_000)).unwrap();
    let opened = |a: &AccountState| a.positions(|_| None)[0].opened_at_ms;
    assert_eq!(opened(&account), 2_000);
    account.apply(fill("s", false, 2_000, 20, 4_000)).unwrap();
    account.apply(fill("b3", true, 1_000, 10, 5_000)).unwrap();
    assert_eq!(opened(&account), 5_000);
}
