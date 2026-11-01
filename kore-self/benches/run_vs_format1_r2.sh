#!/bin/sh
# Paired comparison of the round-2 binary's formats 1, 2 and 3 (same binary, back to back), because
# the round-2 main run pairs only against the round-1 binary. Quick settings, no tamper step.
#   run_vs_format1_r2.sh OUTDIR "10000 100000" ROUNDS
set -e
OUT=${1:?outdir}; SIZES=${2:-"10000 100000"}; ROUNDS=${3:-3}
NEW=${KORE_NEW_EXE:-C:/Users/skathera/kw/target-selfkorefmt2/release/examples/bench_log_formats.exe}
DATA=${KORE_DATA_DIR:-C:/Users/skathera/AppData/Local/Temp/kfb}
WORK=${KORE_WORK_DIR:-C:/Users/skathera/AppData/Local/Temp/r2w}
mkdir -p "$OUT" "$WORK"
for n in $SIZES; do
  r=1
  while [ $r -le $ROUNDS ]; do
    for f in 1 2 3; do
      typeperf "\Processor(_Total)\% Processor Time" -sc 2 2>/dev/null | grep -E '^"[0-9]' | tr '\n' ' ' >> "$OUT/load.txt"; echo >> "$OUT/load.txt"
      "$NEW" --data "$DATA/rdata_$n.jsonl" --n $n --work "$WORK/wf_${n}_$f" --out "$OUT/rdata_${n}_n${f}_r$r.json" --build-reps 2 --reps 3 --format $f --sql-lookups 3 --tamper-trials 0 > "$OUT/log_${n}_n${f}_r$r.txt" 2>&1
    done
    r=$((r+1))
  done
done
echo DONE > "$OUT/DONE"
