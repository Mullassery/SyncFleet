use crate::config::{PeerConfig, SyncFleetConfig};
use crate::error::{Result, SyncFleetError};
use crate::identity::NodeIdentity;
use crate::replication;
use crate::storage::{self, Storage};
use crate::tls;
use clap::{Args, Parser, Subcommand};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_rustls::{TlsAcceptor, TlsConnector};

#[derive(Parser, Debug)]
#[command(
    name = "syncfleet",
    version,
    about = "SyncFleet - Peer-to-peer edge database synchronization"
)]
pub struct Cli {
    #[arg(short, long, global = true, help = "Configuration file path")]
    pub config: Option<PathBuf>,

    #[arg(short, long, global = true, help = "Log level")]
    pub log_level: Option<String>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Initialize a new SyncFleet node
    Init(InitArgs),

    /// Show node status
    Status(StatusArgs),

    /// Configuration management
    Config(ConfigArgs),

    /// Dataset management
    Dataset(DatasetArgs),

    /// Peer management
    Peer(PeerArgs),

    /// Synchronization commands
    Sync(SyncArgs),

    /// Conflict management
    Conflicts(ConflictArgs),

    /// Journal inspection and maintenance
    Journal(JournalArgs),

    /// Daemon management
    Daemon(DaemonArgs),

    /// Run diagnostics
    Doctor(DoctorArgs),
}

#[derive(Args, Debug)]
pub struct InitArgs {
    #[arg(long, help = "Node name (unique identifier)")]
    pub node: String,

    #[arg(long, help = "Data directory")]
    pub data_dir: Option<PathBuf>,

    #[arg(long, help = "Passphrase for encrypting private keys")]
    pub passphrase: Option<String>,

    #[arg(long, help = "Force re-initialization")]
    pub force: bool,
}

#[derive(Args, Debug)]
pub struct StatusArgs {
    #[arg(long, help = "Output format (json, text)")]
    pub format: Option<String>,

    #[arg(long, help = "Show detailed status")]
    pub verbose: bool,
}

#[derive(Args, Debug)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub action: ConfigAction,
}

#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    /// Validate configuration
    Validate,
    /// Show effective configuration
    Show,
    /// Generate example configuration
    Example,
}

#[derive(Args, Debug)]
pub struct DatasetArgs {
    #[command(subcommand)]
    pub action: DatasetAction,
}

#[derive(Subcommand, Debug)]
pub enum DatasetAction {
    /// List all datasets
    List,
    /// Register a new dataset
    Register(RegisterDatasetArgs),
    /// Show dataset details
    Show { dataset_id: String },
    /// Remove a dataset (local only)
    Remove { dataset_id: String },
    /// Update a dataset's schema version after a migration
    Schema { dataset_id: String, version: u32 },
    /// Record a local operation in the journal (for replication/testing)
    Apply(ApplyDatasetArgs),
}

#[derive(Args, Debug)]
pub struct ApplyDatasetArgs {
    #[arg(long, help = "Dataset to write into")]
    pub dataset: String,

    #[arg(long, help = "Entity id (row key)")]
    pub entity: String,

    #[arg(
        long,
        default_value = "insert",
        help = "Operation type: insert|update|delete"
    )]
    pub op: String,

    #[arg(long, default_value = "{}", help = "JSON object payload")]
    pub data: String,
}

#[derive(Args, Debug)]
pub struct RegisterDatasetArgs {
    #[arg(long, help = "Dataset name/ID")]
    pub name: String,

    #[arg(long, help = "Comma-separated table allowlist")]
    pub tables: String,

    #[arg(
        long,
        help = "Conflict policy: single_writer, lww",
        default_value = "single_writer"
    )]
    pub policy: String,

    #[arg(long, help = "Schema version", default_value = "1")]
    pub schema_version: u32,

    #[arg(long, help = "Overwrite if the dataset already exists")]
    pub force: bool,
}

#[derive(Args, Debug)]
pub struct PeerArgs {
    #[command(subcommand)]
    pub action: PeerAction,
}

#[derive(Subcommand, Debug)]
pub enum PeerAction {
    /// List known peers
    List,
    /// Pair with a peer (enrollment)
    Pair(PairPeerArgs),
    /// Issue a signed single-use enrollment token for a dataset
    Token(TokenArgs),
    /// Redeem an enrollment token issued by a peer (pin the peer's certificate)
    Redeem(RedeemArgs),
    /// Export this node's TLS certificate
    ExportCert(ExportCertArgs),
    /// Import and pin a peer's TLS certificate (and optionally authorize it)
    ImportCert(ImportCertArgs),
    /// List pinned peer certificates
    Certs,
    /// Show peer details
    Show { peer_id: String },
    /// Revoke a peer
    Revoke { peer_id: String },
    /// Retire a peer (for GC)
    Retire { peer_id: String },
    /// Reissue peer certificate
    Reissue { peer_id: String },
}

#[derive(Args, Debug)]
pub struct TokenArgs {
    #[arg(long, help = "Dataset the token grants access to")]
    pub dataset: String,

    #[arg(
        long,
        help = "Permissions bitmask (1=read, 2=write, 3=read+write)",
        default_value = "3"
    )]
    pub permissions: i32,

    #[arg(long, help = "Token validity in hours", default_value = "24")]
    pub ttl_hours: u32,
}

#[derive(Args, Debug)]
pub struct RedeemArgs {
    #[arg(long, help = "Enrollment token (sf1:...)")]
    pub token: String,
}

#[derive(Args, Debug)]
pub struct ExportCertArgs {
    #[arg(long, help = "Write to a file instead of stdout")]
    pub out: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub struct ImportCertArgs {
    #[arg(long, help = "PEM certificate file to import")]
    pub file: PathBuf,

    #[arg(long, help = "Peer node id (default: read from the certificate SAN)")]
    pub peer_id: Option<String>,

    #[arg(long, help = "Authorize the peer for this dataset after pinning")]
    pub dataset: Option<String>,

    #[arg(
        long,
        help = "Permissions bitmask when authorizing",
        default_value = "3"
    )]
    pub permissions: i32,
}

#[derive(Args, Debug)]
pub struct PairPeerArgs {
    #[arg(long, help = "Peer address (host:port)")]
    pub address: String,

    #[arg(long, help = "Peer ID to authorize")]
    pub peer_id: String,

    #[arg(long, help = "Dataset to authorize")]
    pub dataset: String,

    #[arg(long, help = "Permissions bitmask", default_value = "3")]
    pub permissions: i32,
}

#[derive(Args, Debug)]
pub struct SyncArgs {
    #[command(subcommand)]
    pub action: SyncAction,
}

#[derive(Subcommand, Debug)]
pub enum SyncAction {
    /// Show sync status for all datasets
    Status { dataset_id: Option<String> },
    /// Synchronize with a peer now (Phase 4 incremental replication)
    Now {
        #[arg(long, help = "Restrict to a single dataset")]
        dataset_id: Option<String>,
        #[arg(long, help = "Peer node id to synchronize with")]
        peer: Option<String>,
        #[arg(long, help = "Peer address host:port (overrides config [[peers]])")]
        addr: Option<String>,
    },
}

#[derive(Args, Debug)]
pub struct ConflictArgs {
    #[command(subcommand)]
    pub action: ConflictAction,
}

#[derive(Subcommand, Debug)]
pub enum ConflictAction {
    /// List conflicts
    List,
    /// Show conflict details
    Show { conflict_id: String },
    /// Resolve conflict
    Resolve {
        conflict_id: String,
        resolution: String,
    },
}

#[derive(Args, Debug)]
pub struct JournalArgs {
    #[command(subcommand)]
    pub action: JournalAction,
}

#[derive(Subcommand, Debug)]
pub enum JournalAction {
    /// Show journal statistics
    Stats,
    /// Delete journal entries older than the retention window that every peer
    /// has acknowledged (safe to discard)
    Compact {
        #[arg(
            long,
            help = "Override retention in days (default: storage.journal_retention_days)"
        )]
        retention_days: Option<u32>,
        #[arg(long, help = "Report what would be deleted without deleting")]
        dry_run: bool,
    },
}

#[derive(Args, Debug)]
pub struct DaemonArgs {
    #[command(subcommand)]
    pub action: DaemonAction,
}

#[derive(Subcommand, Debug)]
pub enum DaemonAction {
    /// Start daemon
    Start {
        #[arg(long)]
        background: bool,
    },
    /// Stop daemon
    Stop,
    /// Restart daemon
    Restart,
}

#[derive(Args, Debug)]
pub struct DoctorArgs {
    #[arg(long, help = "Run full diagnostics")]
    pub full: bool,

    #[arg(long, help = "Output format (json, text)")]
    pub format: Option<String>,

    #[arg(long, help = "Attempt automatic repair (use with caution)")]
    pub repair: bool,
}

pub async fn run(cli: Cli) -> Result<()> {
    let config = SyncFleetConfig::load(cli.config.as_deref())?;

    if let Some(level) = cli.log_level {
        std::env::set_var("RUST_LOG", level);
    }

    init_tracing(&config);

    match cli.command {
        Commands::Init(args) => cmd_init(&config, args).await,
        Commands::Status(args) => cmd_status(&config, args).await,
        Commands::Config(args) => cmd_config(&config, args).await,
        Commands::Dataset(args) => cmd_dataset(&config, args).await,
        Commands::Peer(args) => cmd_peer(&config, args).await,
        Commands::Sync(args) => cmd_sync(&config, args).await,
        Commands::Conflicts(args) => cmd_conflicts(&config, args).await,
        Commands::Journal(args) => cmd_journal(&config, args).await,
        Commands::Daemon(args) => cmd_daemon(&config, args).await,
        Commands::Doctor(args) => cmd_doctor(&config, args).await,
    }
}

fn init_tracing(config: &SyncFleetConfig) {
    use tracing_subscriber::{fmt, EnvFilter};

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&config.node.log_level));

    let builder = fmt().with_env_filter(filter);
    if config.node.log_format == "json" {
        builder.json().init();
    } else {
        builder.init();
    }
}

/// Open options derived from configuration.
fn open_options(config: &SyncFleetConfig) -> storage::OpenOptions {
    storage::OpenOptions {
        busy_timeout_ms: config.storage.busy_timeout_ms,
        integrity_check_on_startup: config.storage.integrity_check_on_startup,
    }
}

/// Open the local database, failing with a helpful message if not initialized.
async fn open_storage(config: &SyncFleetConfig) -> Result<Storage> {
    let data_dir = &config.node.data_dir;
    if !NodeIdentity::exists(data_dir) {
        return Err(SyncFleetError::NotFound(
            "Node not initialized. Run 'syncfleet init --node <name>' first.".to_string(),
        ));
    }
    Storage::open(data_dir, &open_options(config)).await
}

fn parse_tables(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// SAN addresses to embed in this node's certificate: loopback plus the
/// concrete IP from `network.listen_addr` (when it is not a wildcard).
fn san_addresses(config: &SyncFleetConfig) -> Vec<IpAddr> {
    let mut addrs = vec![
        IpAddr::from([127, 0, 0, 1]),
        IpAddr::from(std::net::Ipv6Addr::LOCALHOST),
    ];
    if let Ok(sa) = config.network.listen_addr.parse::<std::net::SocketAddr>() {
        if !sa.ip().is_unspecified() && !addrs.contains(&sa.ip()) {
            addrs.push(sa.ip());
        }
    }
    addrs
}

/// Load this node's certificate material, generating it on first use.
fn ensure_node_cert(
    config: &SyncFleetConfig,
    data_dir: &Path,
    node_name: &str,
) -> Result<tls::CertMaterial> {
    let cert_path = config.tls_cert_path(data_dir);
    let key_path = config.tls_key_path(data_dir);
    tls::cert::ensure_material(
        data_dir,
        node_name,
        &san_addresses(config),
        &cert_path,
        &key_path,
    )
}

fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

/// Read the first certificate (DER) from a PEM file.
fn read_cert_der(path: &Path) -> Result<Vec<u8>> {
    let data = std::fs::read(path)?;
    let cert = rustls_pemfile::certs(&mut &data[..])
        .next()
        .transpose()
        .map_err(|e| SyncFleetError::InvalidArgument(format!("invalid certificate: {e}")))?
        .ok_or_else(|| {
            SyncFleetError::InvalidArgument(format!("no certificate found in {}", path.display()))
        })?;
    Ok(cert.as_ref().to_vec())
}

/// Build the node's TLS server acceptor (certificate + pinned trust store).
fn build_server_acceptor(config: &SyncFleetConfig, data_dir: &Path) -> Result<TlsAcceptor> {
    let store = tls::TlsStore::load(
        &config.tls_cert_path(data_dir),
        &config.tls_key_path(data_dir),
    )?;
    let roots = Arc::new(tls::store::load_roots(&config.ca_cert_path(data_dir))?);
    let server_cfg = tls::server_config(&store, roots)?;
    Ok(TlsAcceptor::from(Arc::new(server_cfg)))
}

/// Build the node's TLS client connector (certificate + pinned trust store).
fn build_client_connector(config: &SyncFleetConfig, data_dir: &Path) -> Result<TlsConnector> {
    let store = tls::TlsStore::load(
        &config.tls_cert_path(data_dir),
        &config.tls_key_path(data_dir),
    )?;
    let roots = Arc::new(tls::store::load_roots(&config.ca_cert_path(data_dir))?);
    let client_cfg = tls::client_config(&store, roots)?;
    Ok(TlsConnector::from(Arc::new(client_cfg)))
}

async fn cmd_init(config: &SyncFleetConfig, args: InitArgs) -> Result<()> {
    let data_dir = args.data_dir.as_deref().unwrap_or(&config.node.data_dir);

    if NodeIdentity::exists(data_dir) && !args.force {
        return Err(SyncFleetError::AlreadyExists(format!(
            "Node already initialized at {}. Use --force to overwrite.",
            data_dir.display()
        )));
    }

    std::fs::create_dir_all(data_dir)?;

    let passphrase = args.passphrase.as_deref();
    let identity = NodeIdentity::generate(passphrase)?;
    identity.save(data_dir)?;

    // Initialize the database and record the node identity.
    let storage = Storage::new(data_dir).await?;
    storage::record_node_identity(
        storage.pool(),
        &identity.node_id,
        &identity.ed25519_public,
        &identity.ed25519_private_encrypted,
    )
    .await?;
    storage.close().await;

    // Generate the node's self-signed TLS material (mutual-auth transport).
    let cert_path = config.tls_cert_path(data_dir);
    let key_path = config.tls_key_path(data_dir);
    let cert = tls::cert::generate_and_store(
        &identity,
        &args.node,
        &san_addresses(config),
        &cert_path,
        &key_path,
    )?;

    println!("Initialized SyncFleet node");
    println!("  Node name:  {}", args.node);
    println!("  Node ID:    {}", identity.node_id);
    println!("  Data dir:   {}", data_dir.display());
    println!("  Ed25519:    {}", hex::encode(&identity.ed25519_public));
    println!("  X25519:     {}", hex::encode(&identity.x25519_public));
    println!("  TLS cert:   {}", cert_path.display());
    println!("  Cert fp:    {}", cert.fingerprint);

    if passphrase.is_some() {
        println!("  Private keys encrypted with passphrase.");
    } else {
        println!("  WARNING: private keys stored unencrypted (no --passphrase given).");
    }

    Ok(())
}

async fn cmd_status(config: &SyncFleetConfig, args: StatusArgs) -> Result<()> {
    let data_dir = &config.node.data_dir;

    if !NodeIdentity::exists(data_dir) {
        return Err(SyncFleetError::NotFound(
            "Node not initialized. Run 'syncfleet init' first.".to_string(),
        ));
    }

    let identity = NodeIdentity::load(data_dir)?;
    let storage = Storage::new(data_dir).await?;
    let datasets = storage::datasets::list_datasets(storage.pool()).await?;
    let peers = storage::peers::list_peers(storage.pool()).await?;
    let conflicts = storage::conflicts::list_conflicts(storage.pool()).await?;
    storage.close().await;

    let status = serde_json::json!({
        "node_name": config.node.node_name,
        "node_id": identity.node_id,
        "created_at": identity.created_at,
        "data_dir": data_dir,
        "datasets": datasets.len(),
        "peers": peers.len(),
        "open_conflicts": conflicts.iter().filter(|c| c.resolved_at.is_none()).count(),
    });

    match args.format.as_deref() {
        Some("json") => println!("{}", serde_json::to_string_pretty(&status)?),
        _ => {
            println!("Node:      {}", config.node.node_name);
            println!("Node ID:   {}", identity.node_id);
            println!("Created:   {}", identity.created_at);
            println!("Data dir:  {}", data_dir.display());
            println!("Datasets:  {}", datasets.len());
            println!("Peers:     {}", peers.len());
            if args.verbose {
                for ds in &datasets {
                    println!(
                        "  - {} (schema v{}, policy {})",
                        ds.dataset_id, ds.schema_version, ds.conflict_policy
                    );
                }
                for p in &peers {
                    let state = if p.revoked_at.is_some() {
                        "revoked"
                    } else {
                        "active"
                    };
                    println!("  peer {} -> {} ({})", p.peer_id, p.dataset_id, state);
                }
            }
        }
    }

    Ok(())
}

async fn cmd_config(config: &SyncFleetConfig, args: ConfigArgs) -> Result<()> {
    match args.action {
        ConfigAction::Validate => {
            config.validate()?;
            println!("Configuration is valid.");
        }
        ConfigAction::Show => {
            println!("{}", config.to_toml()?);
        }
        ConfigAction::Example => {
            println!("{}", include_str!("../../config/syncfleet.example.toml"));
        }
    }
    Ok(())
}

async fn cmd_dataset(config: &SyncFleetConfig, args: DatasetArgs) -> Result<()> {
    let storage = open_storage(config).await?;
    let result = match args.action {
        DatasetAction::List => {
            let datasets = storage::datasets::list_datasets(storage.pool()).await?;
            if datasets.is_empty() {
                println!("No datasets registered.");
            } else {
                for ds in datasets {
                    println!(
                        "{}\tschema={}\tpolicy={}\ttables={}",
                        ds.dataset_id,
                        ds.schema_version,
                        ds.conflict_policy,
                        ds.table_allowlist.join(",")
                    );
                }
            }
            Ok(())
        }
        DatasetAction::Register(args) => {
            let tables = parse_tables(&args.tables);
            if tables.is_empty() {
                storage.close().await;
                return Err(SyncFleetError::InvalidArgument(
                    "--tables must list at least one table".to_string(),
                ));
            }
            if let Some(existing) =
                storage::datasets::get_dataset(storage.pool(), &args.name).await?
            {
                if !args.force {
                    storage.close().await;
                    return Err(SyncFleetError::AlreadyExists(format!(
                        "Dataset '{}' already registered (schema v{}). Use --force to overwrite.",
                        existing.dataset_id, existing.schema_version
                    )));
                }
            }
            let dataset = storage::datasets::Dataset {
                dataset_id: args.name.clone(),
                schema_version: args.schema_version,
                conflict_policy: args.policy.clone(),
                table_allowlist: tables,
                created_at: chrono::Utc::now(),
            };
            storage::datasets::register_dataset(storage.pool(), &dataset).await?;
            println!(
                "Registered dataset '{}' (schema v{}, policy {}).",
                args.name, args.schema_version, args.policy
            );
            Ok(())
        }
        DatasetAction::Show { dataset_id } => {
            match storage::datasets::get_dataset(storage.pool(), &dataset_id).await? {
                Some(ds) => {
                    println!("Dataset:    {}", ds.dataset_id);
                    println!("Schema:     v{}", ds.schema_version);
                    println!("Policy:     {}", ds.conflict_policy);
                    println!("Tables:     {}", ds.table_allowlist.join(", "));
                    println!("Created:    {}", ds.created_at);
                    Ok(())
                }
                None => {
                    storage.close().await;
                    return Err(SyncFleetError::NotFound(format!(
                        "Dataset '{}' not found",
                        dataset_id
                    )));
                }
            }
        }
        DatasetAction::Schema {
            dataset_id,
            version,
        } => {
            if storage::datasets::set_schema_version(storage.pool(), &dataset_id, version).await? {
                println!(
                    "Dataset '{}' schema_version set to {}.",
                    dataset_id, version
                );
            } else {
                println!("Dataset '{}' not found.", dataset_id);
            }
            Ok(())
        }
        DatasetAction::Remove { dataset_id } => {
            if storage::datasets::remove_dataset(storage.pool(), &dataset_id).await? {
                println!("Removed dataset '{}'.", dataset_id);
                Ok(())
            } else {
                storage.close().await;
                return Err(SyncFleetError::NotFound(format!(
                    "Dataset '{}' not found",
                    dataset_id
                )));
            }
        }
        DatasetAction::Apply(args) => {
            let dataset =
                match storage::datasets::get_dataset(storage.pool(), &args.dataset).await? {
                    Some(ds) => ds,
                    None => {
                        storage.close().await;
                        return Err(SyncFleetError::NotFound(format!(
                            "Dataset '{}' not registered; register it before writing.",
                            args.dataset
                        )));
                    }
                };
            let op_type = replication::parse_op_type(&args.op)?;
            let value: serde_json::Value = serde_json::from_str(&args.data).map_err(|e| {
                SyncFleetError::InvalidArgument(format!("--data is not valid JSON: {e}"))
            })?;
            if !value.is_object() {
                storage.close().await;
                return Err(SyncFleetError::InvalidArgument(
                    "--data must be a JSON object".to_string(),
                ));
            }
            let identity = NodeIdentity::load(&config.node.data_dir)?;
            let op = replication::local_write(
                storage.pool(),
                &identity.node_id,
                &args.dataset,
                &args.entity,
                op_type,
                dataset.schema_version,
                replication::encode_payload(&value),
            )
            .await?;
            println!(
                "Applied {} on '{}' entity '{}' (seq {}, op {}).",
                args.op, args.dataset, args.entity, op.origin_seq, op.id
            );
            Ok(())
        }
    };
    storage.close().await;
    result
}

async fn cmd_peer(config: &SyncFleetConfig, args: PeerArgs) -> Result<()> {
    let storage = open_storage(config).await?;
    let result = match args.action {
        PeerAction::List => {
            let peers = storage::peers::list_peers(storage.pool()).await?;
            if peers.is_empty() {
                println!("No authorized peers.");
            } else {
                for p in peers {
                    let state = if p.revoked_at.is_some() {
                        "revoked"
                    } else {
                        "active"
                    };
                    println!(
                        "{}\t{}\tperms={}\t{}",
                        p.peer_id, p.dataset_id, p.permissions, state
                    );
                }
            }
            Ok(())
        }
        PeerAction::Pair(args) => {
            if storage::datasets::get_dataset(storage.pool(), &args.dataset)
                .await?
                .is_none()
            {
                storage.close().await;
                return Err(SyncFleetError::NotFound(format!(
                    "Dataset '{}' not registered locally; register it before pairing.",
                    args.dataset
                )));
            }
            storage::peers::authorize_peer(
                storage.pool(),
                &args.peer_id,
                &args.dataset,
                args.permissions,
            )
            .await?;
            let token = NodeIdentity::enrollment_token();
            println!(
                "Authorized peer '{}' for dataset '{}' (perms {}).",
                args.peer_id, args.dataset, args.permissions
            );
            println!("Peer address: {}", args.address);
            println!(
                "Enrollment token (share out-of-band, expires {}): {}",
                token.expires_at, token.token
            );
            Ok(())
        }
        PeerAction::Token(a) => {
            if storage::datasets::get_dataset(storage.pool(), &a.dataset)
                .await?
                .is_none()
            {
                storage.close().await;
                return Err(SyncFleetError::NotFound(format!(
                    "Dataset '{}' not registered locally; register it before issuing a token.",
                    a.dataset
                )));
            }
            let cert = ensure_node_cert(config, &config.node.data_dir, &config.node.node_name)?;
            let signing_key =
                tls::cert::signing_key_from_pem(&config.tls_key_path(&config.node.data_dir))?;
            let token = tls::enrollment::issue(
                &cert,
                &signing_key,
                &a.dataset,
                a.permissions as u32,
                a.ttl_hours,
            )?;
            let expires = chrono::Utc::now() + chrono::Duration::hours(a.ttl_hours as i64);
            // Record the token so redemption can be enforced single-use on this
            // node when the redeeming peer connects.
            storage::enrollment::issue(
                storage.pool(),
                &sha256_hex(token.as_bytes()),
                &cert.node_id,
                &a.dataset,
                a.permissions as u32,
                expires,
            )
            .await?;
            println!("Enrollment token (share out-of-band):");
            println!("{token}");
            println!("  Issuer:   {} ({})", config.node.node_name, cert.node_id);
            println!("  Dataset:  {}", a.dataset);
            println!("  Perms:    {}", a.permissions);
            println!("  Expires:  {}", expires.to_rfc3339());
            println!("  Cert fp:  {}", cert.fingerprint);
            println!("  Lifetime: {}h", a.ttl_hours);
            Ok(())
        }
        PeerAction::Redeem(a) => {
            let token = tls::enrollment::parse(&a.token)?;
            tls::enrollment::verify(&token)?;
            let issuer = tls::enrollment::issuer_node_id(&token);
            let dataset = tls::enrollment::dataset(&token);
            let perms = tls::enrollment::permissions(&token);
            let cert_der = tls::enrollment::issuer_cert_der(&token)?;
            let fp = tls::cert::fingerprint(&cert_der);
            let ca_path = config.ca_cert_path(&config.node.data_dir);
            tls::store::add_cert(&ca_path, &cert_der)?;
            storage::peers::pin_certificate(storage.pool(), &issuer, &fp).await?;

            println!("Redeemed enrollment token:");
            println!("  Issuer:    {issuer}");
            println!("  Dataset:   {dataset}");
            println!("  Perms:     {perms}");
            println!("  Pinned:    {} ({fp})", ca_path.display());

            let own = ensure_node_cert(config, &config.node.data_dir, &config.node.node_name)?;
            println!(
                "  Your cert: {} (send it to the issuer so it can authorize you)",
                own.fingerprint
            );
            println!("  Export it with: syncfleet peer export-cert --out mycert.pem");
            Ok(())
        }
        PeerAction::ExportCert(a) => {
            let cert = ensure_node_cert(config, &config.node.data_dir, &config.node.node_name)?;
            match a.out {
                Some(path) => {
                    std::fs::write(&path, &cert.pem)?;
                    println!(
                        "Wrote certificate to {} ({})",
                        path.display(),
                        cert.fingerprint
                    );
                }
                None => print!("{}", cert.pem),
            }
            Ok(())
        }
        PeerAction::ImportCert(a) => {
            let der = read_cert_der(&a.file)?;
            let peer_id = match &a.peer_id {
                Some(id) => id.clone(),
                None => tls::cert::node_id_from_cert(&der)?,
            };
            let fp = tls::cert::fingerprint(&der);
            let ca_path = config.ca_cert_path(&config.node.data_dir);
            let added = tls::store::add_cert(&ca_path, &der)?;
            storage::peers::pin_certificate(storage.pool(), &peer_id, &fp).await?;

            println!("Imported certificate for peer '{peer_id}' ({fp})");
            println!(
                "  Trust store: {}{}",
                ca_path.display(),
                if added { "" } else { " (already pinned)" }
            );

            if let Some(dataset) = &a.dataset {
                storage::peers::authorize_peer(storage.pool(), &peer_id, dataset, a.permissions)
                    .await?;
                println!(
                    "  Authorized '{peer_id}' for dataset '{dataset}' (perms {})",
                    a.permissions
                );
            }
            Ok(())
        }
        PeerAction::Certs => {
            let certs = storage::peers::list_peer_certificates(storage.pool()).await?;
            if certs.is_empty() {
                println!("No pinned peer certificates.");
            } else {
                for c in certs {
                    println!("{}\t{}\t{}", c.peer_id, c.fingerprint, c.pinned_at);
                }
            }
            Ok(())
        }
        PeerAction::Show { peer_id } => {
            let peers = storage::peers::list_peers(storage.pool()).await?;
            let matches: Vec<_> = peers.into_iter().filter(|p| p.peer_id == peer_id).collect();
            if matches.is_empty() {
                storage.close().await;
                return Err(SyncFleetError::NotFound(format!(
                    "Peer '{}' not found",
                    peer_id
                )));
            }
            for p in matches {
                let state = if p.revoked_at.is_some() {
                    "revoked"
                } else {
                    "active"
                };
                println!("Peer:       {}", p.peer_id);
                println!("Dataset:    {}", p.dataset_id);
                println!("Perms:      {}", p.permissions);
                println!("Granted at: {}", p.granted_at);
                println!("State:      {}", state);
            }
            Ok(())
        }
        PeerAction::Revoke { peer_id } => {
            let peers = storage::peers::list_peers(storage.pool()).await?;
            let datasets: Vec<String> = peers
                .into_iter()
                .filter(|p| p.peer_id == peer_id && p.revoked_at.is_none())
                .map(|p| p.dataset_id)
                .collect();
            if datasets.is_empty() {
                storage.close().await;
                return Err(SyncFleetError::NotFound(format!(
                    "No active authorization for peer '{}'",
                    peer_id
                )));
            }
            for ds in &datasets {
                storage::peers::revoke_peer(storage.pool(), &peer_id, ds).await?;
            }
            println!(
                "Revoked peer '{}' from {} dataset(s).",
                peer_id,
                datasets.len()
            );
            Ok(())
        }
        PeerAction::Retire { peer_id } => {
            println!("Retire is not implemented yet for peer '{}'.", peer_id);
            Ok(())
        }
        PeerAction::Reissue { peer_id } => {
            println!(
                "Certificate reissue is not implemented yet for peer '{}'.",
                peer_id
            );
            Ok(())
        }
    };
    storage.close().await;
    result
}

async fn cmd_sync(config: &SyncFleetConfig, args: SyncArgs) -> Result<()> {
    let storage = open_storage(config).await?;
    let result = match args.action {
        SyncAction::Status { dataset_id } => {
            let datasets = storage::datasets::list_datasets(storage.pool()).await?;
            let selected: Vec<_> = match &dataset_id {
                Some(id) => datasets
                    .into_iter()
                    .filter(|d| &d.dataset_id == id)
                    .collect(),
                None => datasets,
            };
            if selected.is_empty() {
                println!("No matching datasets.");
            }
            for ds in selected {
                let checkpoints =
                    storage::checkpoints::list_for_dataset(storage.pool(), &ds.dataset_id).await?;
                if checkpoints.is_empty() {
                    println!("{}\tno checkpoint yet", ds.dataset_id);
                }
                for cp in checkpoints {
                    println!(
                        "{}\torigin={}\tlast_acked={}\tlast_applied={}\tupdated={}",
                        ds.dataset_id,
                        cp.peer_id,
                        cp.last_acked_seq,
                        cp.last_applied_seq,
                        cp.updated_at
                    );
                }
            }
            Ok(())
        }
        SyncAction::Now {
            dataset_id,
            peer,
            addr,
        } => {
            let identity = NodeIdentity::load(&config.node.data_dir)?;

            let peer_id = peer.ok_or_else(|| {
                SyncFleetError::InvalidArgument(
                    "--peer <node_id> is required for 'sync now' unless a [[peers]] entry matches"
                        .to_string(),
                )
            })?;

            // Resolve the address: explicit --addr wins, else the config [[peers]].
            let address = match addr {
                Some(a) => a,
                None => config
                    .peers
                    .iter()
                    .find(|p| p.node_id == peer_id)
                    .map(|p| p.address.clone())
                    .ok_or_else(|| {
                        SyncFleetError::NotFound(format!(
                            "no address for peer '{peer_id}': pass --addr or add a [[peers]] entry"
                        ))
                    })?,
            };

            // Datasets to attempt: the requested one, or every locally known one.
            let local = storage::datasets::list_datasets(storage.pool()).await?;
            let datasets: Vec<String> = match &dataset_id {
                Some(id) => {
                    if !local.iter().any(|d| &d.dataset_id == id) {
                        storage.close().await;
                        return Err(SyncFleetError::NotFound(format!(
                            "Dataset '{id}' not registered locally."
                        )));
                    }
                    vec![id.clone()]
                }
                None => local.iter().map(|d| d.dataset_id.clone()).collect(),
            };
            if datasets.is_empty() {
                println!("No datasets to synchronize.");
                storage.close().await;
                return Ok(());
            }

            let (sockaddr, server_name) = crate::net::resolve_peer_addr(&address)?;
            let connector = build_client_connector(config, &config.node.data_dir)?;
            let hello = crate::net::Hello {
                proto: crate::net::PROTO_VERSION,
                node_id: identity.node_id.clone(),
                node_name: config.node.node_name.clone(),
                datasets: datasets.clone(),
            };

            let (reply, report) = crate::net::connect_and_sync(
                sockaddr,
                server_name,
                &connector,
                &peer_id,
                hello,
                storage.pool(),
                config.network.max_message_size,
                config.replication.max_ops_per_response,
                Duration::from_secs(config.network.connect_timeout_sec),
                Duration::from_secs(config.network.idle_timeout_sec),
            )
            .await?;

            println!("Synchronized with {} ({})", peer_id, reply.peer_node_name);
            println!(
                "  datasets={} pulled={} pushed={}",
                report.datasets, report.pulled, report.pushed
            );
            if report.datasets < datasets.len() {
                println!(
                    "  note: {} requested dataset(s) not authorized by the peer",
                    datasets.len() - report.datasets
                );
            }
            Ok(())
        }
    };
    storage.close().await;
    result
}

async fn cmd_conflicts(config: &SyncFleetConfig, args: ConflictArgs) -> Result<()> {
    let storage = open_storage(config).await?;
    let result = match args.action {
        ConflictAction::List => {
            let conflicts = storage::conflicts::list_conflicts(storage.pool()).await?;
            if conflicts.is_empty() {
                println!("No conflicts recorded.");
            } else {
                for c in conflicts {
                    let state = if c.resolved_at.is_some() {
                        "resolved"
                    } else {
                        "open"
                    };
                    println!("{}\t{}\t{}", c.id, c.conflict_type, state);
                }
            }
            Ok(())
        }
        ConflictAction::Show { conflict_id } => {
            match storage::conflicts::get_conflict(storage.pool(), &conflict_id).await? {
                Some(c) => {
                    println!("Conflict:   {}", c.id);
                    println!("Type:       {}", c.conflict_type);
                    println!("Operation:  {}", c.operation_id);
                    println!(
                        "Local op:   {}",
                        c.local_operation_id.as_deref().unwrap_or("-")
                    );
                    println!("Remote op:  {}", c.remote_operation_id);
                    println!("Detected:   {}", c.detected_at);
                    println!(
                        "Resolution: {}",
                        c.resolution.as_deref().unwrap_or("unresolved")
                    );
                    Ok(())
                }
                None => {
                    storage.close().await;
                    return Err(SyncFleetError::NotFound(format!(
                        "Conflict '{}' not found",
                        conflict_id
                    )));
                }
            }
        }
        ConflictAction::Resolve {
            conflict_id,
            resolution,
        } => {
            if storage::conflicts::resolve_conflict(storage.pool(), &conflict_id, &resolution)
                .await?
            {
                println!("Resolved conflict '{}' as '{}'.", conflict_id, resolution);
                Ok(())
            } else {
                storage.close().await;
                return Err(SyncFleetError::NotFound(format!(
                    "Open conflict '{}' not found",
                    conflict_id
                )));
            }
        }
    };
    storage.close().await;
    result
}

async fn cmd_journal(config: &SyncFleetConfig, args: JournalArgs) -> Result<()> {
    let storage = open_storage(config).await?;
    let result = match args.action {
        JournalAction::Stats => {
            let stats = storage::operations::journal_stats(storage.pool()).await?;
            println!("Journal operations:   {}", stats.operations);
            println!("Applied operations:   {}", stats.applied_operations);
            println!(
                "Oldest operation:     {}",
                stats
                    .oldest
                    .map(|t| t.to_rfc3339())
                    .unwrap_or_else(|| "-".to_string())
            );
            println!(
                "Newest operation:     {}",
                stats
                    .newest
                    .map(|t| t.to_rfc3339())
                    .unwrap_or_else(|| "-".to_string())
            );
            Ok(())
        }
        JournalAction::Compact {
            retention_days,
            dry_run,
        } => {
            let days = retention_days.unwrap_or(config.storage.journal_retention_days);
            let cutoff = chrono::Utc::now() - chrono::Duration::days(days as i64);

            // Active (non-revoked) peers protect entries they have not acked.
            let peers = storage::peers::list_peers(storage.pool()).await?;
            let protected: Vec<(String, String)> = peers
                .into_iter()
                .filter(|p| p.revoked_at.is_none())
                .map(|p| (p.peer_id, p.dataset_id))
                .collect();

            if dry_run {
                let plan = storage::operations::plan_compaction(storage.pool(), cutoff, &protected)
                    .await?;
                println!(
                    "{} journal entr{} eligible for compaction (retention {} days).",
                    plan.len(),
                    if plan.len() == 1 { "y is" } else { "ies are" },
                    days
                );
                Ok(())
            } else {
                let deleted =
                    storage::operations::compact_journal(storage.pool(), cutoff, &protected)
                        .await?;
                println!(
                    "Compacted {} journal entr{} (retention {} days).",
                    deleted,
                    if deleted == 1 { "y" } else { "ies" },
                    days
                );
                Ok(())
            }
        }
    };
    storage.close().await;
    result
}

fn read_daemon_pid(config: &SyncFleetConfig) -> Option<u32> {
    std::fs::read_to_string(&config.daemon.pid_file)
        .ok()
        .and_then(|text| text.trim().parse().ok())
}

#[cfg(unix)]
fn daemon_pid_alive(pid: u32) -> bool {
    // Signal 0 performs error checking without sending a signal. EPERM means
    // the process exists but belongs to another user, which still counts.
    let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(unix))]
fn daemon_pid_alive(_pid: u32) -> bool {
    false
}

fn write_daemon_pid(config: &SyncFleetConfig, pid: u32) -> Result<()> {
    if let Some(parent) = config.daemon.pid_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&config.daemon.pid_file, format!("{pid}\n"))?;
    Ok(())
}

fn clear_daemon_pid(config: &SyncFleetConfig) {
    let _ = std::fs::remove_file(&config.daemon.pid_file);
}

struct PidFileGuard<'a> {
    config: &'a SyncFleetConfig,
}

impl Drop for PidFileGuard<'_> {
    fn drop(&mut self) {
        clear_daemon_pid(self.config);
    }
}

#[cfg(unix)]
fn spawn_background_daemon(config: &SyncFleetConfig) -> Result<()> {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    let exe = std::env::current_exe()?;
    let args: Vec<std::ffi::OsString> = std::env::args_os()
        .skip(1)
        .filter(|arg| arg != "--background")
        .collect();
    std::fs::create_dir_all(&config.node.data_dir)?;
    let log_path = config.node.data_dir.join("syncfleet.log");
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    let log_err = log.try_clone()?;

    let mut command = Command::new(exe);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(log_err);
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = command.spawn()?;
    let child_pid = child.id();

    for _ in 0..50 {
        if let Some(pid) = read_daemon_pid(config) {
            println!(
                "SyncFleet daemon started in the background (pid {pid}). Logs: {}",
                log_path.display()
            );
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(SyncFleetError::Internal(format!(
        "daemon child (pid {child_pid}) did not write {}; see {}",
        config.daemon.pid_file.display(),
        log_path.display()
    )))
}

#[cfg(not(unix))]
fn spawn_background_daemon(_config: &SyncFleetConfig) -> Result<()> {
    Err(SyncFleetError::Internal(
        "background daemon mode is only supported on Unix-like platforms".to_string(),
    ))
}

async fn stop_daemon(config: &SyncFleetConfig) -> Result<()> {
    #[cfg(not(unix))]
    {
        let _ = config;
        return Err(SyncFleetError::Internal(
            "daemon stop is only supported on Unix-like platforms".to_string(),
        ));
    }

    #[cfg(unix)]
    {
        let Some(pid) = read_daemon_pid(config) else {
            return Err(SyncFleetError::NotFound(
                "daemon is not running (no pid file)".to_string(),
            ));
        };
        if !daemon_pid_alive(pid) {
            clear_daemon_pid(config);
            return Err(SyncFleetError::NotFound(format!(
                "daemon pid {pid} is not running; removed stale pid file"
            )));
        }
        let result = unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
        if result != 0 {
            return Err(SyncFleetError::Internal(format!(
                "failed to signal daemon pid {pid}: {}",
                std::io::Error::last_os_error()
            )));
        }
        let deadline =
            tokio::time::Instant::now() + Duration::from_secs(config.daemon.shutdown_timeout_sec);
        while tokio::time::Instant::now() < deadline {
            if !daemon_pid_alive(pid) {
                clear_daemon_pid(config);
                println!("SyncFleet daemon stopped (pid {pid}).");
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err(SyncFleetError::Internal(format!(
            "daemon pid {pid} did not exit within {}s",
            config.daemon.shutdown_timeout_sec
        )))
    }
}

async fn cmd_daemon(config: &SyncFleetConfig, args: DaemonArgs) -> Result<()> {
    match args.action {
        DaemonAction::Start { background } => {
            let data_dir = &config.node.data_dir;
            if !NodeIdentity::exists(data_dir) {
                return Err(SyncFleetError::NotFound(
                    "Node not initialized. Run 'syncfleet init' first.".to_string(),
                ));
            }
            let identity = NodeIdentity::load(data_dir)?;

            // Ensure this node's TLS material exists, and that at least one peer
            // certificate is pinned (otherwise no client can complete mTLS).
            ensure_node_cert(config, data_dir, &config.node.node_name)?;
            let ca_path = config.ca_cert_path(data_dir);
            if !ca_path.exists() {
                return Err(SyncFleetError::NotFound(format!(
                    "No pinned peer certificates at {}. Enroll a peer first \
                     ('peer redeem' or 'peer import-cert').",
                    ca_path.display()
                )));
            }

            if background {
                return spawn_background_daemon(config);
            }
            if let Some(pid) = read_daemon_pid(config) {
                if daemon_pid_alive(pid) {
                    return Err(SyncFleetError::Internal(format!(
                        "daemon is already running (pid {pid})"
                    )));
                }
                clear_daemon_pid(config);
            }

            let acceptor = build_server_acceptor(config, data_dir)?;
            let listener = TcpListener::bind(&config.network.listen_addr).await?;
            let local_addr = listener.local_addr()?;
            write_daemon_pid(config, std::process::id())?;
            let _pid_guard = PidFileGuard { config };
            let storage = open_storage(config).await?;

            let handle = crate::net::spawn_server_with_options(
                listener,
                acceptor,
                storage.pool().clone(),
                config.node.node_name.clone(),
                crate::net::ServerOptions {
                    max_message_size: config.network.max_message_size,
                    idle_timeout: Duration::from_secs(config.network.idle_timeout_sec),
                    handshake_timeout: Duration::from_secs(config.network.connect_timeout_sec),
                    max_ops_per_response: config.replication.max_ops_per_response,
                    replicate: true,
                },
            )?;

            // Start mDNS discovery (if enabled) and bridge discovered,
            // authorized peers into the sync loop.
            #[cfg(feature = "mdns")]
            let (discovery_handle, discovery_rx) = if config.discovery.enabled {
                match crate::discovery::Discovery::start(
                    &config.discovery,
                    &identity.node_id,
                    &config.node.node_name,
                    local_addr.port(),
                    env!("CARGO_PKG_VERSION"),
                ) {
                    Ok(mut discovery) => match discovery.browse(identity.node_id.clone()) {
                        Ok(mut rx) => {
                            let (tx, peer_rx) = tokio::sync::mpsc::unbounded_channel();
                            let pool = storage.pool().clone();
                            tokio::spawn(async move {
                                while let Some(peer) = rx.recv().await {
                                    match crate::sync_loop::authorized_peer_ids(&pool).await {
                                        Ok(ids) if ids.contains(&peer.node_id) => {
                                            let cfg = PeerConfig {
                                                node_id: peer.node_id.clone(),
                                                address: peer.address(),
                                            };
                                            if tx.send(cfg).is_err() {
                                                break;
                                            }
                                        }
                                        Ok(_) => tracing::debug!(
                                            peer = %peer.node_id,
                                            "discovered peer is not authorized; ignoring"
                                        ),
                                        Err(e) => tracing::warn!(
                                            error = %e,
                                            "failed to check discovered peer authorization"
                                        ),
                                    }
                                }
                            });
                            (Some(discovery), Some(peer_rx))
                        }
                        Err(e) => {
                            eprintln!("mDNS browse failed: {e}");
                            (Some(discovery), None)
                        }
                    },
                    Err(e) => {
                        eprintln!("mDNS discovery disabled: {e}");
                        (None, None)
                    }
                }
            } else {
                (None, None)
            };

            #[cfg(not(feature = "mdns"))]
            let (discovery_handle, discovery_rx): (
                Option<()>,
                Option<tokio::sync::mpsc::UnboundedReceiver<PeerConfig>>,
            ) = {
                if config.discovery.enabled {
                    eprintln!(
                        "discovery.enabled is set but this build lacks the 'mdns' feature; \
                         continuing without discovery."
                    );
                }
                (None, None)
            };

            // Outbound sync loop: static peers plus any discovered at runtime.
            let sync_handle = if config.peers.is_empty() && discovery_rx.is_none() {
                None
            } else {
                let connector = build_client_connector(config, data_dir)?;
                let opts = crate::sync_loop::SyncLoopOptions {
                    max_message_size: config.network.max_message_size,
                    max_ops_per_response: config.replication.max_ops_per_response,
                    connect_timeout: Duration::from_secs(config.network.connect_timeout_sec),
                    idle_timeout: Duration::from_secs(config.network.idle_timeout_sec),
                    interval: Duration::from_secs(config.replication.sync_interval_sec),
                    backoff_max: Duration::from_secs(config.replication.reconnect_backoff_max_sec),
                };
                let pool = storage.pool().clone();
                let node_id = identity.node_id.clone();
                let node_name = config.node.node_name.clone();
                Some(match discovery_rx {
                    Some(rx) => crate::sync_loop::spawn_sync_loop_with_updates(
                        pool,
                        connector,
                        node_id,
                        node_name,
                        config.peers.clone(),
                        opts,
                        rx,
                    ),
                    None => crate::sync_loop::spawn_sync_loop(
                        pool,
                        connector,
                        node_id,
                        node_name,
                        config.peers.clone(),
                        opts,
                    ),
                })
            };

            println!(
                "SyncFleet daemon listening on {local_addr} (node '{}', {}).",
                config.node.node_name,
                storage_peer_summary(&storage).await?
            );
            if config.discovery.enabled {
                println!(
                    "mDNS discovery active (service '{}').",
                    config.discovery.service_name
                );
            }
            match &sync_handle {
                Some(h) if h.peer_count() > 0 => {
                    println!("Syncing with {} peer(s).", h.peer_count())
                }
                Some(_) => println!("Sync loop running; discovering peers."),
                None => println!("No peers configured; serving inbound connections only."),
            }
            println!("Press Ctrl-C to stop.");

            tokio::signal::ctrl_c().await?;
            println!("Shutting down.");
            if let Some(sync_handle) = sync_handle {
                sync_handle.shutdown().await;
            }
            #[cfg(feature = "mdns")]
            if let Some(discovery) = discovery_handle {
                let _ = discovery.shutdown();
            }
            #[cfg(not(feature = "mdns"))]
            let _ = discovery_handle;
            handle.shutdown().await;
            storage.close().await;
            Ok(())
        }
        DaemonAction::Stop => stop_daemon(config).await,
        DaemonAction::Restart => {
            match stop_daemon(config).await {
                Ok(()) => {}
                Err(SyncFleetError::NotFound(_)) => {}
                Err(error) => return Err(error),
            }
            spawn_background_daemon(config)
        }
    }
}

async fn storage_peer_summary(storage: &Storage) -> Result<String> {
    let peers = storage::peers::list_peers(storage.pool()).await?;
    let datasets = storage::datasets::list_datasets(storage.pool()).await?;
    Ok(format!(
        "{} dataset(s), {} peer authorization(s)",
        datasets.len(),
        peers.len()
    ))
}

async fn cmd_doctor(config: &SyncFleetConfig, args: DoctorArgs) -> Result<()> {
    let data_dir = &config.node.data_dir;
    let mut checks: Vec<(String, bool, String)> = Vec::new();

    let initialized = NodeIdentity::exists(data_dir);
    checks.push((
        "node identity".to_string(),
        initialized,
        if initialized {
            "present".to_string()
        } else {
            format!("missing at {}", data_dir.display())
        },
    ));

    checks.push((
        "config validation".to_string(),
        config.validate().is_ok(),
        match config.validate() {
            Ok(()) => "ok".to_string(),
            Err(e) => e.to_string(),
        },
    ));

    if initialized {
        match Storage::open(data_dir, &open_options(config)).await {
            Ok(storage) => {
                let health = storage.health_check().await;
                checks.push((
                    "database".to_string(),
                    health.is_ok(),
                    health
                        .map(|_| "ok".to_string())
                        .unwrap_or_else(|e| e.to_string()),
                ));

                if args.full {
                    match storage.integrity_check().await {
                        Ok(v) => checks.push(("integrity_check".to_string(), v == "ok", v)),
                        Err(e) => {
                            checks.push(("integrity_check".to_string(), false, e.to_string()))
                        }
                    }

                    match storage::operations::journal_stats(storage.pool()).await {
                        Ok(s) => checks.push((
                            "journal".to_string(),
                            true,
                            format!(
                                "{} operations, {} applied",
                                s.operations, s.applied_operations
                            ),
                        )),
                        Err(e) => checks.push(("journal".to_string(), false, e.to_string())),
                    }
                }

                if args.repair {
                    let checkpoint = storage.checkpoint_wal().await;
                    let checkpoint_ok = checkpoint.is_ok();
                    match &checkpoint {
                        Ok(()) => {
                            checks.push(("wal_checkpoint".to_string(), true, "ok".to_string()))
                        }
                        Err(e) => checks.push(("wal_checkpoint".to_string(), false, e.to_string())),
                    }

                    // Remove journal entries that are old and fully acked by peers.
                    if checkpoint_ok {
                        let cutoff = chrono::Utc::now()
                            - chrono::Duration::days(config.storage.journal_retention_days as i64);
                        match storage::peers::list_peers(storage.pool()).await {
                            Ok(peers) => {
                                let protected: Vec<(String, String)> = peers
                                    .into_iter()
                                    .filter(|p| p.revoked_at.is_none())
                                    .map(|p| (p.peer_id, p.dataset_id))
                                    .collect();
                                match storage::operations::compact_journal(
                                    storage.pool(),
                                    cutoff,
                                    &protected,
                                )
                                .await
                                {
                                    Ok(n) => checks.push((
                                        "compaction".to_string(),
                                        true,
                                        format!("{n} entries removed"),
                                    )),
                                    Err(e) => checks.push((
                                        "compaction".to_string(),
                                        false,
                                        e.to_string(),
                                    )),
                                }
                            }
                            Err(e) => checks.push(("compaction".to_string(), false, e.to_string())),
                        }
                    }
                }
                storage.close().await;
            }
            Err(e) => checks.push(("database".to_string(), false, e.to_string())),
        }
    }

    let all_ok = checks.iter().all(|(_, ok, _)| *ok);

    if args.format.as_deref() == Some("json") {
        let json: Vec<_> = checks
            .iter()
            .map(|(name, ok, detail)| serde_json::json!({ "check": name, "ok": ok, "detail": detail }))
            .collect();
        println!("{}", serde_json::to_string_pretty(&json)?);
    } else {
        for (name, ok, detail) in &checks {
            println!("[{}] {}: {}", if *ok { "ok" } else { "FAIL" }, name, detail);
        }
    }

    if !all_ok {
        return Err(SyncFleetError::Internal(
            "one or more diagnostics failed".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod daemon_pid_tests {
    use super::*;

    #[test]
    fn pid_file_round_trips_and_clears() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = SyncFleetConfig::default();
        config.daemon.pid_file = dir.path().join("syncfleet.pid");

        assert_eq!(read_daemon_pid(&config), None);
        write_daemon_pid(&config, 4242).unwrap();
        assert_eq!(read_daemon_pid(&config), Some(4242));
        clear_daemon_pid(&config);
        assert_eq!(read_daemon_pid(&config), None);
    }
}
