"""
kore.py -- Python bindings for the KORE engine (ctypes, Python 3.8+).

Covers:
  * DataBlock / ML API  (KoreBlock, KoreModel)
  * SQL Session API     (KoreSession)

Build the native library first:
    cargo build --release -p kore-ffi

Then either:
    export KORE_LIB=/path/to/libkore_ffi.so   # override path
    python kore.py                             # smoke test
"""

import ctypes
import csv
import json
import math
import os
import sys
import tempfile
from enum import IntEnum
from pathlib import Path
from typing import Dict, List, Optional, Union


# -- Library loading -----------------------------------------------------------

def _find_lib() -> ctypes.CDLL:
    """Search for the KORE shared library, respecting KORE_LIB env var."""
    env_path = os.environ.get("KORE_LIB")
    if env_path:
        return ctypes.CDLL(env_path)

    base = Path(__file__).resolve().parent
    search_roots = [
        base / ".." / ".." / ".." / "target" / "release",
        base / ".." / ".." / "target" / "release",
        base / ".." / "target" / "release",
        Path.cwd() / "target" / "release",
    ]
    names = {
        "win32":  "kore_ffi.dll",
        "linux":  "libkore_ffi.so",
        "darwin": "libkore_ffi.dylib",
    }
    lib_name = names.get(sys.platform, "libkore_ffi.so")

    for root in search_roots:
        candidate = (root / lib_name).resolve()
        if candidate.exists():
            return ctypes.CDLL(str(candidate))

    raise OSError(
        f"KORE shared library '{lib_name}' not found.\n"
        "Build with: cargo build --release -p kore-ffi\n"
        "Then set KORE_LIB=/path/to/lib or place the library in kore/target/release/"
    )


_lib: Optional[ctypes.CDLL] = None


def _get_lib() -> ctypes.CDLL:
    global _lib
    if _lib is None:
        _lib = _find_lib()
        _setup_signatures(_lib)
    return _lib


def _setup_signatures(lib: ctypes.CDLL) -> None:
    """Declare all C function signatures for type-safety and correct ABI."""
    # -- Error handling
    lib.kore_last_error.restype  = ctypes.c_char_p
    lib.kore_last_error.argtypes = []

    # -- DataBlock
    lib.kore_block_new.restype   = ctypes.c_void_p
    lib.kore_block_new.argtypes  = []
    lib.kore_block_free.restype  = None
    lib.kore_block_free.argtypes = [ctypes.c_void_p]

    lib.kore_block_num_rows.restype  = ctypes.c_uint64
    lib.kore_block_num_rows.argtypes = [ctypes.c_void_p]
    lib.kore_block_num_cols.restype  = ctypes.c_uint32
    lib.kore_block_num_cols.argtypes = [ctypes.c_void_p]

    lib.kore_block_add_f64.restype  = ctypes.c_int
    lib.kore_block_add_f64.argtypes = [
        ctypes.c_void_p, ctypes.c_char_p,
        ctypes.POINTER(ctypes.c_double), ctypes.c_uint64,
    ]
    lib.kore_block_add_i64.restype  = ctypes.c_int
    lib.kore_block_add_i64.argtypes = [
        ctypes.c_void_p, ctypes.c_char_p,
        ctypes.POINTER(ctypes.c_int64), ctypes.c_uint64,
    ]
    lib.kore_block_get_f64.restype  = ctypes.c_int64
    lib.kore_block_get_f64.argtypes = [
        ctypes.c_void_p, ctypes.c_char_p,
        ctypes.POINTER(ctypes.c_double), ctypes.c_uint64,
    ]

    # -- HashJoin
    lib.kore_hash_join.restype  = ctypes.c_void_p
    lib.kore_hash_join.argtypes = [
        ctypes.c_void_p, ctypes.c_void_p,
        ctypes.c_char_p, ctypes.c_char_p, ctypes.c_int,
    ]

    # -- ML Models
    lib.kore_model_new.restype   = ctypes.c_void_p
    lib.kore_model_new.argtypes  = [ctypes.c_int, ctypes.c_int, ctypes.c_int]
    lib.kore_model_free.restype  = None
    lib.kore_model_free.argtypes = [ctypes.c_void_p]

    lib.kore_model_fit.restype  = ctypes.c_int
    lib.kore_model_fit.argtypes = [
        ctypes.c_void_p,
        ctypes.POINTER(ctypes.c_double), ctypes.c_uint64, ctypes.c_uint64,
        ctypes.POINTER(ctypes.c_double),
    ]
    lib.kore_model_predict.restype  = ctypes.c_int
    lib.kore_model_predict.argtypes = [
        ctypes.c_void_p,
        ctypes.POINTER(ctypes.c_double), ctypes.c_uint64, ctypes.c_uint64,
        ctypes.POINTER(ctypes.c_double),
    ]

    # -- SQL Session
    lib.kore_session_new.restype   = ctypes.c_void_p
    lib.kore_session_new.argtypes  = []
    lib.kore_session_free.restype  = None
    lib.kore_session_free.argtypes = [ctypes.c_void_p]

    lib.kore_session_load_csv.restype  = ctypes.c_int
    lib.kore_session_load_csv.argtypes = [
        ctypes.c_void_p, ctypes.c_char_p, ctypes.c_char_p,
    ]
    lib.kore_session_register_block.restype  = ctypes.c_int
    lib.kore_session_register_block.argtypes = [
        ctypes.c_void_p, ctypes.c_char_p, ctypes.c_void_p,
    ]
    # kore_session_query returns a heap-allocated char* that must be freed
    lib.kore_session_query.restype  = ctypes.c_void_p
    lib.kore_session_query.argtypes = [ctypes.c_void_p, ctypes.c_char_p]

    lib.kore_session_row_count.restype  = ctypes.c_int64
    lib.kore_session_row_count.argtypes = [ctypes.c_void_p, ctypes.c_char_p]

    lib.kore_free_string.restype  = None
    lib.kore_free_string.argtypes = [ctypes.c_void_p]


def _check_error(lib: ctypes.CDLL) -> None:
    err = lib.kore_last_error()
    if err:
        raise RuntimeError("KORE error: " + err.decode("utf-8", errors="replace"))


# -- KoreBlock -----------------------------------------------------------------

class KoreBlock:
    """In-memory columnar DataBlock. Supports f64 and i64 columns."""

    def __init__(self, _handle: Optional[int] = None) -> None:
        lib = _get_lib()
        self._lib = lib
        self._ptr = _handle if _handle is not None else lib.kore_block_new()
        if not self._ptr:
            _check_error(lib)
            raise RuntimeError("kore_block_new() returned NULL")

    def __del__(self) -> None:
        if getattr(self, "_ptr", None):
            self._lib.kore_block_free(self._ptr)
            self._ptr = None

    @property
    def num_rows(self) -> int:
        return int(self._lib.kore_block_num_rows(self._ptr))

    @property
    def num_cols(self) -> int:
        return int(self._lib.kore_block_num_cols(self._ptr))

    def add_f64(self, name: str, data: List[float]) -> None:
        """Add a float64 column. Use math.nan for NULL values."""
        arr = (ctypes.c_double * len(data))(*data)
        rc = self._lib.kore_block_add_f64(
            self._ptr, name.encode(), arr, ctypes.c_uint64(len(data))
        )
        if rc != 0:
            _check_error(self._lib)

    def add_i64(self, name: str, data: List[int]) -> None:
        """Add an int64 column. Use -(2**63) for NULL values."""
        arr = (ctypes.c_int64 * len(data))(*data)
        rc = self._lib.kore_block_add_i64(
            self._ptr, name.encode(), arr, ctypes.c_uint64(len(data))
        )
        if rc != 0:
            _check_error(self._lib)

    def get_f64(self, col: str) -> List[float]:
        """Read a float64 column as a Python list."""
        n = self.num_rows
        buf = (ctypes.c_double * n)()
        rc = self._lib.kore_block_get_f64(
            self._ptr, col.encode(), buf, ctypes.c_uint64(n)
        )
        if rc < 0:
            _check_error(self._lib)
        return list(buf[:rc])

    def join(
        self,
        right: "KoreBlock",
        left_key: str,
        right_key: str,
        how: str = "inner",
    ) -> "KoreBlock":
        """Hash-join this block with *right*. how in {'inner','left','full'}."""
        join_type = {"inner": 0, "left": 1, "full": 2}.get(how.lower(), 0)
        ptr = self._lib.kore_hash_join(
            self._ptr, right._ptr,
            left_key.encode(), right_key.encode(),
            ctypes.c_int(join_type),
        )
        if not ptr:
            _check_error(self._lib)
        return KoreBlock(_handle=ptr)

    @staticmethod
    def from_dict(d: Dict[str, List]) -> "KoreBlock":
        """Build a KoreBlock from {column_name: [values]} dict."""
        block = KoreBlock()
        for name, values in d.items():
            if values and isinstance(values[0], int):
                block.add_i64(name, values)
            else:
                block.add_f64(name, [float(v) for v in values])
        return block

    def __repr__(self) -> str:
        return f"KoreBlock(rows={self.num_rows}, cols={self.num_cols})"


# -- ML Models -----------------------------------------------------------------

class ModelType(IntEnum):
    RF_REGRESSOR     = 0
    RF_CLASSIFIER    = 1
    GBM_REGRESSOR    = 2
    LINEAR_REGRESSOR = 3
    LOGISTIC         = 4
    KNN_REGRESSOR    = 5
    KNN_CLASSIFIER   = 6
    SVM              = 7


class KoreModel:
    """KORE built-in ML model (random forest, gradient boosting, linear, ...)."""

    def __init__(
        self,
        model_type: ModelType,
        param1: int = 100,
        param2: int = 3,
    ) -> None:
        lib = _get_lib()
        self._lib = lib
        self._ptr = lib.kore_model_new(
            ctypes.c_int(int(model_type)),
            ctypes.c_int(param1),
            ctypes.c_int(param2),
        )
        if not self._ptr:
            _check_error(lib)
            raise RuntimeError("kore_model_new() returned NULL")

    def __del__(self) -> None:
        if getattr(self, "_ptr", None):
            self._lib.kore_model_free(self._ptr)
            self._ptr = None

    def fit(self, X: List[List[float]], y: List[float]) -> "KoreModel":
        """Fit the model. X is a 2-D row-major list; y is the label vector."""
        n_rows = len(X)
        n_cols = len(X[0]) if X else 0
        x_flat = [v for row in X for v in row]
        x_arr = (ctypes.c_double * len(x_flat))(*x_flat)
        y_arr = (ctypes.c_double * n_rows)(*y)
        rc = self._lib.kore_model_fit(
            self._ptr, x_arr,
            ctypes.c_uint64(n_rows), ctypes.c_uint64(n_cols),
            y_arr,
        )
        if rc != 0:
            _check_error(self._lib)
        return self

    def predict(self, X: List[List[float]]) -> List[float]:
        """Return predictions for X."""
        n_rows = len(X)
        n_cols = len(X[0]) if X else 0
        x_flat = [v for row in X for v in row]
        x_arr = (ctypes.c_double * len(x_flat))(*x_flat)
        out = (ctypes.c_double * n_rows)()
        rc = self._lib.kore_model_predict(
            self._ptr, x_arr,
            ctypes.c_uint64(n_rows), ctypes.c_uint64(n_cols),
            out,
        )
        if rc != 0:
            _check_error(self._lib)
        return list(out)

    def __repr__(self) -> str:
        return "KoreModel()"


# -- SQL Session ---------------------------------------------------------------

class KoreSession:
    """
    High-level SQL session backed by KORE's in-memory query engine.

    Each session is an independent in-memory database.  Tables are populated
    via CSV files or plain Python dicts.

    Example::

        with KoreSession() as sess:
            sess.load_table("sales", [
                {"region": "North", "amount": 1000.0},
                {"region": "South", "amount": 2000.0},
            ])
            rows = sess.query(
                "SELECT region, SUM(amount) AS total "
                "FROM sales GROUP BY region"
            )
            print(rows)
    """

    def __init__(self, lib_path: Optional[str] = None) -> None:
        if lib_path:
            lib = ctypes.CDLL(lib_path)
            _setup_signatures(lib)
            self._lib = lib
        else:
            self._lib = _get_lib()
        self._ptr = self._lib.kore_session_new()
        if not self._ptr:
            _check_error(self._lib)
            raise RuntimeError("kore_session_new() returned NULL")

    # -- Data loading ----------------------------------------------------------

    def load_csv(self, table: str, path: str) -> None:
        """Register a CSV file as a named table."""
        abs_path = str(Path(path).resolve())
        rc = self._lib.kore_session_load_csv(
            self._ptr, table.encode(), abs_path.encode()
        )
        if rc != 0:
            _check_error(self._lib)
            raise RuntimeError(
                f"kore_session_load_csv failed: table={table!r}, path={abs_path!r}"
            )

    def load_table(self, name: str, data: List[dict]) -> None:
        """
        Load a list of dicts as a named table.

        Data is serialised to a temporary CSV file, loaded into the session,
        then the temp file is removed.
        """
        if not data:
            raise ValueError("data must contain at least one row")
        fieldnames = list(data[0].keys())
        with tempfile.NamedTemporaryFile(
            mode="w", suffix=".csv", delete=False, newline=""
        ) as fh:
            tmp_path = fh.name
            writer = csv.DictWriter(fh, fieldnames=fieldnames)
            writer.writeheader()
            writer.writerows(data)
        try:
            self.load_csv(name, tmp_path)
        finally:
            try:
                os.unlink(tmp_path)
            except OSError:
                pass

    def register_block(self, table: str, block: KoreBlock) -> None:
        """Register a KoreBlock as a named SQL table (data is copied)."""
        rc = self._lib.kore_session_register_block(
            self._ptr, table.encode(), block._ptr
        )
        if rc != 0:
            _check_error(self._lib)

    # -- Query -----------------------------------------------------------------

    def query(self, sql: str) -> List[dict]:
        """
        Execute a SQL query and return results as a list of dicts.

        The engine returns JSON; this method deserialises it into Python objects.
        The underlying heap string is freed automatically.
        """
        raw_ptr = self._lib.kore_session_query(self._ptr, sql.encode())
        if not raw_ptr:
            _check_error(self._lib)
            raise RuntimeError("kore_session_query returned NULL for: " + repr(sql))
        try:
            json_bytes = ctypes.cast(raw_ptr, ctypes.c_char_p).value
        finally:
            self._lib.kore_free_string(raw_ptr)
        if json_bytes is None:
            return []
        return json.loads(json_bytes.decode("utf-8"))

    # -- Metadata --------------------------------------------------------------

    def row_count(self, table: str) -> int:
        """Return the number of rows in *table*, or raise KeyError if absent."""
        n = self._lib.kore_session_row_count(self._ptr, table.encode())
        if n < 0:
            _check_error(self._lib)
            raise KeyError(f"Table {table!r} not found in session")
        return int(n)

    # -- Lifecycle -------------------------------------------------------------

    def close(self) -> None:
        """Release the session; called automatically by __del__."""
        if getattr(self, "_ptr", None):
            self._lib.kore_session_free(self._ptr)
            self._ptr = None

    def __del__(self) -> None:
        self.close()

    def __enter__(self) -> "KoreSession":
        return self

    def __exit__(self, *_) -> None:
        self.close()

    def __repr__(self) -> str:
        addr = hex(self._ptr) if getattr(self, "_ptr", None) else "closed"
        return f"KoreSession(ptr={addr})"


# -- Smoke test ----------------------------------------------------------------

if __name__ == "__main__":
    print("=== KORE Python bindings smoke test ===\n")

    # DataBlock API
    print("1. DataBlock API")
    block = KoreBlock()
    block.add_f64("x", [1.0, 2.0, 3.0, 4.0])
    block.add_i64("id", [10, 20, 30, 40])
    print(f"   {block}")
    print(f"   x column: {block.get_f64('x')}")

    # ML Model
    print("\n2. ML Model (LinearRegressor)")
    model = KoreModel(ModelType.LINEAR_REGRESSOR)
    X_train = [[1.0], [2.0], [3.0], [4.0], [5.0]]
    y_train = [2.0, 4.0, 6.0, 8.0, 10.0]
    model.fit(X_train, y_train)
    preds = model.predict([[6.0], [7.0]])
    print(f"   Predictions for x=6,7: {preds}")

    # SQL Session
    print("\n3. SQL Session API")
    with KoreSession() as sess:
        sess.load_table("t", [
            {"x": 1, "y": 2.0},
            {"x": 3, "y": 4.0},
            {"x": 5, "y": 6.0},
        ])
        print(f"   row_count('t') = {sess.row_count('t')}")
        print(f"   SELECT SUM(y): {sess.query('SELECT SUM(y) AS total FROM t')}")
        print(f"   WHERE x>1:     {sess.query('SELECT x, y FROM t WHERE x > 1')}")

    # Block -> Session roundtrip
    print("\n4. register_block -> SQL query")
    blk = KoreBlock.from_dict({"a": [10, 20, 30], "b": [1.1, 2.2, 3.3]})
    with KoreSession() as sess2:
        sess2.register_block("blk", blk)
        result = sess2.query("SELECT SUM(b) AS s FROM blk")
        print(f"   SUM(b): {result}")

    print("\nAll tests passed.")