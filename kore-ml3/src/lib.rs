//! KORE Layer 23 — Extended ML: LinearRegression, KNN, SVM, LogisticRegression
//!
//! All algorithms work on raw `&[Vec<f64>]` feature matrices and implement
//! the `kore_core::Estimator` trait for DataBlock compatibility.

pub mod linear;
pub mod knn;
pub mod svm;
pub mod logistic;
pub mod metrics;

pub use linear::LinearRegressor;
pub use knn::KNearestNeighbors;
pub use svm::LinearSVM;
pub use logistic::LogisticRegressor;
pub use metrics::*;
