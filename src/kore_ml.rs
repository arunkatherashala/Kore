use std::collections::HashMap;
use crate::kore_v2::{KoreReader, KVal};

// ============================================================================
// Result types
// ============================================================================

/// Linear regression result
#[derive(Debug, Clone)]
pub struct LinearModel {
    pub intercept:   f64,
    pub coefficients: Vec<(String, f64)>,   // (feature_name, weight)
    pub r_squared:   f64,
    pub rmse:        f64,
    pub n_samples:   usize,
}

/// K-Means cluster assignment per row
#[derive(Debug, Clone)]
pub struct ClusterResult {
    pub n_clusters:   usize,
    pub centroids:    Vec<Vec<f64>>,          // [k][feature]
    pub labels:       Vec<usize>,             // row -> cluster id
    pub inertia:      f64,                   // sum of squared distances to centroid
    pub iterations:   usize,
}

/// Decision tree split rule
#[derive(Debug, Clone)]
pub enum TreeNode {
    Leaf { class: String, count: usize },
    Split { feature: String, threshold: f64, left: Box<TreeNode>, right: Box<TreeNode>, samples: usize },
}

/// Feature importance entry
#[derive(Debug, Clone)]
pub struct FeatureImportance {
    pub feature:    String,
    pub importance: f64,   // 0..1 normalised
    pub corr:       f64,   // Pearson correlation with target
}

/// Forecast result (simple exponential smoothing)
#[derive(Debug, Clone)]
pub struct ForecastResult {
    pub history:    Vec<f64>,
    pub forecast:   Vec<f64>,
    pub alpha:      f64,
    pub mse:        f64,
}

// ============================================================================
// KoreML public API
// ============================================================================

/// Layer 8: Zero-dependency pure-Rust ML engine.
///
/// Algorithms: OLS linear regression, k-means clustering, CART decision tree,
/// Pearson feature importance, simple exponential smoothing forecast,
/// min-max / z-score normalisation, train/test split.
pub struct KoreML;

impl KoreML {
    // ── 1. OLS linear regression ─────────────────────────────────────────────

    /// Ordinary least-squares regression.
    /// `target_col` is the dependent variable; `feature_cols` are predictors.
    /// Uses the normal equation: β = (XᵀX)⁻¹Xᵀy  (Cholesky-free Gaussian elim).
    pub fn linear_regression(
        path:         &str,
        target_col:   &str,
        feature_cols: &[&str],
    ) -> Result<LinearModel, String> {
        let (cols, raw) = kload(path)?;
        let ti  = find_col(&cols, target_col)?;
        let fis: Vec<usize> = feature_cols.iter().map(|f| find_col(&cols, f)).collect::<Result<Vec<_>, _>>()?;
        let p = fis.len();          // number of features
        let n = raw.len();
        if n < p + 2 { return Err(format!("Need at least {} rows, got {}", p+2, n)); }

        // Build X (n×(p+1) with bias col) and y (n×1)
        let mut x: Vec<Vec<f64>> = Vec::with_capacity(n);
        let mut y: Vec<f64>      = Vec::with_capacity(n);
        for row in &raw {
            let yv = kf64(row.get(ti).unwrap_or(&KVal::Null));
            let mut xrow = vec![1.0f64];  // bias
            for &fi in &fis { xrow.push(kf64(row.get(fi).unwrap_or(&KVal::Null))); }
            x.push(xrow);
            y.push(yv);
        }

        // Normal equations: A = XᵀX  b = Xᵀy
        let cols_x = p + 1;
        let mut a = vec![vec![0.0f64; cols_x]; cols_x];
        let mut b = vec![0.0f64; cols_x];
        for i in 0..n {
            for j in 0..cols_x {
                b[j] += x[i][j] * y[i];
                for k in 0..cols_x { a[j][k] += x[i][j] * x[i][k]; }
            }
        }

        // Gaussian elimination with partial pivoting
        let beta = gauss_solve(&mut a, &mut b)?;

        // R² and RMSE
        let y_mean: f64 = y.iter().sum::<f64>() / n as f64;
        let ss_tot: f64 = y.iter().map(|&yi| (yi - y_mean).powi(2)).sum();
        let ss_res: f64 = (0..n).map(|i| {
            let pred: f64 = x[i].iter().zip(beta.iter()).map(|(xi, bi)| xi * bi).sum();
            (y[i] - pred).powi(2)
        }).sum();
        let r2   = if ss_tot < 1e-12 { 0.0 } else { 1.0 - ss_res / ss_tot };
        let rmse = (ss_res / n as f64).sqrt();

        let coefficients = feature_cols.iter().zip(beta[1..].iter())
            .map(|(&f, &b)| (f.to_string(), b))
            .collect();

        Ok(LinearModel { intercept: beta[0], coefficients, r_squared: r2, rmse, n_samples: n })
    }

    // ── 2. K-Means clustering ────────────────────────────────────────────────

    /// K-Means++ initialisation + Lloyd's iterations (max 300 iters or tol=1e-6).
    pub fn kmeans(
        path:         &str,
        feature_cols: &[&str],
        k:            usize,
        max_iters:    usize,
    ) -> Result<ClusterResult, String> {
        let (cols, raw) = kload(path)?;
        let fis: Vec<usize> = feature_cols.iter().map(|f| find_col(&cols, f)).collect::<Result<Vec<_>, _>>()?;
        let p = fis.len();
        let n = raw.len();
        if k == 0 || k > n { return Err(format!("k={} invalid for n={}", k, n)); }

        let data: Vec<Vec<f64>> = raw.iter().map(|row|
            fis.iter().map(|&fi| kf64(row.get(fi).unwrap_or(&KVal::Null))).collect()
        ).collect();

        // K-Means++ initialisation
        let mut centroids: Vec<Vec<f64>> = Vec::with_capacity(k);
        centroids.push(data[0].clone());
        for _ in 1..k {
            let dists: Vec<f64> = data.iter().map(|pt| {
                centroids.iter().map(|c| sq_dist(pt, c)).fold(f64::MAX, f64::min)
            }).collect();
            let total: f64 = dists.iter().sum();
            if total < 1e-12 { break; }
            let mut r = (pseudo_rand(centroids.len() as u64, n as u64) as f64 / n as f64) * total;
            let mut pick = 0;
            for (i, &d) in dists.iter().enumerate() { r -= d; if r <= 0.0 { pick = i; break; } pick = i; }
            centroids.push(data[pick].clone());
        }
        while centroids.len() < k { centroids.push(data[centroids.len() % n].clone()); }

        let iters_cap = max_iters.max(1).min(300);
        let mut labels = vec![0usize; n];
        let mut iters_done = 0;

        for _iter in 0..iters_cap {
            iters_done += 1;
            // Assignment
            let mut changed = false;
            for (i, pt) in data.iter().enumerate() {
                let c = (0..k).min_by(|&a, &b|
                    sq_dist(pt, &centroids[a]).partial_cmp(&sq_dist(pt, &centroids[b])).unwrap_or(std::cmp::Ordering::Equal)
                ).unwrap_or(0);
                if c != labels[i] { labels[i] = c; changed = true; }
            }
            if !changed { break; }
            // Update centroids
            let mut sums  = vec![vec![0.0f64; p]; k];
            let mut cnts  = vec![0usize; k];
            for (i, pt) in data.iter().enumerate() {
                let c = labels[i];
                cnts[c] += 1;
                for d in 0..p { sums[c][d] += pt[d]; }
            }
            for c in 0..k {
                if cnts[c] > 0 {
                    for d in 0..p { centroids[c][d] = sums[c][d] / cnts[c] as f64; }
                }
            }
        }

        let inertia: f64 = data.iter().enumerate().map(|(i, pt)| sq_dist(pt, &centroids[labels[i]])).sum();
        Ok(ClusterResult { n_clusters: k, centroids, labels, inertia, iterations: iters_done })
    }

    // ── 3. CART decision tree (classification, Gini impurity) ────────────────

    /// Grow a CART decision tree up to `max_depth`.  Returns root `TreeNode`.
    pub fn decision_tree(
        path:         &str,
        target_col:   &str,
        feature_cols: &[&str],
        max_depth:    usize,
    ) -> Result<TreeNode, String> {
        let (cols, raw) = kload(path)?;
        let ti  = find_col(&cols, target_col)?;
        let fis: Vec<usize> = feature_cols.iter().map(|f| find_col(&cols, f)).collect::<Result<Vec<_>, _>>()?;
        let data: Vec<(Vec<f64>, String)> = raw.iter().map(|row| {
            let feats: Vec<f64> = fis.iter().map(|&fi| kf64(row.get(fi).unwrap_or(&KVal::Null))).collect();
            let label = fmt(row.get(ti).unwrap_or(&KVal::Null));
            (feats, label)
        }).collect();
        let indices: Vec<usize> = (0..data.len()).collect();
        let fnames: Vec<String> = feature_cols.iter().map(|s| s.to_string()).collect();
        Ok(build_tree(&data, &indices, &fnames, max_depth))
    }

    // ── 4. Feature importance (Pearson r² + variance) ────────────────────────

    /// Rank features by |Pearson correlation| with the numeric target column.
    pub fn feature_importance(
        path:         &str,
        target_col:   &str,
        feature_cols: &[&str],
    ) -> Result<Vec<FeatureImportance>, String> {
        let (cols, raw) = kload(path)?;
        let ti  = find_col(&cols, target_col)?;
        let fis: Vec<usize> = feature_cols.iter().map(|f| find_col(&cols, f)).collect::<Result<Vec<_>, _>>()?;
        let n = raw.len() as f64;
        let y: Vec<f64> = raw.iter().map(|r| kf64(r.get(ti).unwrap_or(&KVal::Null))).collect();
        let ym = mean(&y);
        let yv = variance(&y, ym);

        let mut importance: Vec<FeatureImportance> = fis.iter().zip(feature_cols.iter()).map(|(&fi, &fname)| {
            let x: Vec<f64> = raw.iter().map(|r| kf64(r.get(fi).unwrap_or(&KVal::Null))).collect();
            let xm = mean(&x);
            let cov: f64 = x.iter().zip(y.iter()).map(|(&xi, &yi)| (xi - xm) * (yi - ym)).sum::<f64>() / n;
            let xv = variance(&x, xm);
            let corr = if xv < 1e-12 || yv < 1e-12 { 0.0 } else { cov / (xv.sqrt() * yv.sqrt()) };
            FeatureImportance { feature: fname.to_string(), importance: corr.abs(), corr }
        }).collect();

        // Normalise importance to sum=1
        let total: f64 = importance.iter().map(|f| f.importance).sum();
        if total > 1e-12 { for fi in &mut importance { fi.importance /= total; } }
        importance.sort_by(|a, b| b.importance.partial_cmp(&a.importance).unwrap_or(std::cmp::Ordering::Equal));
        Ok(importance)
    }

    // ── 5. Simple exponential smoothing forecast ─────────────────────────────

    /// Fit simple exponential smoothing (SES) on `value_col` rows, then forecast `horizon` steps.
    /// `alpha` ∈ (0,1]: smoothing factor. If 0.0, auto-fit by minimising MSE (golden section).
    pub fn forecast(
        path:      &str,
        value_col: &str,
        horizon:   usize,
        alpha:     f64,
    ) -> Result<ForecastResult, String> {
        let (cols, raw) = kload(path)?;
        let vi  = find_col(&cols, value_col)?;
        let series: Vec<f64> = raw.iter().map(|r| kf64(r.get(vi).unwrap_or(&KVal::Null))).collect();
        if series.is_empty() { return Err("Empty series".into()); }

        let a = if alpha <= 0.0 || alpha > 1.0 {
            // Golden-section search for best alpha in (0.01, 0.99)
            let mut lo = 0.01f64; let mut hi = 0.99f64;
            for _ in 0..40 {
                let m1 = hi - (hi - lo) / 1.618;
                let m2 = lo + (hi - lo) / 1.618;
                if ses_mse(&series, m1) < ses_mse(&series, m2) { hi = m2; } else { lo = m1; }
            }
            (lo + hi) / 2.0
        } else { alpha };

        let history = ses_smooth(&series, a);
        let mut last = *history.last().unwrap_or(&series[0]);
        let forecast: Vec<f64> = (0..horizon).map(|_| { last = a * series.last().cloned().unwrap_or(last) + (1.0 - a) * last; last }).collect();
        let mse = ses_mse(&series, a);
        Ok(ForecastResult { history, forecast, alpha: a, mse })
    }

    // ── 6. Normalise columns ─────────────────────────────────────────────────

    /// Min-max normalise specified columns to [0, 1].
    /// Returns (headers, normalised_rows_as_strings).
    pub fn normalize_minmax(
        path:    &str,
        columns: &[&str],
    ) -> Result<(Vec<String>, Vec<Vec<String>>), String> {
        let (cols, raw) = kload(path)?;
        let idxs: Vec<usize> = columns.iter().map(|c| find_col(&cols, c)).collect::<Result<Vec<_>,_>>()?;
        let mut mins = vec![f64::MAX; idxs.len()];
        let mut maxs = vec![f64::MIN; idxs.len()];
        for row in &raw {
            for (k, &ci) in idxs.iter().enumerate() {
                let f = kf64(row.get(ci).unwrap_or(&KVal::Null));
                if f < mins[k] { mins[k] = f; }
                if f > maxs[k] { maxs[k] = f; }
            }
        }
        let result: Vec<Vec<String>> = raw.iter().map(|row| {
            let mut r: Vec<String> = row.iter().map(fmt).collect();
            for (k, &ci) in idxs.iter().enumerate() {
                let f = kf64(row.get(ci).unwrap_or(&KVal::Null));
                let rng = maxs[k] - mins[k];
                let norm = if rng < 1e-12 { 0.0 } else { (f - mins[k]) / rng };
                r[ci] = format!("{:.6}", norm);
            }
            r
        }).collect();
        Ok((cols, result))
    }

    // ── 7. Train / test split ────────────────────────────────────────────────

    /// Split rows into train / test sets (deterministic shuffle via LCG).
    /// Returns (train_headers, train_rows, test_headers, test_rows).
    pub fn train_test_split(
        path:       &str,
        test_ratio: f64,
    ) -> Result<(Vec<String>, Vec<Vec<String>>, Vec<String>, Vec<Vec<String>>), String> {
        let (cols, raw) = kload(path)?;
        let n = raw.len();
        let mut indices: Vec<usize> = (0..n).collect();
        // LCG shuffle (deterministic, seed=42)
        lcg_shuffle(&mut indices, 42);
        let test_n = ((n as f64) * test_ratio.clamp(0.01, 0.99)).round() as usize;
        let (test_idx, train_idx) = indices.split_at(test_n);
        let fmt_rows = |idxs: &[usize]| -> Vec<Vec<String>> {
            idxs.iter().map(|&i| raw[i].iter().map(fmt).collect()).collect()
        };
        Ok((cols.clone(), fmt_rows(train_idx), cols, fmt_rows(test_idx)))
    }

    // ── 8. Render a tree as text ─────────────────────────────────────────────

    pub fn tree_to_string(node: &TreeNode) -> String {
        let mut out = String::new();
        render_tree(node, &mut out, "", true);
        out
    }

    // ── 9. Render table ──────────────────────────────────────────────────────

    pub fn table_str(headers: &[String], rows: &[Vec<String>]) -> String {
        render(headers, rows)
    }
}

// ============================================================================
// Decision tree internals
// ============================================================================

fn build_tree(data: &[(Vec<f64>, String)], idxs: &[usize], fnames: &[String], depth: usize) -> TreeNode {
    if idxs.is_empty() { return TreeNode::Leaf { class: "?".into(), count: 0 }; }
    // Majority class
    let mut freq: HashMap<&str, usize> = HashMap::new();
    for &i in idxs { *freq.entry(data[i].1.as_str()).or_insert(0) += 1; }
    let majority = freq.iter().max_by_key(|(_, &c)| c).map(|(&s, _)| s.to_string()).unwrap_or_default();

    if depth == 0 || freq.len() == 1 {
        return TreeNode::Leaf { class: majority, count: idxs.len() };
    }

    let p = fnames.len();
    let mut best_gini  = f64::MAX;
    let mut best_feat  = 0usize;
    let mut best_thr   = 0.0f64;
    let n = idxs.len() as f64;

    for fi in 0..p {
        let mut vals: Vec<f64> = idxs.iter().map(|&i| data[i].0[fi]).collect();
        vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        vals.dedup_by(|a, b| (*a - *b).abs() < 1e-12);

        // Limit to at most 100 candidate thresholds (evenly sampled) for large datasets
        const MAX_THRESHOLDS: usize = 100;
        let step = ((vals.len().saturating_sub(1)) / MAX_THRESHOLDS).max(1);

        let mut wi = 0;
        while wi + 1 < vals.len() {
            let thr = (vals[wi] + vals[wi+1]) / 2.0;
            let (left, right): (Vec<_>, Vec<_>) = idxs.iter().partition(|&&i| data[i].0[fi] <= thr);
            let gl = gini(&data, &left);
            let gr = gini(&data, &right);
            let g  = (left.len() as f64 / n) * gl + (right.len() as f64 / n) * gr;
            if g < best_gini { best_gini = g; best_feat = fi; best_thr = thr; }
            wi += step;
        }
    }

    if best_gini >= f64::MAX - 1.0 { return TreeNode::Leaf { class: majority, count: idxs.len() }; }
    let (left_idxs, right_idxs): (Vec<_>, Vec<_>) = idxs.iter().copied().partition(|&i| data[i].0[best_feat] <= best_thr);
    if left_idxs.is_empty() || right_idxs.is_empty() {
        return TreeNode::Leaf { class: majority, count: idxs.len() };
    }

    TreeNode::Split {
        feature:   fnames[best_feat].clone(),
        threshold: best_thr,
        samples:   idxs.len(),
        left:      Box::new(build_tree(data, &left_idxs,  fnames, depth-1)),
        right:     Box::new(build_tree(data, &right_idxs, fnames, depth-1)),
    }
}

fn gini(data: &[(Vec<f64>, String)], idxs: &[usize]) -> f64 {
    if idxs.is_empty() { return 0.0; }
    let mut freq: HashMap<&str, usize> = HashMap::new();
    for &i in idxs { *freq.entry(data[i].1.as_str()).or_insert(0) += 1; }
    let n = idxs.len() as f64;
    1.0 - freq.values().map(|&c| (c as f64 / n).powi(2)).sum::<f64>()
}

fn render_tree(node: &TreeNode, out: &mut String, prefix: &str, last: bool) {
    let connector = if last { "`-- " } else { "|-- " };
    match node {
        TreeNode::Leaf { class, count } => {
            out.push_str(&format!("{}{}{} [n={}]\n", prefix, connector, class, count));
        }
        TreeNode::Split { feature, threshold, left, right, samples } => {
            out.push_str(&format!("{}{}{} <= {:.4}  [n={}]\n", prefix, connector, feature, threshold, samples));
            let child_pfx = format!("{}{}   ", prefix, if last { " " } else { "|" });
            render_tree(left,  out, &child_pfx, false);
            render_tree(right, out, &child_pfx, true);
        }
    }
}

// ============================================================================
// Gaussian elimination (no external deps)
// ============================================================================

fn gauss_solve(a: &mut Vec<Vec<f64>>, b: &mut Vec<f64>) -> Result<Vec<f64>, String> {
    let n = b.len();
    for col in 0..n {
        // Partial pivot
        let pivot = (col..n).max_by(|&r1, &r2| a[r1][col].abs().partial_cmp(&a[r2][col].abs()).unwrap_or(std::cmp::Ordering::Equal)).unwrap();
        a.swap(col, pivot); b.swap(col, pivot);
        let diag = a[col][col];
        if diag.abs() < 1e-14 { return Err("Singular matrix — check for collinear features".into()); }
        for j in col..n { a[col][j] /= diag; } b[col] /= diag;
        for row in 0..n {
            if row == col { continue; }
            let factor = a[row][col];
            for j in col..n { let tmp = a[col][j]; a[row][j] -= factor * tmp; }
            b[row] -= factor * b[col];
        }
    }
    Ok(b.clone())
}

// ============================================================================
// SES helpers
// ============================================================================

fn ses_smooth(series: &[f64], alpha: f64) -> Vec<f64> {
    let mut out = vec![series[0]];
    for &v in &series[1..] { let prev = *out.last().unwrap(); out.push(alpha * v + (1.0-alpha) * prev); }
    out
}

fn ses_mse(series: &[f64], alpha: f64) -> f64 {
    if series.len() < 2 { return f64::MAX; }
    let smoothed = ses_smooth(series, alpha);
    smoothed.iter().zip(series[1..].iter()).map(|(&s, &v)| (s - v).powi(2)).sum::<f64>() / (series.len()-1) as f64
}

// ============================================================================
// Misc math helpers
// ============================================================================

fn sq_dist(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b.iter()).map(|(x, y)| (x - y).powi(2)).sum()
}

fn mean(v: &[f64]) -> f64 { if v.is_empty() { 0.0 } else { v.iter().sum::<f64>() / v.len() as f64 } }

fn variance(v: &[f64], m: f64) -> f64 {
    if v.len() < 2 { return 0.0; }
    v.iter().map(|&x| (x - m).powi(2)).sum::<f64>() / v.len() as f64
}

fn pseudo_rand(seed: u64, modulo: u64) -> u64 {
    let x = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    x % modulo
}

fn lcg_shuffle(v: &mut Vec<usize>, seed: u64) {
    let n = v.len();
    let mut s = seed;
    for i in (1..n).rev() {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let j = (s >> 33) as usize % (i + 1);
        v.swap(i, j);
    }
}

// ============================================================================
// Data loading + formatting
// ============================================================================

fn kload(path: &str) -> Result<(Vec<String>, Vec<Vec<KVal>>), String> {
    let r     = KoreReader::open(path).map_err(|e| e.to_string())?;
    let cols: Vec<String> = r.columns.iter().map(|c| c.name.clone()).collect();
    let raw   = r.read_all_columns();
    let nrows = r.nrows;
    let ncols = raw.len();
    let mut rows: Vec<Vec<KVal>> = (0..nrows).map(|_| vec![KVal::Null; ncols]).collect();
    for (ci, col_data) in raw.iter().enumerate() {
        for (ri, val) in col_data.iter().enumerate() {
            if ri < nrows { rows[ri][ci] = val.clone(); }
        }
    }
    Ok((cols, rows))
}

fn find_col(cols: &[String], name: &str) -> Result<usize, String> {
    let sn = name.rfind('.').map(|i| &name[i+1..]).unwrap_or(name);
    cols.iter().position(|c| c.eq_ignore_ascii_case(name) || c.eq_ignore_ascii_case(sn))
        .ok_or_else(|| format!("Column '{}' not found", name))
}

fn kf64(v: &KVal) -> f64 {
    match v { KVal::Int(x) => *x as f64, KVal::Float(x) => *x, KVal::Str(s) => s.parse().unwrap_or(0.0), _ => 0.0 }
}

fn fmt(v: &KVal) -> String {
    match v {
        KVal::Int(x)   => x.to_string(),
        KVal::Float(x) => { let s = format!("{:.4}", x); s.trim_end_matches('0').trim_end_matches('.').to_string() }
        KVal::Str(s)   => s.clone(),
        KVal::Bool(b)  => b.to_string(),
        KVal::Null     => "NULL".into(),
        _              => format!("{:?}", v),
    }
}

fn render(hdrs: &[String], rows: &[Vec<String>]) -> String {
    if rows.is_empty() { return "  (no rows)\n  0 rows".into(); }
    let mut w: Vec<usize> = hdrs.iter().map(|h| h.len()).collect();
    for row in rows { for (i, c) in row.iter().enumerate() { if i < w.len() { w[i] = w[i].max(c.len()); } } }
    let sep: String = w.iter().map(|&ww| format!("+{}", "-".repeat(ww+2))).collect::<String>() + "+";
    let mut out = format!("{}\n", sep);
    out += &format!("| {} |\n", hdrs.iter().zip(&w).map(|(h,&ww)| format!("{:<ww$}", h, ww=ww)).collect::<Vec<_>>().join(" | "));
    out += &format!("{}\n", sep);
    for row in rows {
        out += &format!("| {} |\n", (0..hdrs.len()).map(|i| {
            let c = row.get(i).map(|s| s.as_str()).unwrap_or("");
            format!("{:<ww$}", c, ww=w[i])
        }).collect::<Vec<_>>().join(" | "));
    }
    out += &format!("{}\n  {} rows", sep, rows.len());
    out
}
