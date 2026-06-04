# KORE v1.3.3 Security Configuration Guide

**Last Updated:** June 3, 2026  
**Status:** Production Ready  
**Version:** v1.0

---

## 📋 Table of Contents

1. [Encryption Setup](#encryption-setup)
2. [Authentication & Authorization](#authentication--authorization)
3. [Network Security](#network-security)
4. [File Permissions](#file-permissions)
5. [Audit Logging](#audit-logging)
6. [Security Best Practices](#security-best-practices)

---

## Encryption Setup

### Enable AES-256 Encryption

**Environment Variable:**
```bash
export KORE_ENCRYPTION_KEY="$(openssl rand -hex 32)"
export KORE_ENCRYPTION_ENABLED=true
```

**PowerShell:**
```powershell
$env:KORE_ENCRYPTION_KEY = (openssl rand -hex 32)
$env:KORE_ENCRYPTION_ENABLED = "true"
```

**Configuration File (config.yaml):**
```yaml
security:
  encryption:
    enabled: true
    algorithm: "aes-256-ctr"
    key_source: "environment"  # or "keyfile"
    key_rotation_days: 90
```

### Key Management

**Store Master Key Securely:**

```bash
# Option 1: Environment Variable (development)
export KORE_ENCRYPTION_KEY="your-256-bit-hex-key"

# Option 2: Key File (production)
echo "your-256-bit-hex-key" > /etc/kore/master.key
chmod 600 /etc/kore/master.key

# Option 3: Secrets Manager (cloud)
aws secretsmanager create-secret --name kore/master-key --secret-string "..."
```

**Key Rotation:**

```bash
#!/bin/bash
# rotate-keys.sh

OLD_KEY=$KORE_ENCRYPTION_KEY
NEW_KEY=$(openssl rand -hex 32)

# Re-encrypt all files with new key
kore --command reencrypt --old-key "$OLD_KEY" --new-key "$NEW_KEY"

# Update environment
export KORE_ENCRYPTION_KEY="$NEW_KEY"
```

---

## Authentication & Authorization

### API Token Management

**Generate Token:**
```bash
# Generate secure random token
openssl rand -hex 32

# Store in secure location
echo "token123abc456def..." > ~/.kore/token
chmod 600 ~/.kore/token
```

**Use Token in Requests:**
```bash
# In header
curl -H "Authorization: Bearer token123abc..." http://localhost:8000/api/files

# In environment
export KORE_API_TOKEN="token123abc..."
curl -H "Authorization: Bearer $KORE_API_TOKEN" http://localhost:8000/api/files
```

### User Roles (v1.7.0 Plan)

**Current (v1.3.3):** Single admin role

**Planned Roles:**
```yaml
roles:
  admin:
    permissions: ["read", "write", "delete", "admin"]
    
  analyst:
    permissions: ["read"]
    
  editor:
    permissions: ["read", "write"]
```

**Configuration:**
```yaml
users:
  - name: alice@example.com
    role: admin
    api_token: token_alice_...
    
  - name: bob@example.com
    role: analyst
    api_token: token_bob_...
```

---

## Network Security

### TLS/SSL Setup

**Generate Self-Signed Certificate:**
```bash
# Generate private key and certificate (valid 365 days)
openssl req -x509 -newkey rsa:4096 -keyout server.key -out server.crt -days 365 -nodes

# Or use Let's Encrypt (production)
certbot certonly --standalone -d kore.example.com
```

**Configure KORE to Use TLS:**

```yaml
api:
  host: "0.0.0.0"
  port: 8443
  tls:
    enabled: true
    cert_path: "/etc/kore/server.crt"
    key_path: "/etc/kore/server.key"
```

**Connect via HTTPS:**
```bash
# Allow self-signed certs (development only)
curl -k https://localhost:8443/api/health

# Production with valid cert
curl https://kore.example.com/api/health
```

### Firewall Rules

**Recommended Firewall Configuration:**

```bash
# Allow KORE API port only from trusted IPs
sudo ufw allow from 10.0.0.0/8 to any port 8000 comment "KORE API - Internal only"
sudo ufw allow from 192.168.1.0/24 to any port 8000 comment "KORE API - Office network"

# Deny all other access
sudo ufw deny 8000/tcp
```

**Kubernetes NetworkPolicy:**
```yaml
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: kore-network-policy
spec:
  podSelector:
    matchLabels:
      app: kore
  policyTypes:
  - Ingress
  ingress:
  - from:
    - podSelector:
        matchLabels:
          role: api-gateway
    ports:
    - protocol: TCP
      port: 8000
```

---

## File Permissions

### Linux File Security

```bash
# Data files: Owner read/write only
chmod 600 /data/kore/*.kore

# Key files: Owner read only
chmod 400 /etc/kore/master.key

# Configuration: Owner read/write, group read
chmod 640 /etc/kore/config.yaml

# Directories: Owner rwx only
chmod 700 /data/kore/
chmod 700 /etc/kore/
```

### Ownership

```bash
# Create dedicated kore user
sudo useradd -r -s /bin/false kore

# Set ownership
sudo chown -R kore:kore /data/kore/
sudo chown -R kore:kore /etc/kore/

# Run KORE as unprivileged user
sudo -u kore /opt/kore/kore --config /etc/kore/config.yaml
```

---

## Audit Logging

### Enable Audit Logging

**Configuration:**
```yaml
logging:
  level: "info"
  format: "json"
  output: "/var/log/kore/audit.log"
  
  audit:
    enabled: true
    log_level: "info"
    events:
      - "file_read"
      - "file_write"
      - "file_delete"
      - "query_execute"
      - "key_rotation"
      - "user_login"
```

### Audit Log Format

```json
{
  "timestamp": "2026-06-03T10:30:45Z",
  "event": "file_read",
  "user": "alice@example.com",
  "file_id": "f-5a2b8c9d",
  "columns": ["id", "name", "value"],
  "rows_read": 1000,
  "status": "success",
  "duration_ms": 2.34,
  "source_ip": "192.168.1.100"
}
```

### Log Retention

```bash
#!/bin/bash
# retention-policy.sh

# Keep audit logs for 90 days
find /var/log/kore/audit* -type f -mtime +90 -delete

# Compress logs older than 7 days
find /var/log/kore/audit* -type f -mtime +7 -exec gzip {} \;

# Archive to S3
aws s3 sync /var/log/kore/archive s3://kore-audit-logs/
```

---

## Security Best Practices

✅ **DO:**
- Rotate encryption keys every 90 days
- Use strong API tokens (32+ bytes)
- Enable TLS in production
- Use environment variables for secrets
- Monitor audit logs regularly
- Limit API access by IP/network
- Run KORE as unprivileged user
- Use prepared statements (if SQL APIs added)
- Enable firewall rules
- Encrypt backups

❌ **DON'T:**
- Hardcode secrets in code
- Use default/weak passwords
- Share API tokens
- Disable TLS in production
- Store keys in git repositories
- Run KORE as root
- Allow public API access
- Ignore security warnings
- Use old encryption keys
- Commit config files with secrets

---

## Incident Response

### Detect Breach

```bash
#!/bin/bash
# Check for unauthorized access attempts

# Look for failed API attempts
grep "UNAUTHORIZED\|401" /var/log/kore/audit.log | tail -20

# Check for unusual file access
grep "file_read" /var/log/kore/audit.log | awk '{print $5}' | sort | uniq -c | sort -rn

# Monitor key rotation events
grep "key_rotation" /var/log/kore/audit.log
```

### Respond to Breach

```bash
#!/bin/bash
# Immediate response to suspected breach

# 1. Rotate encryption key immediately
export KORE_ENCRYPTION_KEY=$(openssl rand -hex 32)

# 2. Invalidate all API tokens
# Edit config.yaml and remove old tokens

# 3. Stop KORE service
sudo systemctl stop kore

# 4. Review audit logs
grep -E "UNAUTHORIZED|DELETE|DROP" /var/log/kore/audit.log > /tmp/breach-analysis.log

# 5. Backup evidence
tar czf /backups/breach-evidence-$(date +%s).tar.gz /var/log/kore/

# 6. Restart with new key
sudo systemctl start kore
```

---

## Compliance Checklist

- [ ] Encryption enabled (AES-256)
- [ ] TLS/SSL configured (production)
- [ ] Audit logging enabled
- [ ] API tokens strong (32+ bytes)
- [ ] File permissions restricted (600 for data)
- [ ] Firewall rules configured
- [ ] Key rotation scheduled (90 days)
- [ ] Backup encryption enabled
- [ ] KORE runs as unprivileged user
- [ ] Audit logs retained 90+ days
- [ ] Access control documented
- [ ] Security review completed

---

## Version History

| Version | Date | Changes |
|---------|------|---------|
| v1.0 | 2026-06-03 | Security configuration guide for KORE v1.3.3 |

---

**Status: ✅ Production Ready**
