# KORE-self: lightweight continuous learning (default — system won't hang)
# Learns every tick but max 2 Wikipedia fetches per heartbeat (4s timeout each).

param([string]$Owner = "sai")

$KoreRoot = Split-Path -Parent $PSScriptRoot
Set-Location $KoreRoot

$env:KORE_CONTINUOUS = "1"
$env:KORE_HEARTBEAT_SECS = "1"
$env:KORE_LIGHTWEIGHT = "1"
$env:KORE_LEARN_MAX_HTTP = "2"
$env:KORE_HTTP_TIMEOUT_SECS = "4"
$env:KORE_FILL_GAPS = "1"
$env:KORE_LANG_FAST = "1"

Write-Host "KORE lightweight continuous — owner=$Owner"
Write-Host "  max 2 Wikipedia calls/tick, 4s timeout — no hang"
Write-Host "  aggressive: KORE_LIGHTWEIGHT=0 KORE_LEARN_MAX_HTTP=8"
Write-Host ""

cargo run -p kore-self -- $Owner
