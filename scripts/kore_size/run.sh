#!/bin/sh
# One harness run of the kore_size benchmark (docs/kore_size.md).
#
#   sh scripts/kore_size/run.sh OUTDIR "10000 100000 1000000" ["r u"]
#
# Datasets: r = realistic (format_bench_gen_realistic.py, seed 20261101), u = uniform (format_bench_gen.py,
# seed 20261031); both generators come from the Digital Life checkout (kore-self/benches).
# Per dataset and size, on the same data file, back to back:
#   1. dl_log_side --format 1   Digital Life log, format 1 (segments = default kore-store writer)  [kept]
#   2. dl_log_side --format 3   Digital Life log, format 3 (own codec + journal MANIFEST)
#   3. bench_kore_size          the format 1 log re-encoded: default writer vs opt-in version 4; dumps rows
#   4. py_side.py               CSV (+gzip/bz2/xz/zstd), SQLite, Parquet, ORC, Arrow IPC, DuckDB on the dumped rows
# Environment (defaults are the paths used for docs/kore_size.md):
#   DL_EXE    dl_log_side.exe built in a Digital Life checkout whose kore-store/ is this branch's
#   STORE_EXE bench_kore_size.exe built from this branch
#   GEN_DIR   kore-self/benches of the Digital Life checkout (generators)
#   PY_STD    stdlib Python for the generators; PY_ENG Python with pyarrow + duckdb (optional)
set -e
OUT=${1:?outdir}; SIZES=${2:-"10000 100000"}; SETS=${3:-"r u"}
DL_EXE=${DL_EXE:-C:/Users/skathera/kw/target-ff2-dl/release/examples/dl_log_side.exe}
STORE_EXE=${STORE_EXE:-C:/Users/skathera/kw/target-ff2/release/examples/bench_kore_size.exe}
GEN_DIR=${GEN_DIR:-C:/Users/skathera/kw/ff2-dl/kore-self/benches}
PY_STD=${PY_STD:-C:/Users/skathera/Downloads/asistent/.venv/Scripts/python.exe}
PY_ENG=${PY_ENG:-C:/Users/skathera/AppData/Local/miniconda3/envs/engines/python.exe}
HERE=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$OUT/data"
load() { typeperf "\\Processor(_Total)\\% Processor Time" -sc 3 2>/dev/null | grep '^"[0-9]' | sed "s/^/$1 /" >> "$OUT/load.txt" || true; }
for set in $SETS; do
  for n in $SIZES; do
    if [ "$n" -ge 1000000 ]; then
      free=$(df -k "$OUT" | awk 'NR==2 {print int($4/1048576)}')
      if [ "$free" -lt 12 ]; then echo "SKIP $set $n: only ${free} GB free" | tee -a "$OUT/skipped.txt"; continue; fi
    fi
    reps=3; lk=20; slk=20; [ "$n" -ge 1000000 ] && { reps=2; slk=5; }
    if [ "$set" = r ]; then d="$OUT/data/rdata_$n.jsonl"; [ -f "$d" ] || "$PY_STD" -I "$GEN_DIR/format_bench_gen_realistic.py" "$d" "$n"
    else d="$OUT/data/data_$n.jsonl"; [ -f "$d" ] || "$PY_STD" -I "$GEN_DIR/format_bench_gen.py" "$d" "$n"; fi
    sha256sum "$d" >> "$OUT/data_sha256.txt"
    w="$OUT/w_${set}_$n"; rm -rf "$w"; mkdir -p "$w"
    load "start_${set}_$n"
    echo "== $set $n: dl format 1"; "$DL_EXE" --data "$d" --n "$n" --work "$w/f1" --format 1 --out "$OUT/dl_f1_${set}_$n.json" --reps $reps --lookups 100
    echo "== $set $n: dl format 3"; "$DL_EXE" --data "$d" --n "$n" --work "$w/f3" --format 3 --out "$OUT/dl_f3_${set}_$n.json" --reps $reps --lookups 100
    rm -rf "$w/f3"
    echo "== $set $n: store default vs v4"; "$STORE_EXE" --log "$w/f1/log/bench/log" --work "$w/store" --out "$OUT/store_${set}_$n.json" --reps $reps --lookups $lk --dump "$w/dump" --max-level 19
    rm -rf "$w/store"
    load "mid_${set}_$n"
    echo "== $set $n: other formats"; "$PY_ENG" -I "$HERE/py_side.py" --dump "$w/dump" --work "$w/py" --out "$OUT/py_${set}_$n.json" --reps $reps --lookups $lk --scan-lookups $slk
    load "end_${set}_$n"
    rm -rf "$w"
  done
done
echo done
