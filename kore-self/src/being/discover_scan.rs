//! Incremental scan of the revealed examples (fix of the long-run bug "per-step cost grows with the
//! number of examples", docs/longrun.md, section "Rule scan fix").
//!
//! `discover::search_core` and the merge check of `housekeeping` are pure functions of the example list,
//! and that list only grows at its end (one example per attempt at a sequence task). Whether a candidate
//! `(transform, form)` reproduces, misses or does not apply to one example depends on that example
//! alone, so it never has to be computed twice. `ScanCache` keeps, per signature and candidate, how far
//! it has looked, which examples it reproduced (a bitset) and whether it was ever applicable-and-wrong;
//! per tracked rule its prediction on every example; and it extends them only over the examples that
//! arrived since. Every answer is the answer of the full rescan, bit for bit (same accepted rules, same
//! support counts, same cited examples, same candidate counts and truncation): the tests in
//! `tests_scan.rs` compare both after every new example.
//!
//! A candidate that was applicable-and-wrong once can never be accepted again (the examples it was wrong
//! on stay in the list), so its scan stops there: the original search computes its status on every
//! example but uses none of it. If the example list is replaced by one that is not an extension of
//! the scanned one (checked on the length and on the event ids of the first and of the last scanned
//! example), the cache starts over from nothing.
//!
//! Between runs (`being_run` calls) of one process the cache is kept (`stash`, `adopt`) and reused only if
//! the new run's examples begin with exactly the scanned ones (compared example for example). A new process
//! starts with an empty cache: its first pass scans every example once, spread over threads when that is
//! a lot of work (the status of each candidate on each example is computed on its own, so the answer does
//! not depend on the threads).

use super::discover::{candidates_for, is_held_out, predict_x, Example, Form, Found, SearchOut, Xf, MAX_CITE};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};

thread_local! {
    static REFERENCE: Cell<bool> = const { Cell::new(false) };
    static STASH: RefCell<Option<(ScanCache, Vec<Example>)>> = const { RefCell::new(None) };
}

/// Whether engines created on this thread start with `reference_scan` (the full rescan); false by default.
pub fn reference_default() -> bool {
    REFERENCE.with(|r| r.get())
}

/// Run `f` with engines created on this thread using the full rescan (`on`) or the incremental scan; for the
/// tests and the measurement harness that compare both. Restores the previous setting afterwards.
pub fn with_reference<T>(on: bool, f: impl FnOnce() -> T) -> T {
    let before = REFERENCE.with(|r| r.replace(on));
    let out = f();
    REFERENCE.with(|r| r.set(before));
    out
}

/// Keep the cache of a finished run for the next run on this thread (the next `being_run` call of the same
/// process), with a copy of the examples it scanned.
pub fn stash(cache: ScanCache, examples: &[Example]) {
    if cache.seen == examples.len() {
        STASH.with(|s| *s.borrow_mut() = Some((cache, examples.to_vec())));
    }
}

/// The stashed cache if the examples it scanned are, example for example (event id, signature, terms, next
/// term), the first examples of `examples`; otherwise an empty cache. Either way the stash is emptied.
pub fn adopt(examples: &[Example]) -> ScanCache {
    match STASH.with(|s| s.borrow_mut().take()) {
        Some((c, seen)) if seen.len() <= examples.len() && seen[..] == examples[..seen.len()] => c,
        _ => ScanCache::default(),
    }
}

/// From this many predictions still to make before a pass (a new engine on a long history), they are made on
/// several threads first.
const PARALLEL_FROM: usize = 20_000;

type Key = (u8, Form);

#[derive(Debug, Clone, Default)]
struct Cand {
    /// Examples of the signature scanned so far.
    upto: usize,
    /// Applicable-and-wrong on at least one scanned example.
    dead: bool,
    /// Bit i: reproduced the next term of the signature's example i.
    right: Vec<u64>,
    tr_ok: u32,
    va_ok: u32,
}

#[derive(Debug, Clone, Default)]
struct Track {
    /// Prediction on every scanned example of the signature (None: not applicable).
    preds: Vec<Option<i64>>,
    /// Number of scanned examples where it applies, and the event ids of the first `MAX_CITE` of them.
    applies: usize,
    cites: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct Pair {
    upto: usize,
    /// On every scanned example where the first applies, the second applies and predicts the same.
    agrees: bool,
}

#[derive(Debug, Clone, Default)]
struct Sig {
    /// Positions of the signature's examples in the full list, in log order.
    idx: Vec<usize>,
    /// Bit i set: example i of the signature is held out.
    val_mask: Vec<u64>,
    cands: HashMap<Key, Cand>,
    tracks: HashMap<Key, Track>,
    pairs: HashMap<(Key, Key), Pair>,
}

#[derive(Debug, Clone, Default)]
pub struct ScanCache {
    seen: usize,
    first_id: String,
    last_id: String,
    sigs: BTreeMap<u8, Sig>,
    /// Number of `predict_x` calls made by the cache (for tests and measurements).
    pub predictions: u64,
}

fn bit(v: &[u64], i: usize) -> bool {
    v.get(i / 64).map_or(false, |w| w >> (i % 64) & 1 == 1)
}

fn set(v: &mut Vec<u64>, i: usize) {
    if v.len() <= i / 64 {
        v.resize(i / 64 + 1, 0);
    }
    v[i / 64] |= 1 << (i % 64);
}

/// Scan candidate `key` over the examples of its signature not yet scanned, up to the first one where it is
/// applicable-and-wrong. Returns the number of predictions made.
fn extend(c: &mut Cand, key: Key, idx: &[usize], val_mask: &[u64], examples: &[Example]) -> u64 {
    let (xf, form) = (Xf(key.0), key.1);
    let mut predictions = 0u64;
    while !c.dead && c.upto < idx.len() {
        let k = c.upto;
        let e = &examples[idx[k]];
        predictions += 1;
        match predict_x(xf, &form, &e.terms) {
            None => {}
            Some(v) if v == e.next => {
                set(&mut c.right, k);
                if bit(val_mask, k) {
                    c.va_ok += 1;
                } else {
                    c.tr_ok += 1;
                }
            }
            Some(_) => c.dead = true,
        }
        c.upto += 1;
    }
    predictions
}

/// The order in which the search tries the candidates of one signature (transferred hypotheses first).
fn order<'c>(cands: &'c [(Xf, Form)], prefer: &[(u8, Xf, Form)], sig: u8) -> Vec<&'c (Xf, Form)> {
    if prefer.iter().any(|p| p.0 == sig) {
        let first: Vec<&(Xf, Form)> = prefer.iter().filter(|p| p.0 == sig).filter_map(|p| cands.iter().find(|c| c.0 == p.1 && c.1 == p.2)).fold(vec![], |mut v, c| {
            if !v.iter().any(|x| std::ptr::eq(*x, c)) {
                v.push(c);
            }
            v
        });
        let rest = cands.iter().filter(|c| !first.iter().any(|x| std::ptr::eq(*x, *c)));
        first.iter().copied().chain(rest).collect()
    } else {
        cands.iter().collect()
    }
}

impl ScanCache {
    pub fn new() -> ScanCache {
        ScanCache::default()
    }

    /// Approximate heap bytes held by the cache (for the measurements).
    pub fn approx_bytes(&self) -> usize {
        let mut b = 0usize;
        for s in self.sigs.values() {
            b += s.idx.capacity() * 8 + s.val_mask.capacity() * 8;
            b += s.cands.values().map(|c| 64 + c.right.capacity() * 8).sum::<usize>();
            b += s.tracks.values().map(|t| 64 + t.preds.capacity() * 16 + t.cites.iter().map(|c| 24 + c.capacity()).sum::<usize>()).sum::<usize>();
            b += s.pairs.len() * 48;
        }
        b
    }

    /// Take in the examples appended since the last call (or start over if `examples` is not an extension).
    fn sync(&mut self, examples: &[Example]) {
        let extends = examples.len() >= self.seen && (self.seen == 0 || (examples[0].event_id == self.first_id && examples[self.seen - 1].event_id == self.last_id));
        if !extends {
            *self = ScanCache { predictions: self.predictions, ..ScanCache::default() };
        }
        for (i, e) in examples.iter().enumerate().skip(self.seen) {
            let s = self.sigs.entry(e.sig).or_default();
            let k = s.idx.len();
            s.idx.push(i);
            if is_held_out(k) {
                set(&mut s.val_mask, k);
            }
        }
        if let (Some(f), Some(l)) = (examples.first(), examples.last()) {
            self.first_id = f.event_id.clone();
            self.last_id = l.event_id.clone();
        }
        self.seen = examples.len();
    }

    /// When the candidates the pass will try still have many examples to scan (a new engine on a long history),
    /// scan them on several threads first. The pass then finds them up to date; nothing else changes.
    fn pre_extend(&mut self, examples: &[Example], cands: &[(Xf, Form)], prefer: &[(u8, Xf, Form)], max_candidates: usize) -> u64 {
        let mut wanted: Vec<(u8, Key)> = vec![];
        'sigs: for sig in self.sigs.keys() {
            for (xf, form) in order(cands, prefer, *sig) {
                if wanted.len() >= max_candidates {
                    break 'sigs;
                }
                wanted.push((*sig, (xf.0, *form)));
            }
        }
        let pending: usize = wanted
            .iter()
            .map(|(sig, k)| {
                let s = &self.sigs[sig];
                s.cands.get(k).map_or(s.idx.len(), |c| if c.dead { 0 } else { s.idx.len() - c.upto })
            })
            .sum();
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(8);
        if pending < PARALLEL_FROM || threads < 2 {
            return 0;
        }
        let mut jobs: Vec<(&mut Cand, Key, &[usize], &[u64])> = vec![];
        for (sig, s) in self.sigs.iter_mut() {
            let Sig { idx, val_mask, cands, .. } = s;
            for (sg, k) in &wanted {
                if sg == sig {
                    cands.entry(*k).or_default();
                }
            }
            for (k, c) in cands.iter_mut() {
                if !c.dead && c.upto < idx.len() && wanted.iter().any(|(sg, w)| sg == sig && w == k) {
                    jobs.push((c, *k, idx.as_slice(), val_mask.as_slice()));
                }
            }
        }
        let per = jobs.len().div_ceil(threads).max(1);
        let mut total = 0u64;
        std::thread::scope(|sc| {
            let handles: Vec<_> = jobs.chunks_mut(per).map(|chunk| sc.spawn(move || chunk.iter_mut().map(|(c, k, idx, vm)| extend(c, *k, idx, vm, examples)).sum::<u64>())).collect();
            for h in handles {
                total += h.join().expect("scan thread");
            }
        });
        total
    }

    /// Exactly `discover::search_core(examples, k_train, k_val, blocks, max_candidates, prefer)`.
    pub fn search(&mut self, examples: &[Example], k_train: u32, k_val: u32, blocks: &[Form], max_candidates: usize, prefer: &[(u8, Xf, Form)]) -> SearchOut {
        self.sync(examples);
        let cands = candidates_for(blocks);
        let mut predictions = self.pre_extend(examples, &cands, prefer, max_candidates);
        let sig_keys: Vec<u8> = self.sigs.keys().copied().collect();
        let mut out = vec![];
        let (mut tried, mut truncated) = (0usize, false);
        'sigs: for sig in &sig_keys {
            let sig = *sig;
            let s = self.sigs.get_mut(&sig).expect("signature");
            let n = s.idx.len();
            let mut covered: Vec<u64> = vec![0; n.div_ceil(64)];
            for (xf, form) in order(&cands, prefer, sig) {
                if tried >= max_candidates {
                    truncated = true;
                    break 'sigs;
                }
                tried += 1;
                let c = s.cands.entry((xf.0, *form)).or_default();
                predictions += extend(c, (xf.0, *form), &s.idx, &s.val_mask, examples);
                if c.dead {
                    continue;
                }
                let (mut tr_new, mut va_new) = (0u32, 0u32);
                for (w, r) in c.right.iter().enumerate() {
                    let fresh = r & !covered.get(w).copied().unwrap_or(0);
                    let val = s.val_mask.get(w).copied().unwrap_or(0);
                    va_new += (fresh & val).count_ones();
                    tr_new += (fresh & !val).count_ones();
                }
                if tr_new >= k_train && va_new >= k_val {
                    let mut cite: Vec<String> = vec![];
                    for pass_new in [true, false] {
                        for (w, r) in c.right.iter().enumerate() {
                            let cov = covered.get(w).copied().unwrap_or(0);
                            let mut bits = if pass_new { r & !cov } else { r & cov };
                            while bits != 0 && cite.len() < MAX_CITE {
                                let b = bits.trailing_zeros() as usize;
                                bits &= bits - 1;
                                cite.push(examples[s.idx[w * 64 + b]].event_id.clone());
                            }
                        }
                    }
                    for (w, r) in c.right.iter().enumerate() {
                        covered[w] |= r;
                    }
                    out.push(Found { sig, xf: *xf, form: *form, train_ok: c.tr_ok, val_ok: c.va_ok, new_train: tr_new, new_val: va_new, cite });
                }
            }
        }
        self.predictions += predictions;
        SearchOut { found: out, candidates: tried, wanted: sig_keys.len() * cands.len(), truncated }
    }

    fn track(&mut self, examples: &[Example], sig: u8, key: Key) {
        let mut predictions = 0u64;
        if let Some(s) = self.sigs.get_mut(&sig) {
            let t = s.tracks.entry(key).or_default();
            for k in t.preds.len()..s.idx.len() {
                let e = &examples[s.idx[k]];
                predictions += 1;
                let p = predict_x(Xf(key.0), &key.1, &e.terms);
                if p.is_some() {
                    t.applies += 1;
                    if t.cites.len() < MAX_CITE {
                        t.cites.push(e.event_id.clone());
                    }
                }
                t.preds.push(p);
            }
        }
        self.predictions += predictions;
    }

    /// For the merge check: on how many examples of `sig` the rule `(xf, form)` applies, and the event ids
    /// of the first `MAX_CITE` of them (log order).
    pub fn applies(&mut self, examples: &[Example], sig: u8, xf: Xf, form: &Form) -> (usize, Vec<String>) {
        self.sync(examples);
        self.track(examples, sig, (xf.0, *form));
        self.sigs.get(&sig).and_then(|s| s.tracks.get(&(xf.0, *form))).map_or((0, vec![]), |t| (t.applies, t.cites.clone()))
    }

    /// For the merge check: on every example of `sig` where `a` applies, `b` applies and predicts the same.
    pub fn agrees(&mut self, examples: &[Example], sig: u8, a: (Xf, Form), b: (Xf, Form)) -> bool {
        self.sync(examples);
        let (ka, kb) = ((a.0 .0, a.1), (b.0 .0, b.1));
        self.track(examples, sig, ka);
        self.track(examples, sig, kb);
        let Some(s) = self.sigs.get_mut(&sig) else { return true };
        let n = s.idx.len();
        let (ta, tb) = (&s.tracks[&ka], &s.tracks[&kb]);
        let p = s.pairs.entry((ka, kb)).or_insert(Pair { upto: 0, agrees: true });
        while p.agrees && p.upto < n {
            if let Some(v) = ta.preds[p.upto] {
                if tb.preds[p.upto] != Some(v) {
                    p.agrees = false;
                }
            }
            p.upto += 1;
        }
        p.agrees
    }
}
