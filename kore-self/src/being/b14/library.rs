//! Track B14, part 1: the rule library (first world; opt-in, default off).
//!
//! The library is not a second store: it is what the being's own log already holds about rules,
//! read as an index by context (the signature: domain and difficulty), plus three kinds of record
//! that B14 adds (all ordinary events and revisions of the versioned log, so log formats 1 to 4 are
//! unchanged and older logs fold exactly as before):
//!
//! * `rule_lib_import` notes: entries carried over from an earlier life (another log). They are
//!   CANDIDATES only: the being does not apply them until they pass a check on its own revealed
//!   examples of the same signature (`LibCfg::reuse_train` training and `reuse_val` held-out
//!   examples reproduced exactly, never applicable-and-wrong, the 3 spare equations of the B2 guard).
//!   A candidate that passes becomes an ordinary `rule` note (tag `via_library:<entry>`) with a
//!   revision (author `rule:being-library-reuse`) citing the import note and the examples it
//!   reproduced. A rule of one domain is also a candidate for the same difficulty of the other
//!   sequence domain (B6's `pattern_b`) and, with the constant term toggled, under the same check.
//! * per-rule records: for every live rule, its applicable revealed examples (right or wrong),
//!   folded from the outcome events. The planner uses them (with the pooled record of all
//!   rule-produced answers) as the success estimate of the `discovered` strategy on an offer whose
//!   prefix the rule applies to; the decision revision cites the rule's note and the latest
//!   examples of its record.
//! * retirement on evidence: when, over the latest `retire_window` applicable revealed examples
//!   (at least `retire_min`), the 95% Wilson upper bound of the rule's share right is below
//!   `retire_ub`, the rule is retired: an ordinary `rule_retired` note (tag `by:library`) and a
//!   revision (author `rule:being-library-retire`) citing every example of the window.
//!
//! Everything here is a pure function of the being's own log (the examples it was shown and told,
//! the rules it accepted, the imports it was given); no hidden world parameter is read.

use super::super::discover::{self, is_held_out, predict_x, toggle_constant, Example, Form, Found, Rule, Xf};
use super::super::run_loop::{Engine, NewRule};
use super::super::state::{tag, BeingState, SOURCE};
use super::super::world::iso_at;
use crate::versioned::{event_id, EventRow, NewEvent, NewRevision};
use serde_json::json;
use std::collections::{BTreeMap, VecDeque};

pub const IMPORT_TAG: &str = "rule_lib_import";
/// Latest applicable examples kept per rule (citations and the retirement window).
pub const REC_KEEP: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LibCfg {
    /// Support a library candidate needs on the being's own examples (training / held-out).
    pub reuse_train: u32,
    pub reuse_val: u32,
    pub retire_window: usize,
    pub retire_min: u32,
    pub retire_ub: f64,
    /// Use the rule records as the planner's estimate of the `discovered` strategy where a rule applies.
    pub estimate: bool,
}

impl Default for LibCfg {
    fn default() -> LibCfg {
        LibCfg { reuse_train: 1, reuse_val: 1, retire_window: 6, retire_min: 4, retire_ub: 0.5, estimate: true }
    }
}

/// One portable library entry: a verified (transform, form) for a signature, and where it came from.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
pub struct LibEntry {
    pub sig: u8,
    pub xf: Xf,
    pub form: Form,
    /// Where it came from (e.g. `life:<world seed>:rule:<id>`).
    pub origin: String,
    /// The `rule_lib_import` note in this log (empty until written).
    pub note_id: String,
}

impl LibEntry {
    pub fn of_rule(r: &Rule, origin: &str) -> LibEntry {
        LibEntry { sig: r.sig, xf: r.xf, form: r.form, origin: format!("{origin}:rule:{}", r.id), note_id: String::new() }
    }
    /// The id the rule gets once accepted (content-derived, the same in every life).
    pub fn id(&self) -> String {
        discover::rule_id_x(self.sig, self.xf, &self.form)
    }
    pub fn event(&self, clock: i64, step: u64) -> NewEvent {
        let (d, k) = discover::sig_split(self.sig);
        let text = format!(
            "rule library import {}: {} difficulty {}: {}{}; carried over from {} as a candidate only: it is applied after it reproduces this being's own revealed examples",
            self.id(),
            d.name(),
            k,
            if self.xf.is_id() { String::new() } else { format!("{} then ", self.xf.words()) },
            self.form.dsl(),
            self.origin
        );
        let mut tags = vec![IMPORT_TAG.to_string(), format!("rule_id:{}", self.id()), format!("sig:{}", self.sig), format!("form:{}", self.form.code()), format!("origin:{}", self.origin.replace(',', ";")), format!("at_step:{step}")];
        if !self.xf.is_id() {
            tags.push(format!("xf:{}", self.xf.code()));
        }
        NewEvent { source: SOURCE.into(), occurred_at: iso_at(clock), kind: "note".into(), text, tags, redacted: false }
    }
    pub fn from_event(e: &EventRow) -> Option<LibEntry> {
        if e.kind != "note" || !e.tags.split(',').any(|t| t == IMPORT_TAG) {
            return None;
        }
        let sig: u8 = tag(&e.tags, "sig")?.parse().ok()?;
        let form = Form::parse(tag(&e.tags, "form")?)?;
        let xf = match tag(&e.tags, "xf") {
            Some(c) => Xf::parse(c)?,
            None => Xf::ID,
        };
        let x = LibEntry { sig, xf, form, origin: tag(&e.tags, "origin").unwrap_or("").to_string(), note_id: e.event_id.clone() };
        (tag(&e.tags, "rule_id")? == x.id()).then_some(x)
    }
}

/// What the library knows about one live rule from the revealed examples.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RuleRec {
    /// Revealed examples of its signature on which it applied, and on how many it was right.
    pub app: u32,
    pub right: u32,
    /// Latest applicable examples: (outcome event id, right), oldest first.
    pub recent: VecDeque<(String, bool)>,
}

/// The planner's estimate of the `discovered` strategy where a library rule applies.
#[derive(Debug, Clone, PartialEq)]
pub struct LibEst {
    pub rule_id: String,
    pub rule_event: String,
    /// Share right of the rule on its applicable revealed examples ((right + 1) / (app + 2)).
    pub acc: f64,
    /// Share right of all rule-produced answers so far ((right + 1) / (uses + 2)): the executor's slips.
    pub exec: f64,
    pub p: f64,
    pub sd: f64,
    pub app: u32,
    pub uses: u32,
    /// Latest applicable examples of the rule (event ids), for the decision's citations.
    pub recent: Vec<String>,
}

/// Per-rule records, kept up to date incrementally from the state (examples only grow).
#[derive(Debug, Clone, Default)]
pub struct LibIndex {
    seen: usize,
    key: Vec<(String, bool)>,
    pub recs: BTreeMap<String, RuleRec>,
}

impl LibIndex {
    pub fn update(&mut self, st: &BeingState) {
        let same = self.key.len() == st.rules.len() && self.key.iter().zip(&st.rules).all(|(k, r)| k.0 == r.id && k.1 == r.retired);
        if !same || st.examples.len() < self.seen {
            self.key = st.rules.iter().map(|r| (r.id.clone(), r.retired)).collect();
            self.seen = 0;
            self.recs.clear();
            for r in st.rules.iter().filter(|r| !r.retired) {
                self.recs.insert(r.id.clone(), RuleRec::default());
            }
        }
        for e in &st.examples[self.seen..] {
            for r in st.rules.iter().filter(|r| !r.retired && r.sig == e.sig) {
                if let Some(v) = predict_x(r.xf, &r.form, &e.terms) {
                    let rec = self.recs.entry(r.id.clone()).or_default();
                    let ok = v == e.next;
                    rec.app += 1;
                    rec.right += ok as u32;
                    rec.recent.push_back((e.event_id.clone(), ok));
                    if rec.recent.len() > REC_KEEP {
                        rec.recent.pop_front();
                    }
                }
            }
        }
        self.seen = st.examples.len();
    }

    pub fn estimate(&self, st: &BeingState, rule: &Rule) -> LibEst {
        let rec = self.recs.get(&rule.id).cloned().unwrap_or_default();
        let (mut uses, mut right) = (0u32, 0u32);
        for u in st.rule_use.values() {
            uses += u.uses;
            right += u.right;
        }
        let acc = (rec.right as f64 + 1.0) / (rec.app as f64 + 2.0);
        let exec = (right as f64 + 1.0) / (uses as f64 + 2.0);
        let p = acc * exec;
        let n = rec.app.min(uses) as f64 + 2.0;
        let recent = rec.recent.iter().rev().take(8).map(|x| x.0.clone()).collect();
        LibEst { rule_id: rule.id.clone(), rule_event: rule.event_id.clone(), acc, exec, p, sd: (p * (1.0 - p) / (n + 1.0)).sqrt(), app: rec.app, uses, recent }
    }
}

/// 95% Wilson upper bound of k successes in n.
pub fn wilson_ub(k: u32, n: u32) -> f64 {
    if n == 0 {
        return 1.0;
    }
    let (z, nf) = (1.96f64, n as f64);
    let ph = k as f64 / nf;
    let c = ph + z * z / (2.0 * nf);
    let r = z * ((ph * (1.0 - ph) + z * z / (4.0 * nf)) / nf).sqrt();
    ((c + r) / (1.0 + z * z / nf)).min(1.0)
}

/// The engine's library: settings, the records, and import notes not yet written.
#[derive(Debug, Clone, Default)]
pub struct Library {
    pub cfg: LibCfg,
    pub index: LibIndex,
    pending: Vec<NewEvent>,
    /// Ids reused (accepted from a candidate) and retired by the library during this engine's life.
    pub reused: Vec<String>,
    pub retired: Vec<String>,
}

/// Check a candidate on the being's own examples of its signature: never applicable-and-wrong, and
/// at least `k_train` / `k_val` exact reproductions on training / held-out examples.
pub fn check_candidate(examples: &[Example], sig: u8, xf: Xf, form: &Form, k_train: u32, k_val: u32) -> Option<Found> {
    let (mut tr, mut va) = (0u32, 0u32);
    let mut cite = vec![];
    for (i, e) in examples.iter().filter(|e| e.sig == sig).enumerate() {
        match predict_x(xf, form, &e.terms) {
            None => {}
            Some(v) if v == e.next => {
                if is_held_out(i) {
                    va += 1
                } else {
                    tr += 1
                }
                if cite.len() < discover::MAX_CITE {
                    cite.push(e.event_id.clone());
                }
            }
            Some(_) => return None,
        }
    }
    (tr >= k_train && va >= k_val).then(|| Found { sig, xf, form: *form, train_ok: tr, val_ok: va, new_train: tr, new_val: va, cite })
}

impl<'a> Engine<'a> {
    /// Turn the library on (measurement and the logged run). `imports`: entries from an earlier life,
    /// written as `rule_lib_import` notes (once; an entry already in the log is not written again).
    pub fn enable_library(&mut self, cfg: LibCfg, imports: &[LibEntry]) {
        let mut lib = Library { cfg, ..Default::default() };
        for x in imports {
            if self.state.b14_imports.iter().any(|y| y.id() == x.id()) {
                continue;
            }
            let ev = x.event(self.state.clock, self.state.step);
            let mut x = x.clone();
            x.note_id = event_id(&ev.source, &ev.occurred_at, &ev.text);
            self.state.b14_imports.push(x);
            lib.pending.push(ev);
        }
        self.lib = Some(lib);
    }

    /// Import notes not yet written to the log.
    pub fn library_notes(&mut self) -> Vec<NewEvent> {
        self.lib.as_mut().map(|l| std::mem::take(&mut l.pending)).unwrap_or_default()
    }

    /// Candidates: imports, and (B6 world) the rules of the same difficulty of the other sequence
    /// domain, as they are and with the constant toggled. (sig, xf, form, origin, event cited).
    fn lib_candidates(&self) -> Vec<(u8, Xf, Form, String, String)> {
        let mut v: Vec<(u8, Xf, Form, String, String)> = vec![];
        for x in &self.state.b14_imports {
            v.push((x.sig, x.xf, x.form, format!("import:{}", x.origin), x.note_id.clone()));
        }
        let mut sigs: Vec<u8> = self.state.examples.iter().map(|e| e.sig).collect();
        sigs.sort();
        sigs.dedup();
        for r in &self.state.rules {
            let other = if r.sig >= 10 { r.sig - 10 } else { r.sig + 10 };
            if !sigs.contains(&other) {
                continue;
            }
            v.push((other, r.xf, r.form, format!("context:{}", r.id), r.event_id.clone()));
            if let Some(f) = toggle_constant(&r.form) {
                v.push((other, r.xf, f, format!("context-toggled:{}", r.id), r.event_id.clone()));
            }
        }
        v
    }

    /// One library pass (after the discovery pass): accept every candidate that passes the check on
    /// the being's own examples and is not held yet (live or retired).
    pub fn library_pass(&mut self) -> Vec<NewRule> {
        let Some(cfg) = self.lib.as_ref().map(|l| l.cfg) else { return vec![] };
        let mut out = vec![];
        for (sig, xf, form, origin, cite_ev) in self.lib_candidates() {
            let id = discover::rule_id_x(sig, xf, &form);
            if self.state.rules.iter().any(|r| r.id == id) {
                continue;
            }
            let Some(f) = check_candidate(&self.state.examples, sig, xf, &form, cfg.reuse_train, cfg.reuse_val) else { continue };
            let Some(mut nr) = self.accept_found(&f, None) else { continue };
            nr.event.tags.push(format!("via_library:{}", origin.replace(',', ";")));
            nr.revision.author = "rule:being-library-reuse".into();
            if !cite_ev.is_empty() {
                nr.revision.caused_by.push(cite_ev.clone());
            }
            let mut rat: serde_json::Value = serde_json::from_str(&nr.revision.rationale).unwrap_or(json!({}));
            rat["library"] = json!({
                "track": "B14", "origin": origin, "origin_event": cite_ev,
                "check": format!("reproduced {} training and {} held-out revealed examples of this being exactly and was never applicable-and-wrong on them (the library's reduced support: {} + {}; a new rule needs {} + {})", f.train_ok, f.val_ok, cfg.reuse_train, cfg.reuse_val, discover::K_TRAIN, discover::K_VAL),
            });
            nr.revision.rationale = rat.to_string();
            if let Some(l) = self.lib.as_mut() {
                l.reused.push(nr.rule.id.clone());
            }
            out.push(nr);
        }
        out
    }

    /// Retire every live rule whose latest applicable revealed examples speak against it.
    pub fn library_retire(&mut self) -> Vec<(NewEvent, NewRevision)> {
        let Some(mut lib) = self.lib.take() else { return vec![] };
        lib.index.update(&self.state);
        let cfg = lib.cfg;
        let mut out = vec![];
        let live: Vec<Rule> = self.state.rules.iter().filter(|r| !r.retired).cloned().collect();
        for rule in live {
            let Some(rec) = lib.index.recs.get(&rule.id) else { continue };
            let win: Vec<(String, bool)> = rec.recent.iter().rev().take(cfg.retire_window).rev().cloned().collect();
            let n = win.len() as u32;
            let right = win.iter().filter(|x| x.1).count() as u32;
            let ub = wilson_ub(right, n);
            if n < cfg.retire_min || ub >= cfg.retire_ub {
                continue;
            }
            let cites: Vec<String> = win.iter().map(|x| x.0.clone()).collect();
            let reason = format!("on its latest {n} applicable revealed examples it predicted the revealed next term {right} times; the 95% upper bound of its share right is {ub:.3}, below {:.2} (library retirement on evidence)", cfg.retire_ub);
            let (mut ev, mut rev) = self.retire(&rule, "evidence", &reason, cites, None, json!({ "applicable_in_window": n, "right_in_window": right, "wilson_upper": ub, "threshold": cfg.retire_ub, "window": cfg.retire_window, "track": "B14" }));
            ev.tags.push("by:library".into());
            rev.author = "rule:being-library-retire".into();
            lib.retired.push(rule.id.clone());
            out.push((ev, rev));
        }
        if !out.is_empty() {
            lib.index.update(&self.state);
        }
        self.lib = Some(lib);
        out
    }

    /// The library as portable entries (live rules), for the next life.
    pub fn library_entries(&self) -> Vec<LibEntry> {
        let origin = format!("life:{}", self.state.genesis.as_ref().map_or(0, |g| g.world_seed));
        self.state.rules.iter().filter(|r| !r.retired).map(|r| LibEntry::of_rule(r, &origin)).collect()
    }
}
