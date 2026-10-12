//! Mutual TLS (TLS 1.3) transport security.
//!
//! - [`cert`]: self-signed Ed25519 node certificates.
//! - [`store`]: pinned trust store (trust-on-first-use).
//! - [`enrollment`]: single-use signed enrollment tokens for pairing.

pub mod cert;
pub mod enrollment;
pub mod store;

use crate::error::{Result, TransportError};
use crate::identity::NodeIdentity;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::{ClientConfig, RootCertStore, ServerConfig, SupportedProtocolVersion};
use std::path::Path;
use std::sync::Arc;

pub use cert::CertMaterial;

/// Certificates and key loaded for serving/connecting.
pub struct TlsStore {
    pub cert_chain: Vec<CertificateDer<'static>>,
    pub key: PrivateKeyDer<'static>,
    pub material: CertMaterial,
}

impl TlsStore {
    /// Load from PEM files on disk.
    pub fn load(cert_path: &Path, key_path: &Path) -> Result<Self> {
        let material = cert::load_cert_pem(cert_path)?;
        let key = cert::load_private_key_pem(key_path)?;
        let cert_chain = vec![CertificateDer::from(material.der.clone())];
        Ok(Self {
            cert_chain,
            key,
            material,
        })
    }
}

const TLS13: &SupportedProtocolVersion = &rustls::version::TLS13;

/// Server configuration: requires a client certificate verified against the
/// pinned trust store, and restricts to TLS 1.3.
pub fn server_config(store: &TlsStore, roots: Arc<RootCertStore>) -> Result<ServerConfig> {
    use rustls::server::WebPkiClientVerifier;

    let verifier = WebPkiClientVerifier::builder(roots)
        .build()
        .map_err(|e| TransportError::Certificate(format!("client verifier: {e}")))?;

    let config = ServerConfig::builder_with_protocol_versions(&[TLS13])
        .with_client_cert_verifier(verifier)
        .with_single_cert(store.cert_chain.clone(), store.key.clone_key())
        .map_err(|e| TransportError::Tls(e))?;
    Ok(config)
}

/// Client configuration: presents our own certificate and verifies the
/// server against the pinned trust store over TLS 1.3.
pub fn client_config(store: &TlsStore, roots: Arc<RootCertStore>) -> Result<ClientConfig> {
    use rustls::client::WebPkiServerVerifier;

    let verifier = WebPkiServerVerifier::builder(roots)
        .build()
        .map_err(|e| TransportError::Certificate(format!("server verifier: {e}")))?;

    let config = ClientConfig::builder_with_protocol_versions(&[TLS13])
        .with_webpki_verifier(verifier)
        .with_client_auth_cert(store.cert_chain.clone(), store.key.clone_key())
        .map_err(|e| TransportError::Tls(e))?;
    Ok(config)
}

/// `Arc`-wrapped server roots (the common ownership form used by both
/// verifier builders).
pub type ArcRoots = Arc<RootCertStore>;

/// Ensure the node has certificate material on disk, generating it when
/// missing. `san_addrs` lists the addresses the node is reachable at.
pub fn ensure_material(
    data_dir: &Path,
    node_name: &str,
    san_addrs: &[std::net::IpAddr],
    cert_path: &Path,
    key_path: &Path,
) -> Result<CertMaterial> {
    cert::ensure_material(data_dir, node_name, san_addrs, cert_path, key_path)
}

/// Load the node identity if required for regeneration paths.
pub fn load_identity(data_dir: &Path) -> Result<NodeIdentity> {
    NodeIdentity::load(data_dir)
}