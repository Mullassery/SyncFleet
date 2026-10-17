use crate::error::{IdentityError, Result};
use aes_gcm::aead::{generic_array::GenericArray, Aead};
use aes_gcm::{aead::OsRng, Aes256Gcm, KeyInit};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use x25519_dalek::{PublicKey as X25519Public, StaticSecret as X25519StaticSecret};

const KEY_FILE: &str = "node_identity.bin";
const PASSPHRASE_SALT: &[u8] = b"syncfleet-passphrase-salt-v1";

/// Iteration count for the PBKDF2-HMAC-SHA256 passphrase KDF.
///
/// Matches OWASP's 2023 recommendation for PBKDF2-HMAC-SHA256 (600,000
/// iterations). Derivation only happens on identity create/unlock, so the
/// cost is paid rarely; the point is to make offline brute-forcing of a
/// passphrase expensive, which a plain (work-factor-free) KDF like HKDF
/// does not.
const PASSPHRASE_KDF_ITERATIONS: u32 = 600_000;

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

    /// Derive the AES-256-GCM key that protects the on-disk private keys
    /// from a passphrase.
    ///
    /// This is deliberately a *password* KDF (PBKDF2-HMAC-SHA256 with a high
    /// iteration count), not a plain keyed hash like HKDF: HKDF has no work
    /// factor, so an attacker who steals `node_identity.bin` can test
    /// passphrase guesses at hashing speed. PBKDF2 adds the tunable cost that
    /// makes offline brute force orders of magnitude slower.
    ///
    /// NOTE: this changes the derived key versus the previous HKDF scheme, so
    /// identity files written by older versions (passphrase-protected ones)
    /// must be regenerated. The crate is pre-1.0.
    fn derive_key(passphrase: &str) -> Result<[u8; 32]> {
        let mut key = [0u8; 32];
        pbkdf2::pbkdf2_hmac::<Sha256>(
            passphrase.as_bytes(),
            PASSPHRASE_SALT,
            PASSPHRASE_KDF_ITERATIONS,
            &mut key,
        );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passphrase_kdf_is_deterministic_and_passphrase_sensitive() {
        let a = NodeIdentity::derive_key("correct horse battery staple").unwrap();
        let b = NodeIdentity::derive_key("correct horse battery staple").unwrap();
        assert_eq!(a, b, "same passphrase must derive the same key");

        let c = NodeIdentity::derive_key("a different passphrase").unwrap();
        assert_ne!(a, c, "different passphrases must derive different keys");
    }

    #[test]
    fn passphrase_kdf_has_a_real_work_factor() {
        // Guards against someone silently swapping this back to a
        // work-factor-free KDF (e.g. HKDF), which was the original defect.
        assert!(
            PASSPHRASE_KDF_ITERATIONS >= 600_000,
            "PBKDF2 iteration count must stay at/above the OWASP 2023 guidance"
        );
    }

    #[test]
    fn passphrase_kdf_matches_pbkdf2_hmac_sha256_known_answer() {
        // RFC 7914 §11 PBKDF2-HMAC-SHA256 vector (P="passwd", S="salt",
        // c=1, dkLen=64). Pins the primitive actually in use to a
        // known-answer rather than merely checking it round-trips.
        let mut out = [0u8; 64];
        pbkdf2::pbkdf2_hmac::<Sha256>(b"passwd", b"salt", 1, &mut out);
        assert_eq!(
            out,
            [
                0x55, 0xac, 0x04, 0x6e, 0x56, 0xe3, 0x08, 0x9f, 0xec, 0x16, 0x91, 0xc2, 0x25,
                0x44, 0xb6, 0x05, 0xf9, 0x41, 0x85, 0x21, 0x6d, 0xde, 0x04, 0x65, 0xe6, 0x8b,
                0x9d, 0x57, 0xc2, 0x0d, 0xac, 0xbc, 0x49, 0xca, 0x9c, 0xcc, 0xf1, 0x79, 0xb6,
                0x45, 0x99, 0x16, 0x64, 0xb3, 0x9d, 0x77, 0xef, 0x31, 0x7c, 0x71, 0xb8, 0x45,
                0xb1, 0xe3, 0x0b, 0xd5, 0x09, 0x11, 0x20, 0x41, 0xd3, 0xa1, 0x97, 0x83,
            ]
        );
    }

    #[test]
    fn passphrase_protected_identity_round_trips_and_rejects_wrong_passphrase() {
        let identity = NodeIdentity::generate(Some("s3cret pass")).unwrap();

        let signing_key = identity.ed25519_signing_key(Some("s3cret pass")).unwrap();
        assert_eq!(
            signing_key.verifying_key().to_bytes().to_vec(),
            identity.ed25519_public,
            "recovered signing key must match the stored public key"
        );

        assert!(
            identity.ed25519_signing_key(Some("wrong passphrase")).is_err(),
            "a wrong passphrase must fail to decrypt"
        );
    }
}
