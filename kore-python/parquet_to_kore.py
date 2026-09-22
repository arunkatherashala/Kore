"""Convert an Apache Parquet file to a KORE (.kore) columnar file.

Self-contained and portable: reads Parquet with DuckDB and writes the kore-store
v2 binary directly, so it needs no compiled KORE toolchain — just `duckdb`.

Compression: each column block is wrapped in Zstd (comp=6) when a zstd codec is
available (Python 3.14+ stdlib `compression.zstd`, or the `zstandard` package).
Without one it writes the lightweight per-column codecs uncompressed (still a
valid, lossless .kore, just larger).

Date/Timestamp columns are stored as int64 epoch (DATE -> days, TIMESTAMP ->
microseconds), matching the Rust kore-parquet reader.

Usage:
    python parquet_to_kore.py <input.parquet> <output.kore> [--level 19]
"""

import argparse
import struct
import sys

import duckdb

# ── kore-store v2 format constants ───────────────────────────────────────────
MAGIC = b"KORE"
VERSION = 2
DT_I64, DT_F64, DT_BOOL, DT_STR = 1, 2, 3, 4
C_RAW, C_DELTA, C_NANRAW, C_ZSTD = 0, 2, 4, 6
MASK64 = 0xFFFFFFFFFFFFFFFF
NULL_F64 = float("nan")


def _get_zstd():
    """Return a `compress(data, level)->bytes` callable, or None if unavailable."""
    try:
        from compression import zstd as _z  # Python 3.14+ stdlib
        return lambda data, level: _z.compress(data, level=level)
    except ImportError:
        pass
    try:
        import zstandard as _z
        return lambda data, level: _z.ZstdCompressor(level=level).compress(data)
    except ImportError:
        return None


# ── column encoders (byte-compatible with kore-store decoders) ───────────────

def enc_i64_delta(vals):
    out = bytearray()
    prev = 0
    for v in vals:
        if v is None:
            out.append(1)
            out += struct.pack("<Q", (0 - prev) & MASK64)
        else:
            out.append(0)
            out += struct.pack("<Q", (v - prev) & MASK64)
            prev = v
    return bytes(out)


def enc_f64_nanraw(vals):
    out = bytearray()
    for v in vals:
        out += struct.pack("<d", NULL_F64 if v is None else float(v))
    return bytes(out)


def enc_bool_raw(vals):
    return bytes(2 if v is None else (1 if v else 0) for v in vals)


def enc_str_raw(vals):
    n = len(vals)
    null_flags = bytearray(n)
    blobs = []
    offset = 0
    offsets = bytearray()
    for i, v in enumerate(vals):
        if v is None:
            null_flags[i] = 1
            b = b""
        else:
            b = v.encode("utf-8") if isinstance(v, str) else str(v).encode("utf-8")
        offsets += struct.pack("<I", offset)
        offset += len(b)
        blobs.append(b)
    offsets += struct.pack("<I", offset)  # sentinel
    out = bytearray()
    out += struct.pack("<I", n)
    out += null_flags
    out += offsets
    for b in blobs:
        out += b
    return bytes(out)


def _plan(schema):
    """Map DuckDB columns to (select_expr, kore_dtype, inner_comp)."""
    select_parts, plan = [], []
    for name, typ, *_ in schema:
        t = typ.upper()
        q = f'"{name}"'
        if t == "DATE":
            select_parts.append(f"datediff('day', DATE '1970-01-01', {q})")
            plan.append((name, DT_I64, C_DELTA))
        elif t.startswith("TIMESTAMP"):
            select_parts.append(f"epoch_us({q})")
            plan.append((name, DT_I64, C_DELTA))
        elif t in ("INTEGER", "BIGINT", "SMALLINT", "TINYINT", "HUGEINT",
                   "UINTEGER", "UBIGINT", "USMALLINT", "UTINYINT"):
            select_parts.append(q)
            plan.append((name, DT_I64, C_DELTA))
        elif t in ("DOUBLE", "FLOAT", "REAL") or t.startswith("DECIMAL"):
            select_parts.append(f"CAST({q} AS DOUBLE)")
            plan.append((name, DT_F64, C_NANRAW))
        elif t == "BOOLEAN":
            select_parts.append(q)
            plan.append((name, DT_BOOL, C_RAW))
        else:  # VARCHAR and anything else -> string
            select_parts.append(f"CAST({q} AS VARCHAR)")
            plan.append((name, DT_STR, C_RAW))
    return select_parts, plan


def convert(parquet_path, out_path, level=19):
    con = duckdb.connect()
    schema = con.execute("DESCRIBE SELECT * FROM read_parquet(?)", [parquet_path]).fetchall()
    select_parts, plan = _plan(schema)
    ncols = len(plan)

    sql = "SELECT " + ", ".join(select_parts) + " FROM read_parquet(?)"
    cols = [[] for _ in range(ncols)]
    res = con.execute(sql, [parquet_path])
    num_rows = 0
    while True:
        rows = res.fetchmany(65536)
        if not rows:
            break
        num_rows += len(rows)
        for r in rows:
            for ci in range(ncols):
                cols[ci].append(r[ci])

    zstd = _get_zstd()
    encoders = {DT_I64: enc_i64_delta, DT_F64: enc_f64_nanraw,
                DT_BOOL: enc_bool_raw, DT_STR: enc_str_raw}

    blocks = []
    for ci, (name, dtype, comp) in enumerate(plan):
        data = encoders[dtype](cols[ci])
        if zstd is not None:
            data = bytes([comp]) + zstd(data, level)
            comp = C_ZSTD
        blocks.append((name, dtype, comp, data))

    with open(out_path, "wb") as f:
        f.write(MAGIC)
        f.write(struct.pack("<H", VERSION))
        f.write(struct.pack("<I", ncols))
        f.write(struct.pack("<Q", num_rows))
        for name, dtype, _comp, _data in blocks:
            nb = name.encode("utf-8")
            f.write(struct.pack("<H", len(nb)))
            f.write(nb)
            f.write(struct.pack("<B", dtype))
        for _name, _dtype, comp, data in blocks:
            f.write(struct.pack("<B", comp))
            f.write(struct.pack("<Q", len(data)))
            f.write(data)

    return num_rows, ncols, zstd is not None


def main():
    ap = argparse.ArgumentParser(description="Convert Parquet to a KORE (.kore) file.")
    ap.add_argument("parquet")
    ap.add_argument("output")
    ap.add_argument("--level", type=int, default=19, help="zstd level when available (default 19)")
    args = ap.parse_args()

    num_rows, ncols, compressed = convert(args.parquet, args.output, args.level)
    codec = f"zstd level {args.level}" if compressed else "uncompressed (no zstd codec found)"
    import os
    size = os.path.getsize(args.output)
    print(f"{num_rows} rows x {ncols} cols -> {args.output}")
    print(f"  codec: {codec}")
    print(f"  size:  {size:,} bytes ({size / 1024 / 1024:.2f} MB)")


if __name__ == "__main__":
    main()
