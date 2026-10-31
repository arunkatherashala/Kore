//! Input-size-dependent recursion probes. A stack overflow aborts the whole process, so every probe runs in a
//! child process (this test binary re-executed with KORE_PROBE=<name>) on the default 2 MB test-thread stack.
//! A probe passes when the child exits normally, whether the query succeeded or returned a clean `Err`.

use kore_core::{Column, ColumnData, DataBlock};
use kore_sql::KqlContext;

fn ctx() -> KqlContext {
    let mut c = KqlContext::new();
    c.register("t", DataBlock { num_rows: 3, columns: vec![
        Column { name: "a".into(), data: ColumnData::Int64(vec![Some(1), Some(2), Some(3)]) },
        Column { name: "s".into(), data: ColumnData::Str(vec![Some("x".into()), Some("y".into()), None]) },
    ]});
    c
}

fn sql_for(name: &str) -> String {
    let (kind, n) = name.rsplit_once('_').unwrap();
    let n: usize = n.parse().unwrap();
    match kind {
        "parens" => format!("SELECT a FROM t WHERE {}a{} = 1", "(".repeat(n), ")".repeat(n)),
        "parens_select" => format!("SELECT {}a{} FROM t", "(".repeat(n), ")".repeat(n)),
        "notchain" => format!("SELECT a FROM t WHERE {}a = 1", "NOT ".repeat(n)),
        "negchain" => format!("SELECT {}a FROM t", "- ".repeat(n)),
        "andchain" => format!("SELECT a FROM t WHERE a >= 0{}", " AND a >= 0".repeat(n)),
        "orchain" => format!("SELECT a FROM t WHERE a = 99{}", " OR a = 99".repeat(n)),
        "addchain" => format!("SELECT a{} FROM t", " + 1".repeat(n)),
        "concatchain" => format!("SELECT s{} FROM t", " || 'x'".repeat(n)),
        "case" => format!("SELECT {}1{} FROM t", "CASE WHEN a > 0 THEN ".repeat(n), " END".repeat(n)),
        "func" => format!("SELECT {}a{} FROM t", "abs(".repeat(n), ")".repeat(n)),
        "subq" => format!("SELECT a FROM {}t{}", "(SELECT a FROM ".repeat(n), ")".repeat(n)),
        "subq_in" => format!("SELECT a FROM t WHERE a IN {}1{}", "(SELECT a FROM t WHERE a IN ".repeat(n), ")".repeat(n)),
        "inlist" => format!("SELECT a FROM t WHERE a IN ({})", (0..n).map(|i| i.to_string()).collect::<Vec<_>>().join(",")),
        "notinlist" => format!("SELECT a FROM t WHERE a NOT IN ({})", (0..n).map(|i| i.to_string()).collect::<Vec<_>>().join(",")),
        "instr" => format!("SELECT a FROM t WHERE s IN ({})", (0..n).map(|i| format!("'v{i}'")).collect::<Vec<_>>().join(",")),
        "unionchain" => format!("SELECT a FROM t{}", " UNION ALL SELECT a FROM t".repeat(n)),
        "likechain" => format!("SELECT a FROM t WHERE s LIKE 'x%'{}", " AND s LIKE 'x%'".repeat(n)),
        "cte" => format!("WITH c0 AS (SELECT a FROM t){} SELECT a FROM c{}", (1..=n).map(|i| format!(", c{i} AS (SELECT a FROM c{})", i - 1)).collect::<String>(), n),
        _ => panic!("unknown probe {kind}"),
    }
}

#[test]
fn probe_child() {
    let Ok(name) = std::env::var("KORE_PROBE") else { return };
    let sql = sql_for(&name);
    let r = ctx().query(&sql);
    println!("PROBE_RESULT {} {}", name, match &r { Ok(b) => format!("ok rows={}", b.num_rows), Err(e) => format!("err {}", format!("{e}").chars().take(80).collect::<String>()) });
    // the result is deliberately dropped here: Drop of a deep tree is part of the probe
}

fn run_probe(name: &str) -> (bool, String) {
    let exe = std::env::current_exe().unwrap();
    let out = std::process::Command::new(exe).args(["probe_child", "--exact", "--nocapture", "--test-threads=1"])
        .env("KORE_PROBE", name).output().unwrap();
    let so = String::from_utf8_lossy(&out.stdout).to_string();
    let line = so.lines().find(|l| l.starts_with("PROBE_RESULT")).unwrap_or("").to_string();
    (out.status.success() && !line.is_empty(), if line.is_empty() { format!("CRASH status={:?} {}", out.status, String::from_utf8_lossy(&out.stderr).lines().next().unwrap_or("")) } else { line })
}

const PROBES: &[&str] = &[
    "parens_5000", "parens_select_5000", "notchain_5000", "negchain_5000", "andchain_5000", "orchain_5000", "addchain_5000",
    "concatchain_5000", "case_5000", "func_5000", "subq_5000", "subq_in_5000", "inlist_100000", "notinlist_100000", "instr_100000",
    "unionchain_5000", "likechain_5000", "cte_2000", "andchain_100000",
];

/// Report-only driver: prints every probe outcome. Set KORE_PROBE_ASSERT=1 to fail on any crash.
#[test]
fn deep_input_report() {
    if std::env::var("KORE_PROBE").is_ok() { return; }
    let mut crashed = vec![];
    for p in PROBES {
        let (ok, line) = run_probe(p);
        println!("{p:22} {} | {line}", if ok { "OK   " } else { "CRASH" });
        if !ok { crashed.push(*p); }
    }
    if std::env::var("KORE_PROBE_ASSERT").is_ok() { assert!(crashed.is_empty(), "stack overflow / abort on: {crashed:?}"); }
}
