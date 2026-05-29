# PHASE_3_IAM_CONTROLS_ASSESSMENT
**Assessment Date**: May 28, 2026  
**Lead**: Access Control Lead  
**Deadline**: May 28, 2026, 5:00 PM  
**Status**: TEMPLATE - FILL IN TODAY

---

## 🔐 IAM CONTROLS ASSESSMENT

### ROLE-BASED ACCESS CONTROL (RBAC)

#### Current RBAC Implementation

| Aspect | Status | Details | Compliance |
|--------|--------|---------|-----------|
| RBAC System Deployed | YES/NO | [Type/Platform] | [Compliant/Concern] |
| Role Hierarchy Defined | YES/NO | Levels: ___ | [Clear/Unclear] |
| Least Privilege Enforced | YES/NO | [Evidence] | [PASS/FAIL] |
| Access Review Frequency | [Frequency] | Last review: [Date] | [Current/Overdue] |
| Automated Provisioning | YES/NO | [Tool] | [Implemented/Manual] |

#### Role Definitions

| Role | Users Count | Primary Permissions | Admin Access | Last Updated |
|------|---------|-----------|-----------|-----------|
| Admin | ___ | Full system access | YES | [Date] |
| Operator | ___ | Run services | NO | [Date] |
| Developer | ___ | Code/test environments | NO | [Date] |
| Viewer | ___ | Read-only access | NO | [Date] |
| [CUSTOM...] | ___ | [Permissions] | [Y/N] | [Date] |

#### Permission Assignments

| Check | Status | Finding | Risk Level |
|-------|--------|---------|-----------|
| All permissions assigned via roles (not directly)? | YES/NO | [Details] | [H/M/L] |
| Roles reviewed within past 6 months? | YES/NO | [Details] | [H/M/L] |
| Unused roles removed? | YES/NO | [Details] | [H/M/L] |
| Role conflicts resolved? | YES/NO | [Details] | [H/M/L] |

---

### PRIVILEGE ESCALATION CONTROLS

#### Privilege Escalation Mechanisms

| Mechanism | Implemented | Monitored | Logged | Alert on Use |
|-----------|-------------|-----------|--------|--------------|
| `sudo` on Unix/Linux | YES/NO | YES/NO | YES/NO | YES/NO |
| Windows `RunAs` | YES/NO | YES/NO | YES/NO | YES/NO |
| Privileged Access Workstations (PAW) | YES/NO | YES/NO | YES/NO | YES/NO |
| Privileged Access Management (PAM) | YES/NO | YES/NO | YES/NO | YES/NO |
| Just-In-Time (JIT) Access | YES/NO | YES/NO | YES/NO | YES/NO |

#### Escalation Events (Last 30 Days)

| Event Type | Count | Approved | Unauthorized | Investigated |
|-----------|-------|----------|--------------|--------------|
| Sudo escalations | ___ | ___ | ___ | ___ |
| RunAs escalations | ___ | ___ | ___ | ___ |
| Admin access requests | ___ | ___ | ___ | ___ |
| Break-glass access used | ___ | ___ | ___ | ___ |
| **TOTAL ESCALATIONS** | **___** | | | |

#### Escalation Audit Trail

| Date | User | Escalation Type | Purpose | Approved By | Status |
|------|------|-----------------|---------|------------|--------|
| [Date] | [User] | [Type] | [Purpose] | [Approver] | [Approved/Denied] |
| [CONTINUE...] | | | | | |

---

### PASSWORD & CREDENTIAL POLICIES

#### Password Policy Enforcement

| Policy Element | Requirement | Current Implementation | Compliant |
|--------|-----------|-----------|-----------|
| Minimum Length | ___ characters | ___ characters | YES/NO |
| Complexity | Required | [Enforced/Not enforced] | YES/NO |
| Expiration | ___ days | ___ days | YES/NO |
| History | ___ previous passwords | ___ | YES/NO |
| Lockout After Failed Attempts | ___ attempts | ___ attempts | YES/NO |
| Account Lockout Duration | ___ minutes | ___ minutes | YES/NO |

#### Credential Management

| Component | Status | Tool | Access Control | Audit |
|-----------|--------|------|-----------------|-------|
| Centralized password vault | YES/NO | [Tool] | [Restricted/Open] | [Logged/Not logged] |
| Service account credentials | Documented/Undocumented | [Tool] | [Vault/Config files] | [Audited/Not audited] |
| API keys managed | YES/NO | [Tool] | [Rotation: frequency] | [Logged/Not logged] |
| SSH keys managed | YES/NO | [Tool] | [Central/Distributed] | [Audited/Not audited] |

#### Password Violation Events (Last 30 Days)

| Event | Count | Action Taken |
|-------|-------|-------------|
| Weak passwords detected | ___ | [Reset/Warned] |
| Password reuse prevented | ___ | [Allow/Block] |
| Expired passwords | ___ | [Grace period/Locked] |
| Concurrent sessions over limit | ___ | [Action taken] |

---

### MULTI-FACTOR AUTHENTICATION (MFA)

#### MFA Status by Role

| Role | Total Users | MFA Enabled | % Coverage | Target | Gap |
|------|---------|----------|----------|--------|-----|
| Admin | ___ | ___ | __% | 100% | ___ |
| Power User | ___ | ___ | __% | 100% | ___ |
| Standard User | ___ | ___ | __% | 80%+ | ___ |
| Service Account | N/A | N/A | N/A | N/A | N/A |
| **TOTAL** | **___** | **___** | **__% **| **90%+** | **___** |

#### MFA Methods Used

| Method | Deployed | Users | Status | Support |
|--------|----------|-------|--------|---------|
| SMS OTP | YES/NO | ___ | [Active/Deprecated] | [End date] |
| Email OTP | YES/NO | ___ | [Active/Active] | [Indefinite] |
| Authenticator App | YES/NO | ___ | [Active/Active] | [Indefinite] |
| Hardware Token | YES/NO | ___ | [Active/Active] | [Indefinite] |
| Biometric | YES/NO | ___ | [Active/Active] | [Indefinite] |
| Windows Hello | YES/NO | ___ | [Active/Active] | [Indefinite] |

#### MFA Enforcement Points

| System | MFA Required | MFA Optional | No MFA | Policy |
|--------|-------------|-------------|--------|--------|
| VPN Access | YES/NO | YES/NO | YES/NO | [Mandate/Optional] |
| Email System | YES/NO | YES/NO | YES/NO | [Mandate/Optional] |
| Admin Consoles | YES/NO | YES/NO | YES/NO | [Mandate/Optional] |
| Cloud Services | YES/NO | YES/NO | YES/NO | [Mandate/Optional] |
| Application Access | YES/NO | YES/NO | YES/NO | [Mandate/Optional] |

---

### ACCESS REVIEWS & RECERTIFICATION

#### Formal Access Review History

| Review Date | Scope | Users Reviewed | Issues Found | Remediated | Owner |
|---|---|---|---|---|---|
| [Date] | All users | ___ | ___ | YES/NO | [Name] |
| [Date] | Admin accounts | ___ | ___ | YES/NO | [Name] |
| [Date] | Contractors | ___ | ___ | YES/NO | [Name] |
| [Date] | External access | ___ | ___ | YES/NO | [Name] |

**Last Comprehensive Review**: [Date]  
**Frequency**: Quarterly / Semi-annual / Annual  
**Next Review Due**: [Date]  
**Review Coverage**: __% of users

#### Issues Found in Recent Reviews

| Issue | Severity | Date Found | Resolution | Owner | Status |
|-------|----------|-----------|-----------|-------|--------|
| [Over-privileged user] | [H/M/L] | [Date] | [Action] | [Owner] | [Open/Closed] |
| [Orphaned account] | [H/M/L] | [Date] | [Action] | [Owner] | [Open/Closed] |
| [Dormant account] | [H/M/L] | [Date] | [Action] | [Owner] | [Open/Closed] |
| [Compliance gap] | [H/M/L] | [Date] | [Action] | [Owner] | [Open/Closed] |

---

### APPLICATION-LEVEL ACCESS CONTROL

#### Application Authorization

| Application | Access Control | Granularity | Audit Trail | Compliance |
|-----------|-----------------|-----------|-----------|-----------|
| [App Name] | [Type: RBAC/ACL/Custom] | [Feature/User/Action] | YES/NO | [PASS/FAIL] |
| [App Name] | [Type] | [Granularity] | YES/NO | [PASS/FAIL] |
| [CONTINUE...] | | | | |

#### Session Management

| Component | Status | Details |
|-----------|--------|---------|
| Session timeout after inactivity | ___ minutes | Enforced: YES/NO |
| Concurrent session limit | ___ sessions | Enforced: YES/NO |
| Session invalidation on logout | YES/NO | Immediate: YES/NO |
| Session data encryption | YES/NO | Algorithm: [Type] |
| Session tokens | [Type] | Validity: ___ minutes |
| Token rotation | YES/NO | Frequency: [When] |

---

## 🚨 AUDIT FINDINGS

### Finding 1: [Title]
**Severity**: 🔴 High / 🟠 Medium / 🟡 Low  
**Current State**: [Description]  
**Risk**: [What could go wrong?]  
**Recommendation**: [How to fix it]  
**Timeline**: [When to fix]  
**Owner**: [Who fixes it]

### Finding 2: [Title]
**Severity**: 🔴 High / 🟠 Medium / 🟡 Low  
**Current State**: [Description]  
**Risk**: [What could go wrong?]  
**Recommendation**: [How to fix it]  
**Timeline**: [When to fix]  
**Owner**: [Who fixes it]

### Finding 3: [Title]
**Severity**: 🔴 High / 🟠 Medium / 🟡 Low  
**Current State**: [Description]  
**Risk**: [What could go wrong?]  
**Recommendation**: [How to fix it]  
**Timeline**: [When to fix]  
**Owner**: [Who fixes it]

---

## 📋 COMPLIANCE CHECKLIST

- [ ] RBAC fully implemented
- [ ] Least privilege enforced
- [ ] MFA enabled for 100% of admins
- [ ] MFA adopted by 80%+ of users
- [ ] Privilege escalation monitored
- [ ] Regular access reviews conducted
- [ ] Password policies enforced
- [ ] Credentials stored securely
- [ ] Session management secure
- [ ] No over-privileged accounts

---

## 📌 SUMMARY

**Audit Date**: May 28, 2026  
**Lead**: Access Control Lead  
**Findings**: ___ total (___ High, ___ Medium, ___ Low)  
**Overall Assessment**: [Excellent/Good/Fair/Poor/Critical]  
**MFA Adoption**: __% (Target: 80%+)  
**Most Critical Issue**: [Describe]  
**Ready for next phase**: YES / NO / With conditions  

---

**Completed By**: [Name]  
**Date Completed**: [Date]  
**Reviewed By**: Assessment Lead  
**Date Reviewed**: [Date]

---

*IAM Controls Assessment - KORE Phase 3 Week 1 Assessment*
