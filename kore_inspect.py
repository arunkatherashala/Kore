"""
Inspect a .kore file and print header/schema/compression summary.

Usage:
  python kore_inspect.py tpch_1m.kore
"""

from __future__ import annotations

import argparse
import csv
import ctypes
import json
import os
import struct
import sys
from dataclasses import dataclass

MAGIC = b"KORE"
READABLE_TRAILER_BEGIN = b"\nKORE-READABLE-BEGIN\n"
READABLE_TRAILER_END = b"KORE-READABLE-END\n"
READABLE_FOOTER_PREFIX = b"KORE-READABLE-FOOTER trailer_len="

DTYPE_NAMES = {
    1: "i64",
    2: "f64",
    3: "bool",
    4: "str",
    5: "str_dict",
}

COMP_NAMES = {
    0: "raw",
    1: "rle",
    2: "delta",
    3: "dict",
    4: "nanraw",
    5: "lz4",
}


def find_kore_lib() -> str:
    if os.name == "nt":
        lib_name = "kore_ffi.dll"
    elif sys.platform == "darwin":
        lib_name = "libkore_ffi.dylib"
    else:
        lib_name = "libkore_ffi.so"

    env = os.getenv("KORE_LIB")
    if env and os.path.exists(env):
        return os.path.abspath(env)

    candidates = [
        os.path.join(os.getcwd(), "target", "release", lib_name),
        os.path.join(os.path.dirname(os.path.abspath(__file__)), "target", "release", lib_name),
    ]
    for c in candidates:
        if os.path.exists(c):
            return c
    raise FileNotFoundError(f"KORE FFI library not found: {lib_name}")


def setup_ffi() -> ctypes.CDLL:
    lib = ctypes.CDLL(find_kore_lib())
    lib.kore_last_error.restype = ctypes.c_char_p
    lib.kore_last_error.argtypes = []

    lib.kore_session_new.restype = ctypes.c_void_p
    lib.kore_session_new.argtypes = []

    lib.kore_session_free.restype = None
    lib.kore_session_free.argtypes = [ctypes.c_void_p]

    lib.kore_session_load_kore.restype = ctypes.c_int
    lib.kore_session_load_kore.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_char_p]

    lib.kore_session_query.restype = ctypes.c_void_p
    lib.kore_session_query.argtypes = [ctypes.c_void_p, ctypes.c_char_p]

    lib.kore_free_string.restype = None
    lib.kore_free_string.argtypes = [ctypes.c_void_p]
    return lib


def ffi_last_error(lib: ctypes.CDLL) -> str:
    err = lib.kore_last_error()
    if not err:
        return "unknown KORE error"
    return err.decode("utf-8", errors="replace")


def query_kore_rows(path: str, limit: int) -> list[dict]:
    lib = setup_ffi()
    sess = lib.kore_session_new()
    if not sess:
        raise RuntimeError(ffi_last_error(lib))

    table = b"inspect_tbl"
    rc = lib.kore_session_load_kore(sess, table, os.path.abspath(path).encode("utf-8"))
    if rc != 0:
        lib.kore_session_free(sess)
        raise RuntimeError(ffi_last_error(lib))

    sql = f"SELECT * FROM inspect_tbl LIMIT {max(1, limit)}".encode("utf-8")
    raw_ptr = lib.kore_session_query(sess, sql)
    if not raw_ptr:
        lib.kore_session_free(sess)
        raise RuntimeError(ffi_last_error(lib))

    try:
        json_bytes = ctypes.cast(raw_ptr, ctypes.c_char_p).value
    finally:
        lib.kore_free_string(raw_ptr)
        lib.kore_session_free(sess)

    if not json_bytes:
        return []
    return json.loads(json_bytes.decode("utf-8"))


def print_preview(rows: list[dict]) -> None:
    if not rows:
        print("\nData Preview: no rows returned")
        return

    cols = list(rows[0].keys())
    widths = {c: len(c) for c in cols}
    for row in rows:
        for c in cols:
            widths[c] = max(widths[c], len(str(row.get(c, ""))))

    print("\nData Preview:")
    header = " | ".join(c.ljust(widths[c]) for c in cols)
    sep = "-+-".join("-" * widths[c] for c in cols)
    print(header)
    print(sep)
    for row in rows:
        print(" | ".join(str(row.get(c, "")).ljust(widths[c]) for c in cols))


def write_csv(path: str, rows: list[dict]) -> None:
    if not rows:
        with open(path, "w", newline="", encoding="utf-8") as f:
            f.write("")
        return
    fieldnames = list(rows[0].keys())
    with open(path, "w", newline="", encoding="utf-8") as f:
        w = csv.DictWriter(f, fieldnames=fieldnames)
        w.writeheader()
        w.writerows(rows)


@dataclass
class ColumnMeta:
    name: str
    dtype: int
    comp: int
    data_len: int


def parse_kore(path: str) -> tuple[int, int, list[ColumnMeta], int]:
    with open(path, "rb") as f:
        data = f.read()

    footer = parse_footer(data)
    binary_data = data[:footer["binary_end"]] if footer else data

    total_size = len(data)
    if len(binary_data) < 18:
        raise ValueError("file too small to be a valid .kore")

    pos = 0
    magic = binary_data[pos:pos + 4]
    pos += 4
    if magic != MAGIC:
        raise ValueError(f"invalid magic: expected {MAGIC!r}, got {magic!r}")

    (version,) = struct.unpack_from("<H", binary_data, pos)
    pos += 2
    (num_cols,) = struct.unpack_from("<I", binary_data, pos)
    pos += 4
    (num_rows,) = struct.unpack_from("<Q", binary_data, pos)
    pos += 8

    schema: list[tuple[str, int]] = []
    for _ in range(num_cols):
        (name_len,) = struct.unpack_from("<H", binary_data, pos)
        pos += 2
        name_b = binary_data[pos:pos + name_len]
        pos += name_len
        dtype = binary_data[pos]
        pos += 1
        schema.append((name_b.decode("utf-8", errors="replace"), dtype))

    cols: list[ColumnMeta] = []
    for i in range(num_cols):
        if pos + 9 > len(binary_data):
            raise ValueError(f"truncated column header at index {i}")
        comp = binary_data[pos]
        pos += 1
        (data_len,) = struct.unpack_from("<Q", binary_data, pos)
        pos += 8
        if pos + data_len > len(binary_data):
            raise ValueError(f"truncated column data at index {i}")
        pos += data_len

        name, dtype = schema[i]
        cols.append(ColumnMeta(name=name, dtype=dtype, comp=comp, data_len=data_len))

    return version, num_rows, cols, total_size


def parse_footer(data: bytes) -> dict | None:
    start = data.rfind(READABLE_FOOTER_PREFIX)
    if start < 0:
        return None
    digits_start = start + len(READABLE_FOOTER_PREFIX)
    digits_end = digits_start + 20
    if digits_end > len(data):
        return None
    try:
        trailer_len = int(data[digits_start:digits_end].decode("ascii"))
    except ValueError:
        return None

    mode = "unknown"
    mode_marker = b" mode="
    mode_start = data.find(mode_marker, digits_end)
    if mode_start >= 0:
        mode_start += len(mode_marker)
        mode_end = data.find(b"\n", mode_start)
        if mode_end >= 0:
            mode = data[mode_start:mode_end].decode("utf-8", errors="replace")

    binary_end = start - trailer_len
    if binary_end < 0:
        return None
    return {
        "trailer_len": trailer_len,
        "footer_start": start,
        "binary_end": binary_end,
        "mode": mode,
    }


def read_trailer_text(path: str) -> str:
    with open(path, "rb") as f:
        data = f.read()
    footer = parse_footer(data)
    if not footer:
        return ""
    trailer = data[footer["binary_end"]:footer["footer_start"]]
    return trailer.decode("utf-8", errors="replace")


def main() -> int:
    ap = argparse.ArgumentParser(description="Inspect a KORE native binary file")
    ap.add_argument("path", help="Path to .kore file")
    ap.add_argument(
        "--json",
        action="store_true",
        help="Print machine-readable JSON instead of table output",
    )
    ap.add_argument(
        "--preview",
        type=int,
        default=0,
        help="Show first N rows as human-readable table (via KORE engine)",
    )
    ap.add_argument(
        "--export-json",
        default="",
        help="Export first N rows to JSON file (requires --preview N)",
    )
    ap.add_argument(
        "--export-csv",
        default="",
        help="Export first N rows to CSV file (requires --preview N)",
    )
    args = ap.parse_args()

    path = args.path
    if not os.path.exists(path):
        print(f"error: file not found: {path}")
        return 1

    try:
        version, rows, cols, total_size = parse_kore(path)
    except Exception as e:
        print(f"error: {e}")
        return 1

    total_payload = sum(c.data_len for c in cols)
    payload_ratio = (total_payload / total_size) * 100.0 if total_size > 0 else 0.0

    if args.json:
        out = {
            "path": os.path.abspath(path),
            "size_bytes": total_size,
            "size_kb": round(total_size / 1024.0, 1),
            "version": version,
            "rows": rows,
            "columns": len(cols),
            "schema": [
                {
                    "index": idx,
                    "name": c.name,
                    "dtype_code": c.dtype,
                    "dtype": DTYPE_NAMES.get(c.dtype, f"unknown({c.dtype})"),
                    "compression_code": c.comp,
                    "compression": COMP_NAMES.get(c.comp, f"unknown({c.comp})"),
                    "payload_bytes": c.data_len,
                    "payload_kb": round(c.data_len / 1024.0, 1),
                }
                for idx, c in enumerate(cols, start=1)
            ],
            "payload_summary": {
                "payload_bytes": total_payload,
                "payload_kb": round(total_payload / 1024.0, 1),
                "payload_to_total_percent": round(payload_ratio, 1),
            },
        }
        print(json.dumps(out, indent=2))
        return 0

    print("KORE File Inspect")
    print(f"  Path: {os.path.abspath(path)}")
    print(f"  Size: {total_size / 1024:.1f} KB")
    print(f"  Version: {version}")
    print(f"  Rows: {rows}")
    print(f"  Columns: {len(cols)}")
    print()

    print("Schema:")
    for idx, c in enumerate(cols, start=1):
        dtype = DTYPE_NAMES.get(c.dtype, f"unknown({c.dtype})")
        comp = COMP_NAMES.get(c.comp, f"unknown({c.comp})")
        print(
            f"  {idx:>2}. {c.name:<18} dtype={dtype:<8} comp={comp:<7} "
            f"payload={c.data_len / 1024:.1f} KB"
        )

    print()
    print(f"Payload Summary: {total_payload / 1024:.1f} KB")
    print(f"Payload/Total: {payload_ratio:.1f}%")

    trailer = read_trailer_text(path)
    if trailer:
        print("\nEmbedded Readable Trailer:")
        print(trailer.rstrip())

    if args.preview > 0:
        try:
            rows_data = query_kore_rows(path, args.preview)
            print_preview(rows_data)
            if args.export_json:
                with open(args.export_json, "w", encoding="utf-8") as f:
                    json.dump(rows_data, f, indent=2)
                print(f"\nSaved preview JSON: {os.path.abspath(args.export_json)}")
            if args.export_csv:
                write_csv(args.export_csv, rows_data)
                print(f"Saved preview CSV: {os.path.abspath(args.export_csv)}")
        except Exception as e:
            print(f"\nwarning: preview/export failed: {e}")
            return 2
    elif args.export_json or args.export_csv:
        print("warning: --export-json/--export-csv require --preview N")
        return 2

    return 0


if __name__ == "__main__":
    sys.exit(main())
