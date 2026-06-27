//! AES-256-GCM encryption support

use crate::error::{Result, SecurityError};
use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use argon2::{
    password_hash::SaltString, Argon2, ParamString, PasswordHasher,
};
use rand::Rng;
use serde::{Deserialize, Serialize};

/// Encryption key (256-bit)
#[derive(Clone, Serialize, Deserialize)]
pub struct EncryptionKey {
    /// Key material (32 bytes for AES-256)
    pub key_material: Vec<u8>,
    /// Key derivation salt
    pub salt: Vec<u8>,
    /// Key creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl EncryptionKey {
    /// Generate new encryption key from password
    pub fn derive_from_password(password: &str) -> Result<Self> {
        let salt = SaltString::generate(rand::thread_rng());
        let argon2 = Argon2::default();

        let hash = argon2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| SecurityError::KeyDerivationError(e.to_string()))?;

        let hash_bytes = hash.hash.ok_or_else(|| {
            SecurityError::KeyDerivationError("Hash generation failed".to_string())
        })?;

        let mut key_material = vec![0u8; 32];
        let hash_str = hash_bytes.to_string();
        let bytes = hash_str.as_bytes();

        for (i, chunk) in bytes.chunks(32).enumerate().take(1) {
            for (j, byte) in chunk.iter().enumerate() {
                if i * 32 + j < 32 {
                    key_material[i * 32 + j] = *byte;
                }
            }
        }

        Ok(EncryptionKey {
            key_material,
            salt: salt.to_string().into_bytes(),
            created_at: chrono::Utc::now(),
        })
    }

    /// Generate random encryption key
    pub fn generate_random() -> Result<Self> {
        let mut rng = rand::thread_rng();
        let mut key_material = vec![0u8; 32];
        rng.fill(&mut key_material[..]);

        let mut salt = vec![0u8; 16];
        rng.fill(&mut salt[..]);

        Ok(EncryptionKey {
            key_material,
            salt,
            created_at: chrono::Utc::now(),
        })
    }
}

/// Encryption cipher trait
#[async_trait::async_trait]
pub trait EncryptionCipher: Send + Sync {
    /// Encrypt plaintext
    async fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>>;

    /// Decrypt ciphertext
    async fn decrypt(&self, ciphertext: &[u8]) -> Result<Vec<u8>>;

    /// Encrypt with associated data
    async fn encrypt_with_aad(&self, plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>>;

    /// Decrypt with associated data
    async fn decrypt_with_aad(&self, ciphertext: &[u8], aad: &[u8]) -> Result<Vec<u8>>;
}

/// AES-256-GCM cipher implementation
pub struct AesGcmCipher {
    key: EncryptionKey,
    cipher: Aes256Gcm,
}

impl AesGcmCipher {
    /// Create new AES-GCM cipher
    pub fn new(key: EncryptionKey) -> Result<Self> {
        if key.key_material.len() != 32 {
            return Err(SecurityError::InvalidKey(
                "Key must be 32 bytes".to_string(),
            ));
        }

        let cipher = Aes256Gcm::new_from_slice(&key.key_material)
            .map_err(|_| SecurityError::InvalidKey("Invalid key material".to_string()))?
            .into();

        Ok(AesGcmCipher { key, cipher })
    }

    /// Get key reference
    pub fn key(&self) -> &EncryptionKey {
        &self.key
    }
}

#[async_trait::async_trait]
impl EncryptionCipher for AesGcmCipher {
    async fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let mut rng = rand::thread_rng();
        let mut nonce_bytes = [0u8; 12];
        rng.fill(&mut nonce_bytes[..]);

        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = self
            .cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| SecurityError::EncryptionError(e.to_string()))?;

        let mut result = nonce_bytes.to_vec();
        result.extend_from_slice(&ciphertext);
        Ok(result)
    }

    async fn decrypt(&self, ciphertext: &[u8]) -> Result<Vec<u8>> {
        if ciphertext.len() < 12 {
            return Err(SecurityError::DecryptionError("Ciphertext too short".to_string()));
        }

        let nonce = Nonce::from_slice(&ciphertext[..12]);
        let encrypted_data = &ciphertext[12..];

        self.cipher
            .decrypt(nonce, encrypted_data)
            .map_err(|e| SecurityError::DecryptionError(e.to_string()))
    }

    async fn encrypt_with_aad(&self, plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
        let mut rng = rand::thread_rng();
        let mut nonce_bytes = [0u8; 12];
        rng.fill(&mut nonce_bytes[..]);

        let nonce = Nonce::from_slice(&nonce_bytes);
        let payload = Payload {
            msg: plaintext,
            aad,
        };

        let ciphertext = self
            .cipher
            .encrypt(nonce, payload)
            .map_err(|e| SecurityError::EncryptionError(e.to_string()))?;

        let mut result = nonce_bytes.to_vec();
        result.extend_from_slice(&ciphertext);
        Ok(result)
    }

    async fn decrypt_with_aad(&self, ciphertext: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
        if ciphertext.len() < 12 {
            return Err(SecurityError::DecryptionError("Ciphertext too short".to_string()));
        }

        let nonce = Nonce::from_slice(&ciphertext[..12]);
        let encrypted_data = &ciphertext[12..];
        let payload = Payload {
            msg: encrypted_data,
            aad,
        };

        self.cipher
            .decrypt(nonce, payload)
            .map_err(|e| SecurityError::DecryptionError(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encryption_key_random() {
        let key = EncryptionKey::generate_random().unwrap();
        assert_eq!(key.key_material.len(), 32);
        assert_eq!(key.salt.len(), 16);
    }

    #[test]
    fn test_encryption_key_from_password() {
        let key = EncryptionKey::derive_from_password("test_password").unwrap();
        assert_eq!(key.key_material.len(), 32);
        assert!(!key.salt.is_empty());
    }

    #[tokio::test]
    async fn test_aes_gcm_encrypt_decrypt() {
        let key = EncryptionKey::generate_random().unwrap();
        let cipher = AesGcmCipher::new(key).unwrap();

        let plaintext = b"Hello, World!";
        let ciphertext = cipher.encrypt(plaintext).await.unwrap();

        assert_ne!(ciphertext, plaintext);

        let decrypted = cipher.decrypt(&ciphertext).await.unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[tokio::test]
    async fn test_aes_gcm_with_aad() {
        let key = EncryptionKey::generate_random().unwrap();
        let cipher = AesGcmCipher::new(key).unwrap();

        let plaintext = b"Secret data";
        let aad = b"public metadata";

        let ciphertext = cipher
            .encrypt_with_aad(plaintext, aad)
            .await
            .unwrap();

        let decrypted = cipher
            .decrypt_with_aad(&ciphertext, aad)
            .await
            .unwrap();

        assert_eq!(decrypted, plaintext);
    }
}
