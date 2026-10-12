use crate::config::SyncFleetConfig;
use crate::error::{Result, SyncFleetError};
use crate::identity::NodeIdentity;
use crate::storage::{self, Storage};
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

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
    /// Force synchronization now
    Now { dataset_id: Option<String> },
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

    println!("Initialized SyncFleet node");
    println!("  Node name:  {}", args.node);
    println!("  Node ID:    {}", identity.node_id);
    println!("  Data dir:   {}", data_dir.display());
    println!("  Ed25519:    {}", hex::encode(&identity.ed25519_public));
    println!("  X25519:     {}", hex::encode(&identity.x25519_public));

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
                let checkpoint =
                    storage::checkpoints::get_checkpoint(storage.pool(), "self", &ds.dataset_id)
                        .await?;
                match checkpoint {
                    Some(cp) => println!(
                        "{}\tlast_acked={}\tlast_applied={}\tupdated={}",
                        ds.dataset_id, cp.last_acked_seq, cp.last_applied_seq, cp.updated_at
                    ),
                    None => println!("{}\tno checkpoint yet", ds.dataset_id),
                }
            }
            Ok(())
        }
        SyncAction::Now { dataset_id: _ } => {
            // The replication engine (Phase 3) drives sync over the network. Until
            // it is wired in, operate as a single node: report local state.
            let datasets = storage::datasets::list_datasets(storage.pool()).await?;
            println!(
                "Local-only mode: {} dataset(s) registered; no peers connected.",
                datasets.len()
            );
            println!("Network replication is not implemented yet (see docs/ROADMAP.md).");
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

async fn cmd_daemon(config: &SyncFleetConfig, args: DaemonArgs) -> Result<()> {
    match args.action {
        DaemonAction::Start { background: _ } => {
            // Network serving requires TLS material; surface a clear error.
            config.validate_for_network()?;
            Err(SyncFleetError::Internal(
                "Daemon networking is not implemented yet (see docs/ROADMAP.md).".to_string(),
            ))
        }
        DaemonAction::Stop => Err(SyncFleetError::Internal(
            "Daemon control is not implemented yet (see docs/ROADMAP.md).".to_string(),
        )),
        DaemonAction::Restart => Err(SyncFleetError::Internal(
            "Daemon control is not implemented yet (see docs/ROADMAP.md).".to_string(),
        )),
    }
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
