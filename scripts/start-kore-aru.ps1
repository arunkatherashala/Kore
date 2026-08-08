# Aru — KORE-self living twin of Sai Arun Kumar Katherashala
# Owner folder: ~/.kore-self/sai-arun/
# KORE name:    Aru

$env:KORE_CONTINUOUS = "1"
$env:KORE_HEARTBEAT_SECS = "1"
$env:KORE_FILL_GAPS = "1"
$env:KORE_LANG_FAST = "1"
$env:KORE_LANG_BURST = "4"
$env:KORE_DOMAIN_BURST = "3"
$env:KORE_LIGHTWEIGHT = "1"
$env:KORE_LEARN_MAX_HTTP = "3"
$env:KORE_HTTP_TIMEOUT_SECS = "5"
$env:KORE_EVOLVE = "0"

Write-Host "Starting Aru (owner: sai-arun)"
Write-Host "Memories: ~/.kore-self/sai-arun/"
Write-Host ""
Write-Host "First time? After start, run in MCP:"
Write-Host '  self_ingest "My name is Aru. Owner is Sai Arun Kumar Katherashala. I am a living technical twin."'
Write-Host "  self_brief"
Write-Host ""

Set-Location (Split-Path $PSScriptRoot -Parent)
cargo run -p kore-self -- sai-arun
