"""
kore.py — Python bindings for the KORE engine.

Usage:
    from kore import KoreBlock, KoreModel, ModelType

    block = KoreBlock()
    block.add_f64("score", [1.0, 2.0, 3.0])
    block.add_i64("id",    [1, 2, 3])

    model = KoreModel(ModelType.LINEAR_REGRESSOR)
    model.fit(x, y)
    preds = model.predict(x)

Build / install:
    pip install maturin
    cd kore/kore-ffi
    maturin develop   # or maturin build --release for a wheel

Alternatively, use ctypes against the pre-built shared library:
    KORE_LIB=/path/to/libkore_ffi.so python kore.py
"""

import ctypes, os, sys, math
import numpy as np
from enum import IntEnum
from pathlib import Path
from typing import List, Optional, Union

# ── Library loading ───────────────────────────────────────────────────────────

def _find_lib() -> ctypes.CDLL:
    env_path = os.environ.get("KORE_LIB")
    if env_path:
        return ctypes.CDLL(env_path)
    candidates = [
        Path(__file__).parent.parent / "target/release/kore_ffi.dll",
        Path(__file__).parent.parent / "target/release/libkore_ffi.so",
        Path(__file__).parent.parent / "target/release/libkore_ffi.dylib",
    ]
    for p in candidates:
        if p.exists():
            return ctypes.CDLL(str(p))
    raise OSError(
        "KORE shared library not found. "
        "Build with: cargo build --release -p kore-ffi\n"
        "Then set KORE_LIB=/path/to/lib or place it in kore/target/release/"
    )

_lib = None
def _get_lib() -> ctypes.CDLL:
    global _lib
    if _lib is None:
        _lib = _find_lib()
        _setup_signatures(_lib)
    return _lib

def _setup_signatures(lib: ctypes.CDLL):
    lib.kore_last_error.restype  = ctypes.c_char_p
    lib.kore_last_error.argtypes = []

    lib.kore_block_new.restype  = ctypes.c_void_p
    lib.kore_block_free.argtypes = [ctypes.c_void_p]
    lib.kore_block_num_rows.restype  = ctypes.c_uint64
    lib.kore_block_num_rows.argtypes = [ctypes.c_void_p]
    lib.kore_block_num_cols.restype  = ctypes.c_uint32
    lib.kore_block_num_cols.argtypes = [ctypes.c_void_p]

    lib.kore_block_add_f64.restype  = ctypes.c_int
    lib.kore_block_add_f64.argtypes = [ctypes.c_void_p, ctypes.c_char_p,
                                        ctypes.POINTER(ctypes.c_double), ctypes.c_uint64]
    lib.kore_block_add_i64.restype  = ctypes.c_int
    lib.kore_block_add_i64.argtypes = [ctypes.c_void_p, ctypes.c_char_p,
                                        ctypes.POINTER(ctypes.c_int64), ctypes.c_uint64]
    lib.kore_block_get_f64.restype  = ctypes.c_int64
    lib.kore_block_get_f64.argtypes = [ctypes.c_void_p, ctypes.c_char_p,
                                        ctypes.POINTER(ctypes.c_double), ctypes.c_uint64]
    lib.kore_hash_join.restype  = ctypes.c_void_p
    lib.kore_hash_join.argtypes = [ctypes.c_void_p, ctypes.c_void_p,
                                    ctypes.c_char_p, ctypes.c_char_p, ctypes.c_int]

    lib.kore_model_new.restype  = ctypes.c_void_p
    lib.kore_model_new.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_int]
    lib.kore_model_free.argtypes = [ctypes.c_void_p]
    lib.kore_model_fit.restype  = ctypes.c_int
    lib.kore_model_fit.argtypes = [ctypes.c_void_p,
                                    ctypes.POINTER(ctypes.c_double),
                                    ctypes.c_uint64, ctypes.c_uint64,
                                    ctypes.POINTER(ctypes.c_double)]
    lib.kore_model_predict.restype  = ctypes.c_int
    lib.kore_model_predict.argtypes = [ctypes.c_void_p,
                                        ctypes.POINTER(ctypes.c_double),
                                        ctypes.c_uint64, ctypes.c_uint64,
                                        ctypes.POINTER(ctypes.c_double)]

def _check_error(lib):
    err = lib.kore_last_error()
    if err: raise RuntimeError(f"KORE error: {err.decode()}")

# ── KoreBlock ─────────────────────────────────────────────────────────────────

class KoreBlock:
    def __init__(self, _handle=None):
        lib = _get_lib()
        self._lib  = lib
        self._ptr  = _handle if _handle is not None else lib.kore_block_new()
        if not self._ptr:
            raise RuntimeError("Failed to create KoreBlock")

    def __del__(self):
        if self._ptr:
            self._lib.kore_block_free(self._ptr)

    @property
    def num_rows(self) -> int:
        return self._lib.kore_block_num_rows(self._ptr)

    @property
    def num_cols(self) -> int:
        return self._lib.kore_block_num_cols(self._ptr)

    def add_f64(self, name: str, data: Union[List[float], np.ndarray]):
        arr = np.asarray(data, dtype=np.float64)
        ptr = arr.ctypes.data_as(ctypes.POINTER(ctypes.c_double))
        rc  = self._lib.kore_block_add_f64(self._ptr, name.encode(), ptr, len(arr))
        if rc != 0: _check_error(self._lib)

    def add_i64(self, name: str, data: Union[List[int], np.ndarray]):
        arr = np.asarray(data, dtype=np.int64)
        ptr = arr.ctypes.data_as(ctypes.POINTER(ctypes.c_int64))
        rc  = self._lib.kore_block_add_i64(self._ptr, name.encode(), ptr, len(arr))
        if rc != 0: _check_error(self._lib)

    def get_f64(self, col: str) -> np.ndarray:
        n   = self.num_rows
        out = np.empty(n, dtype=np.float64)
        ptr = out.ctypes.data_as(ctypes.POINTER(ctypes.c_double))
        rc  = self._lib.kore_block_get_f64(self._ptr, col.encode(), ptr, n)
        if rc < 0: _check_error(self._lib)
        return out[:rc]

    def join(self, right: "KoreBlock", left_key: str, right_key: str, how: str = "inner") -> "KoreBlock":
        jt = {"inner": 0, "left": 1, "full": 2}.get(how.lower(), 0)
        ptr = self._lib.kore_hash_join(
            self._ptr, right._ptr,
            left_key.encode(), right_key.encode(), jt
        )
        if not ptr: _check_error(self._lib)
        return KoreBlock(_handle=ptr)

    @staticmethod
    def from_dict(d: dict) -> "KoreBlock":
        block = KoreBlock()
        for name, values in d.items():
            arr = np.asarray(values)
            if np.issubdtype(arr.dtype, np.integer):
                block.add_i64(name, arr.astype(np.int64))
            else:
                block.add_f64(name, arr.astype(np.float64))
        return block

    def __repr__(self):
        return f"KoreBlock(rows={self.num_rows}, cols={self.num_cols})"

# ── ML Models ─────────────────────────────────────────────────────────────────

class ModelType(IntEnum):
    RF_REGRESSOR    = 0
    RF_CLASSIFIER   = 1
    GBM_REGRESSOR   = 2
    LINEAR_REGRESSOR = 3
    LOGISTIC        = 4
    KNN_REGRESSOR   = 5
    KNN_CLASSIFIER  = 6
    SVM             = 7

class KoreModel:
    def __init__(self, model_type: ModelType, param1: int = 100, param2: int = 3):
        lib  = _get_lib()
        self._lib = lib
        self._ptr = lib.kore_model_new(int(model_type), param1, param2)
        if not self._ptr: _check_error(lib)

    def __del__(self):
        if self._ptr: self._lib.kore_model_free(self._ptr)

    def fit(self, X: np.ndarray, y: np.ndarray):
        X = np.ascontiguousarray(X, dtype=np.float64)
        y = np.ascontiguousarray(y, dtype=np.float64)
        n, d = X.shape
        xptr = X.ctypes.data_as(ctypes.POINTER(ctypes.c_double))
        yptr = y.ctypes.data_as(ctypes.POINTER(ctypes.c_double))
        rc = self._lib.kore_model_fit(self._ptr, xptr, n, d, yptr)
        if rc != 0: _check_error(self._lib)
        return self

    def predict(self, X: np.ndarray) -> np.ndarray:
        X   = np.ascontiguousarray(X, dtype=np.float64)
        n, d = X.shape
        out  = np.empty(n, dtype=np.float64)
        xptr = X.ctypes.data_as(ctypes.POINTER(ctypes.c_double))
        optr = out.ctypes.data_as(ctypes.POINTER(ctypes.c_double))
        rc   = self._lib.kore_model_predict(self._ptr, xptr, n, d, optr)
        if rc != 0: _check_error(self._lib)
        return out

    def __repr__(self):
        return f"KoreModel(type={self._model_type})"


# ── Quick demo ─────────────────────────────────────────────────────────────────

if __name__ == "__main__":
    import numpy as np

    print("KORE Python bindings demo")

    X = np.random.randn(200, 3).astype(np.float64)
    y = X[:, 0] * 2 + X[:, 1] - X[:, 2] + np.random.randn(200) * 0.1

    model = KoreModel(ModelType.LINEAR_REGRESSOR)
    model.fit(X, y)
    preds = model.predict(X)
    ss_res = np.sum((y - preds)**2)
    ss_tot = np.sum((y - y.mean())**2)
    print(f"LinearRegressor R² = {1 - ss_res/ss_tot:.4f}")

    model2 = KoreModel(ModelType.GBM_REGRESSOR, param1=50, param2=3)
    model2.fit(X, y)
    preds2 = model2.predict(X)
    ss_res2 = np.sum((y - preds2)**2)
    print(f"GBM R² = {1 - ss_res2/ss_tot:.4f}")
