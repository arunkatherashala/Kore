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
pub(crate) static NO_PLAN_SIP: AtomicBool = AtomicBool::new(false);
pub(crate) static NO_PLAN_ORDER: AtomicBool = AtomicBool::new(false);
pub(crate) static NO_PLAN_LAZY_BASE: AtomicBool = AtomicBool::new(false);
pub(crate) static NO_PLAN_DEFER: AtomicBool = AtomicBool::new(false);
pub(crate) static NO_PLAN_SETS_FIRST: AtomicBool = AtomicBool::new(false);
pub(crate) static PLAN_SETS_FIRST: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
pub(crate) static NO_PLAN_PACK: AtomicBool = AtomicBool::new(false);
pub(crate) static PLAN_PACKED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Disable the bitmap/hash-set SIP side channel and derived equalities (back to capped `IN (...)` lists), for A/B tests.
pub fn set_no_plan_sip(on: bool) { NO_PLAN_SIP.store(on, Ordering::Relaxed); }
/// Disable the statistics-driven comma-join order (back to lookup-first, then FROM order), for A/B tests.
pub fn set_no_plan_order(on: bool) { NO_PLAN_ORDER.store(on, Ordering::Relaxed); }
/// Load the base table of a comma join eagerly (the old behaviour), for A/B tests.
pub fn set_no_plan_lazy_base(on: bool) { NO_PLAN_LAZY_BASE.store(on, Ordering::Relaxed); }
/// Gather payload columns after every join (the old behaviour) instead of once for the final rows, for A/B tests.
pub fn set_no_plan_defer(on: bool) { NO_PLAN_DEFER.store(on, Ordering::Relaxed); }
/// Hash multi-column join keys instead of packing Int64 key tuples exactly (the old behaviour), for A/B tests.
pub fn set_no_plan_pack(on: bool) { NO_PLAN_PACK.store(on, Ordering::Relaxed); }
/// How many multi-key joins used the exact packed key so far.
pub fn plan_packed_joins() -> usize { PLAN_PACKED.load(Ordering::Relaxed) }
/// Evaluate a table's own predicate before its SIP key sets (the old order), for A/B tests.
pub fn set_no_plan_sets_first(on: bool) { NO_PLAN_SETS_FIRST.store(on, Ordering::Relaxed); }
/// How many table loads ran their key sets before the predicate so far.
pub fn plan_sets_first_loads() -> usize { PLAN_SETS_FIRST.load(Ordering::Relaxed) }
/// Turn every join-planner optimisation off (or on) at once.
pub fn set_no_plan(on: bool) { set_no_plan_sets_first(on); set_no_plan_sip(on); set_no_plan_order(on); set_no_plan_lazy_base(on); set_no_plan_defer(on); set_no_plan_pack(on); }

fn env_flag(name: &str, cell: &'static std::sync::OnceLock<bool>) -> bool {
    *cell.get_or_init(|| std::env::var_os(name).is_some())
}

#[inline]
pub(crate) fn no_plan_sip() -> bool {
    static E: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    NO_PLAN_SIP.load(Ordering::Relaxed) || env_flag("KORE_NO_PLAN_SIP", &E)
}
#[inline]
pub(crate) fn no_plan_order() -> bool {
    static E: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    NO_PLAN_ORDER.load(Ordering::Relaxed) || env_flag("KORE_NO_PLAN_ORDER", &E)
}
#[inline]
pub(crate) fn no_plan_defer() -> bool {
    static E: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    NO_PLAN_DEFER.load(Ordering::Relaxed) || env_flag("KORE_NO_PLAN_DEFER", &E)
}
#[inline]
pub(crate) fn no_plan_pack() -> bool {
    static E: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    NO_PLAN_PACK.load(Ordering::Relaxed) || env_flag("KORE_NO_PLAN_PACK", &E)
}
#[inline]
pub(crate) fn no_plan_sets_first() -> bool {
    static E: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    NO_PLAN_SETS_FIRST.load(Ordering::Relaxed) || env_flag("KORE_NO_PLAN_SETS_FIRST", &E)
}
#[inline]
pub(crate) fn no_plan_lazy_base() -> bool {
    static E: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    NO_PLAN_LAZY_BASE.load(Ordering::Relaxed) || env_flag("KORE_NO_PLAN_LAZY_BASE", &E)
}

pub(crate) static PLAN_SIP_SETS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
pub(crate) static PLAN_DERIVED_EQS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// How many SIP key sets were applied to a table load so far (lets tests assert the planner path was exercised).
pub fn plan_sip_sets() -> usize { PLAN_SIP_SETS.load(Ordering::Relaxed) }
/// How many transitive equalities the planner derived so far.
pub fn plan_derived_equalities() -> usize { PLAN_DERIVED_EQS.load(Ordering::Relaxed) }

pub(crate) static PLAN_DEFERRED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// How many tables had their payload columns gathered after the last join (late materialisation) so far.
pub fn plan_deferred_tables() -> usize { PLAN_DEFERRED.load(Ordering::Relaxed) }

pub(crate) static PLAN_ESTIMATED_PICKS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// How many join steps picked their next table from distinct-key estimates so far.
pub fn plan_estimated_picks() -> usize { PLAN_ESTIMATED_PICKS.load(Ordering::Relaxed) }

pub(crate) static NO_STMT_CACHE: AtomicBool = AtomicBool::new(false);

/// Re-parse every statement instead of reusing the cached AST.
pub fn set_no_stmt_cache(on: bool) { NO_STMT_CACHE.store(on, Ordering::Relaxed); }

#[inline]
pub(crate) fn no_stmt_cache() -> bool { NO_STMT_CACHE.load(Ordering::Relaxed) }

pub(crate) static NO_SERIAL_SMALL: AtomicBool = AtomicBool::new(false);

/// Always take the parallel code path, even for tiny inputs.
pub fn set_no_serial_small(on: bool) { NO_SERIAL_SMALL.store(on, Ordering::Relaxed); }

#[inline]
#[allow(dead_code)]
pub(crate) fn no_serial_small() -> bool { NO_SERIAL_SMALL.load(Ordering::Relaxed) }
pub(crate) static NO_DEFER_GB: AtomicBool = AtomicBool::new(false);
pub(crate) static DEFER_GB_HITS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Gather text/payload group keys before the GROUP BY (the old behaviour) instead of after it, for A/B tests.
pub fn set_no_defer_gb(on: bool) { NO_DEFER_GB.store(on, Ordering::Relaxed); }
/// How many aggregations grouped without the deferred payload keys and gathered them afterwards so far.
pub fn defer_gb_hits() -> usize { DEFER_GB_HITS.load(Ordering::Relaxed) }

#[inline]
pub(crate) fn no_defer_gb() -> bool {
    static E: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    NO_DEFER_GB.load(Ordering::Relaxed) || *E.get_or_init(|| std::env::var_os("KORE_NO_DEFER_GB").is_some())
}

pub(crate) static NO_HAVING_PUSH: AtomicBool = AtomicBool::new(false);
pub(crate) static HAVING_PUSH_HITS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Keep every group of an aggregate subquery until the outer `WHERE agg > c` runs (the old behaviour), for A/B tests.
pub fn set_no_having_push(on: bool) { NO_HAVING_PUSH.store(on, Ordering::Relaxed); }
/// How many aggregations dropped groups failing the HAVING threshold before building their output so far.
pub fn having_push_hits() -> usize { HAVING_PUSH_HITS.load(Ordering::Relaxed) }

#[inline]
pub(crate) fn no_having_push() -> bool {
    static E: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    NO_HAVING_PUSH.load(Ordering::Relaxed) || *E.get_or_init(|| std::env::var_os("KORE_NO_HAVING_PUSH").is_some())
}

pub(crate) static NO_TOPN_EXT: AtomicBool = AtomicBool::new(false);
pub(crate) static TOPN_EXT_HITS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Use the bounded top-N only for NULL-free keys without OFFSET / NULLS clauses (the old behaviour), for A/B tests.
pub fn set_no_topn_ext(on: bool) { NO_TOPN_EXT.store(on, Ordering::Relaxed); }
/// How many ORDER BY ... LIMIT queries used the NULL-aware / OFFSET-aware top-N so far.
pub fn topn_ext_hits() -> usize { TOPN_EXT_HITS.load(Ordering::Relaxed) }

#[inline]
pub(crate) fn no_topn_ext() -> bool { NO_TOPN_EXT.load(Ordering::Relaxed) }
// ---- phase 2: subquery / outer-join operators ----
pub(crate) static NO_SUBQ_DIRECT: AtomicBool = AtomicBool::new(false);
pub(crate) static SUBQ_DIRECT_HITS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Evaluate a correlated scalar aggregate subquery over a single table straight from the catalog table (filter ids +
/// one parallel pass over per-outer-key slots) instead of materialising the inner rows first, for A/B tests.
pub fn set_no_subq_direct(on: bool) { NO_SUBQ_DIRECT.store(on, Ordering::Relaxed); }
/// How many correlated scalar subqueries took the direct path so far.
pub fn subq_direct_hits() -> usize { SUBQ_DIRECT_HITS.load(Ordering::Relaxed) }
/// Turn every phase-2 subquery optimisation off (or on) at once.
pub fn set_no_subq(on: bool) { set_no_subq_direct(on); set_no_subq_leftcount(on); }

#[inline]
pub(crate) fn no_subq_direct() -> bool {
    static E: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    NO_SUBQ_DIRECT.load(Ordering::Relaxed) || env_flag("KORE_NO_SUBQ_DIRECT", &E)
}

pub(crate) static NO_SUBQ_LEFTCOUNT: AtomicBool = AtomicBool::new(false);
pub(crate) static SUBQ_LEFTCOUNT_HITS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// Evaluate `a LEFT JOIN b ... GROUP BY a.k` with COUNTs through the joined rows (the old way), for A/B tests.
pub fn set_no_subq_leftcount(on: bool) { NO_SUBQ_LEFTCOUNT.store(on, Ordering::Relaxed); }
/// How many statements took the fused left-join count path so far.
pub fn subq_leftcount_hits() -> usize { SUBQ_LEFTCOUNT_HITS.load(Ordering::Relaxed) }
#[inline]
pub(crate) fn no_subq_leftcount() -> bool {
    static E: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    NO_SUBQ_LEFTCOUNT.load(Ordering::Relaxed) || env_flag("KORE_NO_SUBQ_LEFTCOUNT", &E)
}

pub(crate) static NO_DEEP_UNCORR: AtomicBool = AtomicBool::new(false);

/// Treat a subquery that contains nested subqueries as correlated whenever any nested column is foreign to its own
/// level (the old, conservative rule), so the scope-aware check can be compared with it.
pub fn set_no_deep_uncorr(on: bool) { NO_DEEP_UNCORR.store(on, Ordering::Relaxed); }

#[inline]
pub(crate) fn no_deep_uncorr() -> bool { NO_DEEP_UNCORR.load(Ordering::Relaxed) }

/// Master switch for the p3-gap optimisations (each also has its own switch): everything off = the 830475a4 behaviour.
pub fn set_no_gap(on: bool) { set_no_deep_uncorr(on); set_no_small_dict(on); set_no_proj_take(on); set_no_sets_accel(on); set_no_semi_fuse(on); }

pub(crate) static NO_SMALL_DICT: AtomicBool = AtomicBool::new(false);

/// Keep text columns of small tables as plain strings through joins (no dictionary codes), for A/B comparison.
pub fn set_no_small_dict(on: bool) { NO_SMALL_DICT.store(on, Ordering::Relaxed); }

#[inline]
pub(crate) fn no_small_dict() -> bool { NO_SMALL_DICT.load(Ordering::Relaxed) }

pub(crate) static NO_PROJ_TAKE: AtomicBool = AtomicBool::new(false);

/// Always clone the source column for a plain-column projection that sits beside computed expressions.
pub fn set_no_proj_take(on: bool) { NO_PROJ_TAKE.store(on, Ordering::Relaxed); }

#[inline]
pub(crate) fn no_proj_take() -> bool { NO_PROJ_TAKE.load(Ordering::Relaxed) }

pub(crate) static NO_SETS_ACCEL: AtomicBool = AtomicBool::new(false);

/// Key-set-first table loads clone the date strings of the candidate rows even when the date accelerator could serve them.
pub fn set_no_sets_accel(on: bool) { NO_SETS_ACCEL.store(on, Ordering::Relaxed); }

#[inline]
pub(crate) fn no_sets_accel() -> bool { NO_SETS_ACCEL.load(Ordering::Relaxed) }

pub(crate) static NO_SEMI_FUSE: AtomicBool = AtomicBool::new(false);
pub(crate) static SEMI_FUSE_HITS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Build the inner row list of EXISTS / NOT EXISTS filters before the scan instead of filtering inside it.
pub fn set_no_semi_fuse(on: bool) { NO_SEMI_FUSE.store(on, Ordering::Relaxed); }

pub fn semi_fuse_hits() -> usize { SEMI_FUSE_HITS.load(Ordering::Relaxed) }

#[inline]
pub(crate) fn no_semi_fuse() -> bool { NO_SEMI_FUSE.load(Ordering::Relaxed) }
