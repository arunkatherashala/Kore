#!/bin/bash
# Correctness gate for the KORE SQL engine. Run from Git Bash after every merge:
#   bash kore-sql/tests/oracle/run_gate.sh            (from the workspace root, or from anywhere: it finds its worktree)
# Steps: (1) cargo tests of kore-sql / kore-join / kore-ffi, (2) fuzz small + mid + threshold sizes with the in-process
# differential (every optimisation switch on/off), dumping data for (3) the SQLite oracle (compare.py).
# Prints one verdict line:  GATE PASS|FAIL tests=<ok|FAIL|skipped> diff=<n> oracle_mismatch=<n> oracle_kore_err=<n> (<minutes> min)
# Env knobs: GATE_SMALL_SEEDS (60) GATE_SMALL_PER (150) GATE_MID_SEEDS (6) GATE_MID_PER (100) GATE_FIRST (1) GATE_THRESH (1: sizes around
#   the executor thresholds 8192/32768/65536/100000) GATE_SKIP_TESTS (1: skip cargo test) GATE_OUT (output dir) GATE_JOBS (4)
#   PY (python with sqlite3; default %LOCALAPPDATA%\miniconda3\envs\spark312\python.exe)
set -u
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
cd "$ROOT"
JOBS=${GATE_JOBS:-4}
OUT=${GATE_OUT:-$ROOT/target/gate}
PY=${PY:-"$LOCALAPPDATA/miniconda3/envs/spark312/python.exe"}
[ -x "$PY" ] || PY=python
FIRST=${GATE_FIRST:-1}
rm -rf "$OUT"; mkdir -p "$OUT"
T0=$(date +%s)
tests=skipped
if [ "${GATE_SKIP_TESTS:-0}" != 1 ]; then
  if cargo test --release -p kore-sql -p kore-join -p kore-ffi --offline -j "$JOBS" --no-fail-fast > "$OUT/tests.log" 2>&1; then tests=ok; else tests=FAIL; fi
  echo "[gate] cargo tests: $tests ($(grep -c '^test result' "$OUT/tests.log") result lines, failures: $(grep -c ' FAILED$' "$OUT/tests.log"))"
fi
cargo test --release -p kore-sql --offline -j "$JOBS" --test fuzz_oracle --no-run > "$OUT/build.log" 2>&1 || { echo "GATE FAIL (fuzz build failed, see $OUT/build.log)"; exit 2; }
EXE=$(ls -t "$ROOT"/target/release/deps/fuzz_oracle-*.exe 2>/dev/null | head -1)
[ -n "$EXE" ] || EXE=$(ls -t "$ROOT"/target/release/deps/fuzz_oracle-* | grep -v '\.d$' | head -1)

diff=0; mism=0; kerr=0
run() {  # name, env... : one fuzz run + oracle replay
  local name=$1; shift
  mkdir -p "$OUT/$name"
  ( cd "$ROOT/kore-sql" && env "$@" FUZZ_DIR="$OUT/$name/data" "$EXE" --ignored --nocapture fuzz_joins > "$OUT/$name/kore.log" 2>&1 )
  local d; d=$(grep -c '^DIFF' "$OUT/$name/kore.log")
  "$PY" "$ROOT/kore-sql/tests/oracle/compare.py" "$OUT/$name/data" > "$OUT/$name/oracle.log" 2>&1
  local m e; m=$(grep -c '^MISMATCH' "$OUT/$name/oracle.log"); e=$(grep -c '^KORE-ERR' "$OUT/$name/oracle.log")
  diff=$((diff + d)); mism=$((mism + m)); kerr=$((kerr + e))
  echo "[gate] $name: diff=$d oracle_mismatch=$m oracle_kore_err=$e :: $(grep -h 'fuzzed' "$OUT/$name/kore.log" | head -1) / $(tail -1 "$OUT/$name/oracle.log")"
  rm -rf "$OUT/$name/data"  # keep logs only (data can be GBs); rerun the seed with FUZZ_FIRST/FUZZ_SEEDS to reproduce
}
run small FUZZ_SCALE=small FUZZ_SEEDS="${GATE_SMALL_SEEDS:-60}" FUZZ_PER="${GATE_SMALL_PER:-150}" FUZZ_FIRST="$FIRST"
run mid   FUZZ_SCALE=mid   FUZZ_SEEDS="${GATE_MID_SEEDS:-6}"    FUZZ_PER="${GATE_MID_PER:-100}"   FUZZ_FIRST="$FIRST"
if [ "${GATE_THRESH:-1}" = 1 ]; then
  # table sizes around the fused window (8192), probe chunk (32768), 65536 windows and SIP_MIN_ROWS (100000)
  for a in 8191 8193 32769 65537 100001; do
    run "thr_$a" FUZZ_SCALE=mid FUZZ_A="$a" FUZZ_SEEDS=1 FUZZ_PER=60 FUZZ_FIRST="$FIRST"
  done
fi
MIN=$(( ($(date +%s) - T0) / 60 ))
v=PASS; [ "$tests" = FAIL ] && v=FAIL; [ "$diff" != 0 ] && v=FAIL; [ "$mism" != 0 ] && v=FAIL; [ "$kerr" != 0 ] && v=FAIL
echo "GATE $v tests=$tests diff=$diff oracle_mismatch=$mism oracle_kore_err=$kerr (${MIN} min, logs in $OUT)"
[ "$v" = PASS ]
