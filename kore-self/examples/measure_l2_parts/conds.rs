//! Conditions of the Track L2 measurement: the B12 condition names, with `+l2` (the L2 meta-prior, defaults
//! `L2Cfg::on()`), `+l2f` (form posterior only) or `+l2t` (rule trust only).

use crate::being::l2::wo::L2Cfg;
use crate::being::world_o::engine::LearnerOCfg;
use crate::report::{learner_b12, Tune};

#[derive(Debug, Clone, Copy)]
pub struct L2Tune {
    pub delta_milli: u32,
    pub min_ex: u32,
}

impl L2Tune {
    pub fn from_args(args: &[String]) -> L2Tune {
        let g = |k: &str, d: u32| crate::report::arg(args, k).map_or(d, |x| x.parse().unwrap());
        let on = L2Cfg::on();
        L2Tune { delta_milli: g("--delta", on.delta_milli), min_ex: g("--min-ex", on.min_ex) }
    }
}

pub fn learner_l2(cond: &str, t: &Tune, lt: &L2Tune) -> LearnerOCfg {
    let parts: Vec<&str> = cond.split('+').collect();
    let base: Vec<&str> = parts.iter().copied().filter(|p| !p.starts_with("l2")).collect();
    let base = if base.is_empty() { "current".to_string() } else { base.join("+") };
    let mut l = learner_b12(&base, t);
    let mk = |forms: bool, trust: bool| L2Cfg { forms, trust, delta_milli: lt.delta_milli, min_ex: lt.min_ex };
    if parts.contains(&"l2") {
        l.l2 = Some(mk(true, true));
    } else if parts.contains(&"l2f") {
        l.l2 = Some(mk(true, false));
    } else if parts.contains(&"l2t") {
        l.l2 = Some(mk(false, true));
    }
    l
}
