"""
PySpark-compatible DataFrame API for KORE Engine.

Operations are **lazy**: each transformation (``select``, ``filter``, …) returns
a new :class:`KoreDataFrame` that records the operation.  Execution happens only
when an *action* is triggered (``collect``, ``show``, ``count``, ``toPandas``).

Internally every operation contributes to a SQL query string that is sent to
the KORE engine (PyO3 ``kore_engine`` module) at execution time.
"""

from __future__ import annotations

import itertools
from typing import Any, Sequence

from .column import Col, _sql_literal, _sql_operand


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _col_expr(c: Any) -> str:
    """Normalise a column argument to a SQL expression string."""
    if isinstance(c, Col):
        return c._expr
    if isinstance(c, str):
        return c
    return str(c)


_next_alias = itertools.count(1)


def _tmp_alias() -> str:
    return f"_t{next(_next_alias)}"


# ---------------------------------------------------------------------------
# GroupedData
# ---------------------------------------------------------------------------

class GroupedData:
    """Result of ``DataFrame.groupBy(…)`` — call an aggregate method to get a
    :class:`KoreDataFrame` back."""

    def __init__(self, df: "KoreDataFrame", group_cols: list[str]):
        self._df = df
        self._group_cols = group_cols

    # -- convenience aggregates -----------------------------------------------

    def agg(self, *exprs: Any) -> "KoreDataFrame":
        """Aggregate with one or more :class:`Col` expressions or a ``dict``
        mapping column names to aggregate function names
        (``{"salary": "avg", "age": "max"}``).
        """
        if len(exprs) == 1 and isinstance(exprs[0], dict):
            agg_parts = [
                f"{func.upper()}({col_name}) AS {func.lower()}_{col_name}"
                for col_name, func in exprs[0].items()
            ]
        else:
            agg_parts = [_col_expr(e) for e in exprs]

        group_str = ", ".join(self._group_cols)
        agg_str = ", ".join(agg_parts)
        select_clause = f"{group_str}, {agg_str}" if group_str else agg_str
        group_by_clause = f" GROUP BY {group_str}" if group_str else ""
        parent_sql = self._df._build_sql()

        alias = _tmp_alias()
        sql = f"SELECT {select_clause} FROM ({parent_sql}) AS {alias}{group_by_clause}"
        return KoreDataFrame._from_raw_sql(self._df._session, sql)

    def count(self) -> "KoreDataFrame":
        return self.agg(Col("COUNT(*) AS count"))

    def sum(self, *cols: str) -> "KoreDataFrame":
        agg_exprs = [Col(f"SUM({c}) AS sum_{c}") for c in cols]
        return self.agg(*agg_exprs)

    def avg(self, *cols: str) -> "KoreDataFrame":
        agg_exprs = [Col(f"AVG({c}) AS avg_{c}") for c in cols]
        return self.agg(*agg_exprs)

    mean = avg

    def min(self, *cols: str) -> "KoreDataFrame":
        agg_exprs = [Col(f"MIN({c}) AS min_{c}") for c in cols]
        return self.agg(*agg_exprs)

    def max(self, *cols: str) -> "KoreDataFrame":
        agg_exprs = [Col(f"MAX({c}) AS max_{c}") for c in cols]
        return self.agg(*agg_exprs)

    def pivot(self, pivot_col: str, values: list[Any] | None = None) -> "GroupedData":
        """Stub for PySpark API compatibility — not yet implemented."""
        raise NotImplementedError("pivot() is not yet supported by KORE Engine")


# ---------------------------------------------------------------------------
# KoreDataFrame
# ---------------------------------------------------------------------------

class KoreDataFrame:
    """A lazy, PySpark-compatible DataFrame backed by the KORE SQL engine.

    Do not instantiate directly; use :class:`~pykore.session.KoreSession`
    methods (``table``, ``sql``, ``createDataFrame``, ``read.csv``, …).
    """

    def __init__(
        self,
        session: Any,
        table_name: str,
        *,
        _operations: list[tuple[str, Any]] | None = None,
        _raw_sql: str | None = None,
        _schema_hint: list[str] | None = None,
    ):
        self._session = session
        self._table_name = table_name
        self._operations: list[tuple[str, Any]] = list(_operations or [])
        self._raw_sql = _raw_sql
        self._schema_hint = _schema_hint

    @classmethod
    def _from_raw_sql(
        cls,
        session: Any,
        sql: str,
        schema_hint: list[str] | None = None,
    ) -> "KoreDataFrame":
        return cls(session, "", _raw_sql=sql, _schema_hint=schema_hint)

    # -- internal SQL builder --------------------------------------------------

    def _build_sql(self) -> str:
        """Compile the chain of operations into a single SQL query string."""
        if self._raw_sql is not None:
            base_sql = self._raw_sql
        else:
            base_sql = f"SELECT * FROM {self._table_name}"

        if not self._operations:
            return base_sql

        select_cols: list[str] | None = None
        filters: list[str] = []
        order_parts: list[str] = []
        limit_n: int | None = None
        extra_selects: list[str] = []
        drops: set[str] = set()

        for op, arg in self._operations:
            if op == "select":
                select_cols = arg
            elif op == "filter":
                filters.append(arg)
            elif op == "order":
                order_parts.append(arg)
            elif op == "limit":
                limit_n = arg
            elif op == "with_column":
                col_name, col_expr = arg
                extra_selects.append(f"{col_expr} AS {col_name}")
            elif op == "drop":
                drops.update(arg)
            elif op == "rename":
                old_name, new_name = arg
                extra_selects.append(f"{old_name} AS {new_name}")
                drops.add(old_name)
            elif op == "distinct":
                base_sql = f"SELECT DISTINCT * FROM ({base_sql}) AS {_tmp_alias()}"
            elif op == "union":
                base_sql = f"{base_sql} UNION ALL ({arg})"
            elif op == "intersect":
                base_sql = f"{base_sql} INTERSECT ({arg})"
            elif op == "subtract":
                base_sql = f"{base_sql} EXCEPT ({arg})"
            elif op == "join":
                join_type, other_sql, on_expr = arg
                alias_l = _tmp_alias()
                alias_r = _tmp_alias()
                base_sql = (
                    f"SELECT * FROM ({base_sql}) AS {alias_l} "
                    f"{join_type} JOIN ({other_sql}) AS {alias_r} ON {on_expr}"
                )

        alias = _tmp_alias()
        projection = "*"
        if select_cols is not None:
            projection = ", ".join(select_cols)
        elif extra_selects or drops:
            parts = ["*"] + extra_selects
            projection = ", ".join(parts)

        where_clause = ""
        if filters:
            where_clause = " WHERE " + " AND ".join(f"({f})" for f in filters)

        order_clause = ""
        if order_parts:
            order_clause = " ORDER BY " + ", ".join(order_parts)

        limit_clause = ""
        if limit_n is not None:
            limit_clause = f" LIMIT {limit_n}"

        return (
            f"SELECT {projection} FROM ({base_sql}) AS {alias}"
            f"{where_clause}{order_clause}{limit_clause}"
        )

    def _get_engine(self) -> Any:
        """Return the native KORE engine session, or ``None``."""
        return getattr(self._session, "_engine", None)

    def _execute(self) -> list[dict[str, Any]]:
        """Execute the query and return rows as dicts."""
        engine = self._get_engine()
        sql = self._build_sql()
        if engine is not None:
            return engine.sql(sql)
        raise RuntimeError(
            "No KORE engine available.  Install the kore_engine native module "
            "or use KoreSession with a mock engine for testing."
        )

    # -- column access ---------------------------------------------------------

    def __getitem__(self, item: str | int | list) -> "Col | KoreDataFrame":
        if isinstance(item, str):
            return Col(item)
        if isinstance(item, (list, tuple)):
            return self.select(*item)
        if isinstance(item, int):
            return Col(str(item))
        if isinstance(item, Col):
            return self.filter(item)
        raise TypeError(f"Unsupported key type: {type(item)}")

    def __getattr__(self, name: str) -> Col:
        if name.startswith("_"):
            raise AttributeError(name)
        return Col(name)

    # -- transformations (lazy) ------------------------------------------------

    def select(self, *cols: Any) -> "KoreDataFrame":
        """Project a set of columns or column expressions."""
        exprs = [_col_expr(c) for c in cols]
        return self._with_op("select", exprs)

    def filter(self, condition: Any) -> "KoreDataFrame":
        """Filter rows by a boolean expression (string or :class:`Col`)."""
        return self._with_op("filter", _col_expr(condition))

    where = filter

    def groupBy(self, *cols: Any) -> GroupedData:  # noqa: N802
        """Group by one or more columns, returning a :class:`GroupedData`."""
        return GroupedData(self, [_col_expr(c) for c in cols])

    groupby = groupBy

    def join(
        self,
        other: "KoreDataFrame",
        on: str | Col | list | None = None,
        how: str = "inner",
    ) -> "KoreDataFrame":
        """Join with *other* DataFrame.

        Parameters
        ----------
        on:
            Column name(s) to join on (equi-join).
        how:
            Join type — ``"inner"``, ``"left"``, ``"right"``, ``"full"``,
            ``"cross"``, ``"left_outer"``, ``"right_outer"``, ``"full_outer"``.
        """
        how_upper = how.upper().replace("_OUTER", "").replace("_", " ")
        if how_upper == "CROSS":
            join_keyword = "CROSS"
        elif how_upper == "LEFT":
            join_keyword = "LEFT"
        elif how_upper == "RIGHT":
            join_keyword = "RIGHT"
        elif how_upper in ("FULL", "OUTER", "FULL OUTER"):
            join_keyword = "FULL"
        else:
            join_keyword = "INNER"

        other_sql = other._build_sql()

        if on is None:
            on_expr = "1 = 1"
        elif isinstance(on, str):
            alias_l = "_jl"
            alias_r = "_jr"
            on_expr = f"{alias_l}.{on} = {alias_r}.{on}"
        elif isinstance(on, Col):
            on_expr = on._expr
        elif isinstance(on, (list, tuple)):
            alias_l = "_jl"
            alias_r = "_jr"
            parts = [f"{alias_l}.{_col_expr(c)} = {alias_r}.{_col_expr(c)}" for c in on]
            on_expr = " AND ".join(parts)
        else:
            on_expr = str(on)

        return self._with_op("join", (join_keyword, other_sql, on_expr))

    def crossJoin(self, other: "KoreDataFrame") -> "KoreDataFrame":  # noqa: N802
        return self.join(other, how="cross")

    def orderBy(self, *cols: Any, ascending: bool | list[bool] | None = None) -> "KoreDataFrame":  # noqa: N802
        """Sort by one or more columns."""
        parts: list[str] = []
        col_list = list(cols)
        if isinstance(ascending, list):
            for c, asc in zip(col_list, ascending):
                direction = "ASC" if asc else "DESC"
                parts.append(f"{_col_expr(c)} {direction}")
        elif ascending is False:
            for c in col_list:
                parts.append(f"{_col_expr(c)} DESC")
        else:
            for c in col_list:
                parts.append(_col_expr(c))
        order_str = ", ".join(parts)
        return self._with_op("order", order_str)

    sort = orderBy

    def limit(self, n: int) -> "KoreDataFrame":
        return self._with_op("limit", n)

    def distinct(self) -> "KoreDataFrame":
        return self._with_op("distinct", None)

    def dropDuplicates(self, subset: list[str] | None = None) -> "KoreDataFrame":  # noqa: N802
        if subset:
            raise NotImplementedError(
                "dropDuplicates with subset is not yet supported; use distinct()"
            )
        return self.distinct()

    drop_duplicates = dropDuplicates

    def withColumn(self, name: str, col: Any) -> "KoreDataFrame":  # noqa: N802
        """Add or replace a column."""
        return self._with_op("with_column", (name, _col_expr(col)))

    def withColumnRenamed(self, existing: str, new: str) -> "KoreDataFrame":  # noqa: N802
        """Rename a column."""
        return self._with_op("rename", (existing, new))

    def drop(self, *cols: str) -> "KoreDataFrame":
        """Drop one or more columns."""
        return self._with_op("drop", list(cols))

    def union(self, other: "KoreDataFrame") -> "KoreDataFrame":
        return self._with_op("union", other._build_sql())

    unionAll = union  # noqa: N815

    def unionByName(self, other: "KoreDataFrame", allowMissingColumns: bool = False) -> "KoreDataFrame":  # noqa: N802, N803
        return self.union(other)

    def intersect(self, other: "KoreDataFrame") -> "KoreDataFrame":
        return self._with_op("intersect", other._build_sql())

    def subtract(self, other: "KoreDataFrame") -> "KoreDataFrame":
        return self._with_op("subtract", other._build_sql())

    exceptAll = subtract  # noqa: N815

    def alias(self, name: str) -> "KoreDataFrame":
        sql = self._build_sql()
        return KoreDataFrame._from_raw_sql(self._session, f"({sql}) AS {name}")

    def sample(
        self,
        withReplacement: bool | None = None,  # noqa: N803
        fraction: float | None = None,
        seed: int | None = None,
    ) -> "KoreDataFrame":
        """Stub — not fully supported; returns self for API compatibility."""
        return self

    def cache(self) -> "KoreDataFrame":
        """No-op (KORE operates in-memory already)."""
        return self

    persist = cache
    unpersist = cache

    def coalesce(self, numPartitions: int) -> "KoreDataFrame":  # noqa: N803
        """No-op — KORE is single-partition."""
        return self

    def repartition(self, numPartitions: int, *cols: Any) -> "KoreDataFrame":  # noqa: N803
        """No-op — KORE is single-partition."""
        return self

    # -- actions (eager) -------------------------------------------------------

    def show(self, n: int = 20, truncate: bool = True) -> None:
        """Execute the query and print a formatted table to stdout."""
        rows = self.limit(n)._execute()
        if not rows:
            print("(empty DataFrame)")
            return
        cols_list = list(rows[0].keys())
        col_widths = {c: len(c) for c in cols_list}
        str_rows: list[dict[str, str]] = []
        for row in rows:
            sr: dict[str, str] = {}
            for c in cols_list:
                val = row.get(c)
                s = "null" if val is None else str(val)
                if truncate and len(s) > 20:
                    s = s[:17] + "..."
                sr[c] = s
                col_widths[c] = max(col_widths[c], len(s))
            str_rows.append(sr)

        sep = "+" + "+".join("-" * (col_widths[c] + 2) for c in cols_list) + "+"
        header = "|" + "|".join(f" {c:>{col_widths[c]}} " for c in cols_list) + "|"
        print(sep)
        print(header)
        print(sep)
        for sr in str_rows:
            line = "|" + "|".join(f" {sr[c]:>{col_widths[c]}} " for c in cols_list) + "|"
            print(line)
        print(sep)

    def collect(self) -> list[dict[str, Any]]:
        """Execute the query and return all rows as a list of dicts.

        In PySpark this returns ``list[Row]``; here dicts serve the same role.
        """
        return self._execute()

    def count(self) -> int:
        """Execute the query and return the number of rows."""
        sql = self._build_sql()
        engine = self._get_engine()
        if engine is not None:
            count_sql = f"SELECT COUNT(*) AS cnt FROM ({sql}) AS _c"
            result = engine.sql(count_sql)
            if result:
                return int(result[0].get("cnt", 0))
            return 0
        return len(self._execute())

    def first(self) -> dict[str, Any] | None:
        rows = self.limit(1)._execute()
        return rows[0] if rows else None

    def head(self, n: int = 1) -> list[dict[str, Any]] | dict[str, Any]:
        rows = self.limit(n)._execute()
        if n == 1:
            return rows[0] if rows else {}
        return rows

    def take(self, n: int) -> list[dict[str, Any]]:
        return self.limit(n)._execute()

    def toPandas(self) -> Any:  # noqa: N802
        """Execute the query and return a ``pandas.DataFrame``."""
        import pandas as pd

        rows = self._execute()
        return pd.DataFrame(rows)

    def toLocalIterator(self) -> Any:  # noqa: N802
        return iter(self._execute())

    def foreach(self, f: Any) -> None:
        for row in self._execute():
            f(row)

    def foreachPartition(self, f: Any) -> None:  # noqa: N802
        f(iter(self._execute()))

    # -- schema / metadata -----------------------------------------------------

    @property
    def columns(self) -> list[str]:
        """Return column names by executing a ``LIMIT 0`` probe."""
        if self._schema_hint:
            return list(self._schema_hint)
        rows = self.limit(1)._execute()
        if rows:
            return list(rows[0].keys())
        return []

    @property
    def dtypes(self) -> list[tuple[str, str]]:
        """Return ``(name, type_string)`` pairs — types are inferred from the
        first row since KORE returns Python-native values.
        """
        rows = self.limit(1)._execute()
        if not rows:
            return []
        type_map = {int: "bigint", float: "double", str: "string", bool: "boolean"}
        return [
            (k, type_map.get(type(v), "string"))
            for k, v in rows[0].items()
        ]

    @property
    def schema(self) -> Any:
        """Return a :class:`~pykore.types.StructType` schema."""
        from .types import StructType, StructField, StringType, LongType, DoubleType, BooleanType

        rows = self.limit(1)._execute()
        if not rows:
            return StructType()
        py_to_dt = {int: LongType, float: DoubleType, str: StringType, bool: BooleanType}
        fields = [
            StructField(k, py_to_dt.get(type(v), StringType)())
            for k, v in rows[0].items()
        ]
        return StructType(fields)

    def printSchema(self) -> None:  # noqa: N802
        """Print the schema in a tree format to stdout."""
        print("root")
        for name, dtype in self.dtypes:
            print(f" |-- {name}: {dtype} (nullable = true)")

    def explain(self, extended: bool = False) -> None:
        """Print the physical (SQL) plan."""
        sql = self._build_sql()
        print(f"== Physical Plan ==\n{sql}")

    def describe(self, *cols: str) -> "KoreDataFrame":
        """Compute summary statistics (count, mean, stddev, min, max).

        Returns a new DataFrame with these stats as rows.
        """
        target_cols = list(cols) if cols else self.columns
        parts: list[str] = []
        for c in target_cols:
            parts.append(
                f"SELECT '{c}' AS summary, "
                f"CAST(COUNT({c}) AS STRING) AS count, "
                f"CAST(AVG({c}) AS STRING) AS mean, "
                f"CAST(MIN({c}) AS STRING) AS min, "
                f"CAST(MAX({c}) AS STRING) AS max "
                f"FROM ({self._build_sql()}) AS _d"
            )
        sql = " UNION ALL ".join(parts)
        return KoreDataFrame._from_raw_sql(self._session, sql)

    def isEmpty(self) -> bool:  # noqa: N802
        return self.count() == 0

    # -- write -----------------------------------------------------------------

    @property
    def write(self) -> "DataFrameWriter":
        from .writer import DataFrameWriter

        return DataFrameWriter(self)

    # -- internals -------------------------------------------------------------

    def _with_op(self, op: str, arg: Any) -> "KoreDataFrame":
        new_ops = list(self._operations) + [(op, arg)]
        return KoreDataFrame(
            self._session,
            self._table_name,
            _operations=new_ops,
            _raw_sql=self._raw_sql,
            _schema_hint=self._schema_hint,
        )

    def __repr__(self) -> str:
        return f"KoreDataFrame[{self._build_sql()[:80]}...]"
