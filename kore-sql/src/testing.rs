//! Switches for differential testing: the same query can be run through the fast paths, the general
//! row-based tail only, and without the column-at-a-time predicate evaluation, and the answers compared.

use std::sync::atomic::{AtomicBool, Ordering};

pub(crate) static FORCE_GENERAL: AtomicBool = AtomicBool::new(false);
pub(crate) static NO_VECEXPR: AtomicBool = AtomicBool::new(false);
pub(crate) static NO_FUSED: AtomicBool = AtomicBool::new(false);

/// Run every SELECT through the general tail (general.rs) instead of the specialised executor paths.
pub fn set_force_general(on: bool) { FORCE_GENERAL.store(on, Ordering::Relaxed); }

/// Evaluate predicates and numeric expressions with the row interpreter only.
pub fn set_no_vecexpr(on: bool) { NO_VECEXPR.store(on, Ordering::Relaxed); }

#[inline]
pub(crate) fn force_general() -> bool { FORCE_GENERAL.load(Ordering::Relaxed) }

#[inline]
pub(crate) fn no_vecexpr() -> bool { NO_VECEXPR.load(Ordering::Relaxed) }

/// Disable the fused filter+aggregate path (fusedagg.rs) so it can be compared with the ordinary one.
pub fn set_no_fused(on: bool) { NO_FUSED.store(on, Ordering::Relaxed); }

#[inline]
pub(crate) fn no_fused() -> bool { NO_FUSED.load(Ordering::Relaxed) }
