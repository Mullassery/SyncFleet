use crate::error::{ConfigError, Result, SyncFleetError};
use config::{Config, File};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NodeConfig {
    pub node_name: String,
    pub data_dir: PathBuf,
    pub log_level: String,
    pub log_format: String,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            node_name: "syncfleet-node".to_string(),
            data_dir: default_data_dir(),
            log_level: "info".to_string(),
            log_format: "text".to_string(),
        }
    }
}

pub fn default_data_dir() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".local/share/syncfleet");
    }
    PathBuf::from("/var/lib/syncfleet")
}

pub fn default_config_dir() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".config/syncfleet");
    }
    PathBuf::from("/etc/syncfleet")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NetworkConfig {
    pub listen_addr: String,
    pub advertise_addr: String,
    pub tls_cert: PathBuf,
    pub tls_key: PathBuf,
    pub ca_cert: PathBuf,
    pub min_tls_version: String,
    pub connect_timeout_sec: u64,
    pub idle_timeout_sec: u64,
    pub max_message_size: usize,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            listen_addr: "0.0.0.0:8443".to_string(),
            advertise_addr: "auto".to_string(),
            // Empty paths resolve to the node's data dir (see
            // `SyncFleetConfig::tls_cert_path`), keeping per-node TLS material
            // isolated even when several nodes share a configuration file.
            tls_cert: PathBuf::new(),
            tls_key: PathBuf::new(),
            ca_cert: PathBuf::new(),
            min_tls_version: "1.3".to_string(),
            connect_timeout_sec: 30,
            idle_timeout_sec: 300,
            max_message_size: 1048576,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DiscoveryConfig {
    pub enabled: bool,
    pub interface: Option<String>,
    pub port: u16,
    pub service_name: String,
    pub ttl_sec: u32,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interface: None,
            port: 5353,
            service_name: "_syncfleet._tcp".to_string(),
            ttl_sec: 120,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StorageConfig {
    pub journal_retention_days: u32,
    pub journal_max_size_gb: u64,
    pub snapshot_interval_hours: u32,
    pub compaction_interval_hours: u32,
    pub wal_mode: bool,
    pub busy_timeout_ms: u32,
    pub integrity_check_on_startup: bool,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            journal_retention_days: 30,
            journal_max_size_gb: 1,
            snapshot_interval_hours: 24,
            compaction_interval_hours: 6,
            wal_mode: true,
            busy_timeout_ms: 5000,
            integrity_check_on_startup: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ReplicationConfig {
    pub max_concurrent_peers: usize,
    pub max_in_flight: usize,
    pub max_ops_per_response: usize,
    pub sync_interval_sec: u64,
    pub initial_sync_timeout_sec: u64,
    pub snapshot_bootstrap_enabled: bool,
    pub max_snapshot_size_mb: u64,
}

impl Default for ReplicationConfig {
    fn default() -> Self {
        Self {
            max_concurrent_peers: 32,
            max_in_flight: 32,
            max_ops_per_response: 1000,
            sync_interval_sec: 30,
            initial_sync_timeout_sec: 60,
            snapshot_bootstrap_enabled: true,
            max_snapshot_size_mb: 500,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SecurityConfig {
    pub enrollment_token_expiry_hours: u32,
    pub require_enrollment_token: bool,
    pub peer_cert_validity_days: u32,
    pub auto_renew_peer_certs: bool,
    pub renewal_window_days: u32,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            enrollment_token_expiry_hours: 24,
            require_enrollment_token: true,
            peer_cert_validity_days: 90,
            auto_renew_peer_certs: true,
            renewal_window_days: 14,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MetricsConfig {
    pub enabled: bool,
    pub listen_addr: String,
    pub path: String,
    pub histograms_enabled: bool,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            listen_addr: "127.0.0.1:9090".to_string(),
            path: "/metrics".to_string(),
            histograms_enabled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DaemonConfig {
    pub background: bool,
    pub pid_file: PathBuf,
    pub user: Option<String>,
    pub group: Option<String>,
    pub shutdown_timeout_sec: u64,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            background: false,
            pid_file: default_data_dir().join("syncfleet.pid"),
            user: None,
            group: None,
            shutdown_timeout_sec: 30,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SyncFleetConfig {
    pub node: NodeConfig,
    pub network: NetworkConfig,
    pub discovery: DiscoveryConfig,
    pub storage: StorageConfig,
    pub replication: ReplicationConfig,
    pub security: SecurityConfig,
    pub metrics: MetricsConfig,
    pub daemon: DaemonConfig,
}

impl SyncFleetConfig {
    pub fn load(config_path: Option<&Path>) -> Result<Self> {
        let mut builder = Config::builder().add_source(
            File::from_str(
                include_str!("../../config/syncfleet.example.toml"),
                config::FileFormat::Toml,
            )
            .required(false),
        );

        if let Some(path) = config_path {
            builder = builder.add_source(File::from(path).required(true));
        } else {
            let config_dir = default_config_dir();
            let candidates = [
                config_dir.join("config.toml"),
                PathBuf::from("/etc/syncfleet/config.toml"),
                PathBuf::from("/usr/local/etc/syncfleet/config.toml"),
            ];
            for p in candidates {
                if p.exists() {
                    builder = builder.add_source(File::from(p).required(false));
                    break;
                }
            }
        }

        builder = builder.add_source(
            config::Environment::with_prefix("SYNCFLEET")
                .prefix_separator("_")
                .separator("__"),
        );

        let config = builder
            .build()
            .map_err(|e| ConfigError::Parse(e.to_string()))?;
        let cfg: SyncFleetConfig = config
            .try_deserialize()
            .map_err(|e| ConfigError::Parse(e.to_string()))?;

        cfg.validate()?;
        Ok(cfg)
    }

    /// Validate configuration that must hold for every command.
    pub fn validate(&self) -> Result<()> {
        if self.node.node_name.is_empty() {
            return Err(ConfigError::MissingField("node.node_name".to_string()).into());
        }

        if self.network.listen_addr.is_empty() {
            return Err(ConfigError::MissingField("network.listen_addr".to_string()).into());
        }

        if self.discovery.enabled && self.discovery.port == 0 {
            return Err(ConfigError::InvalidValue {
                field: "discovery.port".to_string(),
                reason: "must be > 0".to_string(),
            }
            .into());
        }

        if self.storage.journal_retention_days == 0 {
            return Err(ConfigError::InvalidValue {
                field: "storage.journal_retention_days".to_string(),
                reason: "must be > 0".to_string(),
            }
            .into());
        }

        if self.replication.max_concurrent_peers == 0 {
            return Err(ConfigError::InvalidValue {
                field: "replication.max_concurrent_peers".to_string(),
                reason: "must be > 0".to_string(),
            }
            .into());
        }

        Ok(())
    }

    /// Validate items required only when the node serves network traffic.
    pub fn validate_for_network(&self) -> Result<()> {
        if !self.network.tls_cert.exists() {
            return Err(ConfigError::Validation(format!(
                "TLS cert not found: {:?}",
                self.network.tls_cert
            ))
            .into());
        }

        if !self.network.tls_key.exists() {
            return Err(ConfigError::Validation(format!(
                "TLS key not found: {:?}",
                self.network.tls_key
            ))
            .into());
        }

        if !self.network.ca_cert.exists() {
            return Err(ConfigError::Validation(format!(
                "CA cert not found: {:?}",
                self.network.ca_cert
            ))
            .into());
        }

        Ok(())
    }

    pub fn to_toml(&self) -> Result<String> {
        toml::to_string_pretty(self).map_err(SyncFleetError::from)
    }

    /// Certificates live per-node under the data directory unless explicitly
    /// configured via `network.tls_cert` / `network.tls_key` / `network.ca_cert`.
    pub fn tls_cert_path(&self, data_dir: &Path) -> PathBuf {
        if self.network.tls_cert.as_os_str().is_empty() {
            data_dir.join("certs/node.pem")
        } else {
            self.network.tls_cert.clone()
        }
    }

    pub fn tls_key_path(&self, data_dir: &Path) -> PathBuf {
        if self.network.tls_key.as_os_str().is_empty() {
            data_dir.join("certs/node-key.pem")
        } else {
            self.network.tls_key.clone()
        }
    }

    pub fn ca_cert_path(&self, data_dir: &Path) -> PathBuf {
        if self.network.ca_cert.as_os_str().is_empty() {
            data_dir.join("certs/ca.pem")
        } else {
            self.network.ca_cert.clone()
        }
    }
}
