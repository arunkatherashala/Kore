#!/bin/sh
# Interleaved BEFORE/AFTER driver for docs/format_improvement.md.
#   run_format_compare.sh OUTDIR "10000 100000" ROUNDS PREFIX [BUILD_REPS] [REPS]
# PREFIX = rdata (realistic text, benches/format_bench_gen_realistic.py) or data (uniform).
# Variants per round, back to back on the same machine state:
#   old = pre-change binary (built from commit 4a62cba3's versioned code, own target dir)
#   v1  = new binary, log format 1 (default; old encoding, new commit/open code)
#   v2  = new binary, log format 2
set -e
OUT=${1:?outdir}; SIZES=${2:-"10000"}; ROUNDS=${3:-3}; PREFIX=${4:-rdata}; BR=${5:-2}; REPS=${6:-5}
HERE=$(cd "$(dirname "$0")" && pwd)
OLD=${KORE_OLD_EXE:-C:/Users/skathera/kw/target-selfkorefmt-old/release/examples/bench_log_formats.exe}
NEW=${KORE_NEW_EXE:-C:/Users/skathera/kw/target-selfkorefmt/release/examples/bench_log_formats.exe}
DATA=${KORE_DATA_DIR:-C:/Users/skathera/AppData/Local/Temp/kfb}
mkdir -p "$OUT"
load() { typeperf "\\Processor(_Total)\\% Processor Time" -sc 3 2>/dev/null | grep -E '^"[0-9]' | tr '\n' ' ' >> "$OUT/load.txt"; echo >> "$OUT/load.txt"; }
for n in $SIZES; do
  F="$DATA/${PREFIX}_$n.jsonl"
  [ -f "$F" ] || python -I "$HERE/format_bench_gen_realistic.py" "$F" $n
  r=1
  while [ $r -le $ROUNDS ]; do
    for v in old v1 v2; do
      case $v in old) EXE=$OLD; FMT=1;; v1) EXE=$NEW; FMT=1;; v2) EXE=$NEW; FMT=2;; esac
      echo "load before $PREFIX $n r$r $v: $(date -u +%H:%M:%S)" >> "$OUT/load.txt"; load
      "$EXE" --data "$F" --n $n --work "$OUT/w_${PREFIX}_${n}_$v" --out "$OUT/${PREFIX}_${n}_${v}_r$r.json" --build-reps $BR --reps $REPS --format $FMT --sql-lookups 5 --tamper-trials 40 > "$OUT/log_${PREFIX}_${n}_${v}_r$r.txt" 2>&1
      load
    done
    r=$((r+1))
  done
done
echo DONE > "$OUT/DONE"
