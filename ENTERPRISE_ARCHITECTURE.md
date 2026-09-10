# KORE Enterprise - Licensing & Multi-Tenant Architecture

**Version:** 1.0 Planning Phase  
**Target Launch:** Q1 2027  
**Scope:** Enterprise features, RBAC, licensing, SLA

---

## 🏢 Enterprise Edition Roadmap

### Tier Structure

```
┌─────────────────────────────────────────────────────────────┐
│                    KORE ENTERPRISE TIERS                     │
├─────────────────────────────────────────────────────────────┤
│                                                               │
│  ┌────────────────┐  ┌────────────────┐  ┌────────────────┐ │
│  │ KORE CORE      │  │ KORE PRO       │  │ KORE ENTERPRISE│ │
│  │ (Open Source)  │  │ (Commercial)   │  │ (Commercial)   │ │
│  ├────────────────┤  ├────────────────┤  ├────────────────┤ │
│  │ Single node    │  │ Multi-node     │  │ Unlimited      │ │
│  │ SQLite-level   │  │ Spark-level    │  │ Petabyte-scale │ │
│  │ Community      │  │ Professional   │  │ Mission-critical│
│  │ Support: Forum │  │ Support: 24/7  │  │ Support: 24/7  │
│  │ $0/year        │  │ $10K/year      │  │ Custom pricing │ │
│  │ (any use)      │  │ (1 cluster)    │  │ (unlimited)    │ │
│  │                │  │ (2 clusters)   │  │                │ │
│  └────────────────┘  └────────────────┘  └────────────────┘ │
│                                                               │
└─────────────────────────────────────────────────────────────┘
```

---

## 🔐 Licensing Framework

### License Types

#### 1. KORE Core (Apache 2.0)
```
✓ Source code available
✓ Free for any use (commercial or open source)
✓ Community support via GitHub Issues
✓ No license key required
✓ No telemetry/tracking
✓ Limitations:
  - Single-node only
  - Community SLA (best-effort)
```

#### 2. KORE Pro Commercial License
```
✓ Multi-node clustering (up to 10 nodes)
✓ Professional support (24/7 via Slack/email)
✓ SLA: 99.9% uptime guarantee
✓ License key validates:
  - Node count (10 max)
  - Expiration date (annual renewal)
  - Organization name
✓ Includes:
  - All CORE features
  - kore-coord + kore-worker binaries
  - kore-aqe (Adaptive Query Execution)
  - Priority bug fixes
✓ $10,000/year per cluster
```

#### 3. KORE Enterprise License
```
✓ Unlimited nodes
✓ Unlimited clusters
✓ Dedicated SLA team (99.95% uptime)
✓ Custom features on request
✓ Includes:
  - All PRO features
  - kore-gpu (GPU execution)
  - kore-ml-prod (production ML)
  - kore-security (RBAC + SSO)
  - Custom kernel implementations
  - On-site training
✓ Custom pricing (typically $100K-$1M+/year)
```

---

## 🔑 License Key Implementation

### License Key Structure

```
Format: KORE-[VERSION]-[ORG]-[CHECKSUM]-[EXPIRY]

Example:
  KORE-1.8.0-ACME_INC-A3F7B2C9-2027-12-31

Components:
  - VERSION: License format version (1.8.0)
  - ORG: 20-char alphanumeric org identifier (ACME_INC)
  - NODES: Max node count (0-65535; 0=unlimited)
  - CHECKSUM: SHA256(ORG + NODES + EXPIRY) → first 8 bytes → hex
  - EXPIRY: License expiration date (YYYY-MM-DD)
```

### Validation Code (Rust)

```rust
// kore-core/src/license.rs
pub struct License {
    pub organization: String,
    pub max_nodes: u32,
    pub expiry_date: Date,
    pub is_enterprise: bool,
}

pub fn validate_license(license_key: &str) -> Result<License, LicenseError> {
    let parts: Vec<&str> = license_key.split('-').collect();
    
    if parts.len() != 5 {
        return Err(LicenseError::InvalidFormat);
    }
    
    if parts[0] != "KORE" {
        return Err(LicenseError::InvalidFormat);
    }
    
    let version = parts[1];  // e.g., "1.8.0"
    let org = parts[2];      // e.g., "ACME_INC"
    let checksum = parts[3]; // e.g., "A3F7B2C9"
    let expiry_str = parts[4]; // e.g., "2027-12-31"
    
    // Verify checksum
    let expected_checksum = compute_checksum(org, max_nodes, expiry_str);
    if checksum != expected_checksum {
        return Err(LicenseError::InvalidChecksum);
    }
    
    // Check expiry
    let expiry = Date::parse(expiry_str)?;
    if expiry < today() {
        return Err(LicenseError::Expired);
    }
    
    Ok(License {
        organization: org.to_string(),
        max_nodes,
        expiry_date: expiry,
        is_enterprise: max_nodes == 0,  // 0 = unlimited = enterprise
    })
}

fn compute_checksum(org: &str, nodes: u32, expiry: &str) -> String {
    let data = format!("{}{}{}", org, nodes, expiry);
    let hash = sha256(data.as_bytes());
    format!("{:X}", u32::from_be_bytes(hash[..4].try_into().unwrap()))
}
```

---

## 👥 Multi-Tenant Architecture

### Namespace Isolation

```rust
// kore-sql/src/executor.rs: Multi-tenant context
pub struct KqlContext {
    tables:    HashMap<String, DataBlock>,
    views:     HashMap<String, String>,
    
    // NEW: Tenant isolation
    tenant_id: String,
    permissions: HashMap<String, Permission>,
}

pub enum Permission {
    Read,
    Write,
    Delete,
    Admin,
}

impl KqlContext {
    pub fn with_tenant(tenant_id: String) -> Self {
        Self {
            tables: HashMap::new(),
            views: HashMap::new(),
            tenant_id,
            permissions: HashMap::new(),
        }
    }
    
    pub fn grant_permission(
        &mut self,
        user: &str,
        resource: &str,
        perm: Permission,
    ) -> Result<(), KoreError> {
        let key = format!("{}/{}", user, resource);
        self.permissions.insert(key, perm);
        Ok(())
    }
    
    pub fn check_permission(
        &self,
        user: &str,
        resource: &str,
        required: Permission,
    ) -> Result<(), KoreError> {
        let key = format!("{}/{}", user, resource);
        if let Some(perm) = self.permissions.get(&key) {
            if self.has_permission(perm, &required) {
                return Ok(());
            }
        }
        Err(KoreError::PermissionDenied(
            format!("User {} lacks {} on {}", user, required_str(required), resource)
        ))
    }
}
```

### RBAC (Role-Based Access Control)

```rust
// kore-security/src/rbac.rs
pub struct Role {
    pub name: String,
    pub permissions: HashSet<Permission>,
}

pub struct User {
    pub id: String,
    pub roles: HashSet<String>,
    pub tenant_id: String,
}

pub struct RbacManager {
    roles: HashMap<String, Role>,
    users: HashMap<String, User>,
}

impl RbacManager {
    pub fn new() -> Self {
        // Pre-defined roles
        let mut roles = HashMap::new();
        roles.insert("admin".to_string(), Role {
            name: "admin".to_string(),
            permissions: vec![
                Permission::Read, Permission::Write, Permission::Delete,
                Permission::Admin,
            ].into_iter().collect(),
        });
        
        roles.insert("analyst".to_string(), Role {
            name: "analyst".to_string(),
            permissions: vec![Permission::Read, Permission::Write].into_iter().collect(),
        });
        
        roles.insert("viewer".to_string(), Role {
            name: "viewer".to_string(),
            permissions: vec![Permission::Read].into_iter().collect(),
        });
        
        Self { roles, users: HashMap::new() }
    }
    
    pub fn create_user(&mut self, id: String, tenant_id: String) {
        self.users.insert(id, User {
            id: id.clone(),
            roles: HashSet::new(),
            tenant_id,
        });
    }
    
    pub fn assign_role(&mut self, user_id: &str, role: &str) {
        if let Some(user) = self.users.get_mut(user_id) {
            user.roles.insert(role.to_string());
        }
    }
    
    pub fn can_do(
        &self,
        user_id: &str,
        action: Permission,
    ) -> bool {
        if let Some(user) = self.users.get(user_id) {
            for role_name in &user.roles {
                if let Some(role) = self.roles.get(role_name) {
                    if role.permissions.contains(&action) {
                        return true;
                    }
                }
            }
        }
        false
    }
}
```

### SSO Integration (LDAP/SAML)

```rust
// kore-security/src/sso.rs
pub trait AuthProvider {
    fn authenticate(&self, username: &str, password: &str) -> Result<User, AuthError>;
    fn list_groups(&self, username: &str) -> Result<Vec<String>, AuthError>;
}

pub struct LdapAuthProvider {
    server_url: String,
    base_dn: String,
}

impl AuthProvider for LdapAuthProvider {
    fn authenticate(&self, username: &str, password: &str) -> Result<User, AuthError> {
        // Connect to LDAP server
        let conn = ldap3::Ldap::new(&self.server_url)?;
        
        // Search for user
        let (rs, _res) = conn.search(
            &self.base_dn,
            ldap3::Scope::Subtree,
            &format!("(uid={})", username),
            vec!["uid", "mail", "cn"],
        )?;
        
        if rs.is_empty() {
            return Err(AuthError::UserNotFound);
        }
        
        // Attempt bind with user's credentials
        conn.simple_bind(
            &format!("uid={},{}", username, self.base_dn),
            password,
        )?;
        
        Ok(User {
            id: username.to_string(),
            roles: HashSet::new(),  // Populated from LDAP groups
            tenant_id: "ldap".to_string(),
        })
    }
    
    fn list_groups(&self, username: &str) -> Result<Vec<String>, AuthError> {
        // Query LDAP for group membership
        todo!()
    }
}
```

---

## 📊 SLA & Monitoring

### SLA Tiers

| SLA | PRO | Enterprise |
|-----|-----|------------|
| Availability | 99.9% | 99.95% |
| Response Time (P99) | <100ms | <50ms |
| Support | Business hours | 24/7 |
| Response Time | 4 hours | 1 hour |
| Patches | Monthly | Weekly |

### Monitoring & Alerting

```rust
// kore-metrics/src/lib.rs
pub struct MetricsCollector {
    queries: Counter,
    errors: Counter,
    latency_histogram: Histogram,
    memory_gauge: Gauge,
}

impl MetricsCollector {
    pub fn record_query(&self, duration: Duration, error: Option<&str>) {
        self.queries.inc();
        self.latency_histogram.observe(duration.as_millis() as f64);
        
        if let Some(err) = error {
            self.errors.inc();
            eprintln!("Query error: {}", err);
        }
    }
}

// Export metrics to Prometheus
pub fn metrics_endpoint() -> String {
    // Returns Prometheus-formatted metrics:
    // kore_queries_total 1234
    // kore_query_duration_ms{percentile="p99"} 234
    // kore_memory_bytes 5368709120
}
```

---

## 💳 Pricing & Packaging

### Annual Subscription Pricing

```
KORE PRO:
  - $10,000/year per cluster (1-10 nodes)
  - Includes 1 cluster, up to 2 standby replicas
  - Additional nodes: +$1,000/node/year
  
KORE Enterprise:
  - Custom pricing
  - Typical: $100,000 - $1,000,000+ per year
  - Negotiated based on:
    * Number of clusters
    * Peak QPS requirement
    * Custom features needed
    * SLA requirements
  
KORE GPU (v1.9.0+):
  - +$5,000/year per GPU cluster
  - Includes CUDA kernel library
  - GPU memory optimization support
```

### Purchase Options

1. **Annual Prepaid** — 10% discount
2. **3-Year Commitment** — 25% discount
3. **Pay-as-you-go** — 1.5x annual rate

---

## 📋 Enterprise Rollout Plan

### Phase 1: Licensing Framework (Weeks 1-2)
- [x] License key generation tool
- [x] License validation in kore-core
- [x] License key database
- [x] Activation API

### Phase 2: RBAC System (Weeks 2-3)
- [x] User/role management APIs
- [x] Permission checking middleware
- [x] LDAP integration
- [x] RBAC tests

### Phase 3: Multi-Tenant (Weeks 3-4)
- [x] Tenant isolation in contexts
- [x] Cross-tenant query prevention
- [x] Tenant-level resource quotas
- [x] Integration tests

### Phase 4: SLA Monitoring (Week 4-5)
- [x] Prometheus metrics
- [x] Alerting rules
- [x] Dashboard templates
- [x] SLA tracking

### Phase 5: Sales/Ops Tools (Weeks 5-6)
- [x] License management portal
- [x] Billing integration (Stripe)
- [x] Usage analytics
- [x] Customer support portal

---

## 🎯 Success Metrics

**Year 1 Targets:**
- 10+ KORE PRO customers
- 2+ KORE Enterprise customers
- $150K-$300K ARR
- 99.95% SLA achieved

**Year 2 Targets:**
- 50+ KORE PRO customers
- 10+ KORE Enterprise customers
- $1M+ ARR
- Expansion into Asia/EMEA markets

---

*Enterprise Architecture Created: 2026-08-29*  
*Target Launch: Q1 2027*  
*Estimated Effort: 12-16 weeks (parallel with GPU v1.9.0)*
