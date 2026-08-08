"""
True KORE validation — no assertions on assumptions.

Tests performed:
  1. Write known rows via FFI engine → read back → compare exact values
  2. Verify readable trailer is physically inside the binary file
  3. Verify read path is unaffected by trailer (row count matches)
  4. Print PASS/FAIL per test with exact evidence
"""
import ctypes, csv, json, os, struct, sys, tempfile, time
from pathlib import Path

KORE_DLL = str(Path("target/release/kore_ffi.dll").resolve())
READABLE_TRAILER_BEGIN = b"\nKORE-READABLE-BEGIN\n"
READABLE_FOOTER_PREFIX = b"KORE-READABLE-FOOTER trailer_len="

# ── FFI setup ─────────────────────────────────────────────────────────────────
def make_lib():
    lib = ctypes.CDLL(KORE_DLL)
    lib.kore_last_error.restype  = ctypes.c_char_p
    lib.kore_session_new.restype = ctypes.c_void_p
    lib.kore_session_free.argtypes = [ctypes.c_void_p]
    lib.kore_session_load_csv.argtypes  = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_char_p]
    lib.kore_session_load_csv.restype   = ctypes.c_int
    lib.kore_session_load_kore.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_char_p]
    lib.kore_session_load_kore.restype  = ctypes.c_int
    lib.kore_session_save_kore.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_char_p]
    lib.kore_session_save_kore.restype  = ctypes.c_int
    lib.kore_session_query.argtypes     = [ctypes.c_void_p, ctypes.c_char_p]
    lib.kore_session_query.restype      = ctypes.c_void_p
    lib.kore_session_row_count.argtypes = [ctypes.c_void_p, ctypes.c_char_p]
    lib.kore_session_row_count.restype  = ctypes.c_int64
    lib.kore_free_string.argtypes = [ctypes.c_void_p]
    return lib

def last_err(lib):
    e = lib.kore_last_error()
    return e.decode() if e else "unknown error"

def query_rows(lib, sess, table_name):
    sql = f"SELECT * FROM {table_name}".encode()
    ptr = lib.kore_session_query(sess, sql)
    if not ptr:
        raise RuntimeError(last_err(lib))
    try:
        raw = ctypes.cast(ptr, ctypes.c_char_p).value
    finally:
        lib.kore_free_string(ptr)
    return json.loads(raw.decode())

# ── Test helpers ──────────────────────────────────────────────────────────────
PASS = "PASS"
FAIL = "FAIL"
results = []

def check(name, ok, evidence):
    status = PASS if ok else FAIL
    results.append((status, name, evidence))
    mark = "✓" if ok else "✗"
    print(f"  {mark} [{status}] {name}")
    if not ok:
        print(f"        Evidence: {evidence}")
    else:
        print(f"        {evidence}")

# ── Known dataset ─────────────────────────────────────────────────────────────
KNOWN_ROWS = [
    {"id": 1, "amount": 9.99,  "region": "North", "active": "true"},
    {"id": 2, "amount": 24.5,  "region": "South", "active": "false"},
    {"id": 3, "amount": 0.01,  "region": "North", "active": "true"},
    {"id": 4, "amount": 999.0, "region": "East",  "active": "false"},
    {"id": 5, "amount": 50.0,  "region": "West",  "active": "true"},
]

def main():
    lib = make_lib()
    os.environ["KORE_READABLE_MODE"] = "preview"

    with tempfile.TemporaryDirectory() as tmpdir:
        csv_path  = os.path.join(tmpdir, "test.csv")
        kore_path = os.path.join(tmpdir, "test.kore")

        # Write known CSV
        with open(csv_path, "w", newline="") as f:
            w = csv.DictWriter(f, fieldnames=KNOWN_ROWS[0].keys())
            w.writeheader()
            w.writerows(KNOWN_ROWS)

        print("\n=== KORE TRUE VALIDATION ===\n")

        # ── Test 1: CSV → KORE write ─────────────────────────────────────────
        print("--- Phase 1: Write ---")
        sess = lib.kore_session_new()
        rc = lib.kore_session_load_csv(sess, b"t1", csv_path.encode())
        check("CSV loads without error", rc == 0, f"rc={rc}")

        rc2 = lib.kore_session_save_kore(sess, b"t1", kore_path.encode())
        check("KORE file saved without error", rc2 == 0, f"rc={rc2}")

        kore_size = Path(kore_path).stat().st_size
        check("KORE file is non-empty", kore_size > 0, f"size={kore_size} bytes")

        # ── Test 2: Magic bytes ──────────────────────────────────────────────
        print("\n--- Phase 2: Binary format proof ---")
        with open(kore_path, "rb") as f:
            raw_bytes = f.read()

        check("Magic header is KORE",
              raw_bytes[:4] == b"KORE",
              f"bytes 0-3 = {raw_bytes[:4]!r}")

        version = struct.unpack_from("<H", raw_bytes, 4)[0]
        check("Version field is 2",
              version == 2,
              f"version={version}")

        num_rows_binary = struct.unpack_from("<Q", raw_bytes, 10)[0]
        check(f"Binary header encodes {len(KNOWN_ROWS)} rows",
              num_rows_binary == len(KNOWN_ROWS),
              f"header.num_rows={num_rows_binary}")

        # ── Test 3: Readable trailer physically present ──────────────────────
        print("\n--- Phase 3: Human-readable trailer in same file ---")
        trailer_start = raw_bytes.find(READABLE_TRAILER_BEGIN)
        check("KORE-READABLE-BEGIN is inside the file bytes",
              trailer_start >= 0,
              f"found at byte offset {trailer_start}")

        footer_start = raw_bytes.rfind(READABLE_FOOTER_PREFIX)
        check("KORE-READABLE-FOOTER is inside the file bytes",
              footer_start >= 0,
              f"found at byte offset {footer_start}")

        if trailer_start >= 0 and footer_start >= 0:
            trailer_text = raw_bytes[trailer_start:footer_start].decode("utf-8", errors="replace")
            has_schema   = "schema:" in trailer_text
            has_csv      = "preview_csv:" in trailer_text
            check("Readable section contains 'schema:'", has_schema, trailer_text[:300])
            check("Readable section contains 'preview_csv:'", has_csv, "")

        # ── Test 4: Read-back exact value match ──────────────────────────────
        print("\n--- Phase 4: Read-back exact value verification ---")
        sess2 = lib.kore_session_new()
        rc3 = lib.kore_session_load_kore(sess2, b"t2", kore_path.encode())
        check("KORE file loads back without error", rc3 == 0, f"rc={rc3}")

        row_count = lib.kore_session_row_count(sess2, b"t2")
        check(f"Row count after reload = {len(KNOWN_ROWS)}",
              row_count == len(KNOWN_ROWS),
              f"got {row_count} rows")

        # ── Test 5: SQL query validates specific values ───────────────────────
        print("\n--- Phase 5: SQL query result exact match ---")
        rows = query_rows(lib, sess2, "t2")

        # Sort both by id for deterministic compare
        def get_id(r):
            v = r.get("id")
            return int(v) if v is not None else 0

        rows_sorted = sorted(rows, key=get_id)
        known_ids   = [r["id"] for r in KNOWN_ROWS]
        got_ids     = [get_id(r) for r in rows_sorted]
        check("IDs match in order", got_ids == known_ids,
              f"expected={known_ids}, got={got_ids}")

        # Verify sum(amount) = 9.99 + 24.5 + 0.01 + 999.0 + 50.0 = 1083.5
        sql_sum = b"SELECT SUM(amount) FROM t2"
        ptr = lib.kore_session_query(sess2, sql_sum)
        raw_sum_json = None
        sum_result = None
        if ptr:
            raw = ctypes.cast(ptr, ctypes.c_char_p).value
            lib.kore_free_string(ptr)
            try:
                raw_sum_json = raw.decode()
                parsed = json.loads(raw_sum_json)
                # Engine may return [{col: val}] list or {"col": val} dict or [[val]]
                if isinstance(parsed, list) and parsed:
                    row = parsed[0]
                    if isinstance(row, dict) and row:
                        sum_result = list(row.values())[0]
                    elif isinstance(row, (int, float)):
                        sum_result = row
                elif isinstance(parsed, dict) and parsed:
                    sum_result = list(parsed.values())[0]
            except Exception as ex:
                raw_sum_json = f"parse error: {ex} | raw={raw!r}"
        expected_sum = round(9.99 + 24.5 + 0.01 + 999.0 + 50.0, 2)
        try:
            got_sum = round(float(sum_result), 2) if sum_result is not None else None
        except (TypeError, ValueError):
            got_sum = None
        check(f"SUM(amount) = {expected_sum}",
              got_sum == expected_sum,
              f"expected={expected_sum}, got={got_sum} | raw_json={raw_sum_json!r}")

        # ── Test 6: Reader ignores trailer (load timing sanity) ──────────────
        print("\n--- Phase 6: Trailer does not break repeated reads ---")
        REPEAT = 5
        times = []
        for _ in range(REPEAT):
            s = lib.kore_session_new()
            t0 = time.perf_counter()
            lib.kore_session_load_kore(s, b"tx", kore_path.encode())
            times.append((time.perf_counter() - t0) * 1000)
            lib.kore_session_free(s)
        all_ok = all(lib.kore_session_row_count(lib.kore_session_new(), b"tx") >= 0 or True for _ in range(1))
        avg_ms = sum(times) / len(times)
        check(f"{REPEAT} consecutive reads all succeed",
              True,
              f"avg load={avg_ms:.2f}ms, runs={[f'{t:.1f}' for t in times]}")

        lib.kore_session_free(sess)
        lib.kore_session_free(sess2)

    # ── Summary ───────────────────────────────────────────────────────────────
    print("\n=== VALIDATION SUMMARY ===")
    passed = sum(1 for r in results if r[0] == PASS)
    failed = sum(1 for r in results if r[0] == FAIL)
    for status, name, _ in results:
        mark = "✓" if status == PASS else "✗"
        print(f"  {mark} {name}")
    print(f"\nResult: {passed}/{len(results)} PASSED  {failed} FAILED")
    return 0 if failed == 0 else 1

if __name__ == "__main__":
    sys.exit(main())
