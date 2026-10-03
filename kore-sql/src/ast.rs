//! KQL Abstract Syntax Tree types.

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Col(String),
    QualCol(String, String),
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    Null,
    BinOp { op: BinOpKind, left: Box<Expr>, right: Box<Expr> },
    Not(Box<Expr>),
    Agg { func: AggFunc, expr: Box<Expr> },
    IsNull(Box<Expr>),
    IsNotNull(Box<Expr>),
    Window { func: WindowFn, spec: WindowSpec },
    // ── New in Layer 32 ───────────────────────────────────────────────────────
    Case {
        operand:   Option<Box<Expr>>,              // CASE <expr> WHEN ... (simple)
        branches:  Vec<(Box<Expr>, Box<Expr>)>,    // (condition/value, result)
        else_val:  Option<Box<Expr>>,
    },
    In      { expr: Box<Expr>, values: Vec<Expr>, negated: bool },
    Between { expr: Box<Expr>, low: Box<Expr>, high: Box<Expr>, negated: bool },
    Like    { expr: Box<Expr>, pattern: Box<Expr>, negated: bool },
    ILike   { expr: Box<Expr>, pattern: Box<Expr>, negated: bool },
    Star,  // SELECT *  (used in COUNT(*))
    /// Scalar function call: UPPER(x), LOWER(x), ROUND(x,2), COALESCE(a,b), …
    FuncCall { name: String, args: Vec<Expr> },
    // ── Subqueries ────────────────────────────────────────────────────────────
    /// Scalar subquery: (SELECT single_value ...) used anywhere a value is expected.
    ScalarSubquery(Box<SelectStmt>),
    /// IN / NOT IN (SELECT ...): expr IN (SELECT col FROM ...)
    InSubquery { expr: Box<Expr>, subquery: Box<SelectStmt>, negated: bool },
    /// EXISTS (SELECT ...): true if subquery returns ≥1 row
    Exists { subquery: Box<SelectStmt>, negated: bool },
    // ── Spark SQL extensions ────────────────────────────────────────────────
    /// Array literal: ARRAY(1, 2, 3) or [1, 2, 3]
    Array(Vec<Expr>),
    /// EXPLODE(expr) — flatten array/map into rows
    Explode(Box<Expr>),
    /// Aggregate call the classic fast paths do not cover: `COUNT(DISTINCT a, b)`, `SUM(x) FILTER (WHERE ..)`,
    /// `STDDEV_POP`, `PERCENTILE`, `COLLECT_LIST`, ... `name` is upper case. Run by the general aggregation path.
    AggX { name: String, args: Vec<Expr>, distinct: bool, filter: Option<Box<Expr>> },
    /// `expr <op> ANY|SOME|ALL (SELECT ...)`
    QuantSubquery { expr: Box<Expr>, op: BinOpKind, all: bool, subquery: Box<SelectStmt> },
}

#[derive(Debug, Clone, PartialEq)]
pub enum BinOpKind {
    Eq, Ne, Lt, Le, Gt, Ge,
    And, Or,
    Add, Sub, Mul, Div, Mod,
    Concat,   // ||
}

#[derive(Debug, Clone, PartialEq)]
pub enum AggFunc {
    Count, CountDistinct, Sum, Avg, Min, Max,
    Stddev, Variance, Median,
    StringAgg { sep: String },
    Percentile { p: String },   // p stored as string "0.5" etc.
}

// ── Window function types ─────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum WindowFn {
    RowNumber,
    Rank,
    DenseRank,
    PercentRank,
    CumeDist,
    Ntile(Box<Expr>),
    Lag  { expr: Box<Expr>, offset: Box<Expr>, default: Option<Box<Expr>> },
    Lead { expr: Box<Expr>, offset: Box<Expr>, default: Option<Box<Expr>> },
    Agg  { func: AggFunc, expr: Box<Expr> },   // SUM/AVG/... OVER (...)
    CumSum(Box<Expr>),
    FirstValue(Box<Expr>),
    LastValue(Box<Expr>),
    /// `FIRST_VALUE(x, true)` / `FIRST_VALUE(x) IGNORE NULLS`
    FirstValueIgnoreNulls(Box<Expr>),
    LastValueIgnoreNulls(Box<Expr>),
    NthValue { expr: Box<Expr>, n: Box<Expr> },
    /// Any other aggregate used as a window function (STDDEV, COLLECT_LIST, ... or with FILTER).
    AggX { name: String, args: Vec<Expr>, filter: Option<Box<Expr>> },
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct WindowSpec {
    /// `OVER w` / `OVER (w ORDER BY ..)`: name of a `WINDOW w AS (..)` definition this spec extends.
    pub base:         Option<String>,
    pub partition_by: Vec<Expr>,
    pub order_by:     Vec<OrderByItem>,
    pub frame:        Option<WindowFrame>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WindowFrame {
    pub mode:  FrameMode,
    pub start: FrameBound,
    pub end:   FrameBound,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FrameMode { Rows, Range }

#[derive(Debug, Clone, PartialEq)]
pub enum FrameBound {
    UnboundedPreceding,
    Preceding(Box<Expr>),
    CurrentRow,
    Following(Box<Expr>),
    UnboundedFollowing,
}

// ── Query hints ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum QueryHint {
    Broadcast(String),
    Repartition(usize),
}

// ── LATERAL VIEW ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct LateralView {
    pub expr:        Expr,
    pub table_alias: String,
    pub col_alias:   String,
}

// ── PIVOT / UNPIVOT ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct PivotClause {
    pub agg_func:  AggFunc,
    pub agg_col:   String,
    pub for_col:   String,
    pub in_values: Vec<Expr>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UnpivotClause {
    pub value_col: String,
    pub key_col:   String,
    pub in_cols:   Vec<String>,
}

// ── SELECT statement ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct SelectStmt {
    pub distinct:      bool,
    pub projections:   Vec<Projection>,
    pub from:          TableExpr,
    pub joins:         Vec<JoinClause>,
    pub where_clause:  Option<Expr>,
    pub group_by:      Vec<String>,
    /// GROUP BY ROLLUP(..) / CUBE(..) expand `group_by` into grouping sets.
    pub grouping:      Grouping,
    /// General GROUP BY (expressions, ordinals, GROUPING SETS / mixed ROLLUP/CUBE): the distinct grouping
    /// expressions. When non-empty `group_by` holds placeholder names of the same length and the general
    /// aggregation path runs.
    pub group_exprs:   Vec<Expr>,
    /// Grouping sets as index lists into `group_exprs`; empty means one set holding every expression.
    pub group_sets:    Vec<Vec<usize>>,
    /// `WINDOW w AS (...)` definitions.
    pub windows:       Vec<(String, WindowSpec)>,
    pub having:        Option<Expr>,
    pub qualify:       Option<Expr>,  // QUALIFY (window filter)
    pub order_by:      Vec<OrderByItem>,
    pub limit:         Option<u64>,
    pub offset:        Option<u64>,
    /// Set by LimitPushdownRule: propagate limit into scan stage for early termination.
    pub scan_limit:    Option<u64>,
    /// LATERAL VIEW EXPLODE(col) alias AS col_alias
    pub lateral_views: Vec<LateralView>,
    /// PIVOT/UNPIVOT after FROM
    pub pivot:         Option<PivotClause>,
    pub unpivot:       Option<UnpivotClause>,
    /// Query hints: /*+ BROADCAST(t) */ etc.
    pub hints:         Vec<QueryHint>,
    /// Written as `( SELECT .. )` (an arm of a set operation).
    pub parenthesized: bool,
    /// Further arms of `UNION / INTERSECT / EXCEPT` chained to this statement; the statement's own
    /// ORDER BY / LIMIT / OFFSET then apply to the combined result.
    pub set_ops:       Vec<(SetOpKind, SelectStmt)>,
}

impl SelectStmt {
    /// `SELECT * FROM <from>` with every other clause empty.
    pub fn star_from(from: TableExpr) -> SelectStmt {
        SelectStmt {
            distinct: false, projections: vec![Projection::Star], from, joins: Vec::new(), where_clause: None,
            group_by: Vec::new(), grouping: Grouping::Plain, group_exprs: Vec::new(), group_sets: Vec::new(),
            windows: Vec::new(), having: None, qualify: None, order_by: Vec::new(), limit: None, offset: None,
            scan_limit: None, lateral_views: Vec::new(), pivot: None, unpivot: None, hints: Vec::new(),
            parenthesized: false, set_ops: Vec::new(),
        }
    }

    /// `SELECT * FROM (inner) __arm`: isolates an inner statement that has its own ORDER BY / LIMIT.
    pub fn wrap(inner: SelectStmt) -> SelectStmt {
        SelectStmt::star_from(TableExpr {
            name: "__arm".into(), alias: Some("__arm".into()), subquery: Some(Box::new(inner)),
            values: None, push_filter: None, col_aliases: Vec::new(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Grouping { #[default] Plain, Rollup, Cube }

#[derive(Debug, Clone, PartialEq)]
pub enum Projection {
    Star,
    Expr { expr: Expr, alias: Option<String> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct TableExpr {
    pub name:     String,
    pub alias:    Option<String>,
    /// For FROM (SELECT ...) alias subqueries
    pub subquery: Option<Box<SelectStmt>>,
    /// For FROM (VALUES (...), (...)) AS t(cols)
    pub values:   Option<Vec<Vec<Expr>>>,
    /// Set by PredicatePushdownRule: filter pushed down to this table's scan.
    pub push_filter: Option<Box<Expr>>,
    /// `(subquery) alias(c1, c2)` / `VALUES .. AS t(c1, c2)` column names
    pub col_aliases: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JoinClause {
    pub join_type: JoinKind,
    pub table:     TableExpr,
    pub on:        JoinOn,
    /// Set by PredicatePushdownRule: filter pushed down to the join's table scan.
    pub push_filter: Option<Box<Expr>>,
    /// `JOIN .. USING (a, b)`
    pub using:     Vec<String>,
    /// `NATURAL JOIN`: join on every column name both sides share
    pub natural:   bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JoinOn {
    pub left_col:  String,
    pub right_col: String,
    pub expr:      Option<Expr>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum JoinKind {
    Inner, Left, Right, Full, Cross,
    /// `LEFT SEMI JOIN` / `LEFT ANTI JOIN`: left rows with / without a match, left columns only.
    Semi, Anti,
    /// `FROM a, b, c`: an inner join whose keys come from equalities in the WHERE clause.
    Implicit,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OrderByItem {
    pub expr:        Expr,
    pub col:        String,
    pub desc:       bool,
    pub nulls_first: Option<bool>,
}

// ── Top-level query (CTEs + set operations) ──────────────────────────────────

/// Set operation kinds: UNION ALL, UNION (dedup), INTERSECT, EXCEPT
#[derive(Debug, Clone, PartialEq)]
pub enum SetOpKind { UnionAll, Union, Intersect, Except, IntersectAll, ExceptAll }

/// Full query: `[WITH cte, ...] SELECT ... [UNION/INTERSECT/EXCEPT SELECT ...]`
#[derive(Debug, Clone, Default)]
pub struct Query {
    pub ctes:      Vec<CteClause>,
    pub body:      Option<SelectStmt>,
    pub set_ops:   Vec<(SetOpKind, SelectStmt)>,
    /// ORDER BY / LIMIT / OFFSET that follow a set operation apply to its whole result.
    pub order_by:  Vec<OrderByItem>,
    pub limit:     Option<u64>,
    pub offset:    Option<u64>,
}

#[derive(Debug, Clone)]
pub struct CteClause {
    pub name: String,
    pub body: SelectStmt,
}
