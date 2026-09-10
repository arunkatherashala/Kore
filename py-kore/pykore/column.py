"""
PySpark-compatible Column expression builder for KORE Engine.

Every method returns a new :class:`Col` whose internal ``_expr`` is a SQL
fragment.  The expression is materialised only when the owning DataFrame is
executed, so building column expressions is purely string-based and never
touches the Rust engine.
"""

from __future__ import annotations

from typing import Any


class Col:
    """A column expression that compiles to a SQL fragment.

    Instantiate via the module-level helpers :func:`col` and :func:`lit`, or
    through ``DataFrame.__getitem__`` / ``DataFrame.__getattr__``.
    """

    def __init__(self, expr: str):
        self._expr = expr

    # -- representation --------------------------------------------------------

    def __repr__(self) -> str:
        return f"Col('{self._expr}')"

    def __str__(self) -> str:
        return self._expr

    # -- naming ----------------------------------------------------------------

    def alias(self, name: str) -> "Col":
        """Return ``<expr> AS <name>``."""
        return Col(f"{self._expr} AS {name}")

    name = alias  # PySpark alias

    # -- ordering --------------------------------------------------------------

    def asc(self) -> "Col":
        return Col(f"{self._expr} ASC")

    def desc(self) -> "Col":
        return Col(f"{self._expr} DESC")

    # -- type casting ----------------------------------------------------------

    def cast(self, dtype: Any) -> "Col":
        """``CAST(<expr> AS <dtype>)``."""
        type_name = dtype.typeName() if hasattr(dtype, "typeName") else str(dtype)
        return Col(f"CAST({self._expr} AS {type_name.upper()})")

    astype = cast  # Pandas-style alias

    # -- null checks -----------------------------------------------------------

    def isNull(self) -> "Col":  # noqa: N802
        return Col(f"{self._expr} IS NULL")

    def isNotNull(self) -> "Col":  # noqa: N802
        return Col(f"{self._expr} IS NOT NULL")

    # -- membership / range ----------------------------------------------------

    def isin(self, *values: Any) -> "Col":
        formatted = ", ".join(_sql_literal(v) for v in values)
        return Col(f"{self._expr} IN ({formatted})")

    def between(self, lower: Any, upper: Any) -> "Col":
        return Col(
            f"{self._expr} BETWEEN {_sql_literal(lower)} AND {_sql_literal(upper)}"
        )

    def like(self, pattern: str) -> "Col":
        return Col(f"{self._expr} LIKE '{pattern}'")

    def rlike(self, pattern: str) -> "Col":
        return Col(f"{self._expr} RLIKE '{pattern}'")

    def startswith(self, prefix: str) -> "Col":
        return Col(f"{self._expr} LIKE '{prefix}%'")

    def endswith(self, suffix: str) -> "Col":
        return Col(f"{self._expr} LIKE '%{suffix}'")

    def contains(self, value: str) -> "Col":
        return Col(f"{self._expr} LIKE '%{value}%'")

    # -- comparison operators --------------------------------------------------

    def __eq__(self, other: object) -> "Col":  # type: ignore[override]
        return Col(f"{self._expr} = {_sql_operand(other)}")

    def __ne__(self, other: object) -> "Col":  # type: ignore[override]
        return Col(f"{self._expr} != {_sql_operand(other)}")

    def __gt__(self, other: Any) -> "Col":
        return Col(f"{self._expr} > {_sql_operand(other)}")

    def __lt__(self, other: Any) -> "Col":
        return Col(f"{self._expr} < {_sql_operand(other)}")

    def __ge__(self, other: Any) -> "Col":
        return Col(f"{self._expr} >= {_sql_operand(other)}")

    def __le__(self, other: Any) -> "Col":
        return Col(f"{self._expr} <= {_sql_operand(other)}")

    # -- arithmetic operators --------------------------------------------------

    def __add__(self, other: Any) -> "Col":
        return Col(f"({self._expr} + {_sql_operand(other)})")

    def __radd__(self, other: Any) -> "Col":
        return Col(f"({_sql_operand(other)} + {self._expr})")

    def __sub__(self, other: Any) -> "Col":
        return Col(f"({self._expr} - {_sql_operand(other)})")

    def __rsub__(self, other: Any) -> "Col":
        return Col(f"({_sql_operand(other)} - {self._expr})")

    def __mul__(self, other: Any) -> "Col":
        return Col(f"({self._expr} * {_sql_operand(other)})")

    def __rmul__(self, other: Any) -> "Col":
        return Col(f"({_sql_operand(other)} * {self._expr})")

    def __truediv__(self, other: Any) -> "Col":
        return Col(f"({self._expr} / {_sql_operand(other)})")

    def __rtruediv__(self, other: Any) -> "Col":
        return Col(f"({_sql_operand(other)} / {self._expr})")

    def __mod__(self, other: Any) -> "Col":
        return Col(f"({self._expr} % {_sql_operand(other)})")

    def __rmod__(self, other: Any) -> "Col":
        return Col(f"({_sql_operand(other)} % {self._expr})")

    def __neg__(self) -> "Col":
        return Col(f"(-{self._expr})")

    # -- boolean operators (use & | ~ in Python, not and/or/not) ---------------

    def __and__(self, other: Any) -> "Col":
        return Col(f"({self._expr} AND {_sql_operand(other)})")

    def __rand__(self, other: Any) -> "Col":
        return Col(f"({_sql_operand(other)} AND {self._expr})")

    def __or__(self, other: Any) -> "Col":
        return Col(f"({self._expr} OR {_sql_operand(other)})")

    def __ror__(self, other: Any) -> "Col":
        return Col(f"({_sql_operand(other)} OR {self._expr})")

    def __invert__(self) -> "Col":
        return Col(f"(NOT {self._expr})")

    # -- over (window) ---------------------------------------------------------

    def over(self, window_spec: Any) -> "Col":
        """Apply a window specification to this column expression."""
        return Col(f"{self._expr} OVER ({window_spec})")


# -- module-level helpers ------------------------------------------------------


def col(name: str) -> Col:
    """Return a :class:`Col` referencing a column by *name*."""
    return Col(name)


def lit(value: Any) -> Col:
    """Return a :class:`Col` representing a literal *value*."""
    return Col(_sql_literal(value))


# -- internal helpers ----------------------------------------------------------


def _sql_literal(value: Any) -> str:
    """Convert a Python value to a SQL literal string."""
    if value is None:
        return "NULL"
    if isinstance(value, bool):
        return "TRUE" if value else "FALSE"
    if isinstance(value, str):
        escaped = value.replace("'", "''")
        return f"'{escaped}'"
    return str(value)


def _sql_operand(value: Any) -> str:
    """Convert a value or :class:`Col` to a SQL operand string."""
    if isinstance(value, Col):
        return value._expr
    return _sql_literal(value)
