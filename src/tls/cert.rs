//! Self-signed certificate material for mutual TLS.
//!
//! Every node generates a fresh Ed25519 key pair and a self-signed
//! certificate whose subject/SAN carry the node identity. Peers pin each
//! other's certificate (trust-on-first-use) by adding it to a local trust
//! store; the connection layer verifies chains against that store.

use crate::error::{Result, SyncFleetError, TransportError};
use crate::identity::NodeIdentity;
use rcgen::{
    Certificate, CertificateParams, DistinguishedName, DnType, IsCa, KeyPair, PKCS_ED25519,
};
use sha2::{Digest, Sha256};
use std::net::IpAddr;
use std::path::Path;
use x509_parser::prelude::{FromDer, X509Certificate};

/// Validity window for node certificates.
const CERT_VALIDITY_DAYS: i64 = 365 * 2;

#[derive(Debug, Clone)]
pub struct CertMaterial {
    pub der: Vec<u8>,
    pub pem: String,
    /// SHA-256 fingerprint (hex) of the DER certificate.
    pub fingerprint: String,
    /// Node ID extracted from the certificate SAN.
    pub node_id: String,
    /// Human-readable subject common name.
    pub subject: String,
}

pub fn parse_cert(der: &[u8]) -> Result<X509Certificate<'_>> {
    let (_, cert) = X509Certificate::from_der(der)
        .map_err(|e| TransportError::Certificate(format!("parse failed: {e}")))?;
    Ok(cert)
}

pub fn fingerprint(der: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(der);
    hex::encode(h.finalize())
}

/// The node_id bound to a certificate — the sole `dNSName` SAN entry that
/// starts with `node_`. This is how a peer authenticates who it is talking
/// to after the TLS handshake.
pub fn node_id_from_cert(der: &[u8]) -> Result<String> {
    let cert = parse_cert(der)?;
    let san = cert
        .subject_alternative_name()
        .map_err(|e| TransportError::Certificate(format!("SAN parse failed: {e}")))?;
    let san =
        san.ok_or_else(|| TransportError::Certificate("certificate has no SAN".to_string()))?;

    for name in &san.value.general_names {
        if let x509_parser::extensions::GeneralName::DNSName(dns) = name {
            if let Some(id) = dns.strip_prefix("node_") {
                return Ok(id.to_string());
            }
        }
    }
    Err(
        TransportError::Certificate("certificate SAN has no node_<node_id> entry".to_string())
            .into(),
    )
}

pub fn subject_name(der: &[u8]) -> Result<String> {
    let cert = parse_cert(der)?;
    let name = cert
        .subject()
        .iter_common_name()
        .find_map(|atv| atv.as_str().ok())
        .unwrap_or_default()
        .to_string();
    Ok(name)
}

/// Generate a fresh self-signed Ed25519 certificate for `node_id`, returning
/// the certificate material and its matching private key (PKCS#8 PEM).
///
/// `san_addrs` are the IP addresses the node will be reachable at; they are
/// placed in the subjectAltNames so that standard (non-DANGEROUS) rustls
/// verifiers can perform server-name checks when peer addresses are known.
pub fn generate(
    node_id: &str,
    node_name: &str,
    san_addrs: &[IpAddr],
) -> Result<(CertMaterial, String)> {
    let key_pair = KeyPair::generate(&PKCS_ED25519)
        .map_err(|e| TransportError::Certificate(format!("key generation failed: {e}")))?;

    let mut san_names = Vec::with_capacity(san_addrs.len() + 1);
    san_names.push(format!("node_{node_id}"));
    for addr in san_addrs {
        san_names.push(addr.to_string());
    }

    let mut params = CertificateParams::new(san_names);
    params.alg = &PKCS_ED25519;
    params.key_pair = Some(key_pair);
    params.is_ca = IsCa::NoCa;

    let now = time::OffsetDateTime::now_utc();
    params.not_before = now - time::Duration::hours(1);
    params.not_after = now + time::Duration::days(CERT_VALIDITY_DAYS);

    let mut distinguished_name = DistinguishedName::new();
    distinguished_name.push(DnType::CommonName, node_name);
    params.distinguished_name = distinguished_name;

    let cert = Certificate::from_params(params)
        .map_err(|e| TransportError::Certificate(format!("certificate generation failed: {e}")))?;

    let der = cert
        .serialize_der()
        .map_err(|e| TransportError::Certificate(format!("serialization failed: {e}")))?;
    let pem = cert
        .serialize_pem()
        .map_err(|e| TransportError::Certificate(format!("serialization failed: {e}")))?;
    let key_pem = cert.get_key_pair().serialize_pem();

    Ok((
        CertMaterial {
            fingerprint: fingerprint(&der),
            node_id: node_id.to_string(),
            subject: node_name.to_string(),
            der,
            pem,
        },
        key_pem,
    ))
}

/// Load a certificate from a PEM file.
pub fn load_cert_pem(path: &Path) -> Result<CertMaterial> {
    let data = std::fs::read(path)?;
    let cert = rustls_pemfile::certs(&mut &data[..])
        .next()
        .transpose()
        .map_err(|e| SyncFleetError::Transport(TransportError::Certificate(e.to_string())))?
        .ok_or_else(|| {
            SyncFleetError::Transport(TransportError::CertificateNotFound(
                path.display().to_string(),
            ))
        })?;

    let der = cert.as_ref().to_vec();
    Ok(CertMaterial {
        der: der.clone(),
        pem: pem::encode(&pem::Pem {
            tag: "CERTIFICATE".to_string(),
            contents: cert.as_ref().to_vec(),
        }),
        fingerprint: fingerprint(&der),
        node_id: node_id_from_cert(&der)?,
        subject: subject_name(&der)?,
    })
}

/// Load the node's private key from a PKCS#8 PEM file.
pub fn load_private_key_pem(path: &Path) -> Result<rustls::pki_types::PrivateKeyDer<'static>> {
    let data = std::fs::read(path).map_err(|e| {
        SyncFleetError::Transport(TransportError::CertificateNotFound(format!(
            "{}: {e}",
            path.display()
        )))
    })?;
    rustls_pemfile::private_key(&mut &data[..])
        .map_err(|e| SyncFleetError::Transport(TransportError::Certificate(e.to_string())))?
        .ok_or_else(|| {
            SyncFleetError::Transport(TransportError::Certificate(format!(
                "no private key found in {}",
                path.display()
            )))
        })
}

/// Generate certificate material and persist cert + private key (0600) so
/// that runtime connections never require the identity passphrase.
pub fn generate_and_store(
    identity: &NodeIdentity,
    node_name: &str,
    san_addrs: &[IpAddr],
    cert_path: &Path,
    key_path: &Path,
) -> Result<CertMaterial> {
    let (material, key_pem) = generate(&identity.node_id, node_name, san_addrs)?;

    if let Some(parent) = cert_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(cert_path, &material.pem)?;
    std::fs::write(key_path, key_pem.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(key_path, std::fs::Permissions::from_mode(0o600))?;
    }

    Ok(material)
}

/// Ensure certificate material exists for a node, generating it when missing.
pub fn ensure_material(
    data_dir: &Path,
    node_name: &str,
    san_addrs: &[IpAddr],
    cert_path: &Path,
    key_path: &Path,
) -> Result<CertMaterial> {
    if cert_path.exists() && key_path.exists() {
        let material = load_cert_pem(cert_path)?;
        load_private_key_pem(key_path)?;
        return Ok(material);
    }

    let identity = NodeIdentity::load(data_dir)?;
    let material = generate_and_store(&identity, node_name, san_addrs, cert_path, key_path)?;
    Ok(material)
}
