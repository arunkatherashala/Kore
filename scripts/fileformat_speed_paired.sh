#!/bin/sh
# Paired, interleaved runs of the three kore-store copies (old Digital Life copy, a980727d, this branch).
# Git Bash. Usage:
#   fileformat_speed_paired.sh store OUTDIR "10000 100000 1000000" ROUNDS
#   fileformat_speed_paired.sh log   OUTDIR "10000 100000" ROUNDS DATADIR
# store: examples/bench_store from kore-store, one exe per store copy: $BIN/bench_store_{old,a98,new}.exe
# log:   kore-self examples/bench_log_formats --format 1 (the log format that stores segments with
#        kore-store), one exe per store copy: $BIN/bench_log_formats_{old,a98,new}.exe; DATADIR holds
#        data_N.jsonl from kore-self/benches/format_bench_gen.py
# Within a round the three variants run back to back; the order rotates every round (old a98 new,
# a98 new old, new old a98, then reversed rotations), so drift in machine load hits all three alike.
# Total CPU % (typeperf, mean of 3 one-second samples) and free disk are logged before and after
# every process in OUTDIR/load.txt.
MODE=${1:?store|log}; OUT=${2:?outdir}; SIZES=${3:-"10000 100000"}; ROUNDS=${4:-5}; DATA=${5:-}
BIN=${BIN:-C:/Users/skathera/kw/ffbench-bin}
mkdir -p "$OUT"
load() { typeperf "\Processor(_Total)\% Processor Time" -sc 3 2>/dev/null | sed -n 3,5p | tr -d '"' | awk -F, '{s+=$2;n++} END{if(n) printf "%.0f", s/n; else printf "na"}'; }
free() { df -BG /c | awk 'NR==2{print $4}'; }
orders="old a98 new|a98 new old|new old a98|new a98 old|a98 old new|old new a98"
for n in $SIZES; do
  for r in $(seq 1 $ROUNDS); do
    ORDER=$(echo "$orders" | cut -d'|' -f$(( (r - 1) % 6 + 1 )))
    for v in $ORDER; do
      b=$(load); t0=$(date +%s)
      if [ "$MODE" = store ]; then
        reps=9; lk=20; [ "$n" -ge 100000 ] && reps=5; [ "$n" -ge 1000000 ] && { reps=3; lk=5; }
        BENCH_LOOKUPS=$lk timeout 1800 "$BIN/bench_store_$v.exe" $n $reps "$OUT/w_$v" "$OUT/r_${v}_${n}_r$r.json" > /dev/null 2> "$OUT/log_${v}_${n}_r$r.txt"
      else
        tt=40
        timeout 3600 "$BIN/bench_log_formats_$v.exe" --data "$DATA/data_$n.jsonl" --n $n --format 1 --work "$OUT/w_$v" \
          --out "$OUT/r_${v}_${n}_r$r.json" --build-reps 2 --reps 5 --tamper-trials $tt 2> "$OUT/log_${v}_${n}_r$r.txt"
      fi
      rc=$?; a=$(load)
      echo "mode=$MODE n=$n round=$r variant=$v rc=$rc secs=$(( $(date +%s) - t0 )) cpu_before=$b cpu_after=$a free=$(free) $(date +%H:%M:%S)" | tee -a "$OUT/load.txt"
      rm -rf "$OUT/w_$v"
    done
  done
done
echo DONE >> "$OUT/load.txt"
