#!/bin/sh
# Interleaved driver for docs/log_format4.md: log formats 1, 3 and 4, same binary, back to back.
#   run_log_format4.sh OUTDIR "10000 100000" ROUNDS [PREFIX] [BUILD_REPS] [REPS]
# KORE_NEW_EXE / KORE_DATA_DIR / KORE_WORK_DIR override the defaults below.
set -e
OUT=${1:?outdir}; SIZES=${2:-"10000"}; ROUNDS=${3:-3}; PREFIX=${4:-rdata}; BR=${5:-2}; REPS=${6:-5}
HERE=$(cd "$(dirname "$0")" && pwd)
NEW=${KORE_NEW_EXE:-C:/Users/skathera/kw/target-ff2-dl/release/examples/bench_log_formats.exe}
DATA=${KORE_DATA_DIR:-C:/Users/skathera/AppData/Local/Temp/kfb}
WORK=${KORE_WORK_DIR:-C:/Users/skathera/AppData/Local/Temp/lf4w}
mkdir -p "$OUT" "$WORK"
load() { typeperf "\\Processor(_Total)\\% Processor Time" -sc 3 2>/dev/null | grep -E '^"[0-9]' | tr '\n' ' ' >> "$OUT/load.txt"; echo >> "$OUT/load.txt"; }
for n in $SIZES; do
  F="$DATA/${PREFIX}_$n.jsonl"
  [ -f "$F" ] || python -I "$HERE/format_bench_gen_realistic.py" "$F" $n
  TT=40; [ "$n" -gt 100000 ] && TT=0
  r=1
  while [ $r -le $ROUNDS ]; do
    for FMT in 1 3 4; do
      echo "load before $PREFIX $n r$r f$FMT: $(date -u +%H:%M:%S)" >> "$OUT/load.txt"; load
      "$NEW" --data "$F" --n $n --work "$WORK/w_${PREFIX}_${n}_f$FMT" --out "$OUT/${PREFIX}_${n}_f${FMT}_r$r.json" --build-reps $BR --reps $REPS --format $FMT --sql-lookups 5 --tamper-trials $TT > "$OUT/log_${PREFIX}_${n}_f${FMT}_r$r.txt" 2>&1
      load
    done
    r=$((r+1))
  done
done
echo DONE > "$OUT/DONE"
