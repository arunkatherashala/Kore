"""
KORE Python DataFrame API — PySpark-like interface over the KORE engine.

Uses ctypes to call into libkore_python (.so / .dll / .dylib).

Usage:
    from kore import KoreSession

    spark = KoreSession()
    spark.load_csv("/data/orders.csv", "orders")

    df = (spark.table("orders")
          .filter("total > 100")
          .select("id", "total", "region")
          .groupBy("region")
          .agg(total="SUM", cnt="COUNT")
          .orderBy("sum_total", desc=True)
          .limit(10))
    df.show()
    rows = df.collect()
"""

from __future__ import annotations

import ctypes
import json
import os
import platform
import sys
from pathlib import Path
from typing import Any


def _find_library() -> ctypes.CDLL:
    """Locate and load the KORE shared library."""
    system = platform.system()
    if system == "Windows":
        lib_name = "kore_python.dll"
    elif system == "Darwin":
        lib_name = "libkore_python.dylib"
    else:
        lib_name = "libkore_python.so"

    search_dirs = [
        Path(__file__).parent,
        Path(__file__).parent / "lib",
        Path(__file__).parent.parent / "target" / "release",
        Path(__file__).parent.parent / "target" / "debug",
        Path(__file__).parent.parent / "kore-python" / "target" / "release",
        Path(__file__).parent.parent / "kore-python" / "target" / "debug",
    ]

    env_path = os.environ.get("KORE_LIB_PATH")
    if env_path:
        search_dirs.insert(0, Path(env_path))

    for d in search_dirs:
        candidate = d / lib_name
        if candidate.exists():
            return ctypes.CDLL(str(candidate))

    return ctypes.CDLL(lib_name)


_lib = _find_library()

# ── Session FFI ──────────────────────────────────────────────────────────────

_lib.kore_session_new.argtypes = []
_lib.kore_session_new.restype = ctypes.c_void_p

_lib.kore_session_free.argtypes = [ctypes.c_void_p]
_lib.kore_session_free.restype = None

_lib.kore_load_csv.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_char_p]
_lib.kore_load_csv.restype = ctypes.c_int

_lib.kore_load_parquet.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_char_p]
_lib.kore_load_parquet.restype = ctypes.c_int

_lib.kore_write_parquet.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_char_p]
_lib.kore_write_parquet.restype = ctypes.c_int

_lib.kore_query.argtypes = [ctypes.c_void_p, ctypes.c_char_p]
_lib.kore_query.restype = ctypes.c_void_p

_lib.kore_free_string.argtypes = [ctypes.c_void_p]
_lib.kore_free_string.restype = None

# ── DataFrame FFI ────────────────────────────────────────────────────────────

_lib.kore_dataframe_new.argtypes = [ctypes.c_void_p, ctypes.c_char_p]
_lib.kore_dataframe_new.restype = ctypes.c_void_p

_lib.kore_dataframe_filter.argtypes = [ctypes.c_void_p, ctypes.c_char_p]
_lib.kore_dataframe_filter.restype = ctypes.c_void_p

_lib.kore_dataframe_select.argtypes = [ctypes.c_void_p, ctypes.c_char_p]
_lib.kore_dataframe_select.restype = ctypes.c_void_p

_lib.kore_dataframe_group_by.argtypes = [ctypes.c_void_p, ctypes.c_char_p]
_lib.kore_dataframe_group_by.restype = ctypes.c_void_p

_lib.kore_dataframe_agg.argtypes = [ctypes.c_void_p, ctypes.c_char_p]
_lib.kore_dataframe_agg.restype = ctypes.c_void_p

_lib.kore_dataframe_join.argtypes = [ctypes.c_void_p, ctypes.c_char_p]
_lib.kore_dataframe_join.restype = ctypes.c_void_p

_lib.kore_dataframe_order_by.argtypes = [ctypes.c_void_p, ctypes.c_char_p]
_lib.kore_dataframe_order_by.restype = ctypes.c_void_p

_lib.kore_dataframe_limit.argtypes = [ctypes.c_void_p, ctypes.c_int]
_lib.kore_dataframe_limit.restype = ctypes.c_void_p

_lib.kore_dataframe_collect.argtypes = [ctypes.c_void_p]
_lib.kore_dataframe_collect.restype = ctypes.c_void_p

_lib.kore_dataframe_show.argtypes = [ctypes.c_void_p]
_lib.kore_dataframe_show.restype = ctypes.c_void_p

_lib.kore_dataframe_free.argtypes = [ctypes.c_void_p]
_lib.kore_dataframe_free.restype = None


def _read_and_free(ptr: int | None) -> str | None:
    """Read a C string returned by KORE and free it."""
    if not ptr:
        return None
    try:
        value = ctypes.cast(ptr, ctypes.c_char_p).value
        return value.decode("utf-8") if value else None
    finally:
        _lib.kore_free_string(ptr)


class KoreDataFrame:
    """PySpark-style DataFrame backed by the KORE engine."""

    def __init__(self, handle: int, session: "KoreSession", table_name: str = ""):
        self._handle = handle
        self._session = session
        self._table_name = table_name

    def filter(self, predicate: str) -> KoreDataFrame:
        """Add a WHERE predicate."""
        new_handle = _lib.kore_dataframe_filter(
            self._handle, predicate.encode("utf-8")
        )
        self._handle = 0  # ownership transferred
        return KoreDataFrame(new_handle, self._session)

    def select(self, *cols: str) -> KoreDataFrame:
        """Project specific columns."""
        cols_json = json.dumps(list(cols)).encode("utf-8")
        new_handle = _lib.kore_dataframe_select(self._handle, cols_json)
        self._handle = 0
        return KoreDataFrame(new_handle, self._session)

    def groupBy(self, *keys: str) -> KoreDataFrame:
        """Set GROUP BY keys (PySpark naming convention)."""
        keys_json = json.dumps(list(keys)).encode("utf-8")
        new_handle = _lib.kore_dataframe_group_by(self._handle, keys_json)
        self._handle = 0
        return KoreDataFrame(new_handle, self._session)

    def group_by(self, *keys: str) -> KoreDataFrame:
        """Alias for groupBy (snake_case convention)."""
        return self.groupBy(*keys)

    def agg(self, **kwargs: str) -> KoreDataFrame:
        """
        Aggregate columns.

        Usage: df.groupBy("region").agg(sales="SUM", cnt="COUNT")
        Translates to SUM(sales) AS sum_sales, COUNT(cnt) AS count_cnt.
        """
        pairs = [[col, func] for col, func in kwargs.items()]
        agg_json = json.dumps(pairs).encode("utf-8")
        new_handle = _lib.kore_dataframe_agg(self._handle, agg_json)
        self._handle = 0
        return KoreDataFrame(new_handle, self._session)

    def join(self, other: "KoreDataFrame", on: str, how: str = "INNER") -> KoreDataFrame:
        """
        Join with another DataFrame on a shared column name.

        Args:
            other: The DataFrame to join with (must be backed by a registered table).
            on: Column name to join on.
            how: Join type — "INNER", "LEFT", "RIGHT", "FULL" (default: "INNER").
        """
        join_spec = json.dumps({
            "table": other._table_name if hasattr(other, "_table_name") else "",
            "left": on, "right": on, "type": how.upper(),
        }).encode("utf-8")
        new_handle = _lib.kore_dataframe_join(self._handle, join_spec)
        self._handle = 0
        return KoreDataFrame(new_handle, self._session)

    def orderBy(self, col: str, desc: bool = False) -> KoreDataFrame:
        """Sort results by column."""
        order_json = json.dumps([[col, desc]]).encode("utf-8")
        new_handle = _lib.kore_dataframe_order_by(self._handle, order_json)
        self._handle = 0
        return KoreDataFrame(new_handle, self._session)

    def order_by(self, col: str, desc: bool = False) -> KoreDataFrame:
        """Alias for orderBy (snake_case)."""
        return self.orderBy(col, desc)

    def limit(self, n: int) -> KoreDataFrame:
        """Limit the result to at most n rows."""
        new_handle = _lib.kore_dataframe_limit(self._handle, n)
        self._handle = 0
        return KoreDataFrame(new_handle, self._session)

    def show(self) -> None:
        """Execute the query and print a formatted table to stdout."""
        ptr = _lib.kore_dataframe_show(self._handle)
        result = _read_and_free(ptr)
        if result:
            print(result)
        else:
            print("(no results)")

    def collect(self) -> list[dict[str, Any]]:
        """Execute the query and return rows as a list of dicts."""
        ptr = _lib.kore_dataframe_collect(self._handle)
        result = _read_and_free(ptr)
        if result:
            return json.loads(result)
        return []

    def __del__(self):
        if self._handle:
            _lib.kore_dataframe_free(self._handle)
            self._handle = 0


class KoreSession:
    """
    Entry point for the KORE engine — analogous to SparkSession.

    Usage:
        spark = KoreSession()
        spark.load_csv("/data/orders.csv", "orders")
        df = spark.table("orders").filter("total > 50").select("id", "total")
        df.show()
    """

    def __init__(self):
        self._handle = _lib.kore_session_new()
        if not self._handle:
            raise RuntimeError("Failed to create KORE session")

    def load_csv(self, path: str, table_name: str) -> None:
        """Load a CSV file and register it as a named table."""
        rc = _lib.kore_load_csv(
            self._handle,
            table_name.encode("utf-8"),
            path.encode("utf-8"),
        )
        if rc != 0:
            raise RuntimeError(f"Failed to load CSV: {path}")

    def read_parquet(self, path: str, table_name: str) -> None:
        """Load a Parquet file and register it as a named table."""
        rc = _lib.kore_load_parquet(
            self._handle,
            table_name.encode("utf-8"),
            path.encode("utf-8"),
        )
        if rc != 0:
            raise RuntimeError(f"Failed to load Parquet: {path}")

    def write_parquet(self, sql_query: str, path: str) -> None:
        """Execute SQL and write results to a Parquet file."""
        rc = _lib.kore_write_parquet(
            self._handle,
            sql_query.encode("utf-8"),
            path.encode("utf-8"),
        )
        if rc != 0:
            raise RuntimeError(f"Failed to write Parquet: {path}")

    def sql(self, query: str) -> list[dict[str, Any]]:
        """Execute a raw SQL query and return rows as a list of dicts."""
        ptr = _lib.kore_query(self._handle, query.encode("utf-8"))
        result = _read_and_free(ptr)
        if result:
            return json.loads(result)
        raise RuntimeError(f"Query failed: {query}")

    def table(self, name: str) -> KoreDataFrame:
        """Return a DataFrame bound to a registered table."""
        handle = _lib.kore_dataframe_new(
            self._handle, name.encode("utf-8")
        )
        if not handle:
            raise RuntimeError(f"Table not found: {name}")
        return KoreDataFrame(handle, self, table_name=name)

    def close(self) -> None:
        """Free the underlying session."""
        if self._handle:
            _lib.kore_session_free(self._handle)
            self._handle = 0

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()

    def __del__(self):
        self.close()
