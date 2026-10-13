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
pub(crate) static SLOW_WINDOW: AtomicBool = AtomicBool::new(false);

/// Run window functions through the original (quadratic) reference implementation, for A/B tests.
pub fn set_slow_window(on: bool) { SLOW_WINDOW.store(on, Ordering::Relaxed); }

#[inline]
pub(crate) fn slow_window() -> bool { SLOW_WINDOW.load(Ordering::Relaxed) }

/// How often each new aggregation path served a query (tests assert they were really exercised).
pub(crate) static WIDE_HITS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
pub(crate) static SORTED_HITS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
pub(crate) static SUBSTR_DICT_HITS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// (queries with more than three key columns or float keys, sorted-run aggregations, SUBSTR dictionary columns)
pub fn agg_hits() -> (usize, usize, usize) {
    (WIDE_HITS.load(Ordering::Relaxed), SORTED_HITS.load(Ordering::Relaxed), SUBSTR_DICT_HITS.load(Ordering::Relaxed))
}

pub(crate) static FD_HITS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// Aggregations that grouped by the numeric keys only after verifying the text keys follow from them.
pub fn fd_hits() -> usize { FD_HITS.load(Ordering::Relaxed) }

pub(crate) static NO_FD_AGG: AtomicBool = AtomicBool::new(false);

/// Intern every text key column instead of verifying that it follows from the numeric ones.
pub fn set_no_fd_agg(on: bool) { NO_FD_AGG.store(on, Ordering::Relaxed); }

#[inline]
pub(crate) fn no_fd_agg() -> bool { NO_FD_AGG.load(Ordering::Relaxed) }

pub(crate) static NO_WIDE_AGG: AtomicBool = AtomicBool::new(false);

/// Limit the fused GROUP BY to the old shapes (at most three integer / text columns, serial string interning).
pub fn set_no_wide_agg(on: bool) { NO_WIDE_AGG.store(on, Ordering::Relaxed); }

#[inline]
pub(crate) fn no_wide_agg() -> bool { NO_WIDE_AGG.load(Ordering::Relaxed) }

pub(crate) static NO_SORTED_AGG: AtomicBool = AtomicBool::new(false);

/// Do not detect sorted key columns in the fused GROUP BY (always hash).
pub fn set_no_sorted_agg(on: bool) { NO_SORTED_AGG.store(on, Ordering::Relaxed); }

#[inline]
pub(crate) fn no_sorted_agg() -> bool { NO_SORTED_AGG.load(Ordering::Relaxed) }

pub(crate) static NO_SUBSTR_DICT: AtomicBool = AtomicBool::new(false);

/// SUBSTR projections always produce one String per row (no dictionary column for few distinct values).
pub fn set_no_substr_dict(on: bool) { NO_SUBSTR_DICT.store(on, Ordering::Relaxed); }

#[inline]
pub(crate) fn no_substr_dict() -> bool { NO_SUBSTR_DICT.load(Ordering::Relaxed) }

/// Turn off the parallel join mechanics (partitioned hash build, parallel concatenation / gather / composite
/// key / lazy-join pre-steps) so results can be compared with the original serial code, exactly.
pub fn set_no_par_join(on: bool) { kore_join::set_serial_join(on); }

#[inline]
pub(crate) fn no_par_join() -> bool { kore_join::serial_join() }
