//! Lightweight learning — cap network work per heartbeat so KORE stays fast and never hangs the system.

/// Per-tick learning limits (default: lightweight ON — learn steadily, never flood).
#[derive(Debug, Clone, Copy)]
pub struct LearnPolicy {
    pub lightweight: bool,
    pub max_http_per_tick: usize,
    pub http_timeout_secs: u64,
    pub lang_burst_cap: usize,
    pub domain_burst_cap: usize,
    /// In lightweight continuous mode, evolve less often (less CPU / disk).
    pub evolve_every_ticks: u64,
}

pub fn policy(continuous: bool) -> LearnPolicy {
    let lightweight = match std::env::var("KORE_LIGHTWEIGHT") {
        Ok(v) if v == "0" || v.eq_ignore_ascii_case("false") => false,
        Ok(v) if v == "1" || v.eq_ignore_ascii_case("true") => true,
        _ => true, // default: deadly lightweight — no hang even while learning
    };

    let max_http = std::env::var("KORE_LEARN_MAX_HTTP")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(if lightweight {
            if continuous { 2 } else { 1 }
        } else if continuous {
            6
        } else {
            2
        })
        .clamp(1, 12);

    let timeout = std::env::var("KORE_HTTP_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(if lightweight { 4 } else { 8 })
        .clamp(2, 15);

    let lang_cap = if lightweight { 1 } else { 12 };
    let domain_cap = if lightweight {
        1
    } else if continuous {
        5
    } else {
        1
    };

    let evolve_every = if lightweight && continuous {
        50
    } else if continuous {
        1
    } else {
        100
    };

    LearnPolicy {
        lightweight,
        max_http_per_tick: max_http,
        http_timeout_secs: timeout,
        lang_burst_cap: lang_cap,
        domain_burst_cap: domain_cap,
        evolve_every_ticks: evolve_every,
    }
}

pub fn summary(p: &LearnPolicy) -> String {
    format!(
        "Lightweight: {} | max HTTP/tick: {} | timeout: {}s | lang cap: {} | domain cap: {} | evolve every {} ticks\n\
         Set KORE_LIGHTWEIGHT=0 for aggressive learning (more HTTP/tick). Default lightweight keeps system responsive.",
        if p.lightweight { "ON" } else { "off" },
        p.max_http_per_tick,
        p.http_timeout_secs,
        p.lang_burst_cap,
        p.domain_burst_cap,
        p.evolve_every_ticks
    )
}

/// Apply lightweight caps to burst settings.
pub fn cap_lang_burst(requested: usize, p: &LearnPolicy) -> usize {
    requested.min(p.lang_burst_cap)
}

pub fn cap_domain_burst(requested: usize, p: &LearnPolicy) -> usize {
    requested.min(p.domain_burst_cap)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lightweight_default_on() {
        std::env::remove_var("KORE_LIGHTWEIGHT");
        let p = policy(true);
        assert!(p.lightweight);
        assert!(p.max_http_per_tick <= 2);
    }

    #[test]
    fn caps_burst_to_policy() {
        let p = LearnPolicy {
            lightweight: true,
            max_http_per_tick: 2,
            http_timeout_secs: 4,
            lang_burst_cap: 1,
            domain_burst_cap: 1,
            evolve_every_ticks: 50,
        };
        assert_eq!(cap_lang_burst(8, &p), 1);
        assert_eq!(cap_domain_burst(5, &p), 1);
    }
}
