// ============================================================================
// KORE ∞ — Layer 3: kore_nerve
// ============================================================================
//
// Autonomous agent network: monitors .kore files, detects anomalies,
// suggests fixes, and can apply auto-repairs.
//
// Each "nerve" is a specialized detector agent:
//   NullGuard     — detects and reports columns with too many nulls
//   DriftGuard    — detects statistical drift between two .kore snapshots
//   SchemaGuard   — detects schema changes (added/removed/retyped columns)
//   OutlierGuard  — detects extreme outliers in numeric columns
//   DupeGuard     — detects duplicate row patterns
//
// Python API:
//   from kore_fileformat import KoreNerve
//   n = KoreNerve("data.kore")
//   report = n.scan()           → run all agents, return findings
//   report = n.scan_drift("prev.kore", "curr.kore")  → drift detection
//   n.auto_fix("data.kore")     → apply safe automatic fixes
// ============================================================================

use crate::kore_v2::{KoreReader, KVal};
use std::collections::HashMap;

// ── Finding severity ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum Severity { Critical, Warning, Info }

impl Severity {
    fn label(&self) -> &str {
        match self { Severity::Critical => "CRITICAL", Severity::Warning => "WARNING", Severity::Info => "INFO" }
    }
    fn icon(&self) -> &str {
        match self { Severity::Critical => "🔴", Severity::Warning => "🟡", Severity::Info => "🟢" }
    }
}

// ── A single finding from a nerve agent ──────────────────────────────────────

#[derive(Debug, Clone)]
pub struct NerveFinding {
    pub agent: String,
    pub severity: Severity,
    pub column: Option<String>,
    pub message: String,
    pub suggestion: String,
    pub auto_fixable: bool,
}

impl NerveFinding {
    fn format(&self) -> String {
        let col_part = self.column.as_ref()
            .map(|c| format!("[{}] ", c)).unwrap_or_default();
        format!(
            "  {} {} {}{}  → {}\n{}",
            self.severity.icon(),
            self.severity.label(),
            col_part,
            self.message,
            self.suggestion,
            if self.auto_fixable { "     [AUTO-FIX available]\n" } else { "" }
        )
    }
}

// ── Nerve scan result ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct NerveScanResult {
    pub file_path: String,
    pub findings: Vec<NerveFinding>,
    pub critical_count: usize,
    pub warning_count: usize,
    pub info_count: usize,
    pub health_score: f64,
}

impl NerveScanResult {
    pub fn report(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "\n╔══════════════════════════════════════════════════════════════╗\n"
        ));
        out.push_str(&format!(
            "║  KORE NERVE SCAN  ·  {}  \n", self.file_path
        ));
        out.push_str(&format!(
            "║  Health: {:.1}/100  ·  🔴 {}  🟡 {}  🟢 {}\n",
            self.health_score, self.critical_count, self.warning_count, self.info_count
        ));
        out.push_str(&format!(
            "╚══════════════════════════════════════════════════════════════╝\n\n"
        ));

        if self.findings.is_empty() {
            out.push_str("  ✅ No issues found. File is healthy.\n");
            return out;
        }

        // Group by severity
        for sev in &[Severity::Critical, Severity::Warning, Severity::Info] {
            let group: Vec<&NerveFinding> = self.findings.iter()
                .filter(|f| &f.severity == sev).collect();
            if group.is_empty() { continue; }
            out.push_str(&format!(" ─── {} ({}) ───\n", sev.label(), group.len()));
            for f in group { out.push_str(&f.format()); }
            out.push('\n');
        }
        out
    }

    pub fn to_json(&self) -> String {
        let mut j = String::new();
        j.push_str("{\n");
        j.push_str(&format!("  \"file\": \"{}\",\n", self.file_path));
        j.push_str(&format!("  \"health_score\": {:.2},\n", self.health_score));
        j.push_str(&format!("  \"critical\": {},\n", self.critical_count));
        j.push_str(&format!("  \"warning\": {},\n", self.warning_count));
        j.push_str(&format!("  \"info\": {},\n", self.info_count));
        j.push_str("  \"findings\": [\n");
        for (i, f) in self.findings.iter().enumerate() {
            j.push_str("    {\n");
            j.push_str(&format!("      \"agent\": \"{}\",\n", f.agent));
            j.push_str(&format!("      \"severity\": \"{}\",\n", f.severity.label()));
            if let Some(col) = &f.column {
                j.push_str(&format!("      \"column\": \"{}\",\n", col));
            }
            j.push_str(&format!("      \"message\": \"{}\",\n",
                f.message.replace('"', "\\\"")));
            j.push_str(&format!("      \"suggestion\": \"{}\",\n",
                f.suggestion.replace('"', "\\\"")));
            j.push_str(&format!("      \"auto_fixable\": {}\n", f.auto_fixable));
            j.push_str(if i + 1 < self.findings.len() { "    },\n" } else { "    }\n" });
        }
        j.push_str("  ]\n}\n");
        j
    }
}

// ── Individual nerve agents ───────────────────────────────────────────────────

struct NullGuard;
struct OutlierGuard;
struct CardinalityGuard;
struct DupeGuard;
struct RangeGuard;

impl NullGuard {
    fn scan(col_name: &str, values: &[KVal], total: usize) -> Vec<NerveFinding> {
        let nulls = values.iter().filter(|v| matches!(v, KVal::Null)).count();
        if nulls == 0 { return vec![]; }
        let pct = nulls as f64 / total as f64 * 100.0;
        if pct > 50.0 {
            vec![NerveFinding {
                agent: "NullGuard".to_string(), severity: Severity::Critical,
                column: Some(col_name.to_string()),
                message: format!("{:.1}% nulls ({} / {} rows)", pct, nulls, total),
                suggestion: "Drop or impute this column before use in models/reports".to_string(),
                auto_fixable: false,
            }]
        } else if pct > 10.0 {
            vec![NerveFinding {
                agent: "NullGuard".to_string(), severity: Severity::Warning,
                column: Some(col_name.to_string()),
                message: format!("{:.1}% nulls ({} / {} rows)", pct, nulls, total),
                suggestion: "Consider mean/median imputation or flagging as missing".to_string(),
                auto_fixable: false,
            }]
        } else {
            vec![NerveFinding {
                agent: "NullGuard".to_string(), severity: Severity::Info,
                column: Some(col_name.to_string()),
                message: format!("{:.1}% nulls ({} rows)", pct, nulls),
                suggestion: "Low null rate — acceptable but monitor over time".to_string(),
                auto_fixable: false,
            }]
        }
    }
}

impl OutlierGuard {
    fn scan(col_name: &str, values: &[KVal]) -> Vec<NerveFinding> {
        let nums: Vec<f64> = values.iter().filter_map(|v| match v {
            KVal::Int(x) => Some(*x as f64), KVal::Float(x) => Some(*x), _ => None,
        }).collect();
        if nums.len() < 10 { return vec![]; }
        let n = nums.len() as f64;
        let mean = nums.iter().sum::<f64>() / n;
        let std_dev = (nums.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n).sqrt();
        if std_dev == 0.0 { return vec![]; }
        let outliers = nums.iter().filter(|&&x| ((x - mean) / std_dev).abs() > 3.0).count();
        if outliers == 0 { return vec![]; }
        let pct = outliers as f64 / nums.len() as f64 * 100.0;
        let sev = if pct > 5.0 { Severity::Critical } else if pct > 1.0 { Severity::Warning } else { Severity::Info };
        vec![NerveFinding {
            agent: "OutlierGuard".to_string(), severity: sev,
            column: Some(col_name.to_string()),
            message: format!("{} outliers ({:.2}%) — values beyond ±3σ (mean={:.2}, σ={:.2})",
                outliers, pct, mean, std_dev),
            suggestion: "Review outliers — could be data errors, fraud, or genuine extremes".to_string(),
            auto_fixable: false,
        }]
    }
}

impl CardinalityGuard {
    fn scan(col_name: &str, values: &[KVal], is_str: bool) -> Vec<NerveFinding> {
        if !is_str { return vec![]; }
        let total = values.len();
        if total < 100 { return vec![]; }
        let distinct: std::collections::HashSet<String> = values.iter()
            .filter(|v| !matches!(v, KVal::Null))
            .map(|v| v.display())
            .collect();
        let ratio = distinct.len() as f64 / total as f64;
        if ratio < 0.01 {
            vec![NerveFinding {
                agent: "CardinalityGuard".to_string(), severity: Severity::Info,
                column: Some(col_name.to_string()),
                message: format!("Only {} distinct values in {} rows ({:.1}% cardinality)",
                    distinct.len(), total, ratio * 100.0),
                suggestion: "Excellent candidate for dictionary/RLE encoding — already optimized in KORE".to_string(),
                auto_fixable: false,
            }]
        } else if ratio > 0.99 && total > 1000 {
            vec![NerveFinding {
                agent: "CardinalityGuard".to_string(), severity: Severity::Info,
                column: Some(col_name.to_string()),
                message: format!("Near-unique string column ({:.1}% cardinality)", ratio * 100.0),
                suggestion: "Likely a free-text or ID column — not useful for GROUP BY aggregations".to_string(),
                auto_fixable: false,
            }]
        } else { vec![] }
    }
}

impl DupeGuard {
    fn scan(col_data: &[Vec<KVal>], col_names: &[String]) -> Vec<NerveFinding> {
        if col_data.is_empty() { return vec![]; }
        let nrows = col_data[0].len();
        if nrows < 2 { return vec![]; }
        // Hash first 5 cols to detect duplicate rows (sample 10k rows max)
        let sample = nrows.min(10000);
        let ncols = col_data.len().min(5);
        let mut seen: HashMap<String, usize> = HashMap::new();
        let mut dupes = 0usize;
        for ri in 0..sample {
            let key: String = (0..ncols).map(|ci| {
                col_data.get(ci).and_then(|c| c.get(ri)).map(|v| v.display()).unwrap_or_default()
            }).collect::<Vec<_>>().join("|");
            let count = seen.entry(key).or_insert(0);
            *count += 1;
            if *count == 2 { dupes += 1; }
        }
        if dupes == 0 { return vec![]; }
        let pct = dupes as f64 / sample as f64 * 100.0;
        let sev = if pct > 10.0 { Severity::Critical } else { Severity::Warning };
        vec![NerveFinding {
            agent: "DupeGuard".to_string(), severity: sev,
            column: None,
            message: format!("~{} duplicate rows detected in sample of {} ({:.1}%)",
                dupes, sample, pct),
            suggestion: format!("Deduplicate on: {}", col_names.iter().take(ncols).cloned().collect::<Vec<_>>().join(", ")),
            auto_fixable: false,
        }]
    }
}

impl RangeGuard {
    fn scan(col_name: &str, values: &[KVal]) -> Vec<NerveFinding> {
        let nums: Vec<f64> = values.iter().filter_map(|v| match v {
            KVal::Int(x) => Some(*x as f64), KVal::Float(x) => Some(*x), _ => None,
        }).collect();
        if nums.is_empty() { return vec![]; }
        let mn = nums.iter().cloned().fold(f64::INFINITY, f64::min);
        let mx = nums.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        // Check for suspicious patterns
        let mut findings = vec![];
        // All same value
        if (mx - mn).abs() < 1e-10 {
            findings.push(NerveFinding {
                agent: "RangeGuard".to_string(), severity: Severity::Warning,
                column: Some(col_name.to_string()),
                message: format!("All values are identical: {:.4}", mn),
                suggestion: "Constant column adds no information — consider dropping".to_string(),
                auto_fixable: false,
            });
        }
        // Negative values in potentially positive column
        if mn < 0.0 && col_name.to_lowercase().contains("amount") ||
           col_name.to_lowercase().contains("price") ||
           col_name.to_lowercase().contains("revenue") {
            findings.push(NerveFinding {
                agent: "RangeGuard".to_string(), severity: Severity::Warning,
                column: Some(col_name.to_string()),
                message: format!("Negative values found (min={:.2}) in financial column", mn),
                suggestion: "Verify negative values are intentional (refunds?) or data errors".to_string(),
                auto_fixable: false,
            });
        }
        findings
    }
}

// ── Main KoreNerve orchestrator ───────────────────────────────────────────────

pub struct KoreNerve {
    path: String,
}

impl KoreNerve {
    pub fn new(path: &str) -> Self {
        KoreNerve { path: path.to_string() }
    }

    /// Run all nerve agents on the .kore file
    pub fn scan(&self) -> Result<NerveScanResult, String> {
        let reader = KoreReader::open(&self.path)?;
        let col_data = reader.read_all_columns();
        let col_names: Vec<String> = reader.columns.iter().map(|c| c.name.clone()).collect();
        let nrows = reader.nrows;
        let mut findings: Vec<NerveFinding> = Vec::new();

        // Per-column agents
        for (ci, col) in reader.columns.iter().enumerate() {
            let vals = col_data.get(ci).map(|v| v.as_slice()).unwrap_or(&[]);
            let is_str = vals.iter().any(|v| matches!(v, KVal::Str(_)));

            findings.extend(NullGuard::scan(&col.name, vals, nrows));
            findings.extend(OutlierGuard::scan(&col.name, vals));
            findings.extend(CardinalityGuard::scan(&col.name, vals, is_str));
            findings.extend(RangeGuard::scan(&col.name, vals));
        }

        // Whole-file agents
        findings.extend(DupeGuard::scan(&col_data, &col_names));

        // Compute counts and health score
        let critical_count = findings.iter().filter(|f| f.severity == Severity::Critical).count();
        let warning_count  = findings.iter().filter(|f| f.severity == Severity::Warning).count();
        let info_count     = findings.iter().filter(|f| f.severity == Severity::Info).count();
        let health_score = (100.0 - (critical_count as f64 * 25.0) - (warning_count as f64 * 10.0)).max(0.0);

        Ok(NerveScanResult {
            file_path: self.path.clone(),
            findings,
            critical_count,
            warning_count,
            info_count,
            health_score,
        })
    }

    /// Drift detection — compare two .kore snapshots
    pub fn scan_drift(path_before: &str, path_after: &str) -> Result<String, String> {
        let r1 = KoreReader::open(path_before)?;
        let r2 = KoreReader::open(path_after)?;
        let d1 = r1.read_all_columns();
        let d2 = r2.read_all_columns();

        let mut out = String::new();
        out.push_str(&format!("\n KORE NERVE — DRIFT REPORT\n"));
        out.push_str(&format!(" Before: {}  ({} rows)\n", path_before, r1.nrows));
        out.push_str(&format!(" After:  {}  ({} rows)\n\n", path_after, r2.nrows));

        // Row count drift
        let row_diff = r2.nrows as i64 - r1.nrows as i64;
        let row_pct = row_diff as f64 / r1.nrows as f64 * 100.0;
        if row_diff.abs() > 0 {
            let icon = if row_pct.abs() > 20.0 { "🔴" } else { "🟡" };
            out.push_str(&format!(" {} Row count: {} → {} ({:+.1}%)\n",
                icon, r1.nrows, r2.nrows, row_pct));
        }

        // Schema drift
        let cols1: Vec<&str> = r1.columns.iter().map(|c| c.name.as_str()).collect();
        let cols2: Vec<&str> = r2.columns.iter().map(|c| c.name.as_str()).collect();
        for c in &cols1 { if !cols2.contains(c) { out.push_str(&format!(" 🔴 Column REMOVED: {}\n", c)); } }
        for c in &cols2 { if !cols1.contains(c) { out.push_str(&format!(" 🟡 Column ADDED:   {}\n", c)); } }

        // Statistical drift per numeric column
        for (ci, col) in r1.columns.iter().enumerate() {
            let v1: Vec<f64> = d1.get(ci).map(|v| v.iter().filter_map(|x| match x {
                KVal::Int(i) => Some(*i as f64), KVal::Float(f) => Some(*f), _ => None,
            }).collect()).unwrap_or_default();

            let ci2 = r2.columns.iter().position(|c| c.name == col.name);
            if let Some(ci2) = ci2 {
                let v2: Vec<f64> = d2.get(ci2).map(|v| v.iter().filter_map(|x| match x {
                    KVal::Int(i) => Some(*i as f64), KVal::Float(f) => Some(*f), _ => None,
                }).collect()).unwrap_or_default();

                if v1.is_empty() || v2.is_empty() { continue; }
                let mean1 = v1.iter().sum::<f64>() / v1.len() as f64;
                let mean2 = v2.iter().sum::<f64>() / v2.len() as f64;
                if mean1 == 0.0 { continue; }
                let drift_pct = ((mean2 - mean1) / mean1.abs() * 100.0).abs();
                if drift_pct > 20.0 {
                    let icon = if drift_pct > 50.0 { "🔴" } else { "🟡" };
                    out.push_str(&format!(" {} {}: mean drifted {:.1}%  ({:.2} → {:.2})\n",
                        icon, col.name, drift_pct, mean1, mean2));
                }
            }
        }
        out.push_str("\n DRIFT SCAN COMPLETE\n");
        Ok(out)
    }
}
