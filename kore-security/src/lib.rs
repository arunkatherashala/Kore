//! KORE Layer 58 — Security: Auth, RBAC, TLS Configuration
//!
//! Production security for KORE clusters:
//!
//! - **Token authentication** — API key validation for REST API + worker registration
//! - **RBAC** — Role-based access: Reader, Writer, Admin, Worker
//! - **TLS configuration** — certificate paths for TLS (actual TLS via rustls in production)
//! - **Audit logging** — log all auth events with timestamps
//! - **Token rotation** — issue/revoke tokens without restart
//! - **IP allowlisting** — restrict worker registration to trusted IPs

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use serde::{Deserialize, Serialize};

// ─── Roles ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Role {
    /// Can run SELECT queries, read tables.
    Reader,
    /// Can run SELECT, INSERT, UPDATE, DELETE.
    Writer,
    /// Full access: all DML + admin operations.
    Admin,
    /// Can register as a worker and receive tasks.
    Worker,
    /// Can submit distributed jobs.
    JobSubmitter,
}

impl Role {
    /// Returns all permissions granted by this role.
    pub fn permissions(&self) -> Vec<Permission> {
        match self {
            Role::Reader       => vec![Permission::Select],
            Role::Writer       => vec![Permission::Select, Permission::Insert, Permission::Update, Permission::Delete],
            Role::Admin        => Permission::all(),
            Role::Worker       => vec![Permission::RegisterWorker, Permission::ExecuteTask],
            Role::JobSubmitter => vec![Permission::Select, Permission::SubmitJob],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Permission {
    Select, Insert, Update, Delete,
    CreateTable, DropTable,
    RegisterWorker, ExecuteTask,
    SubmitJob,
    Admin,
}

impl Permission {
    pub fn all() -> Vec<Self> {
        vec![
            Self::Select, Self::Insert, Self::Update, Self::Delete,
            Self::CreateTable, Self::DropTable, Self::RegisterWorker,
            Self::ExecuteTask, Self::SubmitJob, Self::Admin,
        ]
    }
}

// ─── Token ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Token {
    pub id:          String,
    pub secret:      String,         // The actual token string (bcrypt-hashed in production)
    pub owner:       String,
    pub roles:       Vec<Role>,
    pub created_at:  u64,
    pub expires_at:  Option<u64>,    // None = never expires
    pub revoked:     bool,
    pub allowed_ips: Vec<String>,    // empty = all IPs allowed
}

impl Token {
    pub fn is_valid(&self) -> bool {
        if self.revoked { return false; }
        if let Some(exp) = self.expires_at {
            if now_ms() > exp { return false; }
        }
        true
    }

    pub fn has_permission(&self, perm: &Permission) -> bool {
        if self.revoked { return false; }
        self.roles.iter().any(|r| r.permissions().contains(perm))
    }

    pub fn has_role(&self, role: &Role) -> bool {
        self.roles.contains(role)
    }

    pub fn can_access_from(&self, ip: &str) -> bool {
        self.allowed_ips.is_empty() || self.allowed_ips.iter().any(|a| a == ip || a == "*")
    }
}

// ─── Auth result ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum AuthResult {
    Allowed { owner: String, roles: Vec<Role> },
    Denied  { reason: String },
}

impl AuthResult {
    pub fn is_allowed(&self) -> bool { matches!(self, Self::Allowed { .. }) }
}

// ─── Audit log ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub timestamp:  u64,
    pub token_id:   String,
    pub owner:      String,
    pub action:     String,
    pub resource:   String,
    pub allowed:    bool,
    pub source_ip:  String,
}

// ─── Security Manager ─────────────────────────────────────────────────────────

pub struct SecurityManager {
    tokens:     Mutex<HashMap<String, Token>>,   // secret → Token
    audit_log:  Mutex<Vec<AuditEntry>>,
    tls_config: Mutex<Option<TlsConfig>>,
}

impl SecurityManager {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            tokens:     Mutex::new(HashMap::new()),
            audit_log:  Mutex::new(Vec::new()),
            tls_config: Mutex::new(None),
        })
    }

    // ── Token management ──────────────────────────────────────────────────────

    /// Issue a new API token.
    pub fn issue_token(
        &self,
        owner:       &str,
        roles:       Vec<Role>,
        ttl_hours:   Option<u64>,
        allowed_ips: Vec<String>,
    ) -> Token {
        let secret     = generate_token();
        let expires_at = ttl_hours.map(|h| now_ms() + h * 3_600_000);
        let token      = Token {
            id:          format!("kore_{}", &secret[..8]),
            secret:      secret.clone(),
            owner:       owner.to_string(),
            roles,
            created_at:  now_ms(),
            expires_at,
            revoked:     false,
            allowed_ips,
        };
        self.tokens.lock().unwrap().insert(secret, token.clone());
        token
    }

    /// Revoke a token immediately.
    pub fn revoke(&self, secret: &str) -> bool {
        let mut tokens = self.tokens.lock().unwrap();
        if let Some(t) = tokens.get_mut(secret) {
            t.revoked = true;
            true
        } else { false }
    }

    /// List all active tokens.
    pub fn list_tokens(&self) -> Vec<Token> {
        self.tokens.lock().unwrap().values()
            .filter(|t| t.is_valid())
            .cloned()
            .collect()
    }

    // ── Authentication ────────────────────────────────────────────────────────

    /// Authenticate a request by token secret + required permission.
    pub fn authenticate(
        &self,
        secret:     &str,
        permission: &Permission,
        source_ip:  &str,
        resource:   &str,
    ) -> AuthResult {
        let tokens  = self.tokens.lock().unwrap();
        let result = match tokens.get(secret) {
            None => AuthResult::Denied { reason: "unknown token".into() },
            Some(t) if !t.is_valid() => AuthResult::Denied { reason: if t.revoked { "token revoked".into() } else { "token expired".into() } },
            Some(t) if !t.can_access_from(source_ip) => AuthResult::Denied { reason: format!("IP {source_ip} not allowed") },
            Some(t) if !t.has_permission(permission) => AuthResult::Denied { reason: format!("missing permission: {permission:?}") },
            Some(t) => AuthResult::Allowed { owner: t.owner.clone(), roles: t.roles.clone() },
        };

        // Audit log
        let (owner, allowed) = match &result {
            AuthResult::Allowed { owner, .. } => (owner.clone(), true),
            AuthResult::Denied  { .. }        => ("anonymous".into(), false),
        };
        self.audit_log.lock().unwrap().push(AuditEntry {
            timestamp: now_ms(), token_id: secret[..8.min(secret.len())].to_string(),
            owner, action: format!("{permission:?}"), resource: resource.to_string(),
            allowed, source_ip: source_ip.to_string(),
        });

        result
    }

    /// Simple token validation (returns the token if valid).
    pub fn validate(&self, secret: &str) -> Option<Token> {
        let tokens = self.tokens.lock().unwrap();
        tokens.get(secret).filter(|t| t.is_valid()).cloned()
    }

    // ── Audit log ─────────────────────────────────────────────────────────────

    pub fn audit_tail(&self, n: usize) -> Vec<AuditEntry> {
        let log = self.audit_log.lock().unwrap();
        log.iter().rev().take(n).cloned().collect()
    }

    pub fn audit_failures(&self) -> Vec<AuditEntry> {
        self.audit_log.lock().unwrap().iter().filter(|e| !e.allowed).cloned().collect()
    }

    // ── TLS ───────────────────────────────────────────────────────────────────

    pub fn configure_tls(&self, config: TlsConfig) {
        *self.tls_config.lock().unwrap() = Some(config);
    }

    pub fn tls_config(&self) -> Option<TlsConfig> {
        self.tls_config.lock().unwrap().clone()
    }

    pub fn tls_enabled(&self) -> bool { self.tls_config.lock().unwrap().is_some() }
}

impl Default for SecurityManager {
    fn default() -> Self {
        Self {
            tokens:     Mutex::new(HashMap::new()),
            audit_log:  Mutex::new(Vec::new()),
            tls_config: Mutex::new(None),
        }
    }
}

// ─── TLS configuration ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsConfig {
    pub cert_path:    String,   // Path to PEM certificate
    pub key_path:     String,   // Path to PEM private key
    pub ca_cert_path: Option<String>,  // CA cert for mTLS (mutual TLS)
    pub verify_client: bool,    // Require client certificate (mTLS)
    pub min_tls_version: String,  // "1.2" or "1.3"
}

impl TlsConfig {
    pub fn server_tls(cert: &str, key: &str) -> Self {
        Self { cert_path: cert.into(), key_path: key.into(), ca_cert_path: None, verify_client: false, min_tls_version: "1.3".into() }
    }

    pub fn mutual_tls(cert: &str, key: &str, ca: &str) -> Self {
        Self { cert_path: cert.into(), key_path: key.into(), ca_cert_path: Some(ca.into()), verify_client: true, min_tls_version: "1.3".into() }
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn generate_token() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().subsec_nanos();
    let r: u64 = t as u64 ^ (t as u64).wrapping_mul(0x9e3779b97f4a7c15);
    format!("kore_{:016x}{:016x}", r, r.wrapping_mul(6364136223846793005))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_issue_validate() {
        let mgr = SecurityManager::new();
        let token = mgr.issue_token("alice", vec![Role::Reader], None, vec![]);
        assert!(token.is_valid());
        assert!(token.has_permission(&Permission::Select));
        assert!(!token.has_permission(&Permission::Admin));
        assert!(mgr.validate(&token.secret).is_some());
    }

    #[test]
    fn test_token_revocation() {
        let mgr = SecurityManager::new();
        let token = mgr.issue_token("bob", vec![Role::Writer], None, vec![]);
        assert!(mgr.validate(&token.secret).is_some());
        mgr.revoke(&token.secret);
        assert!(mgr.validate(&token.secret).is_none());
    }

    #[test]
    fn test_rbac_permissions() {
        let mgr = SecurityManager::new();
        let admin  = mgr.issue_token("admin", vec![Role::Admin],  None, vec![]);
        let reader = mgr.issue_token("r",     vec![Role::Reader], None, vec![]);

        assert!(mgr.authenticate(&admin.secret,  &Permission::Admin,  "127.0.0.1", "table1").is_allowed());
        assert!(mgr.authenticate(&reader.secret, &Permission::Select, "127.0.0.1", "table1").is_allowed());
        assert!(!mgr.authenticate(&reader.secret, &Permission::Delete, "127.0.0.1", "table1").is_allowed());
    }

    #[test]
    fn test_ip_allowlist() {
        let mgr = SecurityManager::new();
        let token = mgr.issue_token("worker", vec![Role::Worker], None, vec!["10.0.0.1".into()]);
        assert!(mgr.authenticate(&token.secret, &Permission::RegisterWorker, "10.0.0.1", "cluster").is_allowed());
        assert!(!mgr.authenticate(&token.secret, &Permission::RegisterWorker, "1.2.3.4", "cluster").is_allowed());
    }

    #[test]
    fn test_audit_log() {
        let mgr = SecurityManager::new();
        let token = mgr.issue_token("user", vec![Role::Reader], None, vec![]);
        mgr.authenticate(&token.secret, &Permission::Select, "127.0.0.1", "t1");
        mgr.authenticate("bad_token", &Permission::Select, "127.0.0.1", "t1");
        let failures = mgr.audit_failures();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].allowed, false);
    }

    #[test]
    fn test_tls_config() {
        let mgr = SecurityManager::new();
        assert!(!mgr.tls_enabled());
        mgr.configure_tls(TlsConfig::server_tls("/certs/server.crt", "/certs/server.key"));
        assert!(mgr.tls_enabled());
        let cfg = mgr.tls_config().unwrap();
        assert_eq!(cfg.min_tls_version, "1.3");
    }
}
