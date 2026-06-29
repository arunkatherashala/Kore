//! Layer 18 — KorePipeline
//!
//! Spark MLlib–style API:
//!   pipeline.fit(data, target)  → trains all transformers + estimator in sequence
//!   pipeline.transform(data)    → applies transformers (inference pre-processing)
//!   pipeline.predict(data)      → transform then predict

pub mod transformer;
pub mod pipeline;

pub use transformer::{LabelEncoder, MinMaxScaler, StandardScaler};
pub use pipeline::Pipeline;
