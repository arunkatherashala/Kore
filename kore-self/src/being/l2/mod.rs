//! Track L2 (opt-in): learning to learn across task families. A hierarchical (empirical-Bayes) meta-prior over
//! what earlier families taught the being (which rule forms families have, how often a wrong form agrees by
//! chance, how reliable its own fresh rules are), used to accept a new family's rule from fewer of its own
//! answers and to trust a fresh rule sooner. Default off; a World O being gets it only when created with it
//! and only when `controller.txt` says `l2_meta_prior = on` (`ctl.rs`, fail closed). See docs/measured.md,
//! Track L2. Author-defined simulation; the only claims are the measured numbers.

pub mod ctl;
pub mod meta;
pub mod wo;
#[cfg(test)]
mod tests;
