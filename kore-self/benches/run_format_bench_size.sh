#!/bin/sh
# Storage-efficiency driver. Usage: run_format_bench_size.sh OUTDIR "10000 100000 1000000"
set -e
OUT=${1:?outdir}; SIZES=${2:-"10000 100000"}
HERE=$(cd "$(dirname "$0")" && pwd)
EXE=${KORE_BENCH_EXE:-C:/Users/skathera/kw/target-selfbench/release/examples/bench_log_formats.exe}
mkdir -p "$OUT"; cd "$OUT"
for n in $SIZES; do
  [ -f rdata_$n.jsonl ] || python -I "$HERE/format_bench_gen_realistic.py" rdata_$n.jsonl $n
  python -I "$HERE/format_bench_size_py.py" --data rdata_$n.jsonl --n $n --work "$OUT/w_sz_py_$n" --out sz_py_$n.json
  "$EXE" --data rdata_$n.jsonl --n $n --work "$OUT/w_sz_kore_$n" --out sz_kore_$n.json --size-only --build-reps 1 --reps 5 --sql-lookups 5
done
