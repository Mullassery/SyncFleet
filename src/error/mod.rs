use thiserror::Error;

#[derive(Error, Debug)]
pub enum SyncFleetError {
    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),

    #[error("Identity error: {0}")]
    Identity(#[from] IdentityError),

    #[error("Storage error: {0}")]
    Storage(#[from] StorageError),

    #[error("Replication error: {0}")]
    Replication(#[from] ReplicationError),

    #[error("Transport error: {0}")]
    Transport(#[from] TransportError),

    #[error("Discovery error: {0}")]
    Discovery(#[from] DiscoveryError),

    #[error("Daemon error: {0}")]
    Daemon(#[from] DaemonError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("TOML parse error: {0}")]
    TomlParse(#[from] toml::de::Error),

    #[error("TOML serialize error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),

    #[error("SQL error: {0}")]
    Sql(#[from] sqlx::Error),

    #[error("Migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),

    #[error("UUID error: {0}")]
    Uuid(#[from] uuid::Error),

    #[error("Time parse error: {0}")]
    TimeParse(#[from] chrono::ParseError),

    #[error("Ed25519 error: {0}")]
    Ed25519(#[from] ed25519_dalek::ed25519::Error),

    #[error("Bincode error: {0}")]
    Bincode(#[from] Box<bincode::ErrorKind>),

    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Already exists: {0}")]
    AlreadyExists(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Config file not found: {0}")]
    NotFound(String),

    #[error("Config parse error: {0}")]
    Parse(String),

    #[error("Config validation error: {0}")]
    Validation(String),

    #[error("Missing required field: {0}")]
    MissingField(String),

    #[error("Invalid value for {field}: {reason}")]
    InvalidValue { field: String, reason: String },
}

#[derive(Error, Debug)]
pub enum IdentityError {
    #[error("Key generation failed: {0}")]
    KeyGeneration(String),

    #[error("Key not found: {0}")]
    KeyNotFound(String),

    #[error("Invalid key format: {0}")]
    InvalidFormat(String),

    #[error("Encryption failed: {0}")]
    EncryptionFailed(String),

    #[error("Decryption failed: {0}")]
    DecryptionFailed(String),

    #[error("Invalid passphrase")]
    InvalidPassphrase,

    #[error("Node ID mismatch")]
    NodeIdMismatch,
}

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Migration error: {0}")]
    Migration(String),

    #[error("Journal error: {0}")]
    Journal(String),

    #[error("Checkpoint error: {0}")]
    Checkpoint(String),

    #[error("Integrity check failed: {0}")]
    IntegrityCheckFailed(String),

    #[error("Operation not found: {0}")]
    OperationNotFound(String),

    #[error("Duplicate operation: {0}")]
    DuplicateOperation(String),

    #[error("Schema mismatch: expected {expected}, got {actual}")]
    SchemaMismatch { expected: u32, actual: u32 },
}

#[derive(Error, Debug)]
pub enum ReplicationError {
    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Handshake failed: {0}")]
    HandshakeFailed(String),

    #[error("Sync failed: {0}")]
    SyncFailed(String),

    #[error("Operation validation failed: {0}")]
    ValidationFailed(String),

    #[error("Conflict detected: {0}")]
    Conflict(String),

    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Peer not authorized for dataset: {0}")]
    PeerNotAuthorized(String),

    #[error("Sequence gap detected: expected {expected}, got {actual}")]
    SequenceGap { expected: u64, actual: u64 },

    #[error("Checkpoint mismatch")]
    CheckpointMismatch,
}

#[derive(Error, Debug)]
pub enum TransportError {
    #[error("TLS error: {0}")]
    Tls(#[from] rustls::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Connection error: {0}")]
    Connection(String),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Certificate error: {0}")]
    Certificate(String),

    #[error("Certificate not found: {0}")]
    CertificateNotFound(String),

    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    #[error("Enrollment token invalid: {0}")]
    EnrollmentInvalid(String),

    #[error("Enrollment token expired")]
    EnrollmentExpired,

    #[error("Enrollment token already used")]
    EnrollmentReused,

    #[error("Peer certificate missing")]
    MissingPeerCertificate,

    #[error("Peer certificate fingerprint mismatch: expected {expected}, got {actual}")]
    FingerprintMismatch { expected: String, actual: String },

    #[error("Peer not authorized: {0}")]
    PeerRejected(String),

    #[error("Message too large: {size} > {limit}")]
    MessageTooLarge { size: usize, limit: usize },

    #[error("Timeout")]
    Timeout,

    #[error("Peer disconnected")]
    Disconnected,
}

#[derive(Error, Debug)]
pub enum DiscoveryError {
    #[error("mDNS error: {0}")]
    Mdns(#[from] mdns_sd::Error),

    #[error("Service registration failed: {0}")]
    RegistrationFailed(String),

    #[error("Discovery disabled")]
    Disabled,
}

#[derive(Error, Debug)]
pub enum DaemonError {
    #[error("Already running")]
    AlreadyRunning,

    #[error("Not running")]
    NotRunning,

    #[error("PID file error: {0}")]
    PidFile(String),

    #[error("Privilege drop failed: {0}")]
    PrivilegeDrop(String),

    #[error("Shutdown timeout")]
    ShutdownTimeout,
}

pub type Result<T> = std::result::Result<T, SyncFleetError>;
