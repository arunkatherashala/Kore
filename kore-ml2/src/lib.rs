//! Layer 17 — KoreML2
//!
//! Algorithms:
//!   • Random Forest  (Classifier + Regressor)   — bagging over Decision Trees
//!   • Gradient Boosting Regressor                — sequential residual fitting
//!   • Gaussian Naive Bayes                       — log-space Gaussian likelihood

pub mod decision_tree;
pub mod random_forest;
pub mod gradient_boost;
pub mod naive_bayes;
pub mod rng;

pub use decision_tree::DecisionTree;
pub use random_forest::{RandomForestClassifier, RandomForestRegressor};
pub use gradient_boost::GradientBoostingRegressor;
pub use naive_bayes::GaussianNaiveBayes;
pub use rng::Rng;
