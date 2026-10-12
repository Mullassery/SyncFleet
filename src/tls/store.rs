//! Trusted-root store for pinned peer certificates (trust-on-first-use).
//!
//! The store is a single PEM file containing the certificates this node has
//! vouched for. Both the server verifier (client certificates) and the client
//! verifier (server certificates) are built from it, so a peer can only
//! connect once its certificate has been explicitly pinned.

use crate::error::{Result, TransportError};
use rustls::RootCertStore;
use sha2::{Digest, Sha256};
use std::path::Path;

/// Load (creating if missing) the trust store from a PEM file.
pub fn load_roots(path: &Path) -> Result<RootCertStore> {
    let mut store = RootCertStore::empty();
    if !path.exists() {
        return Ok(store);
    }
    let data = std::fs::read(path)?;
    for cert in rustls_pemfile::certs(&mut &data[..]) {
        let cert = cert.map_err(|e| TransportError::Certificate(e.to_string()))?;
        store
            .add(cert)
            .map_err(|_| TransportError::Certificate("unable to add pinned cert".to_string()))?;
    }
    Ok(store)
}

/// Pin a DER certificate by appending it to the store (no-op if already
/// present). Returns `true` when a new certificate was added.
pub fn add_cert(path: &Path, der: &[u8]) -> Result<bool> {
    use std::io::Write as _;
    let fp = fingerprint(der);
    if contains(path, &fp)? {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let pem = pem::encode(&pem::Pem {
        tag: "CERTIFICATE".to_string(),
        contents: der.to_vec(),
    });
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?
        .write_all(pem.as_bytes())?;
    Ok(true)
}

fn contains(path: &Path, fp: &str) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let data = std::fs::read(path)?;
    for cert in rustls_pemfile::certs(&mut &data[..]) {
        let cert = cert.map_err(|e| TransportError::Certificate(e.to_string()))?;
        if fingerprint(cert.as_ref()) == fp {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Remove a pinned certificate by SHA-256 fingerprint. Returns `true` when a
/// certificate was removed.
pub fn remove_cert(path: &Path, fp: &str) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let data = std::fs::read(path)?;
    let mut remaining = Vec::new();
    for cert in rustls_pemfile::certs(&mut &data[..]) {
        let cert = cert.map_err(|e| TransportError::Certificate(e.to_string()))?;
        if fingerprint(cert.as_ref()) != fp {
            remaining.push(cert);
        }
    }
    let original_count = rustls_pemfile::certs(&mut &data[..])
        .count();
    if remaining.len() == original_count {
        return Ok(false);
    }
    let mut out = String::new();
    for cert in remaining {
        out.push_str(&pem::encode(&pem::Pem {
            tag: "CERTIFICATE".to_string(),
            contents: cert.as_ref().to_vec(),
        }));
        out.push('\n');
    }
    std::fs::write(path, out)?;
    Ok(true)
}

pub fn fingerprint(der: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(der);
    hex::encode(h.finalize())
}