# MAY 31 INFRASTRUCTURE PROVISIONING CHECKLIST
## Phase 1 Readiness (3 Days Before Jun 2 Launch)

**Status**: READY FOR EXECUTION  
**Date**: May 31, 2026  
**Owner**: DevOps Team (2 engineers)  
**Deadline**: All items complete by 6:00 PM May 31  
**Launch**: Jun 2, 2026, 9:00 AM (daily standups + Phase 1 execution)

---

## 🎯 MISSION

Provision complete infrastructure for KORE Phase 1 performance optimization sprint. Systems must be ready for 8-engineer team starting Jun 2.

---

## ⏰ TIMELINE (May 31, Full Day)

### 8:00 AM - 12:00 PM: AWS INFRASTRUCTURE (4 hours)

| Task | Owner | Action | Deadline |
|------|-------|--------|----------|
| AWS Login | DevOps1 | Log into AWS console with KORE_Phase1 account | 8:05 AM |
| Verify Quotas | DevOps1 | Check t3.2xlarge quota (need 3 servers) | 8:10 AM |
| Create Security Group | DevOps1 | New SG: KORE-Phase1 (SSH, benchmarking ports open) | 8:15 AM |
| Provision Server 1 | DevOps1 | t3.2xlarge (Ubuntu 22.04, 8 vCPU, 32GB RAM) - Performance testing | 8:20 AM |
| Provision Server 2 | DevOps1 | t3.2xlarge (Ubuntu 22.04, 8 vCPU, 32GB RAM) - Benchmarking | 8:25 AM |
| Provision Server 3 | DevOps1 | t3.2xlarge (Ubuntu 22.04, 8 vCPU, 32GB RAM) - Baseline comparison | 8:30 AM |
| Configure Storage | DevOps1 | 500GB EBS attached to each server (TPC-H data) | 8:40 AM |
| Setup Networking | DevOps1 | VPC, subnets, network ACLs configured | 8:50 AM |
| Enable Monitoring | DevOps1 | CloudWatch dashboards for CPU, memory, disk I/O | 9:00 AM |
| SSH Access | DevOps1 | Download key pairs, test SSH connection to all 3 servers | 9:10 AM |

**Success**: 3 servers running, SSH access working, monitoring live

---

### 12:00 PM - 2:00 PM: BENCHMARKING TOOLS (2 hours)

| Task | Owner | Action | Deadline |
|------|-------|--------|----------|
| SSH to Server 1 | DevOps2 | Connect to performance testing server | 12:05 PM |
| Install Rust | DevOps2 | curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh | 12:10 PM |
| Install Cargo | DevOps2 | cargo --version (verify 1.96.0+) | 12:15 PM |
| Clone KORE Repo | DevOps2 | git clone https://github.com/arunkatherashala/Kore.git /opt/kore | 12:20 PM |
| Build KORE Release | DevOps2 | cd /opt/kore && cargo build --release | 12:45 PM |
| Install YCSB | DevOps2 | git clone https://github.com/brianfrankcooper/YCSB.git /opt/ycsb | 1:00 PM |
| Build YCSB | DevOps2 | cd /opt/ycsb && mvn clean package -DskipTests | 1:15 PM |
| Download TPC-H Data | DevOps2 | wget [data location] /opt/data/tpc-h-10m.csv (517 MB) | 1:30 PM |
| Verify Data Integrity | DevOps2 | Check file size + MD5 hash matches expected | 1:35 PM |
| Install Monitoring Tools | DevOps2 | prometheus, grafana, node-exporter setup | 1:45 PM |

**Success**: Rust toolchain, KORE binary, YCSB, TPC-H data, monitoring all installed

---

### 2:00 PM - 4:00 PM: CI/CD PIPELINE (2 hours)

| Task | Owner | Action | Deadline |
|------|-------|--------|----------|
| GitHub Actions Setup | DevOps1 | Create self-hosted runner on Server 2 | 2:05 PM |
| Register Runner | DevOps1 | gh runner create --name kore-bench-1 --arch x64 --os linux | 2:10 PM |
| Create Workflow | DevOps1 | New GitHub Actions workflow: .github/workflows/performance-benchmark.yml | 2:20 PM |
| Workflow Steps | DevOps1 | 1) Build KORE, 2) Run YCSB, 3) Log results, 4) Compare baseline | 2:40 PM |
| Test Workflow | DevOps1 | Trigger manual run, verify all steps complete | 3:00 PM |
| Automated Triggers | DevOps1 | Schedule daily runs at 2:00 AM UTC (off-peak hours) | 3:15 PM |
| Results Dashboard | DevOps1 | Create GitHub Actions dashboard showing performance trends | 3:45 PM |

**Success**: CI/CD pipeline automated, daily benchmarks scheduled, results tracked

---

### 4:00 PM - 5:30 PM: PERFORMANCE BASELINE (1.5 hours)

| Task | Owner | Action | Deadline |
|------|-------|--------|----------|
| Run Baseline Test | DevOps2 | Execute KORE on TPC-H 10M rows (current performance) | 4:05 PM |
| Baseline Results | DevOps2 | Record: query_speed, compression_ratio, time, CPU/memory | 4:15 PM |
| Expected: 2.7M rows/sec | DevOps2 | Verify baseline matches known performance (2.7M rows/sec) | 4:20 PM |
| Compare with Parquet | DevOps2 | Run same test on Parquet (compare baseline) | 4:35 PM |
| Document Results | DevOps2 | Create BASELINE_PERFORMANCE_MAY_31.txt (commit to repo) | 4:50 PM |
| Slack Notification | DevOps2 | Notify Sarah: "Baseline established, ready for Phase 1" | 5:00 PM |
| Archive Baseline | DevOps2 | Back up all baseline data (S3 bucket: kore-baselines) | 5:15 PM |

**Success**: Baseline performance established (2.7M rows/sec), documented, ready for comparison

---

### 5:30 PM - 6:00 PM: FINAL VERIFICATION (30 min)

| Task | Owner | Action | Deadline |
|------|-------|--------|----------|
| Sanity Check 1 | DevOps1 | All 3 servers running + responding to SSH | 5:35 PM |
| Sanity Check 2 | DevOps1 | CloudWatch dashboards showing data | 5:40 PM |
| Sanity Check 3 | DevOps2 | KORE binary working, TPC-H data present, YCSB installed | 5:45 PM |
| Sanity Check 4 | DevOps2 | CI/CD pipeline triggered successfully (test run complete) | 5:50 PM |
| Sanity Check 5 | DevOps2 | Baseline performance documented + archived | 5:55 PM |
| Go-Live Email | DevOps1 | Send Sarah + team: "Infrastructure ready for Jun 2 launch" | 6:00 PM |

**Success**: All 5 sanity checks pass, team notified, ready for Jun 2

---

## 📋 INFRASTRUCTURE SPECS

### AWS Resources

**3x t3.2xlarge EC2 Instances**
- OS: Ubuntu 22.04 LTS
- vCPU: 8 cores each (24 total)
- RAM: 32GB each (96GB total)
- Storage: 500GB EBS gp3 each
- Network: VPC, public subnet, security group (SSH, HTTPS, benchmarking ports)
- Monitoring: CloudWatch + Prometheus + Grafana

**Server Allocation**:
- Server 1: Performance testing (KORE optimization target)
- Server 2: CI/CD runner (automated benchmarking)
- Server 3: Baseline comparison (current performance validation)

**Estimated Cost**:
- 3 × t3.2xlarge: ~$0.53/hour each = $1.59/hour (24 hours × $1.59 × 30 days = $1,134/month)
- Storage: 1.5TB × $0.10/GB-month = $150/month
- Data transfer: ~$50/month (TPC-H dataset, logs)
- **Total Monthly**: ~$1,334 (within Phase 1 budget)

---

## 🛠️ TOOL INSTALLATION CHECKLIST

### Server 1 & 2 & 3 (All):
- [ ] Rust toolchain (1.96.0+)
- [ ] Cargo
- [ ] Git
- [ ] wget/curl
- [ ] Java/Maven (for YCSB)
- [ ] Python 3.12
- [ ] Monitoring: node-exporter, Prometheus, Grafana

### Server 1 (Performance):
- [ ] KORE source code (compiled)
- [ ] YCSB
- [ ] TPC-H dataset (10M rows)
- [ ] Benchmarking scripts (Python)

### Server 2 (CI/CD):
- [ ] GitHub Actions self-hosted runner
- [ ] KORE source code (compiled)
- [ ] Automated benchmark workflow

### Server 3 (Baseline):
- [ ] KORE source code (compiled)
- [ ] Parquet (baseline comparison)
- [ ] TPC-H dataset (same as Server 1)
- [ ] Comparison scripts

---

## 📊 PERFORMANCE TRACKING

### Daily Metrics (Automated via CI/CD)

Captured every day at 2:00 AM UTC:

```
Query Performance: X.XM rows/sec
Compression Ratio: XX.X%
Memory Usage: XX GB
CPU Utilization: XX%
Test Data: TPC-H 10M rows
Benchmark Duration: XX seconds
Baseline Comparison: +X% vs current
```

### Dashboard
- GitHub Actions job results (visible to team)
- Grafana dashboard (historical trends)
- Weekly summary email (every Monday)

### Success Criteria
- Baseline matches expected 2.7M rows/sec
- CI/CD pipeline runs daily without errors
- Performance improvements tracked week-by-week
- All data backed up to S3

---

## ✅ GO-LIVE READINESS (Jun 2, 9:00 AM)

By May 31, 6:00 PM, the following must be TRUE:

- [ ] 3 AWS servers provisioned + running
- [ ] SSH access verified on all servers
- [ ] CloudWatch monitoring live
- [ ] KORE compiled + tested on all servers
- [ ] YCSB installed + configured
- [ ] TPC-H 10M row dataset present on all servers
- [ ] GitHub Actions self-hosted runner active
- [ ] CI/CD workflow automated (daily 2 AM UTC)
- [ ] Baseline performance: 2.7M rows/sec documented
- [ ] All backup systems functional (S3 archiving)
- [ ] Sarah notified: "Ready for Jun 2 launch"

**If ALL checks pass**: 🚀 **INFRASTRUCTURE READY FOR PHASE 1**

---

## 🎁 DELIVERABLES (May 31)

| Item | Purpose | Owner | Deadline |
|------|---------|-------|----------|
| AWS Account Access | Server management | DevOps1 | 6:00 PM |
| SSH Key Pairs | Remote access | DevOps1 | 6:00 PM |
| CloudWatch Dashboard | Performance monitoring | DevOps1 | 6:00 PM |
| KORE Binary (Compiled) | Performance testing | DevOps2 | 6:00 PM |
| BASELINE_PERFORMANCE_MAY_31.txt | Current performance baseline (2.7M rows/sec) | DevOps2 | 6:00 PM |
| GitHub Actions Workflow | Automated benchmarking | DevOps1 | 6:00 PM |
| Grafana Dashboard | Historical trends | DevOps1 | 6:00 PM |
| Infrastructure Diagram | Architecture documentation | DevOps1 | 6:00 PM |
| Go-Live Email | Confirmation to Sarah | DevOps1 | 6:00 PM |

---

## 📞 ESCALATION

| Issue | Action | Escalate To |
|-------|--------|-------------|
| AWS quota exceeded | Use on-demand reserved instances | Finance |
| Servers slow to provision | Request expedited launch | AWS support |
| Data download times out | Use S3 direct download or local copy | DevOps lead |
| CI/CD workflow fails | Check GitHub Actions logs, debug workflow | DevOps lead |
| Baseline doesn't match 2.7M | Verify KORE version, recompile, re-test | Sarah (CTO) |

---

## 🎬 POST-PROVISIONING (May 31 Evening)

**6:00-7:00 PM**: Infrastructure Handoff
- Sarah logs in, verifies all systems accessible
- Confirms dashboard monitoring shows real-time data
- Tests baseline performance (should see 2.7M rows/sec)

**7:00-8:00 PM**: Engineer Familiarization
- Michael Torres reviews SIMD optimization target (Server 1)
- David Park checks memory layout (Server 3 baseline)
- Emily Rodriguez reviews QA test environment

**8:00-9:00 PM**: Jun 2 Preparation
- Final checks: All systems respond
- Sarah confirms standby team for launch day
- Backup contact list shared

---

## ✨ SUCCESS CRITERIA

**Infrastructure is READY when:**

✅ 3 servers running (8 vCPU, 32GB RAM each)  
✅ SSH access working on all servers  
✅ KORE compiled + ready to benchmark  
✅ TPC-H data loaded (10M rows, 517 MB)  
✅ Baseline performance: 2.7M rows/sec  
✅ CI/CD pipeline automated (daily runs)  
✅ Monitoring dashboards live  
✅ Backup/archiving systems functional  
✅ Team notified (go-live ready)  

---

**Prepared by**: DevOps Team  
**Approved by**: Sarah Williams (CTO)  
**Date**: May 31, 2026  
**Execution Window**: 8:00 AM - 6:00 PM (full day)  
**Launch**: Jun 2, 2026, 9:00 AM (Phase 1 begins)
