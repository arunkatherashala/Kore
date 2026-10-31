//! LIKE / ILIKE / NOT LIKE / ESCAPE on very long text must neither overflow the stack nor go exponential.
//! (The unit-level differential test against a recursive reference lives in `executor.rs`, module `like_robust_tests`.)

use std::time::{Duration, Instant};
use kore_core::{Column, ColumnData, DataBlock};
use kore_sql::KqlContext;

fn ctx_with(vals: Vec<Option<String>>) -> KqlContext {
    let n = vals.len();
    let mut ctx = KqlContext::new();
    ctx.register("t", DataBlock { num_rows: n, columns: vec![
        Column { name: "id".into(), data: ColumnData::Int64((0..n as i64).map(Some).collect()) },
        Column { name: "s".into(), data: ColumnData::Str(vals) },
    ]});
    ctx
}

fn count(ctx: &KqlContext, sql: &str) -> usize { ctx.query(sql).unwrap().num_rows }

/// Runs on the default test thread stack (2 MB) on purpose: a recursive matcher dies here at ~10 KB of text.
#[test]
fn long_text_all_like_forms() {
    let big = "ab".repeat(1_000_000); // 2 MB
    let with_needle = format!("{}needle{}", "x".repeat(1_000_000), "y".repeat(1_000_000));
    let ctx = ctx_with(vec![Some(big.clone()), Some(with_needle), None, Some(String::new()), Some("a%b_c".into())]);
    // plain, anchored, several wildcards, underscore
    assert_eq!(count(&ctx, "SELECT id FROM t WHERE s LIKE 'ab%'"), 1);
    assert_eq!(count(&ctx, "SELECT id FROM t WHERE s LIKE '%ab'"), 1);
    assert_eq!(count(&ctx, "SELECT id FROM t WHERE s LIKE '%needle%'"), 1);
    assert_eq!(count(&ctx, "SELECT id FROM t WHERE s LIKE '%x%needle%y%'"), 1);
    assert_eq!(count(&ctx, "SELECT id FROM t WHERE s LIKE '%ab%ba%ab%'"), 1);
    assert_eq!(count(&ctx, "SELECT id FROM t WHERE s LIKE 'a_%_b'"), 1);
    assert_eq!(count(&ctx, "SELECT id FROM t WHERE s LIKE '%x_needle%'"), 1);
    assert_eq!(count(&ctx, "SELECT id FROM t WHERE s LIKE '%zzz%'"), 0);
    // ILIKE and NOT LIKE (NULL row never qualifies for either)
    assert_eq!(count(&ctx, "SELECT id FROM t WHERE s ILIKE '%NEEDLE%'"), 1);
    assert_eq!(count(&ctx, "SELECT id FROM t WHERE s ILIKE 'AB%AB'"), 1);
    assert_eq!(count(&ctx, "SELECT id FROM t WHERE s NOT LIKE '%needle%'"), 3); // big, "", "a%b_c"; NULL excluded
    assert_eq!(count(&ctx, "SELECT id FROM t WHERE s NOT ILIKE '%AB%'"), 2);
    // ESCAPE
    assert_eq!(count(&ctx, "SELECT id FROM t WHERE s LIKE 'a#%b#_c' ESCAPE '#'"), 1);
    assert_eq!(count(&ctx, "SELECT id FROM t WHERE s LIKE '%#%b#_c' ESCAPE '#'"), 1);
    assert_eq!(count(&ctx, "SELECT id FROM t WHERE s LIKE '%#%%' ESCAPE '#'"), 1);
    // long pattern too (pattern 200 KB of `a%`)
    let pat = "a%".repeat(100_000);
    let ctx2 = ctx_with(vec![Some("a".repeat(300_000)), Some("b".repeat(300_000))]);
    assert_eq!(count(&ctx2, &format!("SELECT id FROM t WHERE s LIKE '{pat}'")), 1);
}

/// Pathological `%a%a%a...%b` on `aaaa...`: with one saved star position it is O(len(text) * len(pattern)),
/// never exponential. 20 stars on 20k chars must finish far inside the limit.
#[test]
fn adversarial_pattern_not_exponential() {
    let text = "a".repeat(20_000);
    let pat = format!("{}b", "%a".repeat(20));
    let ctx = ctx_with(vec![Some(text)]);
    let t = Instant::now();
    assert_eq!(count(&ctx, &format!("SELECT id FROM t WHERE s LIKE '{pat}'")), 0);
    assert!(t.elapsed() < Duration::from_secs(10), "took {:?}", t.elapsed());
}
