//! Authenticated TLS transport (Phase 3).
//!
//! Provides a minimal, bounded length-prefixed message protocol over the
//! mutual-TLS connection. Peer identity is established by the client
//! certificate's `node_<node_id>` SAN entry and checked against the
//! `peer_authorizations` table before any message is exchanged.

use crate::error::{Result, SyncFleetError, TransportError};
use crate::tls::cert;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use tokio_rustls::{server::TlsStream as ServerTlsStream, TlsAcceptor};

pub const PROTO_VERSION: u32 = 1;

/// Endpoint identity: dataset(s) requested and the dataset-agnostic node id.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hello {
    pub proto: u32,
    pub node_id: String,
    pub node_name: String,
    pub datasets: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelloReply {
    pub peer_node_id: String,
    pub peer_node_name: String,
    pub ok: bool,
    pub authorized_datasets: Vec<String>,
    pub error: Option<String>,
}

pub struct ServerHandle {
    pub local_addr: std::net::SocketAddr,
    task: JoinHandle<Result<()>>,
}

impl ServerHandle {
    /// Stop accepting connections and wait for the accept loop to finish.
    pub async fn shutdown(self) {
        self.task.abort();
    }
}

/// Tunables for the peer-serving loop.
#[derive(Debug, Clone)]
pub struct ServerOptions {
    pub max_message_size: usize,
    pub idle_timeout: std::time::Duration,
    pub handshake_timeout: std::time::Duration,
    /// Upper bound on operations streamed per replication `Ops` message.
    pub max_ops_per_response: usize,
    /// Whether to run the Phase 4 incremental replication exchange after the
    /// peering handshake succeeds.
    pub replicate: bool,
}

impl Default for ServerOptions {
    fn default() -> Self {
        Self {
            max_message_size: 1 << 20,
            idle_timeout: std::time::Duration::from_secs(300),
            handshake_timeout: std::time::Duration::from_secs(30),
            max_ops_per_response: crate::replication::DEFAULT_MAX_OPS_PER_RESPONSE,
            replicate: true,
        }
    }
}

/// Bind a listener and spawn the TLS accept loop with the supplied options.
pub fn spawn_server_with_options(
    listener: TcpListener,
    acceptor: TlsAcceptor,
    pool: SqlitePool,
    node_name: String,
    options: ServerOptions,
) -> Result<ServerHandle> {
    let local_addr = listener.local_addr()?;
    let task = tokio::spawn(async move {
        loop {
            let (tcp, peer_addr) = listener.accept().await?;
            tokio::spawn(handle_connection(
                tcp,
                peer_addr,
                acceptor.clone(),
                pool.clone(),
                node_name.clone(),
                options.clone(),
            ));
        }
        #[allow(unreachable_code)]
        {
            Ok(())
        }
    });

    Ok(ServerHandle { local_addr, task })
}

/// Bind a listener and spawn the TLS accept loop (replication enabled).
pub fn spawn_server(
    listener: TcpListener,
    acceptor: TlsAcceptor,
    pool: SqlitePool,
    node_name: String,
    max_message_size: usize,
    idle_timeout: std::time::Duration,
    handshake_timeout: std::time::Duration,
) -> Result<ServerHandle> {
    spawn_server_with_options(
        listener,
        acceptor,
        pool,
        node_name,
        ServerOptions {
            max_message_size,
            idle_timeout,
            handshake_timeout,
            ..ServerOptions::default()
        },
    )
}

async fn handle_connection(
    tcp: TcpStream,
    peer_addr: std::net::SocketAddr,
    acceptor: TlsAcceptor,
    pool: SqlitePool,
    node_name: String,
    options: ServerOptions,
) {
    let result = run_peering(tcp, peer_addr, acceptor, pool, node_name, options).await;
    if let Err(e) = result {
        tracing::debug!(peer = %peer_addr, error = %e, "peering connection closed");
    }
}

async fn run_peering(
    tcp: TcpStream,
    _peer_addr: std::net::SocketAddr,
    acceptor: TlsAcceptor,
    pool: SqlitePool,
    node_name: String,
    options: ServerOptions,
) -> Result<()> {
    let tls = tokio::time::timeout(options.handshake_timeout, acceptor.accept(tcp))
        .await
        .map_err(|_| TransportError::Timeout)?
        .map_err(TransportError::Io)?;

    let peer_node_id = authenticated_node_id(&tls)?;

    let mut stream = tls;
    let msg = tokio::time::timeout(
        options.idle_timeout,
        read_message(&mut stream, options.max_message_size),
    )
    .await
    .map_err(|_| TransportError::Timeout)??;

    let reply = match serde_json::from_slice::<Hello>(&msg) {
        Ok(hello) if hello.proto == PROTO_VERSION => {
            handle_hello(&pool, &node_name, &peer_node_id, &hello).await
        }
        Ok(_) => Err(SyncFleetError::Transport(TransportError::Protocol(
            "unsupported version".to_string(),
        ))),
        Err(e) => Err(SyncFleetError::Transport(TransportError::Protocol(
            format!("bad hello: {e}"),
        ))),
    };

    match reply {
        Ok(reply) => {
            let authorized = reply.authorized_datasets.clone();
            let bytes = serde_json::to_vec(&reply)?;
            write_message(&mut stream, &bytes).await?;

            // Phase 4: continue into the incremental replication exchange.
            // A peer that only completed the Phase 3 handshake will simply
            // hang up (or time out) and we close cleanly.
            if options.replicate {
                if let Err(e) = crate::replication::serve_sync(
                    &mut stream,
                    &pool,
                    &peer_node_id,
                    &authorized,
                    options.max_ops_per_response,
                    options.max_message_size,
                    options.idle_timeout,
                )
                .await
                {
                    tracing::debug!(peer = %peer_node_id, error = %e, "replication ended");
                }
            } else {
                // Keep the stream open long enough for a graceful close.
                tokio::time::sleep(
                    options
                        .idle_timeout
                        .min(std::time::Duration::from_millis(100)),
                )
                .await;
            }
            Ok(())
        }
        Err(e) => {
            let reply = HelloReply {
                peer_node_id: peer_node_id.clone(),
                peer_node_name: node_name.clone(),
                ok: false,
                authorized_datasets: Vec::new(),
                error: Some(e.to_string()),
            };
            let bytes = serde_json::to_vec(&reply)?;
            write_message(&mut stream, &bytes).await?;
            Err(e)
        }
    }
}

async fn handle_hello(
    pool: &SqlitePool,
    self_name: &str,
    peer_node_id: &str,
    hello: &Hello,
) -> Result<HelloReply> {
    if hello.node_id != peer_node_id {
        return Err(SyncFleetError::Transport(
            TransportError::AuthenticationFailed(format!(
                "certificate node_id {} does not match hello node_id {}",
                peer_node_id, hello.node_id
            )),
        ));
    }

    // Permission required for any dataset: read (bit 1).
    let mut authorized = Vec::new();
    for dataset in &hello.datasets {
        if crate::storage::peers::is_authorized(pool, peer_node_id, dataset, 1).await? {
            authorized.push(dataset.clone());
        }
    }
    if authorized.is_empty() {
        return Err(SyncFleetError::Transport(TransportError::PeerRejected(
            format!("{peer_node_id} has no active authorization"),
        )));
    }

    Ok(HelloReply {
        peer_node_id: peer_node_id.to_string(),
        peer_node_name: self_name.to_string(),
        ok: true,
        authorized_datasets: authorized,
        error: None,
    })
}

/// Extract the authenticated node id from the client certificate.
fn authenticated_node_id(stream: &ServerTlsStream<TcpStream>) -> Result<String> {
    let state = stream.get_ref().1;
    let certs = state
        .peer_certificates()
        .ok_or(TransportError::MissingPeerCertificate)?;
    let leaf = certs
        .first()
        .ok_or(TransportError::MissingPeerCertificate)?;
    cert::node_id_from_cert(leaf.as_ref())
}

pub(crate) async fn read_message(
    stream: &mut (dyn tokio::io::AsyncRead + Unpin + Send),
    max_message_size: usize,
) -> Result<Vec<u8>> {
    let mut len = [0u8; 4];
    let n = stream.read_exact(&mut len).await.map_err(io_timeout)?;
    if n == 0 {
        return Err(SyncFleetError::Transport(TransportError::Disconnected));
    }
    let size = u32::from_be_bytes(len) as usize;
    if size > max_message_size {
        return Err(SyncFleetError::Transport(TransportError::MessageTooLarge {
            size,
            limit: max_message_size,
        }));
    }
    let mut buf = vec![0u8; size];
    stream.read_exact(&mut buf).await.map_err(io_timeout)?;
    Ok(buf)
}

pub(crate) async fn write_message(
    stream: &mut (dyn tokio::io::AsyncWrite + Unpin + Send),
    data: &[u8],
) -> Result<()> {
    let len = (data.len() as u32).to_be_bytes();
    stream.write_all(&len).await.map_err(io_timeout)?;
    stream.write_all(data).await.map_err(io_timeout)?;
    Ok(())
}

fn io_timeout(e: tokio::io::Error) -> TransportError {
    if e.kind() == tokio::io::ErrorKind::UnexpectedEof {
        TransportError::Disconnected
    } else {
        TransportError::Io(e)
    }
}

/// Outbound peering exchange: connect, authenticate, send a hello and read
/// the authorization reply.
pub async fn connect(
    addr: std::net::SocketAddr,
    server_name: rustls::pki_types::ServerName<'static>,
    connector: &tokio_rustls::TlsConnector,
    expected_peer_node_id: &str,
    hello: Hello,
    max_message_size: usize,
    timeout: std::time::Duration,
) -> Result<HelloReply> {
    let tcp = tokio::time::timeout(timeout, TcpStream::connect(addr))
        .await
        .map_err(|_| TransportError::Timeout)??;

    let tls = tokio::time::timeout(timeout, connector.connect(server_name, tcp))
        .await
        .map_err(|_| TransportError::Timeout)?
        .map_err(TransportError::Io)?;

    let certs = tls
        .get_ref()
        .1
        .peer_certificates()
        .ok_or(TransportError::MissingPeerCertificate)?;
    let leaf = certs
        .first()
        .ok_or(TransportError::MissingPeerCertificate)?;
    let server_node_id = cert::node_id_from_cert(leaf.as_ref())?;
    if server_node_id != expected_peer_node_id {
        return Err(SyncFleetError::Transport(
            TransportError::AuthenticationFailed(format!(
                "expected peer {expected_peer_node_id}, server presented {server_node_id}"
            )),
        ));
    }

    let mut stream = tls;
    write_message(&mut stream, &serde_json::to_vec(&hello)?).await?;
    let reply = tokio::time::timeout(timeout, read_message(&mut stream, max_message_size))
        .await
        .map_err(|_| TransportError::Timeout)??;
    let reply: HelloReply = serde_json::from_slice(&reply)?;
    if !reply.ok {
        return Err(SyncFleetError::Transport(TransportError::PeerRejected(
            reply
                .error
                .unwrap_or_else(|| "peer rejected connection".to_string()),
        )));
    }
    Ok(reply)
}

/// Connect, authenticate, peering-handshake, then run one Phase 4 incremental
/// replication pass against the peer for every authorized dataset.
///
/// Returns the peering reply and a summary of operations pulled/pushed.
#[allow(clippy::too_many_arguments)]
pub async fn connect_and_sync(
    addr: std::net::SocketAddr,
    server_name: rustls::pki_types::ServerName<'static>,
    connector: &tokio_rustls::TlsConnector,
    expected_peer_node_id: &str,
    hello: Hello,
    pool: &SqlitePool,
    max_message_size: usize,
    max_ops_per_response: usize,
    connect_timeout: std::time::Duration,
    idle_timeout: std::time::Duration,
) -> Result<(HelloReply, crate::replication::SyncReport)> {
    let tcp = tokio::time::timeout(connect_timeout, TcpStream::connect(addr))
        .await
        .map_err(|_| TransportError::Timeout)??;

    let mut stream = tokio::time::timeout(connect_timeout, connector.connect(server_name, tcp))
        .await
        .map_err(|_| TransportError::Timeout)?
        .map_err(TransportError::Io)?;

    let certs = stream
        .get_ref()
        .1
        .peer_certificates()
        .ok_or(TransportError::MissingPeerCertificate)?;
    let leaf = certs
        .first()
        .ok_or(TransportError::MissingPeerCertificate)?;
    let server_node_id = cert::node_id_from_cert(leaf.as_ref())?;
    if server_node_id != expected_peer_node_id {
        return Err(SyncFleetError::Transport(
            TransportError::AuthenticationFailed(format!(
                "expected peer {expected_peer_node_id}, server presented {server_node_id}"
            )),
        ));
    }

    write_message(&mut stream, &serde_json::to_vec(&hello)?).await?;
    let raw = tokio::time::timeout(idle_timeout, read_message(&mut stream, max_message_size))
        .await
        .map_err(|_| TransportError::Timeout)??;
    let reply: HelloReply = serde_json::from_slice(&raw)?;
    if !reply.ok {
        return Err(SyncFleetError::Transport(TransportError::PeerRejected(
            reply
                .error
                .unwrap_or_else(|| "peer rejected connection".to_string()),
        )));
    }

    let datasets = reply.authorized_datasets.clone();
    let report = crate::replication::client_sync(
        &mut stream,
        pool,
        &datasets,
        max_ops_per_response,
        max_message_size,
        idle_timeout,
    )
    .await?;

    Ok((reply, report))
}

/// Resolve a `host:port` (or `ip:port`) peer address into a socket address and
/// the TLS server name to present during the handshake.
pub fn resolve_peer_addr(
    addr: &str,
) -> Result<(std::net::SocketAddr, rustls::pki_types::ServerName<'static>)> {
    use std::net::{SocketAddr, ToSocketAddrs};

    if let Ok(sa) = addr.parse::<SocketAddr>() {
        return Ok((sa, rustls::pki_types::ServerName::IpAddress(sa.ip().into())));
    }

    let (host, port) = addr.rsplit_once(':').ok_or_else(|| {
        SyncFleetError::InvalidArgument(format!(
            "invalid peer address '{addr}' (expected host:port)"
        ))
    })?;
    let port: u16 = port
        .parse()
        .map_err(|_| SyncFleetError::InvalidArgument(format!("invalid port in '{addr}'")))?;
    let name = rustls::pki_types::ServerName::try_from(host.to_string())
        .map_err(|e| SyncFleetError::InvalidArgument(format!("invalid server name '{host}': {e}")))?
        .to_owned();
    let sa = (host, port)
        .to_socket_addrs()
        .map_err(|e| SyncFleetError::InvalidArgument(format!("cannot resolve '{host}': {e}")))?
        .next()
        .ok_or_else(|| {
            SyncFleetError::InvalidArgument(format!("no addresses resolved for '{host}'"))
        })?;
    Ok((sa, name))
}
