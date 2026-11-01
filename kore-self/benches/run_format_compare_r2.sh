#!/bin/sh
# Interleaved BEFORE/AFTER driver for docs/format_improvement_r2.md.
#   run_format_compare_r2.sh OUTDIR "10000 100000" ROUNDS PREFIX [BUILD_REPS] [REPS]
# Variants per round, back to back on the same machine state:
#   r1  = round-1 binary (commit 6fc666ec), log format 2   (BEFORE)
#   f2  = round-2 binary, log format 2                      (AFTER, same on-disk shape as r1)
#   f3  = round-2 binary, log format 3 (journal MANIFEST)   (AFTER, new flag)
# KORE_R1_EXE / KORE_NEW_EXE / KORE_DATA_DIR override the defaults below.
set -e
OUT=${1:?outdir}; SIZES=${2:-"10000"}; ROUNDS=${3:-3}; PREFIX=${4:-rdata}; BR=${5:-2}; REPS=${6:-5}
HERE=$(cd "$(dirname "$0")" && pwd)
R1=${KORE_R1_EXE:-C:/Users/skathera/kw/fmtwork/r1bin/bench_log_formats.exe}
NEW=${KORE_NEW_EXE:-C:/Users/skathera/kw/target-selfkorefmt2/release/examples/bench_log_formats.exe}
DATA=${KORE_DATA_DIR:-C:/Users/skathera/AppData/Local/Temp/kfb}
WORK=${KORE_WORK_DIR:-C:/Users/skathera/AppData/Local/Temp/r2w}
mkdir -p "$OUT" "$WORK"
load() { typeperf "\\Processor(_Total)\\% Processor Time" -sc 3 2>/dev/null | grep -E '^"[0-9]' | tr '\n' ' ' >> "$OUT/load.txt"; echo >> "$OUT/load.txt"; }
for n in $SIZES; do
  F="$DATA/${PREFIX}_$n.jsonl"
  [ -f "$F" ] || python -I "$HERE/format_bench_gen_realistic.py" "$F" $n
  TT=40; [ "$n" -gt 100000 ] && TT=0
  r=1
  while [ $r -le $ROUNDS ]; do
    for v in r1 f2 f3; do
      case $v in r1) EXE=$R1; FMT=2;; f2) EXE=$NEW; FMT=2;; f3) EXE=$NEW; FMT=3;; esac
      echo "load before $PREFIX $n r$r $v: $(date -u +%H:%M:%S)" >> "$OUT/load.txt"; load
      "$EXE" --data "$F" --n $n --work "$WORK/w_${PREFIX}_${n}_$v" --out "$OUT/${PREFIX}_${n}_${v}_r$r.json" --build-reps $BR --reps $REPS --format $FMT --sql-lookups 5 --tamper-trials $TT > "$OUT/log_${PREFIX}_${n}_${v}_r$r.txt" 2>&1
      load
    done
    r=$((r+1))
  done
done
echo DONE > "$OUT/DONE"
