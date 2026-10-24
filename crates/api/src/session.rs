//! Sessions (#85): the API's bearer tokens, and Privy logins.
//!
//! A session token is `v1.<account id>.<expires at, ms>.<mac>`, the MAC keccak-256 over the
//! server's secret and the rest. Keccak isn't open to length extension, so a prefix MAC is
//! sound. A Privy login is its identity token (an ES256 JWT, verified against the app's
//! verification key), which names the user and their linked wallets.

use alloy_primitives::{B256, keccak256};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use p256::ecdsa::signature::Verifier;
use p256::ecdsa::{Signature, VerifyingKey};
use p256::pkcs8::DecodePublicKey;
use types::chain::Address;

use crate::model::hex;

/// A session lasts a week.
pub const SESSION_MS: u64 = 7 * 24 * 3_600_000;

pub struct Sessions {
    secret: B256,
}

impl Sessions {
    pub fn new(secret: &[u8]) -> Self {
        Self {
            secret: keccak256(secret),
        }
    }

    fn mac(&self, message: &str) -> String {
        let mut input = self.secret.to_vec();
        input.extend(message.as_bytes());
        hex(keccak256(input).as_slice())
    }

    /// A token for `account_id` that expires at `expires_at_ms`.
    pub fn issue(&self, account_id: &str, expires_at_ms: u64) -> String {
        let message = format!("v1.{account_id}.{expires_at_ms}");
        format!("{message}.{}", self.mac(&message))
    }

    /// The account a token is for, if its MAC holds and it hasn't expired by `now_ms`.
    pub fn verify(&self, token: &str, now_ms: u64) -> Option<String> {
        let (message, mac) = token.rsplit_once('.')?;
        let mut parts = message.split('.');
        let (Some("v1"), Some(account), Some(expires), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return None;
        };
        let expires: u64 = expires.parse().ok()?;
        (same(&self.mac(message), mac) && now_ms < expires).then(|| account.to_string())
    }
}

/// Compares in time that doesn't depend on where the strings differ, so a forger can't find a
/// MAC byte by byte.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |diff, (x, y)| diff | (x ^ y))
            == 0
}

/// A Privy app: its ID (the tokens' audience) and its ES256 verification key.
pub struct Privy {
    app_id: String,
    key: VerifyingKey,
}

/// What a verified Privy identity token says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrivyUser {
    /// Privy's user ID (`did:privy:…`): the account's key, so a return login finds it.
    pub user_id: String,
    /// The user's Privy embedded wallet, if they have one.
    pub wallet: Option<Address>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PrivyError {
    BadKey,
    Malformed,
    BadSignature,
    /// Wrong issuer or audience, or expired.
    Rejected,
}

impl Privy {
    /// `verification_key` is the app's PEM public key, from the Privy dashboard.
    pub fn new(app_id: &str, verification_key: &str) -> Result<Self, PrivyError> {
        let key =
            VerifyingKey::from_public_key_pem(verification_key).map_err(|_| PrivyError::BadKey)?;
        Ok(Self {
            app_id: app_id.to_string(),
            key,
        })
    }

    /// Verifies an identity token at `now_s` (Unix seconds).
    pub fn verify(&self, token: &str, now_s: u64) -> Result<PrivyUser, PrivyError> {
        let mut parts = token.split('.');
        let (Some(header), Some(claims), Some(signature), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(PrivyError::Malformed);
        };
        let decode = |part: &str| {
            URL_SAFE_NO_PAD
                .decode(part)
                .map_err(|_| PrivyError::Malformed)
        };
        let header: serde_json::Value =
            serde_json::from_slice(&decode(header)?).map_err(|_| PrivyError::Malformed)?;
        if header["alg"] != "ES256" {
            return Err(PrivyError::Malformed);
        }
        let signature =
            Signature::from_slice(&decode(signature)?).map_err(|_| PrivyError::Malformed)?;
        let (signed, _) = token.rsplit_once('.').ok_or(PrivyError::Malformed)?;
        self.key
            .verify(signed.as_bytes(), &signature)
            .map_err(|_| PrivyError::BadSignature)?;
        let claims: serde_json::Value =
            serde_json::from_slice(&decode(claims)?).map_err(|_| PrivyError::Malformed)?;
        let audience_ok = claims["aud"] == self.app_id.as_str()
            || claims["aud"]
                .as_array()
                .is_some_and(|a| a.iter().any(|v| v == self.app_id.as_str()));
        let fresh = claims["exp"].as_u64().is_some_and(|exp| now_s < exp);
        if claims["iss"] != "privy.io" || !audience_ok || !fresh {
            return Err(PrivyError::Rejected);
        }
        let user_id = claims["sub"]
            .as_str()
            .ok_or(PrivyError::Malformed)?
            .to_string();
        Ok(PrivyUser {
            user_id,
            wallet: embedded_wallet(&claims["linked_accounts"]),
        })
    }
}

/// The Privy embedded wallet among an identity token's linked accounts. Privy sends them as a
/// JSON string; a JSON array is read too.
fn embedded_wallet(linked: &serde_json::Value) -> Option<Address> {
    let parsed;
    let accounts = match linked {
        serde_json::Value::String(text) => {
            parsed = serde_json::from_str::<serde_json::Value>(text).ok()?;
            &parsed
        }
        other => other,
    };
    accounts.as_array()?.iter().find_map(|account| {
        let embedded = account["type"] == "wallet"
            && (account["wallet_client_type"] == "privy" || account["wci"] == "privy");
        let address = account["address"].as_str().or(account["adr"].as_str())?;
        embedded
            .then(|| address.strip_prefix("0x")?.parse().ok())
            .flatten()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_comparison_finds_every_difference() {
        assert!(same("abc", "abc"));
        // 'a' ^ 'b' twice: an XOR of the differences would cancel out.
        assert!(!same("ab", "ba"));
        assert!(!same("abc", "abd"));
        assert!(!same("abc", "ab"));
    }

    #[test]
    fn a_token_verifies_until_it_expires_and_not_once_altered() {
        let sessions = Sessions::new(b"secret");
        let token = sessions.issue("guest-1", 2_000);
        assert_eq!(sessions.verify(&token, 1_999), Some("guest-1".into()));
        assert_eq!(sessions.verify(&token, 2_000), None);
        let forged = token.replace("guest-1", "guest-2");
        assert_eq!(sessions.verify(&forged, 1_000), None);
        let extended = token.replace(".2000.", ".9000.");
        assert_eq!(sessions.verify(&extended, 1_000), None);
        assert_eq!(Sessions::new(b"other").verify(&token, 1_000), None);
    }
}
