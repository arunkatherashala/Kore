import ctypes, json, os, sys
from pathlib import Path

dll = Path(r"C:\Users\skathera\Downloads\asistent\kore\target\release\kore_ffi.dll")
lib = ctypes.CDLL(str(dll))

lib.kore_session_new.restype    = ctypes.c_void_p
lib.kore_session_free.restype   = None; lib.kore_session_free.argtypes = [ctypes.c_void_p]
lib.kore_session_load_csv.restype = ctypes.c_int
lib.kore_session_load_csv.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_char_p]
lib.kore_session_query.restype  = ctypes.c_char_p
lib.kore_session_query.argtypes = [ctypes.c_void_p, ctypes.c_char_p]
lib.kore_session_row_count.restype  = ctypes.c_int64
lib.kore_session_row_count.argtypes = [ctypes.c_void_p, ctypes.c_char_p]

sess = lib.kore_session_new()
assert sess, "session created"

csv_path = r"C:\Users\skathera\Downloads\asistent\bench_export.csv"
rc = lib.kore_session_load_csv(sess, b"bench", csv_path.encode())
assert rc == 0, f"load failed: {rc}"
n = lib.kore_session_row_count(sess, b"bench")
print(f"Loaded {n:,} rows from real CSV")

raw = lib.kore_session_query(sess, b"SELECT category, COUNT(*) as cnt, SUM(amount) as total FROM bench GROUP BY category ORDER BY total DESC")
rows = json.loads(raw)
print(f"GROUP BY ({len(rows)} groups): {rows}")

raw2 = lib.kore_session_query(sess, b"SELECT id, amount FROM bench WHERE amount > 999 ORDER BY amount DESC LIMIT 3")
print(f"Top 3: {json.loads(raw2)}")

lib.kore_session_free(sess)
print("KORE universal engine DLL works from Python ctypes")
print("Same DLL: Java / Go / C# / Ruby / PHP / Node.js")
