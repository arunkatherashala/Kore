# Mama KORE-self — continuous learning with full technical stack (programming + bash + linux)
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

Write-Host "Starting mama — technical + world knowledge fill"
Write-Host "Catalog: self_world_catalog action=programming | shells | linux | technical"
Write-Host "Gaps:    self_world_unknown  then  self_fill_self"

Set-Location (Split-Path $PSScriptRoot -Parent)
cargo run -p kore-self -- mama
