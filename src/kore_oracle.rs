// ============================================================================
// KORE ∞ — Layer 4: kore_oracle
// ============================================================================
//
// Causal reasoning engine: WHY, PREDICT, WHAT IF — all in pure Rust.
// No ML frameworks, no internet, no dependencies.
//
// Techniques used:
//   Pearson correlation   → find which columns move together
//   Linear regression     → OLS in ~20 lines of Rust
//   Conditional means     → "what if category = sales?"
//   Trend detection       → slope over ordered numeric column
//
// Python API:
//   from kore_fileformat import KoreOracle
//   o = KoreOracle("data.kore")
//
//   o.why("score")                    → what correlates with score?
//   o.predict("amount", {"score": 900, "category": "sales"})
//   o.what_if("category", "sales")    → how do stats change?
//   o.correlations()                  → full correlation matrix
//   o.trend("amount")                 → is amount trending up/down?
// ============================================================================

use crate::kore_v2::{KoreReader, KVal};
use std::collections::HashMap;

// ── Math helpers (pure stdlib) ────────────────────────────────────────────────

fn mean(v: &[f64]) -> f64 {
    if v.is_empty() { return 0.0; }
    v.iter().sum::<f64>() / v.len() as f64
}

fn pearson(x: &[f64], y: &[f64]) -> f64 {
    let n = x.len().min(y.len());
    if n < 2 { return 0.0; }
    let mx = mean(&x[..n]);
    let my = mean(&y[..n]);
    let num: f64 = x[..n].iter().zip(y[..n].iter()).map(|(a, b)| (a - mx) * (b - my)).sum();
    let dx = x[..n].iter().map(|a| (a - mx).powi(2)).sum::<f64>().sqrt();
    let dy = y[..n].iter().map(|b| (b - my).powi(2)).sum::<f64>().sqrt();
    if dx == 0.0 || dy == 0.0 { return 0.0; }
    (num / (dx * dy)).clamp(-1.0, 1.0)
}

/// OLS linear regression: returns (slope, intercept)
fn ols(x: &[f64], y: &[f64]) -> (f64, f64) {
    let n = x.len().min(y.len()) as f64;
    if n < 2.0 { return (0.0, 0.0); }
    let mx = mean(x); let my = mean(y);
    let num: f64 = x.iter().zip(y.iter()).map(|(a, b)| (a - mx) * (b - my)).sum();
    let den: f64 = x.iter().map(|a| (a - mx).powi(2)).sum();
    if den == 0.0 { return (0.0, my); }
    let slope = num / den;
    (slope, my - slope * mx)
}

// ── Extract numeric columns from reader ──────────────────────────────────────

fn numeric_cols(col_data: &[Vec<KVal>]) -> Vec<(usize, Vec<f64>)> {
    col_data.iter().enumerate().filter_map(|(ci, vals)| {
        let nums: Vec<f64> = vals.iter().filter_map(|v| match v {
            KVal::Int(i) => Some(*i as f64),
            KVal::Float(f) => Some(*f),
            _ => None,
        }).collect();
        if nums.len() > 10 { Some((ci, nums)) } else { None }
    }).collect()
}

// ── Oracle ────────────────────────────────────────────────────────────────────

pub struct KoreOracle {
    path: String,
}

impl KoreOracle {
    pub fn new(path: &str) -> Self {
        KoreOracle { path: path.to_string() }
    }

    // ── WHY: what drives this column? ────────────────────────────────────────
    pub fn why(&self, target_col: &str) -> Result<String, String> {
        let reader = KoreReader::open(&self.path)?;
        let col_data = reader.read_all_columns();
        let col_names: Vec<String> = reader.columns.iter().map(|c| c.name.clone()).collect();

        let ti = col_names.iter().position(|c| c == target_col)
            .ok_or(format!("Column '{}' not found", target_col))?;
        let target_vals: Vec<f64> = col_data[ti].iter().filter_map(|v| match v {
            KVal::Int(i) => Some(*i as f64), KVal::Float(f) => Some(*f), _ => None,
        }).collect();
        if target_vals.len() < 10 {
            return Err(format!("'{}' has too few numeric values for correlation", target_col));
        }

        let num_cols = numeric_cols(&col_data);
        let mut correlations: Vec<(String, f64)> = num_cols.iter()
            .filter(|(ci, _)| *ci != ti)
            .map(|(ci, vals)| {
                let r = pearson(&target_vals, vals);
                (col_names[*ci].clone(), r)
            }).collect();
        correlations.sort_by(|a, b| b.1.abs().partial_cmp(&a.1.abs()).unwrap());

        let mut out = String::new();
        out.push_str(&format!("\n KORE ORACLE — WHY '{}'\n\n", target_col));
        out.push_str(&format!(" Mean: {:.4}  |  {} rows analyzed\n\n", mean(&target_vals), target_vals.len()));
        out.push_str(" CORRELATIONS (strongest first):\n");
        out.push_str(" ──────────────────────────────────────────────\n");

        for (name, r) in &correlations {
            let strength = match r.abs() {
                v if v > 0.7 => "STRONG",
                v if v > 0.4 => "moderate",
                v if v > 0.2 => "weak",
                _ => "negligible",
            };
            let direction = if *r > 0.0 { "↑ positive" } else { "↓ negative" };
            let bar_len = (r.abs() * 20.0) as usize;
            let bar = "█".repeat(bar_len);
            out.push_str(&format!("  {:<20} r={:+.4}  {}  {} {}\n",
                name, r, bar, strength, direction));
        }

        // Top driver
        if let Some((top_name, top_r)) = correlations.first() {
            out.push_str(&format!("\n TOP DRIVER: '{}' (r={:.4})\n", top_name, top_r));
            let direction = if *top_r > 0.0 { "increases" } else { "decreases" };
            out.push_str(&format!(" → When '{}' goes up, '{}' {} too\n",
                top_name, target_col, direction));
        }
        Ok(out)
    }

    // ── PREDICT: linear regression prediction ────────────────────────────────
    pub fn predict(&self, target_col: &str, features: HashMap<String, f64>) -> Result<String, String> {
        let reader = KoreReader::open(&self.path)?;
        let col_data = reader.read_all_columns();
        let col_names: Vec<String> = reader.columns.iter().map(|c| c.name.clone()).collect();

        let ti = col_names.iter().position(|c| c == target_col)
            .ok_or(format!("Column '{}' not found", target_col))?;
        let target_vals: Vec<f64> = col_data[ti].iter().filter_map(|v| match v {
            KVal::Int(i) => Some(*i as f64), KVal::Float(f) => Some(*f), _ => None,
        }).collect();

        let mut out = String::new();
        out.push_str(&format!("\n KORE ORACLE — PREDICT '{}'\n\n", target_col));
        out.push_str(" INPUT FEATURES:\n");
        for (k, v) in &features {
            out.push_str(&format!("   {} = {:.4}\n", k, v));
        }
        out.push_str("\n OLS REGRESSION MODELS:\n");
        out.push_str(" ──────────────────────────────────────\n");

        // Per-feature OLS
        let mut predictions: Vec<(String, f64, f64)> = Vec::new(); // (col, pred, r2)
        for (feat_name, feat_val) in &features {
            if let Some(fi) = col_names.iter().position(|c| c == feat_name) {
                let feat_vals: Vec<f64> = col_data[fi].iter().filter_map(|v| match v {
                    KVal::Int(i) => Some(*i as f64), KVal::Float(f) => Some(*f), _ => None,
                }).collect();
                let n = feat_vals.len().min(target_vals.len());
                if n < 10 { continue; }
                let (slope, intercept) = ols(&feat_vals[..n], &target_vals[..n]);
                let pred = slope * feat_val + intercept;
                // R² = 1 - SS_res/SS_tot
                let y_mean = mean(&target_vals[..n]);
                let ss_res: f64 = feat_vals[..n].iter().zip(target_vals[..n].iter())
                    .map(|(x, y)| (y - (slope * x + intercept)).powi(2)).sum();
                let ss_tot: f64 = target_vals[..n].iter().map(|y| (y - y_mean).powi(2)).sum();
                let r2 = if ss_tot > 0.0 { 1.0 - ss_res / ss_tot } else { 0.0 };
                out.push_str(&format!("  {} → {} = {:.4}*{} + {:.4}  (R²={:.4})\n",
                    feat_name, target_col, slope, feat_name, intercept, r2));
                out.push_str(&format!("    Prediction: {} = {:.2}\n\n", target_col, pred));
                predictions.push((feat_name.clone(), pred, r2));
            }
        }

        // Weighted ensemble if multiple features
        if predictions.len() > 1 {
            let total_r2: f64 = predictions.iter().map(|(_, _, r2)| r2.max(0.0)).sum();
            let ensemble = if total_r2 > 0.0 {
                predictions.iter().map(|(_, p, r2)| p * r2.max(0.0) / total_r2).sum::<f64>()
            } else {
                predictions.iter().map(|(_, p, _)| p).sum::<f64>() / predictions.len() as f64
            };
            out.push_str(&format!(" ENSEMBLE PREDICTION: {} ≈ {:.2}  (R²-weighted)\n", target_col, ensemble));
        } else if let Some((_, pred, r2)) = predictions.first() {
            out.push_str(&format!(" PREDICTION: {} ≈ {:.2}  (R²={:.4})\n", target_col, pred, r2));
        } else {
            out.push_str(" ⚠ No numeric feature columns matched — cannot predict\n");
        }
        Ok(out)
    }

    // ── WHAT IF: conditional statistics ──────────────────────────────────────
    pub fn what_if(&self, filter_col: &str, filter_val: &str) -> Result<String, String> {
        let reader = KoreReader::open(&self.path)?;
        let col_data = reader.read_all_columns();
        let col_names: Vec<String> = reader.columns.iter().map(|c| c.name.clone()).collect();

        let fi = col_names.iter().position(|c| c == filter_col)
            .ok_or(format!("Column '{}' not found", filter_col))?;
        let filter_vals = &col_data[fi];

        // Find matching rows
        let matching: Vec<usize> = filter_vals.iter().enumerate()
            .filter(|(_, v)| v.display().to_lowercase() == filter_val.to_lowercase())
            .map(|(i, _)| i).collect();

        if matching.is_empty() {
            return Ok(format!(" No rows where {} = '{}'\n", filter_col, filter_val));
        }

        let total = reader.nrows;
        let pct = matching.len() as f64 / total as f64 * 100.0;

        let mut out = String::new();
        out.push_str(&format!("\n KORE ORACLE — WHAT IF {} = '{}'\n\n", filter_col, filter_val));
        out.push_str(&format!(" {} matching rows ({:.1}% of {} total)\n\n",
            matching.len(), pct, total));
        out.push_str(" COLUMN STATS (filtered vs overall):\n");
        out.push_str(" ─────────────────────────────────────────────────────────\n");
        out.push_str(&format!(" {:<20} {:>12} {:>12} {:>10}\n",
            "COLUMN", "OVERALL_MEAN", "FILTERED_MEAN", "DELTA"));
        out.push_str(" ─────────────────────────────────────────────────────────\n");

        for (ci, col) in reader.columns.iter().enumerate() {
            if ci == fi { continue; }
            let all_nums: Vec<f64> = col_data[ci].iter().filter_map(|v| match v {
                KVal::Int(i) => Some(*i as f64), KVal::Float(f) => Some(*f), _ => None,
            }).collect();
            if all_nums.is_empty() { continue; }

            let filt_nums: Vec<f64> = matching.iter().filter_map(|&ri|
                col_data[ci].get(ri).and_then(|v| match v {
                    KVal::Int(i) => Some(*i as f64), KVal::Float(f) => Some(*f), _ => None,
                })
            ).collect();
            if filt_nums.is_empty() { continue; }

            let overall_mean = mean(&all_nums);
            let filt_mean = mean(&filt_nums);
            let delta = filt_mean - overall_mean;
            let delta_pct = if overall_mean != 0.0 { delta / overall_mean.abs() * 100.0 } else { 0.0 };
            let arrow = if delta > 0.0 { "▲" } else if delta < 0.0 { "▼" } else { "=" };
            out.push_str(&format!(" {:<20} {:>12.2} {:>12.2} {:>+8.1}%  {}\n",
                col.name, overall_mean, filt_mean, delta_pct, arrow));
        }
        Ok(out)
    }

    // ── TREND: is this column trending up or down? ────────────────────────────
    pub fn trend(&self, col_name: &str) -> Result<String, String> {
        let reader = KoreReader::open(&self.path)?;
        let col_data = reader.read_all_columns();
        let col_names: Vec<String> = reader.columns.iter().map(|c| c.name.clone()).collect();

        let ci = col_names.iter().position(|c| c == col_name)
            .ok_or(format!("Column '{}' not found", col_name))?;
        let vals: Vec<f64> = col_data[ci].iter().filter_map(|v| match v {
            KVal::Int(i) => Some(*i as f64), KVal::Float(f) => Some(*f), _ => None,
        }).collect();
        if vals.len() < 10 {
            return Err(format!("'{}' has too few values for trend analysis", col_name));
        }

        let x: Vec<f64> = (0..vals.len()).map(|i| i as f64).collect();
        let (slope, intercept) = ols(&x, &vals);
        let trend_pct = slope / mean(&vals).abs() * 100.0;

        let direction = if slope > 0.01 { "📈 UPTREND" }
            else if slope < -0.01 { "📉 DOWNTREND" }
            else { "➡️  FLAT" };

        // Split into 4 quartiles and show progression
        let q = vals.len() / 4;
        let q1 = mean(&vals[..q]);
        let q2 = mean(&vals[q..2*q]);
        let q3 = mean(&vals[2*q..3*q]);
        let q4 = mean(&vals[3*q..]);

        let mut out = String::new();
        out.push_str(&format!("\n KORE ORACLE — TREND '{}'\n\n", col_name));
        out.push_str(&format!(" {} ({:+.6} per row, {:.4}% per row)\n\n", direction, slope, trend_pct));
        out.push_str(&format!(" OLS: {} = {:.6}×(row) + {:.4}\n\n", col_name, slope, intercept));
        out.push_str(" QUARTILE PROGRESSION:\n");
        out.push_str(&format!("   Q1 (rows  0–25%) : {:.4}\n", q1));
        out.push_str(&format!("   Q2 (rows 25–50%) : {:.4}\n", q2));
        out.push_str(&format!("   Q3 (rows 50–75%) : {:.4}\n", q3));
        out.push_str(&format!("   Q4 (rows 75–100%): {:.4}\n", q4));

        let total_change = q4 - q1;
        let total_pct = if q1 != 0.0 { total_change / q1.abs() * 100.0 } else { 0.0 };
        out.push_str(&format!("\n Total drift Q1→Q4: {:+.2} ({:+.1}%)\n", total_change, total_pct));
        Ok(out)
    }

    // ── CORRELATIONS: full correlation matrix ─────────────────────────────────
    pub fn correlations(&self) -> Result<String, String> {
        let reader = KoreReader::open(&self.path)?;
        let col_data = reader.read_all_columns();
        let col_names: Vec<String> = reader.columns.iter().map(|c| c.name.clone()).collect();
        let num_cols = numeric_cols(&col_data);

        if num_cols.len() < 2 {
            return Ok(" Not enough numeric columns for correlation matrix\n".to_string());
        }

        let names: Vec<&str> = num_cols.iter().map(|(ci, _)| col_names[*ci].as_str()).collect();
        let w = 10;
        let mut out = String::new();
        out.push_str("\n KORE ORACLE — CORRELATION MATRIX\n\n");

        // Header
        out.push_str(&format!("  {:<20}", ""));
        for n in &names { out.push_str(&format!("{:>width$}", &n[..n.len().min(w)], width = w+1)); }
        out.push('\n');
        out.push_str(&format!("  {}\n", "─".repeat(22 + names.len() * (w+1))));

        for (i, (_, vals_i)) in num_cols.iter().enumerate() {
            out.push_str(&format!("  {:<20}", &names[i][..names[i].len().min(20)]));
            for (j, (_, vals_j)) in num_cols.iter().enumerate() {
                let r = if i == j { 1.0 } else { pearson(vals_i, vals_j) };
                let cell = format!("{:+.2}", r);
                out.push_str(&format!("{:>width$}", cell, width = w+1));
            }
            out.push('\n');
        }
        out.push_str("\n  Strong correlations (|r| > 0.5):\n");
        for (i, (ci_i, vals_i)) in num_cols.iter().enumerate() {
            for (j, (ci_j, vals_j)) in num_cols.iter().enumerate() {
                if j <= i { continue; }
                let r = pearson(vals_i, vals_j);
                if r.abs() > 0.5 {
                    let dir = if r > 0.0 { "positive" } else { "negative" };
                    out.push_str(&format!("  {} ↔ {}  r={:+.4}  ({})\n",
                        col_names[*ci_i], col_names[*ci_j], r, dir));
                }
            }
        }
        Ok(out)
    }
}
