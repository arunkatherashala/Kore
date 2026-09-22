"""kore — Python bindings for the KORE columnar engine and .kore file format.

Example::

    import kore

    sess = kore.KoreSession()
    sess.load_kore("orders", "orders.kore")          # read a .kore file
    rows = sess.query("SELECT id, total FROM orders WHERE total > 100")
    sess.save_kore("orders", "orders_copy.kore")      # write a .kore file
"""

from __future__ import annotations

import ctypes
import json
from pathlib import Path

__all__ = ["KoreSession", "KoreError", "__version__"]
__version__ = "0.1.0"


class KoreError(RuntimeError):
    """Raised when a KORE native call fails."""


def _load_native() -> ctypes.CDLL:
    names = ["kore_ffi.dll", "libkore_ffi.so", "libkore_ffi.dylib"]
    here = Path(__file__).resolve().parent
    for name in names:
        candidate = here / name
        if candidate.exists():
            return ctypes.CDLL(str(candidate))
    # Fall back to the OS loader (PATH / LD_LIBRARY_PATH).
    for name in names:
        try:
            return ctypes.CDLL(name)
        except OSError:
            continue
    raise FileNotFoundError(
        "Could not locate the KORE native library (kore_ffi). "
        "Expected one of {} next to {}.".format(names, here)
    )


def _configure(lib: ctypes.CDLL) -> None:
    lib.kore_session_new.restype = ctypes.c_void_p
    lib.kore_session_new.argtypes = []

    lib.kore_session_free.restype = None
    lib.kore_session_free.argtypes = [ctypes.c_void_p]

    for fn in ("kore_session_load_csv", "kore_session_load_kore",
               "kore_session_save_kore", "kore_session_load_parquet"):
        f = getattr(lib, fn)
        f.restype = ctypes.c_int
        f.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_char_p]

    lib.kore_session_query.restype = ctypes.c_void_p
    lib.kore_session_query.argtypes = [ctypes.c_void_p, ctypes.c_char_p]

    lib.kore_session_row_count.restype = ctypes.c_int64
    lib.kore_session_row_count.argtypes = [ctypes.c_void_p, ctypes.c_char_p]

    lib.kore_free_string.restype = None
    lib.kore_free_string.argtypes = [ctypes.c_void_p]

    lib.kore_last_error.restype = ctypes.c_void_p
    lib.kore_last_error.argtypes = []


_LIB = _load_native()
_configure(_LIB)


def _last_error() -> str:
    ptr = _LIB.kore_last_error()
    if not ptr:
        return "unknown KORE error"
    return ctypes.string_at(ptr).decode("utf-8", "replace")


class KoreSession:
    """A KORE SQL session that can read and write the native ``.kore`` format."""

    def __init__(self) -> None:
        self._sess = _LIB.kore_session_new()
        if not self._sess:
            raise KoreError("kore_session_new returned NULL")

    def load_kore(self, table: str, path: str) -> None:
        """Register a native ``.kore`` binary file as ``table``."""
        if _LIB.kore_session_load_kore(self._sess, table.encode(), str(path).encode()) != 0:
            raise KoreError(f"load_kore('{table}', '{path}') failed: {_last_error()}")

    def save_kore(self, table: str, path: str) -> None:
        """Write registered ``table`` to a native ``.kore`` binary file."""
        if _LIB.kore_session_save_kore(self._sess, table.encode(), str(path).encode()) != 0:
            raise KoreError(f"save_kore('{table}', '{path}') failed: {_last_error()}")

    def load_csv(self, table: str, path: str) -> None:
        """Register a CSV file as ``table``."""
        if _LIB.kore_session_load_csv(self._sess, table.encode(), str(path).encode()) != 0:
            raise KoreError(f"load_csv('{table}', '{path}') failed: {_last_error()}")

    def load_parquet(self, table: str, path: str) -> None:
        """Register an Apache Parquet file as ``table``."""
        if _LIB.kore_session_load_parquet(self._sess, table.encode(), str(path).encode()) != 0:
            raise KoreError(f"load_parquet('{table}', '{path}') failed: {_last_error()}")

    def query(self, sql: str) -> list[dict]:
        """Execute ``sql`` and return the rows as a list of dicts."""
        raw = _LIB.kore_session_query(self._sess, sql.encode())
        if not raw:
            raise KoreError(f"query failed: {_last_error()}")
        try:
            payload = ctypes.string_at(raw).decode("utf-8", "replace")
        finally:
            _LIB.kore_free_string(raw)
        return json.loads(payload)

    def row_count(self, table: str) -> int:
        """Return the number of rows in ``table`` (raises if not found)."""
        n = _LIB.kore_session_row_count(self._sess, table.encode())
        if n < 0:
            raise KeyError(f"table not found: {table}")
        return int(n)

    def close(self) -> None:
        if getattr(self, "_sess", None):
            _LIB.kore_session_free(self._sess)
            self._sess = None

    def __enter__(self) -> "KoreSession":
        return self

    def __exit__(self, *exc) -> None:
        self.close()

    def __del__(self) -> None:
        try:
            self.close()
        except Exception:
            pass
