#!/usr/bin/env bash
# Track D1c timing (targets O1-O8, docs/d1c_design.md section 7). NOT run by the implementation.
# Run only on a quiet machine: no other build or test. Every result line is JSON (stdout and
# $WORK/d1c_timing.jsonl); restart rows carry machine_busy_share (rough if > 0.3).
#
#   SRC=C:/Users/skathera/kw/longrun-data2 WORK=C:/Users/skathera/kw/d1c-timing bash scripts/d1c_timing.sh
#
# SRC  = root of the 1,000,000-step world-1 long run (holds longrun/log), only read.
# WORK = scratch root for the copies (created; about 2 x 0.7 GB per cut and variant at a time).
# Optional: CUTS="12001 40064" (versions), RUNS=2, CHUNK=5000, MIN_FREE_GB=4.
set -euo pipefail
: "${SRC:?set SRC to the root of the 1M-step long run}"
: "${WORK:?set WORK to a scratch directory}"
CUTS="${CUTS:-12001 40064}"
RUNS="${RUNS:-2}"
CHUNK="${CHUNK:-5000}"
MIN_FREE_GB="${MIN_FREE_GB:-4}"
cd "$(dirname "$0")/.."
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-target}"
cargo build -p kore-self --release --offline
cargo build -p kore-self --release --offline --example longrun_being
H="$CARGO_TARGET_DIR/release/examples/longrun_being"
export LONGRUN_BIN="$CARGO_TARGET_DIR/release/kore-self"
mkdir -p "$WORK"
OUT="$WORK/d1c_timing.jsonl"
log() { echo "$1" | tee -a "$OUT"; }
G="--min-free-gb $MIN_FREE_GB"
export LONGRUN_CKPT_EVERY=5000   # checkpoint on in both variants (as in D1)

for AT in $CUTS; do
  for V in off on; do
    R="$WORK/cut_${AT}_${V}"
    rm -rf "$R"
    # disk guard: the harness refuses to run below MIN_FREE_GB (cut checks after copying, d1c-prep before)
    log "{\"type\":\"phase\",\"cut\":$AT,\"variant\":\"$V\",\"step\":\"cut\"}"
    unset LONGRUN_FAST_OPEN
    "$H" cut --src "$SRC" --at "$AT" --root "$R" | tee -a "$OUT"
    if [ "$V" = on ]; then
      # O8 first: the format-3 copy with the key on opens through the full path (counter = all files)
      "$H" openfast --root "$R" --fast 1 $G | tee -a "$OUT"
      LONGRUN_FAST_OPEN=1 "$H" d1c-prep --root "$R" $G | tee -a "$OUT"
    fi
    # untimed priming call: D1b checkpoint, and (on) the first index run
    log "{\"type\":\"phase\",\"cut\":$AT,\"variant\":\"$V\",\"step\":\"priming\"}"
    "$H" restart --root "$R" --chunk "$CHUNK" | tee -a "$OUT"
    FAST=0; [ "$V" = on ] && FAST=1
    for N in $(seq 1 "$RUNS"); do
      log "{\"type\":\"phase\",\"cut\":$AT,\"variant\":\"$V\",\"run\":$N}"
      # O1, O3: in-process open (time, files and bytes read, peak working set)
      "$H" openfast --root "$R" --fast "$FAST" $G | tee -a "$OUT"
      # O2, O5, O6: whole 5,000-step restart call, cold process (index_runs.maintain.ms = run write, O5)
      "$H" restart --root "$R" --chunk "$CHUNK" | tee -a "$OUT"
      # O4: KILL present before the call, then KILL created 100 ms after the call is sent
      "$H" restart --root "$R" --chunk "$CHUNK" --kill-ms 0 | tee -a "$OUT"
      "$H" restart --root "$R" --chunk "$CHUNK" --kill-ms 100 | tee -a "$OUT"
    done
  done
  # O7: rows appended after the cut, off vs on (audit rows, probes and checkpoint anchors listed separately)
  "$H" compare --root "$WORK/cut_${AT}_off" --other "$WORK/cut_${AT}_on" --at "$AT" | tee -a "$OUT"
  # files on disk (O3 context, no target) are in the openfast rows (files_on_disk)
  rm -rf "$WORK/cut_${AT}_off" "$WORK/cut_${AT}_on"
done
log "{\"type\":\"done\"}"
