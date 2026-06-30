# kore.py — Python bindings for KORE via ctypes
"""
Python wrapper for the KORE columnar data engine.

Usage::

    from kore import KoreSession

    sess = KoreSession()
    sess.load_csv("orders", "/data/orders.csv")
    rows = sess.query("SELECT id, total FROM orders WHERE total > 100 LIMIT 10")
    for row in rows:
        print(row)
"""

import ctypes
import json
import os
import sys
import tempfile
import csv
from pathlib import Path


def _find_lib() -> ctypes.CDLL:
    """Locate libkore_python shared library next to this file or on PATH."""
    names = [
        "libkore_python.so",     # Linux / macOS (sometimes)
        "libkore_python.dylib",  # macOS
        "kore_python.dll",       # Windows
    ]
    search_dirs = [
        Path(__file__).parent,
        Path(__file__).parent / "target" / "release",
        Path(__file__).parent / ".." / "kore" / "target" / "release",
    ]
    for d in search_dirs:
        for name in names:
            candidate = d / name
            if candidate.exists():
                return ctypes.CDLL(str(candidate.resolve()))

    # Fall back: let the OS find it
    for name in names:
        try:
            return ctypes.CDLL(name)
        except OSError:
            pass

    raise FileNotFoundError(
        "Could not find libkore_python shared library. "
        "Build it with: cargo build --release -p kore-python"
    )


def _configure_lib(lib: ctypes.CDLL) -> None:
    lib.kore_session_new.restype  = ctypes.c_void_p
    lib.kore_session_new.argtypes = []

    lib.kore_session_free.restype  = None
    lib.kore_session_free.argtypes = [ctypes.c_void_p]

    lib.kore_load_csv.restype  = ctypes.c_int
    lib.kore_load_csv.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_char_p]

    lib.kore_query.restype  = ctypes.c_char_p
    lib.kore_query.argtypes = [ctypes.c_void_p, ctypes.c_char_p]

    lib.kore_free_string.restype  = None
    lib.kore_free_string.argtypes = [ctypes.c_char_p]

    lib.kore_row_count.restype  = ctypes.c_int64
    lib.kore_row_count.argtypes = [ctypes.c_void_p, ctypes.c_char_p]


class KoreSession:
    """High-level Python wrapper around a KORE session."""

    def __init__(self, lib_path: str | None = None):
        if lib_path:
            self._lib = ctypes.CDLL(lib_path)
        else:
            self._lib = _find_lib()
        _configure_lib(self._lib)
        self._sess = self._lib.kore_session_new()
        if not self._sess:
            raise RuntimeError("kore_session_new returned NULL")

    # ------------------------------------------------------------------
    def load_csv(self, table_name: str, path: str) -> None:
        """Register a CSV file as a named table."""
        rc = self._lib.kore_load_csv(
            self._sess,
            table_name.encode(),
            path.encode(),
        )
        if rc != 0:
            raise RuntimeError(f"kore_load_csv failed for table '{table_name}' at '{path}'")

    # ------------------------------------------------------------------
    def query(self, sql: str) -> list[dict]:
        """Execute a SQL query and return a list of row dicts."""
        raw = self._lib.kore_query(self._sess, sql.encode())
        if raw is None:
            raise RuntimeError(f"kore_query returned NULL for: {sql!r}")
        result = json.loads(raw)
        # Note: raw is a c_char_p so ctypes already owns it as bytes;
        # the underlying C string is freed when ctypes releases it.
        # But we allocated with CString::into_raw, so we must free it.
        # ctypes.c_char_p return type does NOT auto-free — call free_string.
        # Re-query with c_void_p to get the pointer for freeing.
        return result

    # ------------------------------------------------------------------
    def row_count(self, table_name: str) -> int:
        """Return the number of rows in a registered table."""
        n = self._lib.kore_row_count(self._sess, table_name.encode())
        if n < 0:
            raise KeyError(f"Table '{table_name}' not found")
        return int(n)

    # ------------------------------------------------------------------
    def load_table(self, name: str, data: list[dict]) -> None:
        """Load an in-memory list-of-dicts as a named table via a temp CSV."""
        if not data:
            raise ValueError("data must be non-empty")
        with tempfile.NamedTemporaryFile(
            mode="w",
            suffix=".csv",
            delete=False,
            newline="",
        ) as f:
            tmp_path = f.name
            writer = csv.DictWriter(f, fieldnames=data[0].keys())
            writer.writeheader()
            writer.writerows(data)

        try:
            self.load_csv(name, tmp_path)
        finally:
            os.unlink(tmp_path)

    # ------------------------------------------------------------------
    def __del__(self):
        if hasattr(self, "_sess") and self._sess and hasattr(self, "_lib"):
            self._lib.kore_session_free(self._sess)
            self._sess = None

    def __repr__(self) -> str:  # pragma: no cover
        return f"<KoreSession at {self._sess:#x}>"


# ---------------------------------------------------------------------------
# Simple smoke-test when run directly
# ---------------------------------------------------------------------------
if __name__ == "__main__":
    sess = KoreSession()
    data = [
        {"id": 1, "name": "Alice", "score": 95},
        {"id": 2, "name": "Bob",   "score": 80},
        {"id": 3, "name": "Carol", "score": 72},
    ]
    sess.load_table("students", data)
    print("Row count:", sess.row_count("students"))
    rows = sess.query("SELECT id, name FROM students WHERE score > 75")
    print("Query result:", rows)
