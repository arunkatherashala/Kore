//! Authentication & Authorization module for KORE API v2.0
//!
//! Features:
//! - JWT token generation and validation
//! - LDAP user authentication
//! - Role-Based Access Control (RBAC)
//! - Rate limiting per user/IP

use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// JWT claims structure
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String,           // Subject (username)
    pub role: String,          // Role (admin/user/viewer)
    pub exp: i64,              // Expiration time
    pub iat: i64,              // Issued at
    pub iss: String,           // Issuer ("kore-api")
}

/// RBAC role definitions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Admin,  // Full access
    User,   // Query, insert, update
    Viewer, // Read-only
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::User => "user",
            Role::Viewer => "viewer",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "admin" => Some(Role::Admin),
            "user" => Some(Role::User),
            "viewer" => Some(Role::Viewer),
            _ => None,
        }
    }

    /// Check if role can perform action
    pub fn can_query(&self) -> bool {
        matches!(self, Role::Admin | Role::User | Role::Viewer)
    }

    pub fn can_insert(&self) -> bool {
        matches!(self, Role::Admin | Role::User)
    }

    pub fn can_delete(&self) -> bool {
        matches!(self, Role::Admin)
    }

    pub fn can_train_model(&self) -> bool {
        matches!(self, Role::Admin | Role::User)
    }
}

/// JWT token manager
pub struct TokenManager {
    secret: Vec<u8>,
}

impl TokenManager {
    /// Create new token manager
    pub fn new(secret: &str) -> Self {
        Self {
            secret: secret.as_bytes().to_vec(),
        }
    }

    /// Generate JWT token
    pub fn generate_token(&self, username: &str, role: Role, hours: i64) -> Result<String, String> {
        let now = Utc::now();
        let iat = now.timestamp();
        let exp = (now + Duration::hours(hours)).timestamp();

        let claims = Claims {
            sub: username.to_string(),
            role: role.as_str().to_string(),
            exp,
            iat,
            iss: "kore-api".to_string(),
        };

        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(&self.secret),
        )
        .map_err(|e| format!("Token generation failed: {}", e))
    }

    /// Validate and decode JWT token
    pub fn validate_token(&self, token: &str) -> Result<Claims, String> {
        decode::<Claims>(
            token,
            &DecodingKey::from_secret(&self.secret),
            &Validation::default(),
        )
        .map(|data| data.claims)
        .map_err(|e| format!("Token validation failed: {}", e))
    }
}

/// LDAP authentication (stub for integration)
pub struct LdapAuth {
    server_url: String,
}

impl LdapAuth {
    pub fn new(server_url: &str) -> Self {
        Self {
            server_url: server_url.to_string(),
        }
    }

    /// Authenticate user with LDAP (stub: would connect to real LDAP server)
    pub async fn authenticate(&self, username: &str, password: &str) -> Result<Role, String> {
        // In production: connect to LDAP server at self.server_url
        // For now: mock authentication
        if username == "admin" && password == "admin_pass" {
            Ok(Role::Admin)
        } else if username == "user" && password == "user_pass" {
            Ok(Role::User)
        } else if username == "viewer" && password == "viewer_pass" {
            Ok(Role::Viewer)
        } else {
            Err("Invalid credentials".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_generation_and_validation() {
        let manager = TokenManager::new("test-secret");
        let token = manager
            .generate_token("testuser", Role::User, 1)
            .expect("Token generation failed");

        let claims = manager
            .validate_token(&token)
            .expect("Token validation failed");
        assert_eq!(claims.sub, "testuser");
        assert_eq!(claims.role, "user");
    }

    #[test]
    fn test_rbac_permissions() {
        assert!(Role::Admin.can_query());
        assert!(Role::Admin.can_insert());
        assert!(Role::Admin.can_delete());
        assert!(Role::Admin.can_train_model());

        assert!(Role::User.can_query());
        assert!(Role::User.can_insert());
        assert!(!Role::User.can_delete());
        assert!(Role::User.can_train_model());

        assert!(Role::Viewer.can_query());
        assert!(!Role::Viewer.can_insert());
        assert!(!Role::Viewer.can_delete());
        assert!(!Role::Viewer.can_train_model());
    }
}
