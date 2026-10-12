use crate::error::{IdentityError, Result};
use aes_gcm::aead::{generic_array::GenericArray, Aead};
use aes_gcm::{aead::OsRng, Aes256Gcm, KeyInit};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use hkdf::Hkdf;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use x25519_dalek::{PublicKey as X25519Public, StaticSecret as X25519StaticSecret};

const KEY_FILE: &str = "node_identity.bin";
const PASSPHRASE_SALT: &[u8] = b"syncfleet-passphrase-salt-v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeIdentity {
    pub node_id: String,
    pub ed25519_public: Vec<u8>,
    pub ed25519_private_encrypted: Vec<u8>,
    pub x25519_public: Vec<u8>,
    pub x25519_private_encrypted: Vec<u8>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl NodeIdentity {
    pub fn generate(passphrase: Option<&str>) -> Result<Self> {
        let mut csprng = OsRng;

        let mut ed25519_seed = [0u8; 32];
        csprng.fill_bytes(&mut ed25519_seed);
        let ed25519_signing = SigningKey::from_bytes(&ed25519_seed);
        let ed25519_verifying = ed25519_signing.verifying_key();

        let mut x25519_seed = [0u8; 32];
        csprng.fill_bytes(&mut x25519_seed);
        let x25519_static = X25519StaticSecret::from(x25519_seed);
        let x25519_public = X25519Public::from(&x25519_static);

        let node_id = Self::derive_node_id(&ed25519_verifying);

        let (ed25519_private_encrypted, x25519_private_encrypted) = if let Some(pass) = passphrase {
            let key = Self::derive_key(pass)?;
            let ed = Self::encrypt_with_key(&key, ed25519_signing.to_bytes().as_ref(), "Ed25519")?;
            let x = Self::encrypt_with_key(&key, x25519_seed.as_ref(), "X25519")?;
            (ed, x)
        } else {
            (ed25519_signing.to_bytes().to_vec(), x25519_seed.to_vec())
        };

        Ok(Self {
            node_id,
            ed25519_public: ed25519_verifying.to_bytes().to_vec(),
            ed25519_private_encrypted,
            x25519_public: x25519_public.as_bytes().to_vec(),
            x25519_private_encrypted,
            created_at: chrono::Utc::now(),
        })
    }

    fn derive_node_id(verifying_key: &VerifyingKey) -> String {
        let mut hasher = Sha256::new();
        hasher.update(verifying_key.to_bytes());
        let hash = hasher.finalize();
        hex::encode(&hash[..16])
    }

    fn derive_key(passphrase: &str) -> Result<[u8; 32]> {
        let hkdf = Hkdf::<Sha256>::new(Some(PASSPHRASE_SALT), passphrase.as_bytes());
        let mut key = [0u8; 32];
        hkdf.expand(b"syncfleet-node-key", &mut key)
            .map_err(|e| IdentityError::KeyGeneration(e.to_string()))?;
        Ok(key)
    }

    fn random_nonce() -> GenericArray<u8, typenum::U12> {
        let mut nonce = GenericArray::default();
        OsRng.fill_bytes(&mut nonce);
        nonce
    }

    /// Encrypt `plaintext` with AES-256-GCM, storing the random 12-byte nonce
    /// prepended to the ciphertext so it can be recovered on decryption.
    fn encrypt_with_key(key: &[u8; 32], plaintext: &[u8], label: &str) -> Result<Vec<u8>> {
        let cipher = Aes256Gcm::new(GenericArray::from_slice(key));
        let nonce = Self::random_nonce();
        let ciphertext = cipher
            .encrypt(&nonce, plaintext)
            .map_err(|e| IdentityError::EncryptionFailed(format!("{label}: {e}")))?;

        let mut out = Vec::with_capacity(12 + ciphertext.len());
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ciphertext);
        Ok(out)
    }

    /// Reverse of [`Self::encrypt_with_key`].
    fn decrypt_with_key(key: &[u8; 32], blob: &[u8], label: &str) -> Result<Vec<u8>> {
        if blob.len() < 12 {
            return Err(IdentityError::DecryptionFailed(format!("{label}: truncated blob")).into());
        }
        let (nonce_bytes, ciphertext) = blob.split_at(12);
        let cipher = Aes256Gcm::new(GenericArray::from_slice(key));
        cipher
            .decrypt(GenericArray::from_slice(nonce_bytes), ciphertext)
            .map_err(|e| IdentityError::DecryptionFailed(format!("{label}: {e}")).into())
    }

    pub fn save(&self, data_dir: &Path) -> Result<()> {
        fs::create_dir_all(data_dir)?;
        let path = data_dir.join(KEY_FILE);
        let data = bincode::serialize(self)?;
        fs::write(&path, data)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    pub fn load(data_dir: &Path) -> Result<Self> {
        let path = data_dir.join(KEY_FILE);
        let data = fs::read(&path).map_err(|e| IdentityError::KeyNotFound(e.to_string()))?;
        let identity: NodeIdentity = bincode::deserialize(&data)?;
        Ok(identity)
    }

    pub fn ed25519_signing_key(&self, passphrase: Option<&str>) -> Result<SigningKey> {
        let key_bytes = if let Some(pass) = passphrase {
            let key = Self::derive_key(pass)?;
            Self::decrypt_with_key(&key, &self.ed25519_private_encrypted, "Ed25519")?
        } else {
            self.ed25519_private_encrypted.clone()
        };

        if key_bytes.len() != 32 {
            return Err(IdentityError::InvalidFormat(
                "Invalid Ed25519 private key length".to_string(),
            )
            .into());
        }

        let mut array = [0u8; 32];
        array.copy_from_slice(&key_bytes);
        Ok(SigningKey::from_bytes(&array))
    }

    pub fn x25519_static_secret(&self, passphrase: Option<&str>) -> Result<X25519StaticSecret> {
        let key_bytes = if let Some(pass) = passphrase {
            let key = Self::derive_key(pass)?;
            Self::decrypt_with_key(&key, &self.x25519_private_encrypted, "X25519")?
        } else {
            self.x25519_private_encrypted.clone()
        };

        if key_bytes.len() != 32 {
            return Err(IdentityError::InvalidFormat(
                "Invalid X25519 private key length".to_string(),
            )
            .into());
        }

        let mut array = [0u8; 32];
        array.copy_from_slice(&key_bytes);
        Ok(X25519StaticSecret::from(array))
    }

    pub fn ed25519_verifying_key(&self) -> Result<VerifyingKey> {
        if self.ed25519_public.len() != 32 {
            return Err(IdentityError::InvalidFormat(
                "Invalid Ed25519 public key length".to_string(),
            )
            .into());
        }
        let mut array = [0u8; 32];
        array.copy_from_slice(&self.ed25519_public);
        Ok(VerifyingKey::from_bytes(&array)?)
    }

    pub fn x25519_public_key(&self) -> Result<X25519Public> {
        if self.x25519_public.len() != 32 {
            return Err(IdentityError::InvalidFormat(
                "Invalid X25519 public key length".to_string(),
            )
            .into());
        }
        let mut array = [0u8; 32];
        array.copy_from_slice(&self.x25519_public);
        Ok(X25519Public::from(array))
    }

    pub fn sign(&self, message: &[u8], passphrase: Option<&str>) -> Result<Signature> {
        let signing_key = self.ed25519_signing_key(passphrase)?;
        Ok(signing_key.sign(message))
    }

    pub fn verify(&self, message: &[u8], signature: &Signature) -> Result<()> {
        let verifying_key = self.ed25519_verifying_key()?;
        verifying_key.verify(message, signature)?;
        Ok(())
    }

    pub fn exists(data_dir: &Path) -> bool {
        data_dir.join(KEY_FILE).exists()
    }

    /// Generate a short-lived, single-use enrollment token for a new peer.
    pub fn enrollment_token() -> EnrollmentToken {
        EnrollmentToken::generate()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollmentToken {
    pub token: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub used: bool,
}

impl EnrollmentToken {
    pub fn generate() -> Self {
        let mut token_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut token_bytes);
        Self {
            token: hex::encode(token_bytes),
            expires_at: chrono::Utc::now() + chrono::Duration::hours(24),
            used: false,
        }
    }

    pub fn is_valid(&self) -> bool {
        !self.used && chrono::Utc::now() < self.expires_at
    }

    pub fn mark_used(&mut self) {
        self.used = true;
    }
}
