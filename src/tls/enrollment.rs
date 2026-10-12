//! Single-use, signed enrollment tokens for peer pairing.
//!
//! A node that owns a dataset issues a token granting write/read/admin
//! permission to whoever redeems it. The token embeds the issuer's node ID and
//! certificate so redemption can pin the issuer's certificate to a verified
//! identity without any out-of-band key exchange beyond the token itself.

use crate::error::{Result, TransportError};
use crate::identity::NodeIdentity;
use crate::tls::cert;
use ed25519_dalek::{Signature, Verifier};
use rand::RngCore;
use serde::{Deserialize, Serialize};

pub const TOKEN_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollmentClaims {
    pub v: u32,
    pub issuer: String,
    /// DER-encoded issuer certificate, hex-encoded.
    pub cert: String,
    pub dataset: String,
    pub permissions: u32,
    /// Random 128-bit single-use marker, hex-encoded.
    pub nonce: String,
    pub iat: String,
    pub exp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollmentToken {
    pub claims: EnrollmentClaims,
    /// Ed25519 signature over the canonical JSON of `claims`, hex-encoded.
    pub sig: String,
}

pub fn random_nonce() -> String {
    let mut bytes = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Issue a token for `dataset` with `permissions`, valid for `ttl_hours`.
///
/// The token is self-contained: it carries the issuer node's certificate and
/// is signed with the issuer's Ed25519 key, binding the certificate to the
/// node identity.
pub fn issue(
    identity: &NodeIdentity,
    node_cert: &cert::CertMaterial,
    dataset: &str,
    permissions: u32,
    ttl_hours: u32,
) -> Result<String> {
    let now = chrono::Utc::now();
    let claims = EnrollmentClaims {
        v: TOKEN_VERSION,
        issuer: identity.node_id.clone(),
        cert: hex::encode(&node_cert.der),
        dataset: dataset.to_string(),
        permissions,
        nonce: random_nonce(),
        iat: now.to_rfc3339(),
        exp: (now + chrono::Duration::hours(ttl_hours as i64)).to_rfc3339(),
    };

    let claims_json = serde_json::to_vec(&claims)?;
    let signature = identity.sign(&claims_json, None)?;
    let token = EnrollmentToken {
        claims,
        sig: hex::encode(signature.to_bytes()),
    };

    Ok(format!("sf1:{}", hex::encode(serde_json::to_vec(&token)?)))
}

/// Parse a token string.
pub fn parse(raw: &str) -> Result<EnrollmentToken> {
    let rest = raw
        .strip_prefix("sf1:")
        .ok_or_else(|| TransportError::EnrollmentInvalid("missing sf1: prefix".to_string()))?;
    let bytes = hex::decode(rest)
        .map_err(|e| TransportError::EnrollmentInvalid(format!("bad encoding: {e}")))?;
    serde_json::from_slice(&bytes)
        .map_err(|e| TransportError::EnrollmentInvalid(format!("bad payload: {e}")))
        .map_err(Into::into)
}

/// Validate a token: version, expiry, issuer certificate binding and
/// signature. Returns the verified token on success.
pub fn verify(token: &EnrollmentToken) -> Result<()> {
    if token.claims.v != TOKEN_VERSION {
        return Err(TransportError::EnrollmentInvalid(format!(
            "unsupported version {}",
            token.claims.v
        ))
        .into());
    }

    let exp = token
        .claims
        .exp
        .parse::<chrono::DateTime<chrono::Utc>>()
        .map_err(|e| TransportError::EnrollmentInvalid(format!("bad expiry: {e}")))?;
    if chrono::Utc::now() > exp {
        return Err(TransportError::EnrollmentExpired.into());
    }

    // The embedded certificate must claim the same node ID as the issuer.
    let cert_der = hex::decode(&token.claims.cert)
        .map_err(|e| TransportError::EnrollmentInvalid(format!("bad cert: {e}")))?;
    let cert_node_id = cert::node_id_from_cert(&cert_der)
        .map_err(|_| TransportError::EnrollmentInvalid("certificate has no node ID".to_string()))?;
    if cert_node_id != token.claims.issuer {
        return Err(TransportError::EnrollmentInvalid(
            "certificate does not belong to issuer".to_string(),
        )
        .into());
    }

    // Verify the signature against the certificate's public key.
    let pubkey_bytes = cert::parse_cert(&cert_der)
        .map_err(|e| TransportError::EnrollmentInvalid(format!("bad cert: {e}")))?
        .public_key()
        .subject_public_key
        .data
        .to_vec();
    let verifying_key = ed25519_dalek::VerifyingKey::from_bytes(
        pubkey_bytes
            .as_slice()
            .try_into()
            .map_err(|_| TransportError::EnrollmentInvalid("bad public key".to_string()))?,
    )
    .map_err(|e| TransportError::EnrollmentInvalid(format!("bad public key: {e}")))?;

    let mut sig = [0u8; 64];
    sig.copy_from_slice(
        &hex::decode(&token.sig)
            .map_err(|e| TransportError::EnrollmentInvalid(format!("bad signature: {e}")))?,
    );
    let claims_json = serde_json::to_vec(&token.claims)?;
    verifying_key
        .verify(&claims_json, &Signature::from_bytes(&sig))
        .map_err(|_| TransportError::EnrollmentInvalid("signature verification failed".to_string()))
        .map_err(Into::into)
}

/// The node the token was issued by (requires a prior [`verify`]).
pub fn issuer_node_id(token: &EnrollmentToken) -> String {
    token.claims.issuer.clone()
}

pub fn issuer_cert_der(token: &EnrollmentToken) -> Result<Vec<u8>> {
    hex::decode(&token.claims.cert)
        .map_err(|e| TransportError::EnrollmentInvalid(format!("bad cert: {e}")).into())
}

pub fn dataset(token: &EnrollmentToken) -> String {
    token.claims.dataset.clone()
}

pub fn permissions(token: &EnrollmentToken) -> i32 {
    token.claims.permissions as i32
}

/// Canonical nonce marker used to dedupe redemption.
pub fn nonce(token: &EnrollmentToken) -> String {
    token.claims.nonce.clone()
}

pub fn expiry(token: &EnrollmentToken) -> chrono::DateTime<chrono::Utc> {
    token
        .claims
        .exp
        .parse::<chrono::DateTime<chrono::Utc>>()
        .unwrap_or_else(|_| chrono::Utc::now())
}