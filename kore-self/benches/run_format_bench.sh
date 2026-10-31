#!/bin/sh
# Log-format benchmark driver (Git Bash). Usage: run_format_bench.sh OUTDIR "10000 100000 1000000"
# Needs: release build of the example (see docs/format_bench.md), python 3.14 on PATH.
set -e
OUT=${1:?outdir}
SIZES=${2:-"10000 100000"}
HERE=$(cd "$(dirname "$0")" && pwd)
EXE=${KORE_BENCH_EXE:-C:/Users/skathera/kw/target-selfbench/release/examples/bench_log_formats.exe}
mkdir -p "$OUT"
cd "$OUT"
load() { typeperf "\\Processor(_Total)\\% Processor Time" -sc 5 2>/dev/null | sed -n 2,6p | tr -d '"' | awk -F, '{s+=$2;n++} END{printf "%.0f", s/n}'; }
for n in $SIZES; do
  [ -f data_$n.jsonl ] || python -I "$HERE/format_bench_gen.py" data_$n.jsonl $n
  REPS=5; BREPS=5
  if [ "$n" -ge 1000000 ]; then BREPS=2; fi
  echo "N=$n cpu_load_before=$(load)%" | tee -a load.txt
  timeout 3000 python -I "$HERE/format_bench_py.py" --data data_$n.jsonl --n $n --work "$OUT/w_py_$n" --out py_$n.json --build-reps $BREPS --reps $REPS
  echo "N=$n cpu_load_between=$(load)%" | tee -a load.txt
  timeout 3000 "$EXE" --data data_$n.jsonl --n $n --work "$OUT/w_kore_$n" --out kore_$n.json --build-reps $BREPS --reps $REPS
  echo "N=$n cpu_load_after=$(load)%" | tee -a load.txt
done
python -I "$HERE/format_bench_tables.py" "$OUT" > tables.md
