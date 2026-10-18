#!/usr/bin/env bash
# Demo of the versioned log through the real MCP binary, in a temp data root.
# Usage: demo/run_demo.sh [path-to-kore-self-binary]   (default: target/debug/kore-self)
set -euo pipefail
BIN="${1:-${CARGO_TARGET_DIR:-target}/debug/kore-self}"
[ -x "$BIN" ] || [ -x "$BIN.exe" ] || { echo "binary not found: $BIN (build with cargo build -p kore-self)"; exit 1; }
ROOT="$(mktemp -d)"
export KORE_SELF_HOME="$ROOT"
echo "data root: $ROOT"

# event_id = sha256(source|occurred_at|text); occurred_at is normalised to ...T00:00:00Z
eid() { printf '%s|%s|%s' "$1" "$2" "$3" | sha256sum | cut -d' ' -f1; }
S=demo
T1="Reviewed a pull request that mixed tabs and spaces"
T2="Merge conflict caused by mixed indentation"
T3="Team agreed on spaces in the contributing guide"
T4="Formatter check added to CI"
E1=$(eid $S 2024-01-03T00:00:00Z "$T1"); E2=$(eid $S 2024-02-14T00:00:00Z "$T2")
E3=$(eid $S 2024-02-20T00:00:00Z "$T3"); E4=$(eid $S 2024-04-09T00:00:00Z "$T4")

n=0
call() { n=$((n+1)); printf '{"jsonrpc":"2.0","id":%d,"method":"tools/call","params":{"name":"%s","arguments":%s}}\n' "$n" "$1" "$2"; }
{
  call self_event_add "{\"text\":\"$T1\",\"occurred_at\":\"2024-01-03\",\"source\":\"$S\"}"
  call self_event_add "{\"text\":\"$T2\",\"occurred_at\":\"2024-02-14\",\"source\":\"$S\"}"
  call self_belief_revise "{\"key\":\"indent_style\",\"stance\":\"tabs\",\"confidence\":0.55,\"caused_by\":[\"$E1\"]}"
  call self_event_add "{\"text\":\"$T3\",\"occurred_at\":\"2024-02-20\",\"source\":\"$S\"}"
  call self_event_add "{\"text\":\"$T4\",\"occurred_at\":\"2024-04-09\",\"source\":\"$S\"}"
  call self_belief_revise "{\"key\":\"indent_style\",\"stance\":\"spaces\",\"confidence\":0.8,\"caused_by\":[\"$E2\",\"$E3\",\"$E4\"]}"
  call self_sql '{"sql":"SELECT key, stance, confidence FROM beliefs_current","as_of_version":3}'
  call self_sql '{"sql":"SELECT key, stance, confidence FROM beliefs_current"}'
  call self_belief_history '{"key":"indent_style"}'
  call self_versions '{}'
  call self_verify '{}'
} | "$BIN" demo 2>/dev/null | while IFS= read -r line; do
  # print the tool's text payload on its own lines
  printf '%s\n' "$line" | sed -e 's/.*"text":"//' -e 's/","type":"text.*$//' -e 's/\n/\n/g' -e 's/\\"/"/g'
  echo "-----"
done

echo "flipping one byte in a segment file, then verifying again"
SEG="$(ls "$ROOT"/demo/log/events/*.kore | head -1)"
printf '\xff' | dd of="$SEG" bs=1 seek=40 count=1 conv=notrunc 2>/dev/null
printf '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"self_verify","arguments":{}}}\n' | "$BIN" demo 2>/dev/null | sed -e 's/.*"text":"//' -e 's/".*$//'
rm -rf "$ROOT"
