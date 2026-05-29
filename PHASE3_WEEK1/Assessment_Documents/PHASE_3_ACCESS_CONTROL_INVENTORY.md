# PHASE_3_ACCESS_CONTROL_INVENTORY
**Assessment Date**: May 27, 2026  
**Lead**: Access Control Lead  
**Deadline**: May 27, 2026, 5:00 PM  
**Status**: TEMPLATE - FILL IN TODAY

---

## 📊 ACCESS CONTROL INVENTORY

### USER POPULATION SUMMARY

| Category | Count | Status | Notes |
|----------|-------|--------|-------|
| Active Users | ___ | Current as of [Date] | Include all departments |
| Admin/Privileged Users | ___ | Reviewed? [Y/N] | List separately |
| Service Accounts | ___ | Documented? [Y/N] | List purpose |
| External Users (Vendors/Partners) | ___ | Agreements in place? [Y/N] | Third-party contracts |
| Contractors | ___ | Background checked? [Y/N] | Agreements on file? |
| **TOTAL ACTIVE ACCOUNTS** | **___** | | |
| Inactive Users | ___ | Deactivated within 90 days? [Y/N] | Delete schedule? |
| **TOTAL ACCOUNTS** | **___** | | |

---

## 👥 USER & ROLE INVENTORY

### Active Users (SAMPLE - Fill in complete list)

| User ID | Name | Department | Job Title | Primary Role | Admin Access? | MFA Enabled? | Last Reviewed | Status |
|---------|------|-----------|-----------|--------------|---------------|-------------|---|--------|
| USR001 | [Name] | Engineering | Senior Developer | Developer | No | Yes | [Date] | Active |
| USR002 | [Name] | Security | Security Manager | Security Admin | Yes | Yes | [Date] | Active |
| USR003 | [Name] | Operations | SysAdmin | Infrastructure | Yes | Yes | [Date] | Active |
| USR004 | [Name] | Finance | Finance Manager | Finance | No | No | [Date] | ⚠️ AUDIT |
| USR005 | [Name] | Marketing | Marketing Director | Marketing | No | No | [Date] | ⚠️ AUDIT |
| [CONTINUE...] | | | | | | | | |

**Total Active Users**: ___  
**MFA Adoption Rate**: ___% 
**Users Without MFA**: ___ (AUDIT REQUIRED)

---

### Admin & Privileged Accounts

| Account ID | Type | Owner | Purpose | Last Used | Password Age | Rotation Policy | Audit Frequency |
|---|---|---|---|---|---|---|---|
| ADMIN001 | Local Admin | [Name] | System administration | [Date] | [Days] | 90 days | Daily |
| ADMIN002 | Domain Admin | [Name] | AD management | [Date] | [Days] | 90 days | Daily |
| SVCACCT001 | Service Account | App Team | KORE app service | [Date] | N/A | 180 days | Weekly |
| SVCACCT002 | Service Account | DB Team | Database backup | [Date] | N/A | 180 days | Weekly |
| BREAK-GLASS | Emergency Access | Security | Break-glass account | [Date] | N/A | Manual use only | Every use |
| [CONTINUE...] | | | | | | | |

**Critical Issues**:
- [ ] All admin accounts have strong passwords (14+ chars, complexity)
- [ ] All admin accounts have MFA enabled
- [ ] All admin accounts monitored and audited
- [ ] Break-glass account tested within 90 days
- [ ] Service account passwords stored in vault
- [ ] No hardcoded credentials in code or configs

---

### Service Accounts

| Account | System | Purpose | Authentication Method | Credential Storage | Owner | Last Rotated | Audit |
|---------|--------|---------|----------------------|-------------------|-------|---|---|
| app-svc | KORE App | Database connection | Username/Password | Vault | [Name] | [Date] | Monthly ✓ |
| backup-svc | Backup System | Daily backups | SSH Key | Key vault | [Name] | [Date] | Monthly ✓ |
| replication-svc | Database | Replication | Connection string | Encrypted config | [Name] | [Date] | Monthly ✓ |
| [CONTINUE...] | | | | | | | |

**Security Status**:
- [ ] All service accounts documented
- [ ] All service accounts have minimal required permissions
- [ ] All service account passwords rotated every 180 days
- [ ] No service account passwords in code repositories
- [ ] All service account activity logged

---

### External Users (Vendors, Partners, Contractors)

| Organization | Contact | Account | System Access | Approval | Agreement | Expires | Status |
|---|---|---|---|---|---|---|---|
| [Vendor Name] | [Contact] | [Account] | Read-only access | [Approver] | Data Processing Agreement | [Date] | Active |
| [Partner] | [Contact] | [Account] | API access | [Approver] | Service Agreement | [Date] | Active |
| [Contractor] | [Contact] | [Account] | Time-limited access | [Approver] | NDA + Service | [Date] | Active |
| [CONTINUE...] | | | | | | | |

**External Access Review**:
- [ ] All external access approved by management
- [ ] All external access has legal agreements
- [ ] All external access has defined end dates
- [ ] All external access is logged
- [ ] Quarterly access review completed

---

## 🔐 AUTHENTICATION & MFA

### Authentication Methods

| Method | Systems | Users | Status | Security Level |
|--------|---------|-------|--------|-----------------|
| Username/Password | [List] | All | Mandatory | Standard (⚠️ Weak if no MFA) |
| MFA | [List] | ___ | Enabled for [%] | Strong |
| SSO/SAML | [List] | [Count] | Enabled? [Y/N] | Strong |
| API Keys | [List] | [Count] | Rotated? [Y/N] | [Assess] |
| SSH Keys | [List] | [Count] | Rotated? [Y/N] | [Assess] |
| Certificates | [List] | [Count] | Current? [Y/N] | [Assess] |
| [CONTINUE...] | | | | |

### MFA Status

| Category | MFA Enabled | MFA Disabled | Compliance |
|----------|-----------|------------|-----------|
| Privileged Users (Admins) | ___ / ___ | ___ | [PASS/FAIL] ⚠️ CRITICAL |
| Power Users | ___ / ___ | ___ | [PASS/FAIL] ⚠️ |
| Standard Users | ___ / ___ | ___ | [PASS/FAIL] |
| Service Accounts | N/A | N/A | N/A (use vault keys) |
| **TOTAL MFA ADOPTION** | **___** | **___** | **___%** |

**MFA Requirement Compliance**:
- [ ] All admin accounts: 100% MFA
- [ ] All power users: 100% MFA
- [ ] All standard users: 80%+ MFA (recommended)

**MFA Gap**: ___ accounts need MFA enabled (ACTION REQUIRED)

---

### SSO / Directory Integration

| System | Directory | Integration | Status | Last Sync | Issues |
|--------|-----------|------------|--------|-----------|--------|
| Windows | Active Directory | Domain-joined | Connected | [Time] | None |
| Linux | LDAP | PAM | Connected | [Time] | None |
| Cloud Apps | Azure AD | SSO | Connected | [Time] | [Issues?] |
| VPN | RADIUS | Integration | Connected | [Time] | [Issues?] |
| [CONTINUE...] | | | | | |

**Directory Sync Issues**:
- [ ] All systems syncing properly
- [ ] User deprovisioning works automatically
- [ ] Password policies enforced
- [ ] MFA enforced at directory level

---

## 🎯 ROLE-BASED ACCESS CONTROL (RBAC)

### Defined Roles

| Role Name | Description | User Count | Permissions | Last Reviewed | Owner |
|-----------|-------------|-----------|-------------|---|---|
| Admin | Full system access | ___ | [List] | [Date] | [Name] |
| Operator | Run and manage services | ___ | [List] | [Date] | [Name] |
| Developer | Code and test | ___ | [List] | [Date] | [Name] |
| Viewer | Read-only access | ___ | [List] | [Date] | [Name] |
| [CUSTOM ROLES...] | | | | | |

### Permission Matrix

Create a matrix showing which roles can perform which actions:

```
Action → | Admin | Operator | Developer | Viewer | Service Acct |
-----------|-------|----------|-----------|--------|--------------|
Create user | ✓ | | | | 
Modify user | ✓ | | | | 
Delete user | ✓ | | | | 
Start service | ✓ | ✓ | | | 
View logs | ✓ | ✓ | ✓ | ✓ | ✓ 
Modify config | ✓ | | | | 
Create backup | ✓ | ✓ | | | 
Restore backup | ✓ | | | | ✓ 
View data | ✓ | ✓ | ✓ | ✓ | ✓ 
Modify data | ✓ | ✓ | ✓ | | ✓ 
```

**RBAC Assessment**:
- [ ] All roles documented
- [ ] Least privilege principle enforced
- [ ] All permissions assigned via roles (not individually)
- [ ] All users assigned exactly one role (no overlap)
- [ ] Roles reviewed annually

---

## 📊 ACCESS REVIEW HISTORY

### Formal Access Reviews

| Review Date | Scope | Users Reviewed | Issues Found | Remediated | Owner |
|---|---|---|---|---|---|
| [Date] | All users | ___ | ___ | YES/NO | [Name] |
| [Date] | Admin accounts | ___ | ___ | YES/NO | [Name] |
| [Date] | Contractors | ___ | ___ | YES/NO | [Name] |
| [Date] | External | ___ | ___ | YES/NO | [Name] |

**Last Comprehensive Access Review**: [Date]  
**Frequency**: Quarterly / Semi-annual / Annual: [Select]  
**Next Review Due**: [Date]

---

## ⚠️ ACCESS ISSUES IDENTIFIED

### Over-Privileged Users

| User | Current Role | Recommended Role | Reason | Priority | Status |
|------|--------------|-----------------|--------|----------|--------|
| [User] | Admin | Operator | Doesn't need admin access | HIGH | Needs remediation |
| [User] | Developer + Admin | Developer | Separated roles | HIGH | Needs remediation |
| [CONTINUE...] | | | | | |

**Action Required**: Remove excess privileges for ___ users

### Orphaned / Unused Accounts

| Account | Last Used | Days Inactive | Action | Owner |
|---------|-----------|--------------|--------|-------|
| [User] | [Date] | ___ days | Deactivate | [Name] |
| [Account] | [Date] | ___ days | Delete | [Name] |
| [CONTINUE...] | | | | |

**Action Required**: Deactivate/delete ___ accounts

### Access Without Documentation

| User | System | Permission | Documentation | Status |
|------|--------|-----------|---------------|--------|
| [User] | [System] | [Access] | Missing | Needs doc |
| [CONTINUE...] | | | | |

**Action Required**: Document ___ access permissions

---

## 📋 ACCESS PROVISIONING & DEPROVISIONING

### User Provisioning Process

**Current Process**:
1. Manager submits access request
2. Security reviews and approves
3. IT provisions account
4. User receives credentials
5. System auto-enrolls in MFA
6. Documented in audit log

**Time to Provision**: ___ days  
**Issues**: [List any issues]

**Checklist**:
- [ ] Written policy exists
- [ ] All steps documented
- [ ] Manager approval required
- [ ] Security review required
- [ ] Audit log maintained
- [ ] MFA auto-provisioned
- [ ] Welcome email sent
- [ ] Onboarding completed

### User Deprovisioning Process

**Current Process**:
1. Termination notice received
2. IT receives deprovisioning ticket
3. All accounts disabled within [X] hours
4. Files transferred to manager
5. Equipment collected
6. Access removed from all systems
7. Documented in audit log

**Time to Deprovision**: ___ days  
**Issues**: [List any issues]

**Checklist**:
- [ ] Written policy exists
- [ ] IT notified immediately
- [ ] Same-day disablement
- [ ] File preservation completed
- [ ] All system access removed
- [ ] VPN/physical access revoked
- [ ] Equipment collected
- [ ] Audit log maintained

---

## 🔍 ACCESS LOGGING & MONITORING

### Audit Logging

| System | Audit Log | Retention | Monitoring | Alerts | Last Review |
|--------|-----------|-----------|-----------|--------|-------------|
| Active Directory | Enabled | 90 days | Yes | Failed logins | [Date] |
| Database | Enabled | 365 days | Yes | Privilege use | [Date] |
| Application | Enabled | 180 days | Yes | Sensitive access | [Date] |
| SSH/RDP | Enabled | 30 days | Yes | Root login | [Date] |
| [CONTINUE...] | | | | | |

### Suspicious Activity Detected

| Event | Date | User | System | Severity | Status | Action |
|-------|------|------|--------|----------|--------|--------|
| [Event] | [Date] | [User] | [System] | High | Investigated | [Resolution] |
| [CONTINUE...] | | | | | | |

---

## 🚨 CRITICAL FINDINGS

### Critical Issues (Must Fix)

1. **Issue**: [Description]
   - **Impact**: Security/Compliance
   - **Example**: [Specific case]
   - **Recommendation**: [Action]
   - **Timeline**: ASAP

2. [Additional issues...]

### High Priority Issues

1. **Issue**: [Description]
   - **Impact**: Security/Compliance
   - **Recommendation**: [Action]
   - **Timeline**: Within 4 weeks

---

## 📌 SUMMARY

**Total Users**: ___  
**MFA Adoption**: ___%  
**Orphaned Accounts**: ___  
**Over-Privileged Users**: ___  
**Access Issues**: ___  

**Compliance Assessment**:
- [ ] All users documented
- [ ] All access justified
- [ ] Least privilege enforced
- [ ] MFA enabled for 100% of admins
- [ ] Regular access reviews conducted
- [ ] Provisioning/deprovisioning working
- [ ] All activity logged
- [ ] Suspicious activity monitored

**Next Steps**:
1. Remediate critical access issues
2. Remove orphaned accounts
3. Enable MFA for remaining users
4. Document all access
5. Schedule monthly reviews

---

**Completed By**: [Name]  
**Date Completed**: [Date]  
**Reviewed By**: [Reviewer]  
**Date Reviewed**: [Date]

---

*Access Control Inventory - KORE Phase 3 Week 1 Assessment*
