//! Per column of a rule-mode (version 6) file: payload bytes and the rule, with its exception counts.
//!
//!   rules_describe FILE.kore
fn main() {
    let p = std::env::args().nth(1).expect("FILE");
    let d = std::fs::read(&p).unwrap();
    for (name, dtype, bytes, rule) in kore_store::rules::describe(&d).unwrap() {
        println!("{name}\t{dtype:?}\t{bytes}\t{}", rule.unwrap_or_else(|| "v4 payload".into()));
    }
}
