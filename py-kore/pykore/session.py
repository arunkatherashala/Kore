"""
PySpark-compatible SparkSession API for KORE Engine.

Usage::

    from pykore import SparkSession

    spark = SparkSession.builder \\
        .appName("my-app") \\
        .getOrCreate()

    df = spark.read.csv("data.csv")
    df.show()
"""

from __future__ import annotations

import csv as _csv_mod
import io
import json
import os
from typing import Any, Sequence

from .column import Col, _sql_literal
from .dataframe import KoreDataFrame, _col_expr


# ---------------------------------------------------------------------------
# Engine loader (lazy, best-effort)
# ---------------------------------------------------------------------------

def _try_load_engine() -> Any:
    """Try to import the compiled ``kore_engine`` PyO3 module.

    Returns an instantiated ``KoreSession`` from the native module, or
    ``None`` if it is not available.
    """
    try:
        import kore_engine  # type: ignore[import-untyped]

        return kore_engine.KoreSession()
    except ImportError:
        return None


# ---------------------------------------------------------------------------
# MockEngine (pure-Python fallback for testing)
# ---------------------------------------------------------------------------

class _MockEngine:
    """Trivial in-memory SQL engine used when the Rust native module is
    unavailable.  Stores tables as ``list[dict]`` and evaluates a small
    subset of SQL (enough for the test suite).
    """

    def __init__(self) -> None:
        self._tables: dict[str, list[dict[str, Any]]] = {}

    def load_csv(self, path: str, table_name: str) -> None:
        with open(path, newline="", encoding="utf-8") as fh:
            reader = _csv_mod.DictReader(fh)
            self._tables[table_name] = [dict(row) for row in reader]

    def read_parquet(self, path: str, table_name: str) -> None:
        raise NotImplementedError("Parquet read requires the native KORE engine")

    def write_parquet(self, query: str, path: str) -> None:
        raise NotImplementedError("Parquet write requires the native KORE engine")

    def tables(self) -> list[str]:
        return list(self._tables.keys())

    def register(self, name: str, rows: list[dict[str, Any]]) -> None:
        self._tables[name] = rows

    def sql(self, query: str) -> list[dict[str, Any]]:
        """Execute *query* against the in-memory tables.

        Supports a minimal SQL subset sufficient for unit tests:
        ``SELECT … FROM <table> [WHERE …] [ORDER BY …] [LIMIT n]``.
        For anything more complex the native engine is required.
        """
        return self._eval(query)

    # -- mini SQL evaluator ----------------------------------------------------

    def _eval(self, query: str) -> list[dict[str, Any]]:
        q = query.strip().rstrip(";")

        if q.upper().startswith("SELECT COUNT(*) AS cnt FROM"):
            inner = self._extract_subquery(q)
            rows = self._eval(inner) if inner else []
            return [{"cnt": len(rows)}]

        table = self._find_table(q)
        if table is None:
            return []

        rows = list(self._tables.get(table, []))

        where = self._extract_clause(q, "WHERE", stop_keywords=("GROUP", "ORDER", "LIMIT"))
        if where:
            rows = [r for r in rows if self._match_row(r, where)]

        order = self._extract_clause(q, "ORDER BY", stop_keywords=("LIMIT",))
        if order:
            rows = self._apply_order(rows, order)

        limit = self._extract_clause(q, "LIMIT", stop_keywords=())
        if limit:
            try:
                rows = rows[: int(limit.strip())]
            except ValueError:
                pass

        select_cols = self._extract_select(q)
        if select_cols and select_cols != ["*"]:
            rows = [{c: r.get(c) for c in select_cols} for r in rows]

        return rows

    def _find_table(self, q: str) -> str | None:
        upper = q.upper()
        idx = upper.find("FROM ")
        if idx == -1:
            return None
        rest = q[idx + 5:].strip()
        if rest.startswith("("):
            end = self._find_matching_paren(rest)
            inner = rest[1:end]
            alias_part = rest[end + 1:].strip()
            if alias_part.upper().startswith("AS "):
                pass
            inner_rows = self._eval(inner)
            tmp_name = f"__sub_{id(inner_rows)}"
            self._tables[tmp_name] = inner_rows
            return tmp_name
        token = rest.split()[0] if rest.split() else ""
        return token.strip("(),")

    @staticmethod
    def _find_matching_paren(s: str) -> int:
        depth = 0
        for i, ch in enumerate(s):
            if ch == "(":
                depth += 1
            elif ch == ")":
                depth -= 1
                if depth == 0:
                    return i
        return len(s) - 1

    @staticmethod
    def _extract_clause(q: str, keyword: str, stop_keywords: tuple[str, ...]) -> str | None:
        upper = q.upper()
        idx = upper.find(f" {keyword} ")
        if idx == -1:
            return None
        rest = q[idx + len(keyword) + 2:]
        for sk in stop_keywords:
            sk_idx = rest.upper().find(f" {sk} ")
            if sk_idx != -1:
                rest = rest[:sk_idx]
        return rest.strip()

    @staticmethod
    def _extract_subquery(q: str) -> str | None:
        idx = q.find("(")
        if idx == -1:
            return None
        depth = 0
        for i in range(idx, len(q)):
            if q[i] == "(":
                depth += 1
            elif q[i] == ")":
                depth -= 1
                if depth == 0:
                    return q[idx + 1: i]
        return None

    @staticmethod
    def _extract_select(q: str) -> list[str]:
        upper = q.upper()
        sel_idx = upper.find("SELECT ")
        if sel_idx == -1:
            return ["*"]
        rest = q[sel_idx + 7:]
        from_idx = rest.upper().find(" FROM ")
        if from_idx == -1:
            return ["*"]
        cols_str = rest[:from_idx].strip()
        if cols_str == "*":
            return ["*"]
        return [c.strip() for c in cols_str.split(",")]

    @staticmethod
    def _match_row(row: dict[str, Any], where: str) -> bool:
        parts = where.split(" AND ")
        for part in parts:
            part = part.strip().strip("()")
            for op in (">=", "<=", "!=", "=", ">", "<"):
                if f" {op} " in part:
                    lhs, rhs = part.split(f" {op} ", 1)
                    lhs = lhs.strip()
                    rhs = rhs.strip().strip("'\"")
                    lval = row.get(lhs, lhs)
                    try:
                        lval = float(lval)
                        rval = float(rhs)
                    except (ValueError, TypeError):
                        lval = str(lval) if lval is not None else ""
                        rval = rhs
                    if op == "=" and not (lval == rval):
                        return False
                    if op == "!=" and not (lval != rval):
                        return False
                    if op == ">" and not (lval > rval):
                        return False
                    if op == "<" and not (lval < rval):
                        return False
                    if op == ">=" and not (lval >= rval):
                        return False
                    if op == "<=" and not (lval <= rval):
                        return False
                    break
        return True

    @staticmethod
    def _apply_order(rows: list[dict], clause: str) -> list[dict]:
        parts = [p.strip() for p in clause.split(",")]
        for part in reversed(parts):
            tokens = part.split()
            col_name = tokens[0]
            desc = len(tokens) > 1 and tokens[1].upper() == "DESC"
            rows.sort(
                key=lambda r, c=col_name: (  # type: ignore[misc]
                    _sort_key(r.get(c))
                ),
                reverse=desc,
            )
        return rows


def _sort_key(v: Any) -> tuple[int, Any]:
    if v is None:
        return (1, "")
    try:
        return (0, float(v))
    except (ValueError, TypeError):
        return (0, str(v))


# ---------------------------------------------------------------------------
# Catalog
# ---------------------------------------------------------------------------

class Catalog:
    """Minimal catalog API matching ``spark.catalog``."""

    def __init__(self, session: "KoreSession"):
        self._session = session

    def listTables(self) -> list[str]:  # noqa: N802
        engine = self._session._engine
        if engine is None:
            return []
        if hasattr(engine, "tables"):
            result = engine.tables()
            return list(result) if result else []
        return []

    def tableExists(self, name: str) -> bool:  # noqa: N802
        return name in self.listTables()


# ---------------------------------------------------------------------------
# DataFrameReader
# ---------------------------------------------------------------------------

class DataFrameReader:
    """PySpark-compatible reader returned by ``session.read``."""

    def __init__(self, session: "KoreSession"):
        self._session = session
        self._format: str = "parquet"
        self._options: dict[str, str] = {}
        self._schema: Any = None

    def format(self, fmt: str) -> "DataFrameReader":
        self._format = fmt.lower()
        return self

    def option(self, key: str, value: Any) -> "DataFrameReader":
        self._options[key] = str(value)
        return self

    def options(self, **kwargs: Any) -> "DataFrameReader":
        for k, v in kwargs.items():
            self._options[k] = str(v)
        return self

    def schema(self, schema: Any) -> "DataFrameReader":
        self._schema = schema
        return self

    def load(self, path: str) -> KoreDataFrame:
        if self._format == "csv":
            return self.csv(path)
        if self._format == "parquet":
            return self.parquet(path)
        if self._format == "json":
            return self.json(path)
        raise ValueError(f"Unsupported format: {self._format}")

    def csv(self, path: str, **kwargs: Any) -> KoreDataFrame:
        """Load a CSV file as a :class:`KoreDataFrame`."""
        opts = {**self._options, **{k: str(v) for k, v in kwargs.items()}}
        engine = self._session._engine
        table_name = _table_name_from_path(path)

        if engine is not None:
            if hasattr(engine, "load_csv"):
                engine.load_csv(path, table_name)
            else:
                raise RuntimeError("Engine does not support CSV loading")
        else:
            raise RuntimeError(
                "No KORE engine available.  Install the kore_engine native "
                "module or provide a mock engine."
            )
        return KoreDataFrame(self._session, table_name)

    def parquet(self, path: str) -> KoreDataFrame:
        """Load a Parquet file as a :class:`KoreDataFrame`."""
        engine = self._session._engine
        table_name = _table_name_from_path(path)

        if engine is not None:
            if hasattr(engine, "read_parquet"):
                engine.read_parquet(path, table_name)
            else:
                raise RuntimeError("Engine does not support Parquet loading")
        else:
            raise RuntimeError("No KORE engine available.")
        return KoreDataFrame(self._session, table_name)

    def json(self, path: str) -> KoreDataFrame:
        """Load a JSON file as a :class:`KoreDataFrame` (stub)."""
        raise NotImplementedError(
            "JSON read is not yet supported by the KORE engine. "
            "Load via pandas and use session.createDataFrame()."
        )

    def table(self, name: str) -> KoreDataFrame:
        return self._session.table(name)


def _table_name_from_path(path: str) -> str:
    """Derive a table name from a file path."""
    base = os.path.basename(path)
    name = os.path.splitext(base)[0]
    return name.replace("-", "_").replace(" ", "_")


# ---------------------------------------------------------------------------
# KoreSession (SparkSession-compatible)
# ---------------------------------------------------------------------------

class KoreSession:
    """PySpark-compatible entry point for the KORE engine.

    Usage::

        spark = KoreSession.builder.appName("app").getOrCreate()
        df = spark.read.csv("data.csv")
        df.show()
    """

    # -- Builder (class-level singleton-ish pattern) ---------------------------

    class Builder:
        """Mimics ``SparkSession.Builder``."""

        def __init__(self) -> None:
            self._app_name: str = "KORE"
            self._master: str = "local"
            self._config: dict[str, str] = {}

        def master(self, url: str) -> "KoreSession.Builder":
            self._master = url
            return self

        def appName(self, name: str) -> "KoreSession.Builder":  # noqa: N802
            self._app_name = name
            return self

        def config(self, key: str, value: Any = None) -> "KoreSession.Builder":
            if value is not None:
                self._config[key] = str(value)
            return self

        def enableHiveSupport(self) -> "KoreSession.Builder":  # noqa: N802
            """No-op for API compatibility."""
            return self

        def getOrCreate(self) -> "KoreSession":  # noqa: N802
            return KoreSession(
                app_name=self._app_name,
                config=dict(self._config),
            )

    builder = Builder()

    # -- init / lifecycle ------------------------------------------------------

    def __init__(
        self,
        app_name: str = "KORE",
        config: dict[str, str] | None = None,
        engine: Any = None,
    ):
        self._app_name = app_name
        self._config = config or {}
        self._engine: Any = engine if engine is not None else _try_load_engine()
        if self._engine is None:
            self._engine = _MockEngine()

    def stop(self) -> None:
        """Stop the session (no-op — KORE sessions are lightweight)."""
        self._engine = None

    @property
    def sparkContext(self) -> "KoreSession":  # noqa: N802
        """Return self — there is no separate SparkContext in KORE."""
        return self

    @property
    def conf(self) -> dict[str, str]:
        return self._config

    # -- SQL -------------------------------------------------------------------

    def sql(self, query: str) -> KoreDataFrame:
        """Execute a SQL query and return the result as a :class:`KoreDataFrame`."""
        return KoreDataFrame._from_raw_sql(self, query)

    # -- table access ----------------------------------------------------------

    def table(self, name: str) -> KoreDataFrame:
        """Return a :class:`KoreDataFrame` backed by a registered table."""
        return KoreDataFrame(self, name)

    # -- read / create ---------------------------------------------------------

    @property
    def read(self) -> DataFrameReader:
        """Return a :class:`DataFrameReader`."""
        return DataFrameReader(self)

    def createDataFrame(  # noqa: N802
        self,
        data: Sequence[dict[str, Any] | Sequence] | Any,
        schema: Any = None,
    ) -> KoreDataFrame:
        """Create a :class:`KoreDataFrame` from an in-memory collection.

        Parameters
        ----------
        data:
            A list of dicts, a list of tuples/lists, or a ``pandas.DataFrame``.
        schema:
            A list of column-name strings, a :class:`~pykore.types.StructType`,
            or ``None`` to auto-detect.
        """
        rows = _normalise_data(data, schema)
        table_name = f"_mem_{id(rows)}"
        if isinstance(self._engine, _MockEngine):
            self._engine.register(table_name, rows)
        elif self._engine is not None:
            self._engine.register(table_name, rows) if hasattr(self._engine, "register") else None
        else:
            raise RuntimeError("No engine available")

        col_names = list(rows[0].keys()) if rows else []
        return KoreDataFrame(self, table_name, _schema_hint=col_names)

    # -- catalog ---------------------------------------------------------------

    @property
    def catalog(self) -> Catalog:
        return Catalog(self)

    # -- dunder ----------------------------------------------------------------

    def __repr__(self) -> str:
        return f"KoreSession(app='{self._app_name}')"

    def __enter__(self) -> "KoreSession":
        return self

    def __exit__(self, *exc: Any) -> None:
        self.stop()


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _normalise_data(
    data: Any,
    schema: Any,
) -> list[dict[str, Any]]:
    """Convert *data* + *schema* into a list of dicts."""
    try:
        import pandas as pd

        if isinstance(data, pd.DataFrame):
            return data.to_dict(orient="records")
    except ImportError:
        pass

    if not data:
        return []

    first = data[0] if data else None

    if isinstance(first, dict):
        return [dict(d) for d in data]

    col_names: list[str] = []
    if schema is not None:
        if isinstance(schema, (list, tuple)) and all(isinstance(s, str) for s in schema):
            col_names = list(schema)
        elif hasattr(schema, "names"):
            col_names = schema.names
        elif isinstance(schema, str):
            col_names = [s.strip() for s in schema.split(",")]
    if not col_names:
        col_names = [f"_{i}" for i in range(len(first))]

    return [dict(zip(col_names, row)) for row in data]
