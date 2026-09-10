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
    Lag  { expr: Box<Expr>, offset: Box<Expr> },
    Lead { expr: Box<Expr>, offset: Box<Expr> },
    Agg  { func: AggFunc, expr: Box<Expr> },   // SUM/AVG/... OVER (...)
    CumSum(Box<Expr>),
    FirstValue(Box<Expr>),
    LastValue(Box<Expr>),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct WindowSpec {
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
}

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
}

#[derive(Debug, Clone, PartialEq)]
pub struct JoinClause {
    pub join_type: JoinKind,
    pub table:     TableExpr,
    pub on:        JoinOn,
    /// Set by PredicatePushdownRule: filter pushed down to the join's table scan.
    pub push_filter: Option<Box<Expr>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JoinOn {
    pub left_col:  String,
    pub right_col: String,
    pub expr:      Option<Expr>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum JoinKind { Inner, Left, Right, Full, Cross }

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
pub enum SetOpKind { UnionAll, Union, Intersect, Except }

/// Full query: `[WITH cte, ...] SELECT ... [UNION/INTERSECT/EXCEPT SELECT ...]`
#[derive(Debug, Clone, Default)]
pub struct Query {
    pub ctes:      Vec<CteClause>,
    pub body:      Option<SelectStmt>,
    pub set_ops:   Vec<(SetOpKind, SelectStmt)>,
}

#[derive(Debug, Clone)]
pub struct CteClause {
    pub name: String,
    pub body: SelectStmt,
}
