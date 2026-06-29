//! Evaluation metrics for regression and classification.

/// R² coefficient of determination.
pub fn r2(y_true: &[f64], y_pred: &[f64]) -> f64 {
    if y_true.is_empty() { return 0.0; }
    let mean = y_true.iter().sum::<f64>() / y_true.len() as f64;
    let ss_tot: f64 = y_true.iter().map(|&y| (y - mean).powi(2)).sum();
    let ss_res: f64 = y_true.iter().zip(y_pred.iter()).map(|(&t, &p)| (t - p).powi(2)).sum();
    if ss_tot == 0.0 { 1.0 } else { 1.0 - ss_res / ss_tot }
}

/// Root mean squared error.
pub fn rmse(y_true: &[f64], y_pred: &[f64]) -> f64 {
    let n = y_true.len() as f64;
    (y_true.iter().zip(y_pred.iter()).map(|(&t, &p)| (t - p).powi(2)).sum::<f64>() / n).sqrt()
}

/// Mean absolute error.
pub fn mae(y_true: &[f64], y_pred: &[f64]) -> f64 {
    let n = y_true.len() as f64;
    y_true.iter().zip(y_pred.iter()).map(|(&t, &p)| (t - p).abs()).sum::<f64>() / n
}

/// Accuracy for classification.
pub fn accuracy(y_true: &[f64], y_pred: &[f64]) -> f64 {
    let correct = y_true.iter().zip(y_pred.iter())
        .filter(|(&t, &p)| (t as i64) == (p as i64)).count();
    correct as f64 / y_true.len().max(1) as f64
}

/// Precision for binary classification (positive class = 1).
pub fn precision(y_true: &[f64], y_pred: &[f64]) -> f64 {
    let tp = y_true.iter().zip(y_pred.iter()).filter(|(&t, &p)| t >= 0.5 && p >= 0.5).count() as f64;
    let pp = y_pred.iter().filter(|&&p| p >= 0.5).count() as f64;
    if pp == 0.0 { 0.0 } else { tp / pp }
}

/// Recall for binary classification.
pub fn recall(y_true: &[f64], y_pred: &[f64]) -> f64 {
    let tp = y_true.iter().zip(y_pred.iter()).filter(|(&t, &p)| t >= 0.5 && p >= 0.5).count() as f64;
    let ap = y_true.iter().filter(|&&t| t >= 0.5).count() as f64;
    if ap == 0.0 { 0.0 } else { tp / ap }
}

/// F1 score.
pub fn f1(y_true: &[f64], y_pred: &[f64]) -> f64 {
    let p = precision(y_true, y_pred);
    let r = recall(y_true, y_pred);
    if (p + r) == 0.0 { 0.0 } else { 2.0 * p * r / (p + r) }
}

/// Confusion matrix [[tn, fp], [fn, tp]] for binary classification.
pub fn confusion_matrix(y_true: &[f64], y_pred: &[f64]) -> [[usize; 2]; 2] {
    let mut cm = [[0usize; 2]; 2];
    for (&t, &p) in y_true.iter().zip(y_pred.iter()) {
        let ti = if t >= 0.5 { 1 } else { 0 };
        let pi = if p >= 0.5 { 1 } else { 0 };
        cm[ti][pi] += 1;
    }
    cm
}
