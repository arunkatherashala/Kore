"""
PySpark-compatible SQL functions for KORE Engine.

Import as::

    from pykore import functions as F

Every public function returns a :class:`~pykore.column.Col` whose ``_expr``
is the corresponding SQL fragment.  Expressions are resolved lazily when the
owning DataFrame is executed.
"""

from __future__ import annotations

from typing import Any

from .column import Col, _sql_literal, _sql_operand


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _to_col(c: Any) -> Col:
    """Coerce *c* to a :class:`Col`.  Strings become column references."""
    if isinstance(c, Col):
        return c
    if isinstance(c, str):
        return Col(c)
    return Col(_sql_literal(c))


def _unary_fn(name: str, c: Any) -> Col:
    return Col(f"{name}({_to_col(c)._expr})")


# ---------------------------------------------------------------------------
# Column reference / literal
# ---------------------------------------------------------------------------

def col(name: str) -> Col:
    """Reference a column by name."""
    return Col(name)


def lit(value: Any) -> Col:
    """Create a literal value column."""
    return Col(_sql_literal(value))


# ---------------------------------------------------------------------------
# Conditional
# ---------------------------------------------------------------------------

class _When:
    """Builder for SQL ``CASE WHEN … THEN … END`` expressions."""

    def __init__(self, condition: Col, value: Any):
        self._branches: list[tuple[str, str]] = [
            (_to_col(condition)._expr, _sql_operand(value))
        ]
        self._else_value: str | None = None

    def when(self, condition: Any, value: Any) -> "_When":
        self._branches.append(
            (_to_col(condition)._expr, _sql_operand(value))
        )
        return self

    def otherwise(self, value: Any) -> Col:
        self._else_value = _sql_operand(value)
        return self._build()

    def _build(self) -> Col:
        parts = " ".join(
            f"WHEN {cond} THEN {val}" for cond, val in self._branches
        )
        else_part = f" ELSE {self._else_value}" if self._else_value is not None else ""
        return Col(f"CASE {parts}{else_part} END")


def when(condition: Any, value: Any) -> _When:
    """Start a ``CASE WHEN`` expression."""
    return _When(_to_col(condition), value)


# ---------------------------------------------------------------------------
# Aggregate functions
# ---------------------------------------------------------------------------

def count(c: Any = "*") -> Col:
    if isinstance(c, str) and c == "*":
        return Col("COUNT(*)")
    return _unary_fn("COUNT", c)


def sum(c: Any) -> Col:  # noqa: A001 — shadows built-in intentionally
    return _unary_fn("SUM", c)


def avg(c: Any) -> Col:
    return _unary_fn("AVG", c)


def mean(c: Any) -> Col:
    return _unary_fn("AVG", c)


def min(c: Any) -> Col:  # noqa: A001
    return _unary_fn("MIN", c)


def max(c: Any) -> Col:  # noqa: A001
    return _unary_fn("MAX", c)


def count_distinct(c: Any, *more: Any) -> Col:
    cols = ", ".join(_to_col(x)._expr for x in (c, *more))
    return Col(f"COUNT(DISTINCT {cols})")


def sumDistinct(c: Any) -> Col:  # noqa: N802
    return Col(f"SUM(DISTINCT {_to_col(c)._expr})")


def first(c: Any) -> Col:
    return _unary_fn("FIRST", c)


def last(c: Any) -> Col:
    return _unary_fn("LAST", c)


# ---------------------------------------------------------------------------
# String functions
# ---------------------------------------------------------------------------

def upper(c: Any) -> Col:
    return _unary_fn("UPPER", c)


def lower(c: Any) -> Col:
    return _unary_fn("LOWER", c)


def trim(c: Any) -> Col:
    return _unary_fn("TRIM", c)


def ltrim(c: Any) -> Col:
    return _unary_fn("LTRIM", c)


def rtrim(c: Any) -> Col:
    return _unary_fn("RTRIM", c)


def length(c: Any) -> Col:
    return _unary_fn("LENGTH", c)


def substring(c: Any, pos: int, length_: int) -> Col:
    return Col(f"SUBSTRING({_to_col(c)._expr}, {pos}, {length_})")


def concat(*cols: Any) -> Col:
    exprs = ", ".join(_to_col(c)._expr for c in cols)
    return Col(f"CONCAT({exprs})")


def concat_ws(sep: str, *cols: Any) -> Col:
    exprs = ", ".join(_to_col(c)._expr for c in cols)
    return Col(f"CONCAT_WS('{sep}', {exprs})")


def regexp_replace(c: Any, pattern: str, replacement: str) -> Col:
    return Col(
        f"REGEXP_REPLACE({_to_col(c)._expr}, '{pattern}', '{replacement}')"
    )


def split(c: Any, pattern: str) -> Col:
    return Col(f"SPLIT({_to_col(c)._expr}, '{pattern}')")


def initcap(c: Any) -> Col:
    return _unary_fn("INITCAP", c)


def reverse(c: Any) -> Col:
    return _unary_fn("REVERSE", c)


def lpad(c: Any, length_: int, pad: str) -> Col:
    return Col(f"LPAD({_to_col(c)._expr}, {length_}, '{pad}')")


def rpad(c: Any, length_: int, pad: str) -> Col:
    return Col(f"RPAD({_to_col(c)._expr}, {length_}, '{pad}')")


# ---------------------------------------------------------------------------
# Null / coalesce
# ---------------------------------------------------------------------------

def coalesce(*cols: Any) -> Col:
    exprs = ", ".join(_to_col(c)._expr for c in cols)
    return Col(f"COALESCE({exprs})")


def ifnull(c1: Any, c2: Any) -> Col:
    return Col(f"IFNULL({_to_col(c1)._expr}, {_to_col(c2)._expr})")


def nullif(c1: Any, c2: Any) -> Col:
    return Col(f"NULLIF({_to_col(c1)._expr}, {_to_col(c2)._expr})")


def isnull(c: Any) -> Col:
    return Col(f"{_to_col(c)._expr} IS NULL")


def isnan(c: Any) -> Col:
    return _unary_fn("ISNAN", c)


# ---------------------------------------------------------------------------
# Math functions
# ---------------------------------------------------------------------------

def abs(c: Any) -> Col:  # noqa: A001
    return _unary_fn("ABS", c)


def round(c: Any, scale: int = 0) -> Col:  # noqa: A001
    return Col(f"ROUND({_to_col(c)._expr}, {scale})")


def ceil(c: Any) -> Col:
    return _unary_fn("CEIL", c)


def floor(c: Any) -> Col:
    return _unary_fn("FLOOR", c)


def sqrt(c: Any) -> Col:
    return _unary_fn("SQRT", c)


def pow(c: Any, p: Any) -> Col:
    return Col(f"POWER({_to_col(c)._expr}, {_sql_operand(p)})")


power = pow


def log(c: Any) -> Col:
    return _unary_fn("LN", c)


def log2(c: Any) -> Col:
    return _unary_fn("LOG2", c)


def log10(c: Any) -> Col:
    return _unary_fn("LOG10", c)


def exp(c: Any) -> Col:
    return _unary_fn("EXP", c)


# ---------------------------------------------------------------------------
# Date / time functions
# ---------------------------------------------------------------------------

def current_date() -> Col:
    return Col("CURRENT_DATE")


def current_timestamp() -> Col:
    return Col("CURRENT_TIMESTAMP")


def year(c: Any) -> Col:
    return _unary_fn("YEAR", c)


def month(c: Any) -> Col:
    return _unary_fn("MONTH", c)


def dayofmonth(c: Any) -> Col:
    return _unary_fn("DAY", c)


day = dayofmonth


def hour(c: Any) -> Col:
    return _unary_fn("HOUR", c)


def minute(c: Any) -> Col:
    return _unary_fn("MINUTE", c)


def second(c: Any) -> Col:
    return _unary_fn("SECOND", c)


def date_add(start: Any, days: int) -> Col:
    return Col(f"DATE_ADD({_to_col(start)._expr}, {days})")


def date_sub(start: Any, days: int) -> Col:
    return Col(f"DATE_SUB({_to_col(start)._expr}, {days})")


def datediff(end: Any, start: Any) -> Col:
    return Col(f"DATEDIFF({_to_col(end)._expr}, {_to_col(start)._expr})")


# ---------------------------------------------------------------------------
# Window functions
# ---------------------------------------------------------------------------

def row_number() -> Col:
    return Col("ROW_NUMBER()")


def rank() -> Col:
    return Col("RANK()")


def dense_rank() -> Col:
    return Col("DENSE_RANK()")


def ntile(n: int) -> Col:
    return Col(f"NTILE({n})")


def lag(c: Any, offset: int = 1, default: Any = None) -> Col:
    d = f", {_sql_literal(default)}" if default is not None else ""
    return Col(f"LAG({_to_col(c)._expr}, {offset}{d})")


def lead(c: Any, offset: int = 1, default: Any = None) -> Col:
    d = f", {_sql_literal(default)}" if default is not None else ""
    return Col(f"LEAD({_to_col(c)._expr}, {offset}{d})")


def cume_dist() -> Col:
    return Col("CUME_DIST()")


def percent_rank() -> Col:
    return Col("PERCENT_RANK()")


# ---------------------------------------------------------------------------
# Array / collection functions
# ---------------------------------------------------------------------------

def array(*cols: Any) -> Col:
    exprs = ", ".join(_to_col(c)._expr for c in cols)
    return Col(f"ARRAY({exprs})")


def size(c: Any) -> Col:
    return _unary_fn("SIZE", c)


def explode(c: Any) -> Col:
    return _unary_fn("EXPLODE", c)


def array_contains(c: Any, value: Any) -> Col:
    return Col(f"ARRAY_CONTAINS({_to_col(c)._expr}, {_sql_literal(value)})")


def sort_array(c: Any, asc: bool = True) -> Col:
    return Col(f"SORT_ARRAY({_to_col(c)._expr}, {str(asc).upper()})")


# ---------------------------------------------------------------------------
# Struct
# ---------------------------------------------------------------------------

def struct(*cols: Any) -> Col:
    exprs = ", ".join(_to_col(c)._expr for c in cols)
    return Col(f"STRUCT({exprs})")


# ---------------------------------------------------------------------------
# Misc
# ---------------------------------------------------------------------------

def monotonically_increasing_id() -> Col:
    return Col("MONOTONICALLY_INCREASING_ID()")


def spark_partition_id() -> Col:
    return Col("SPARK_PARTITION_ID()")


def input_file_name() -> Col:
    return Col("INPUT_FILE_NAME()")


# ---------------------------------------------------------------------------
# Window class
# ---------------------------------------------------------------------------

class WindowSpec:
    """A window specification for use with :meth:`Col.over`."""

    def __init__(
        self,
        partition_cols: list[str] | None = None,
        order_cols: list[str] | None = None,
        frame: str | None = None,
    ):
        self._partition_cols = partition_cols or []
        self._order_cols = order_cols or []
        self._frame = frame

    def partitionBy(self, *cols: Any) -> "WindowSpec":  # noqa: N802
        return WindowSpec(
            [_to_col(c)._expr for c in cols],
            list(self._order_cols),
            self._frame,
        )

    def orderBy(self, *cols: Any) -> "WindowSpec":  # noqa: N802
        return WindowSpec(
            list(self._partition_cols),
            [_to_col(c)._expr for c in cols],
            self._frame,
        )

    def rowsBetween(self, start: int, end: int) -> "WindowSpec":  # noqa: N802
        return WindowSpec(
            list(self._partition_cols),
            list(self._order_cols),
            f"ROWS BETWEEN {_frame_bound(start)} AND {_frame_bound(end)}",
        )

    def rangeBetween(self, start: int, end: int) -> "WindowSpec":  # noqa: N802
        return WindowSpec(
            list(self._partition_cols),
            list(self._order_cols),
            f"RANGE BETWEEN {_frame_bound(start)} AND {_frame_bound(end)}",
        )

    def __str__(self) -> str:
        parts: list[str] = []
        if self._partition_cols:
            parts.append(f"PARTITION BY {', '.join(self._partition_cols)}")
        if self._order_cols:
            parts.append(f"ORDER BY {', '.join(self._order_cols)}")
        if self._frame:
            parts.append(self._frame)
        return " ".join(parts)


class Window:
    """Entry-point for building :class:`WindowSpec` objects.

    Usage::

        w = Window.partitionBy("dept").orderBy("salary")
        df.select(col("name"), row_number().over(w))
    """

    unboundedPreceding: int = -(2**31)
    unboundedFollowing: int = 2**31 - 1
    currentRow: int = 0

    @staticmethod
    def partitionBy(*cols: Any) -> WindowSpec:  # noqa: N802
        return WindowSpec().partitionBy(*cols)

    @staticmethod
    def orderBy(*cols: Any) -> WindowSpec:  # noqa: N802
        return WindowSpec().orderBy(*cols)

    @staticmethod
    def rowsBetween(start: int, end: int) -> WindowSpec:  # noqa: N802
        return WindowSpec().rowsBetween(start, end)

    @staticmethod
    def rangeBetween(start: int, end: int) -> WindowSpec:  # noqa: N802
        return WindowSpec().rangeBetween(start, end)


def _frame_bound(n: int) -> str:
    if n <= -(2**31):
        return "UNBOUNDED PRECEDING"
    if n >= 2**31 - 1:
        return "UNBOUNDED FOLLOWING"
    if n == 0:
        return "CURRENT ROW"
    if n < 0:
        return f"{-n} PRECEDING"
    return f"{n} FOLLOWING"
