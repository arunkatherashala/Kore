use sql_parser::{SQLParser, StmtKind, JoinKind, Expr};

#[test]
fn parse_simple_join_on() {
    let sql = "SELECT a, b FROM t1 JOIN t2 ON t1.id = t2.ref_id WHERE t1.x = 1";
    let mut p = SQLParser::new(sql);
    let stmt = p.parse().expect("parse");
    assert_eq!(stmt.kind, StmtKind::Select);
    let s = stmt.select.expect("select");
    assert_eq!(s.joins.len(), 1);
}

#[test]
fn parse_left_join_with_alias() {
    let sql = "SELECT * FROM users u LEFT JOIN orders o ON u.id = o.user_id";
    let mut p = SQLParser::new(sql);
    let stmt = p.parse().expect("parse");
    let s = stmt.select.expect("select");
    assert_eq!(s.joins.len(), 1);
    let j = &s.joins[0];
    assert!(matches!(j.kind, JoinKind::Left));
    assert_eq!(j.alias.as_deref(), Some("o"));
}

#[test]
fn parse_cross_join() {
    let sql = "SELECT * FROM a CROSS JOIN b";
    let mut p = SQLParser::new(sql);
    let stmt = p.parse().expect("parse");
    let s = stmt.select.expect("select");
    assert_eq!(s.joins.len(), 1);
    assert!(matches!(s.joins[0].kind, JoinKind::Cross));
}

#[test]
fn parse_full_outer_join() {
    let sql = "SELECT * FROM x FULL OUTER JOIN y ON x.id = y.x_id";
    let mut p = SQLParser::new(sql);
    let stmt = p.parse().expect("parse");
    let s = stmt.select.expect("select");
    assert_eq!(s.joins.len(), 1);
    assert!(matches!(s.joins[0].kind, JoinKind::Full));
}

#[test]
fn parse_using_join() {
    let sql = "SELECT * FROM a JOIN b USING(id)";
    let mut p = SQLParser::new(sql);
    let stmt = p.parse().expect("parse");
    let s = stmt.select.expect("select");
    assert_eq!(s.joins.len(), 1);
    // verify USING produced a Binary Expr with left 'USING'
    match &s.joins[0].on {
        Expr::Binary { left, op: _, right: _ } => assert_eq!(left, "USING"),
        _ => panic!("expected binary expr"),
    }
}
