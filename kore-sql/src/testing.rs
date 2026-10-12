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

pub(crate) static NO_DATE_ACCEL: AtomicBool = AtomicBool::new(false);

/// Disable the ISO-date integer accelerator (dateaccel.rs) so it can be compared with the string path.
pub fn set_no_date_accel(on: bool) { NO_DATE_ACCEL.store(on, Ordering::Relaxed); }

#[inline]
pub(crate) fn no_date_accel() -> bool {
    // KORE_NO_DATE_ACCEL=1 turns the accelerator off for A/B timing of whole runs
    static ENV: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    NO_DATE_ACCEL.load(Ordering::Relaxed) || *ENV.get_or_init(|| std::env::var_os("KORE_NO_DATE_ACCEL").is_some())
}

/// How many times the date accelerator served a column so far (lets tests assert it was really exercised).
pub fn date_accel_hits() -> usize { crate::dateaccel::hits() }

pub(crate) static NO_HASH_RESIDUAL: AtomicBool = AtomicBool::new(false);

/// Send every explicit join with a complex ON condition through the old row-at-a-time nested loop (A/B oracle).
pub fn set_no_hash_residual(on: bool) { NO_HASH_RESIDUAL.store(on, Ordering::Relaxed); }

#[inline]
pub(crate) fn no_hash_residual() -> bool { NO_HASH_RESIDUAL.load(Ordering::Relaxed) }
