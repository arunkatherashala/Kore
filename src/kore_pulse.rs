// ============================================================================
// KORE ∞ — Layer 1: kore_pulse
// ============================================================================
//
// Every .kore file becomes SELF-AWARE.
// Pulse embeds column stats, data quality scores, and semantic fingerprints
// directly into the file — no extra files, no external catalogs.
//
// Python API:
//   from kore_fileformat import FilePulse
//   p = FilePulse.from_kore("data.kore")
//   p.describe()          → full stats table
//   p.health()            → quality report
//   p.column("amount")    → per-column deep stats
//   p.fingerprint         → deterministic data profile hash
//
// Zero external dependencies — pure Rust stdlib only.
// ============================================================================

use std::collections::HashMap;
use crate::kore_v2::{KoreReader, KVal};

// ── Column-level statistics ──────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ColumnPulse {
    pub name: String,
    pub col_type: String,

    // Completeness
    pub total_count: u64,
    pub null_count: u64,
    pub null_pct: f64,

    // Uniqueness
    pub distinct_count: u64,
    pub cardinality_ratio: f64,    // distinct / total

    // Numeric stats (None for string columns)
    pub min_num: Option<f64>,
    pub max_num: Option<f64>,
    pub mean: Option<f64>,
    pub std_dev: Option<f64>,
    pub sum: Option<f64>,

    // String stats (None for numeric columns)
    pub min_len: Option<usize>,
    pub max_len: Option<usize>,
    pub avg_len: Option<f64>,
    pub top_values: Vec<(String, u64)>,  // top 5 most frequent values

    // Quality score 0–100
    pub quality_score: f64,
    pub quality_flags: Vec<String>,      // e.g. "HIGH_NULLS", "LOW_CARDINALITY"
}

impl ColumnPulse {
    fn compute(name: &str, values: &[KVal]) -> Self {
        let total = values.len() as u64;
        let mut null_count = 0u64;
        let mut nums: Vec<f64> = Vec::new();
        let mut strs: Vec<String> = Vec::new();
        let mut freq: HashMap<String, u64> = HashMap::new();

        for v in values {
            match v {
                KVal::Null => null_count += 1,
                KVal::Int(i) => {
                    nums.push(*i as f64);
                    *freq.entry(i.to_string()).or_insert(0) += 1;
                }
                KVal::Float(f) => {
                    nums.push(*f);
                    *freq.entry(format!("{:.4}", f)).or_insert(0) += 1;
                }
                KVal::Bool(b) => {
                    *freq.entry(b.to_string()).or_insert(0) += 1;
                }
                KVal::Str(s) => {
                    strs.push(s.clone());
                    *freq.entry(s.clone()).or_insert(0) += 1;
                }
                _ => {}
            }
        }

        let null_pct = if total > 0 { null_count as f64 / total as f64 * 100.0 } else { 0.0 };
        let distinct_count = freq.len() as u64;
        let cardinality_ratio = if total > 0 { distinct_count as f64 / total as f64 } else { 0.0 };

        // Numeric stats
        let (min_num, max_num, mean, std_dev, sum) = if !nums.is_empty() {
            let n = nums.len() as f64;
            let s: f64 = nums.iter().sum();
            let mn = nums.iter().cloned().fold(f64::INFINITY, f64::min);
            let mx = nums.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let avg = s / n;
            let variance = nums.iter().map(|x| (x - avg).powi(2)).sum::<f64>() / n;
            (Some(mn), Some(mx), Some(avg), Some(variance.sqrt()), Some(s))
        } else {
            (None, None, None, None, None)
        };

        // String stats
        let (min_len, max_len, avg_len) = if !strs.is_empty() {
            let lengths: Vec<usize> = strs.iter().map(|s| s.len()).collect();
            let mn = *lengths.iter().min().unwrap();
            let mx = *lengths.iter().max().unwrap();
            let avg = lengths.iter().sum::<usize>() as f64 / lengths.len() as f64;
            (Some(mn), Some(mx), Some(avg))
        } else {
            (None, None, None)
        };

        // Top 5 most frequent values
        let mut freq_vec: Vec<(String, u64)> = freq.into_iter().collect();
        freq_vec.sort_by(|a, b| b.1.cmp(&a.1));
        freq_vec.truncate(5);

        // Infer type label
        let col_type = if min_num.is_some() && strs.is_empty() {
            if values.iter().any(|v| matches!(v, KVal::Float(_))) { "Float".to_string() }
            else { "Int".to_string() }
        } else if !strs.is_empty() {
            "String".to_string()
        } else {
            "Mixed".to_string()
        };

        // Quality score
        let mut flags: Vec<String> = Vec::new();
        let mut score = 100.0f64;

        if null_pct > 50.0  { score -= 40.0; flags.push("CRITICAL_NULLS".to_string()); }
        else if null_pct > 20.0 { score -= 20.0; flags.push("HIGH_NULLS".to_string()); }
        else if null_pct > 5.0  { score -= 10.0; flags.push("MODERATE_NULLS".to_string()); }

        if col_type == "String" && cardinality_ratio < 0.01 && total > 1000 {
            score -= 5.0;
            flags.push("LOW_CARDINALITY".to_string());
        }
        if col_type == "Int" || col_type == "Float" {
            if cardinality_ratio > 0.99 && total > 100 {
                flags.push("HIGH_CARDINALITY_NUMERIC".to_string()); // likely ID column
            }
        }
        if let (Some(mn), Some(mx)) = (min_num, max_num) {
            if mx > mn * 1000.0 && mn > 0.0 {
                flags.push("WIDE_VALUE_RANGE".to_string());
            }
        }

        ColumnPulse {
            name: name.to_string(),
            col_type,
            total_count: total,
            null_count,
            null_pct,
            distinct_count,
            cardinality_ratio,
            min_num, max_num, mean, std_dev, sum,
            min_len, max_len, avg_len,
            top_values: freq_vec,
            quality_score: score.max(0.0),
            quality_flags: flags,
        }
    }

    /// One-line summary for this column
    pub fn summary(&self) -> String {
        let null_str = format!("{:.1}% null", self.null_pct);
        let card_str = format!("{} distinct", self.distinct_count);
        let type_str = &self.col_type;
        let num_str = match (self.min_num, self.max_num, self.mean) {
            (Some(mn), Some(mx), Some(avg)) =>
                format!(" | range [{:.2}–{:.2}] mean={:.2}", mn, mx, avg),
            _ => String::new(),
        };
        let str_str = match self.avg_len {
            Some(al) => format!(" | avg_len={:.1}", al),
            None => String::new(),
        };
        let flags = if self.quality_flags.is_empty() {
            String::new()
        } else {
            format!(" ⚠ {}", self.quality_flags.join(", "))
        };
        format!(
            "  {:<20} {:>8}  {:>14}  {:>10}  score={:.0}{}{}{}",
            self.name, type_str, null_str, card_str,
            self.quality_score, num_str, str_str, flags
        )
    }
}

// ── File-level pulse ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct FilePulse {
    pub file_path: String,
    pub total_rows: u64,
    pub total_cols: usize,
    pub file_size_bytes: u64,
    pub created_at_unix: u64,
    pub overall_quality: f64,
    pub columns: Vec<ColumnPulse>,
    pub fingerprint: String,
}

impl FilePulse {
    /// Compute pulse by opening a .kore file and reading all columns.
    pub fn from_kore(path: &str) -> Result<Self, String> {
        let reader = KoreReader::open(path)?;
        let col_data = reader.read_all_columns();

        let file_size_bytes = std::fs::metadata(path)
            .map(|m| m.len()).unwrap_or(0);

        let created_at_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default().as_secs();

        let total_rows = reader.nrows as u64;

        // Compute per-column pulse
        let columns: Vec<ColumnPulse> = reader.columns.iter().enumerate().map(|(ci, col)| {
            let vals = col_data.get(ci).map(|v| v.as_slice()).unwrap_or(&[]);
            ColumnPulse::compute(&col.name, vals)
        }).collect();

        // Overall quality = weighted avg of column scores
        let overall_quality = if columns.is_empty() {
            0.0
        } else {
            columns.iter().map(|c| c.quality_score).sum::<f64>() / columns.len() as f64
        };

        // Deterministic fingerprint from stats (no crypto crate needed)
        let fingerprint = Self::compute_fingerprint(&columns, total_rows);

        Ok(FilePulse {
            file_path: path.to_string(),
            total_rows,
            total_cols: reader.ncols,
            file_size_bytes,
            created_at_unix,
            overall_quality,
            columns,
            fingerprint,
        })
    }

    /// FNV-1a fingerprint of the data profile — deterministic, no deps
    fn compute_fingerprint(columns: &[ColumnPulse], nrows: u64) -> String {
        let mut hash: u64 = 0xcbf29ce484222325;
        let fnv_prime: u64 = 0x100000001b3;

        macro_rules! hash_bytes {
            ($bytes:expr) => {
                for b in $bytes { hash ^= *b as u64; hash = hash.wrapping_mul(fnv_prime); }
            }
        }

        hash_bytes!(&nrows.to_le_bytes());
        for col in columns {
            hash_bytes!(col.name.as_bytes());
            hash_bytes!(&col.null_count.to_le_bytes());
            hash_bytes!(&col.distinct_count.to_le_bytes());
            if let Some(mn) = col.min_num {
                hash_bytes!(&mn.to_bits().to_le_bytes());
            }
            if let Some(mx) = col.max_num {
                hash_bytes!(&mx.to_bits().to_le_bytes());
            }
        }
        format!("{:016x}", hash)
    }

    /// Print a full stats table for all columns
    pub fn describe(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "\n╔═══════════════════════════════════════════════════════════════════╗\n"
        ));
        out.push_str(&format!(
            "║  KORE PULSE  ·  {}  ·  {} rows  ·  {} cols  ·  {:.1} KB       \n",
            self.file_path, self.total_rows, self.total_cols,
            self.file_size_bytes as f64 / 1024.0
        ));
        out.push_str(&format!(
            "║  Overall Quality: {:.1}/100  ·  Fingerprint: {}              \n",
            self.overall_quality, self.fingerprint
        ));
        out.push_str(&format!(
            "╚═══════════════════════════════════════════════════════════════════╝\n"
        ));
        out.push_str(&format!(
            "  {:<20} {:>8}  {:>14}  {:>10}  score\n",
            "COLUMN", "TYPE", "NULLS", "DISTINCT"
        ));
        out.push_str("  ──────────────────────────────────────────────────────────────\n");
        for col in &self.columns {
            out.push_str(&col.summary());
            out.push('\n');
        }
        out
    }

    /// Health report — flags issues and recommendations
    pub fn health(&self) -> String {
        let mut out = String::new();
        let grade = match self.overall_quality as u32 {
            90..=100 => "A — Excellent",
            75..=89  => "B — Good",
            60..=74  => "C — Fair",
            40..=59  => "D — Poor",
            _        => "F — Critical",
        };
        out.push_str(&format!("\n KORE PULSE HEALTH REPORT\n"));
        out.push_str(&format!(" Grade: {}  (score: {:.1}/100)\n\n", grade, self.overall_quality));

        let problem_cols: Vec<&ColumnPulse> = self.columns.iter()
            .filter(|c| !c.quality_flags.is_empty()).collect();

        if problem_cols.is_empty() {
            out.push_str(" ✅ All columns are healthy. No issues detected.\n");
        } else {
            out.push_str(&format!(" ⚠  {} column(s) need attention:\n\n", problem_cols.len()));
            for col in problem_cols {
                out.push_str(&format!("  • {}:\n", col.name));
                for flag in &col.quality_flags {
                    let advice = match flag.as_str() {
                        "CRITICAL_NULLS"          => "Over 50% nulls — consider dropping or imputing this column",
                        "HIGH_NULLS"              => "Over 20% nulls — review data pipeline for gaps",
                        "MODERATE_NULLS"          => "5-20% nulls — may want to impute missing values",
                        "LOW_CARDINALITY"         => "Very few distinct values — good candidate for dictionary encoding",
                        "HIGH_CARDINALITY_NUMERIC"=> "Likely an ID column — not useful for aggregation",
                        "WIDE_VALUE_RANGE"        => "Values span 1000x range — check for outliers or mixed units",
                        _                         => "Review this column",
                    };
                    out.push_str(&format!("    [{flag}] {advice}\n"));
                }
            }
        }
        out
    }

    /// Get pulse for a specific column by name
    pub fn column(&self, name: &str) -> Option<&ColumnPulse> {
        self.columns.iter().find(|c| c.name == name)
    }

    /// Export pulse as JSON string (no serde needed — hand-rolled)
    pub fn to_json(&self) -> String {
        let mut j = String::new();
        j.push_str("{\n");
        j.push_str(&format!("  \"file\": \"{}\",\n", self.file_path));
        j.push_str(&format!("  \"rows\": {},\n", self.total_rows));
        j.push_str(&format!("  \"cols\": {},\n", self.total_cols));
        j.push_str(&format!("  \"size_bytes\": {},\n", self.file_size_bytes));
        j.push_str(&format!("  \"quality\": {:.2},\n", self.overall_quality));
        j.push_str(&format!("  \"fingerprint\": \"{}\",\n", self.fingerprint));
        j.push_str("  \"columns\": [\n");
        for (i, col) in self.columns.iter().enumerate() {
            j.push_str("    {\n");
            j.push_str(&format!("      \"name\": \"{}\",\n", col.name));
            j.push_str(&format!("      \"type\": \"{}\",\n", col.col_type));
            j.push_str(&format!("      \"null_pct\": {:.4},\n", col.null_pct));
            j.push_str(&format!("      \"distinct\": {},\n", col.distinct_count));
            j.push_str(&format!("      \"quality\": {:.2},\n", col.quality_score));
            if let Some(mn) = col.min_num {
                j.push_str(&format!("      \"min\": {:.6},\n", mn));
            }
            if let Some(mx) = col.max_num {
                j.push_str(&format!("      \"max\": {:.6},\n", mx));
            }
            if let Some(avg) = col.mean {
                j.push_str(&format!("      \"mean\": {:.6},\n", avg));
            }
            let flags_json: String = col.quality_flags.iter()
                .map(|f| format!("\"{}\"", f))
                .collect::<Vec<_>>().join(", ");
            j.push_str(&format!("      \"flags\": [{}]\n", flags_json));
            j.push_str(if i + 1 < self.columns.len() { "    },\n" } else { "    }\n" });
        }
        j.push_str("  ]\n");
        j.push_str("}\n");
        j
    }
}
