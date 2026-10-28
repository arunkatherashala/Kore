//! Test helper for the chat thinker command (tests/e2e_chat.rs). Not a model:
//! a tiny cross-platform program that speaks the thinker protocol (JSON on
//! stdin, JSON on stdout) in a few fixed modes chosen by its first argument.
//!   echo                    reply with the exact stdin it received
//!   propose <tool> <json>   reply "proposing" with one tool call
//!   env                     reply with the environment variable names and the working directory
//!   sleep | flood | badjson | exit1   misbehave on purpose
//!   advise_best [strategy]  advisor protocol (Track B4): reply with the offer of best listed reward per cost
//!   advise_echo             advisor protocol: report whether the request still held the word algebra or a redaction marker
//!   advise_rule <sig> <form>  advisor protocol: propose a rule of the B2 grammar

use std::io::{Read, Write};

fn out(v: serde_json::Value) {
    let mut o = std::io::stdout();
    let _ = o.write_all(v.to_string().as_bytes());
    let _ = o.flush();
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args.first().map(|s| s.as_str()).unwrap_or("echo");
    match mode {
        "sleep" => std::thread::sleep(std::time::Duration::from_secs(60)),
        "flood" => {
            let chunk = vec![b'a'; 65536];
            let mut o = std::io::stdout();
            for _ in 0..64 {
                if o.write_all(&chunk).is_err() {
                    break;
                }
            }
        }
        "badjson" => print!("this is not json"),
        "exit1" => std::process::exit(1),
        "env" => {
            let mut names: Vec<String> = std::env::vars().map(|(k, _)| k).collect();
            names.sort();
            let cwd = std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_default();
            let allowed = std::env::var("KORE_HELPER_ALLOWED").unwrap_or_default();
            out(serde_json::json!({"reply": format!("ENV names={} allowed={} cwd={}", names.join(","), allowed, cwd)}));
        }
        "advise_best" | "advise_echo" | "advise_rule" => {
            let mut s = String::new();
            let _ = std::io::stdin().read_to_string(&mut s);
            let req: serde_json::Value = serde_json::from_str(&s).unwrap_or(serde_json::json!({}));
            match mode {
                "advise_best" => {
                    // the offer with the best listed reward per listed cost, with the strategy given as argument (default careful)
                    let offers = req["offers"].as_array().cloned().unwrap_or_default();
                    let best = offers.iter().max_by(|a, b| {
                        let r = |o: &serde_json::Value| o["listed_reward"].as_f64().unwrap_or(0.0) / o["listed_cost"].as_f64().unwrap_or(1.0).max(1.0);
                        r(a).partial_cmp(&r(b)).unwrap()
                    });
                    let slot = best.and_then(|o| o["slot"].as_u64()).unwrap_or(0);
                    out(serde_json::json!({"slot": slot, "strategy": args.get(1).cloned().unwrap_or("careful".into()), "p_success": 0.8, "note": "helper: best listed reward per cost"}));
                }
                "advise_echo" => out(serde_json::json!({"slot": 0, "strategy": "fast", "note": format!("saw_word={} saw_redacted={}", s.contains("algebra"), s.contains("[REDACTED"))})),
                _ => {
                    // propose the given form for the first signature the request asks about (none asked: a note only)
                    let form = args.get(2).cloned().unwrap_or("poly.1".into());
                    match req["rule_signatures_wanted"].as_array().and_then(|a| a.first()).and_then(|x| x.as_u64()) {
                        Some(sig) => out(serde_json::json!({"note": "helper rule proposal", "rule": {"signature": sig, "form": form}})),
                        None => out(serde_json::json!({"note": "helper: nothing to propose"})),
                    }
                }
            }
        }
        "propose" => {
            let mut s = String::new();
            let _ = std::io::stdin().read_to_string(&mut s);
            let tool = args.get(1).cloned().unwrap_or_default();
            let a: serde_json::Value = args.get(2).and_then(|j| serde_json::from_str(j).ok()).unwrap_or(serde_json::json!({}));
            out(serde_json::json!({"reply": "proposing", "tool_calls": [{"tool": tool, "args": a}]}));
        }
        _ => {
            let mut s = String::new();
            let _ = std::io::stdin().read_to_string(&mut s);
            out(serde_json::json!({"reply": format!("RECEIVED: {s}")}));
        }
    }
}
