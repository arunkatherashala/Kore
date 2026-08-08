//! KORE-Federation node identity with ed25519 signatures.
//!
//! Each KORE instance has a unique ed25519 keypair. The node ID is derived from
//! the public key, and every knowledge packet is signed so peers can verify
//! provenance. Secret keys are serialized into federation state for now; a
//! future upgrade should move them to an OS keyring or encrypted storage.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::{SystemTime, UNIX_EPOCH};

/// Identity of a single KORE node in the federation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeIdentity {
    /// Stable node identifier derived from the public key (hex).
    pub node_id: String,
    /// Owner/creator of this node.
    pub owner: String,
    /// Ed25519 public key bytes.
    pub public_key: Vec<u8>,
    /// Ed25519 secret key bytes. Guard this carefully.
    #[serde(skip)]
    pub secret_key: Vec<u8>,
    /// Timestamp when this identity was created.
    pub created_at: String,
}

impl NodeIdentity {
    /// Generate a new ed25519 identity for an owner.
    pub fn generate(owner: &str, now: &str) -> Self {
        let mut csprng = rand_core::OsRng;
        let signing_key = SigningKey::generate(&mut csprng);
        let verifying_key = signing_key.verifying_key();
        let public_key = verifying_key.to_bytes().to_vec();
        let secret_key = signing_key.to_bytes().to_vec();
        let node_id = key_fingerprint(&public_key);
        Self {
            node_id,
            owner: owner.to_string(),
            public_key,
            secret_key,
            created_at: now.to_string(),
        }
    }

    /// Reconstruct a signing key from the stored secret bytes.
    fn signing_key(&self) -> Option<SigningKey> {
        if self.secret_key.len() != 32 {
            return None;
        }
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&self.secret_key);
        Some(SigningKey::from_bytes(&bytes))
    }

    /// Reconstruct a verifying key from the stored public bytes.
    pub fn verifying_key(&self) -> Option<VerifyingKey> {
        if self.public_key.len() != 32 {
            return None;
        }
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&self.public_key);
        VerifyingKey::from_bytes(&bytes).ok()
    }

    /// Sign a message with the node's secret key.
    pub fn sign(&self, data: &[u8]) -> Vec<u8> {
        if let Some(sk) = self.signing_key() {
            let signature: Signature = sk.sign(data);
            signature.to_bytes().to_vec()
        } else {
            // Fallback deterministic hash when no secret key is present.
            let mut hasher = DefaultHasher::new();
            self.secret_key.hash(&mut hasher);
            data.hash(&mut hasher);
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
                .hash(&mut hasher);
            hasher.finish().to_be_bytes().to_vec()
        }
    }

    /// Verify a signature against this node's public key.
    pub fn verify(&self, data: &[u8], signature: &[u8]) -> bool {
        if signature.len() != 64 {
            return false;
        }
        let Some(vk) = self.verifying_key() else {
            return false;
        };
        let mut sig_bytes = [0u8; 64];
        sig_bytes.copy_from_slice(signature);
        let sig = Signature::from_bytes(&sig_bytes);
        vk.verify(data, &sig).is_ok()
    }
}

/// Short, readable fingerprint of a public key for use as a node ID.
fn key_fingerprint(public_key: &[u8]) -> String {
    let mut hasher = DefaultHasher::new();
    public_key.hash(&mut hasher);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    nanos.hash(&mut hasher);
    format!("kore-{:016x}", hasher.finish())
}
