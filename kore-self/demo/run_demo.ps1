# Demo of the versioned log through the real MCP binary, in a temp data root.
# Usage: demo\run_demo.ps1 [path-to-kore-self.exe]   (default: $env:CARGO_TARGET_DIR\debug\kore-self.exe or target\debug)
param([string]$Bin = "")
$ErrorActionPreference = "Stop"
if (-not $Bin) {
  $t = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { "target" }
  $Bin = Join-Path $t "debug\kore-self.exe"
}
if (-not (Test-Path $Bin)) { throw "binary not found: $Bin (build with cargo build -p kore-self)" }
$Root = Join-Path ([IO.Path]::GetTempPath()) ("kore-self-demo-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $Root | Out-Null
$env:KORE_SELF_HOME = $Root
Write-Host "data root: $Root"

function Eid($src, $at, $text) {
  $bytes = [Text.Encoding]::UTF8.GetBytes("$src|$at|$text")
  ([BitConverter]::ToString([Security.Cryptography.SHA256]::Create().ComputeHash($bytes)) -replace '-', '').ToLower()
}
$S = "demo"
$T1 = "Reviewed a pull request that mixed tabs and spaces"
$T2 = "Merge conflict caused by mixed indentation"
$T3 = "Team agreed on spaces in the contributing guide"
$T4 = "Formatter check added to CI"
$E1 = Eid $S "2024-01-03T00:00:00Z" $T1; $E2 = Eid $S "2024-02-14T00:00:00Z" $T2
$E3 = Eid $S "2024-02-20T00:00:00Z" $T3; $E4 = Eid $S "2024-04-09T00:00:00Z" $T4

$script:n = 0
function Call($tool, $argsObj) {
  $script:n++
  @{ jsonrpc = "2.0"; id = $script:n; method = "tools/call"; params = @{ name = $tool; arguments = $argsObj } } | ConvertTo-Json -Depth 6 -Compress
}
$lines = @(
  (Call "self_event_add" @{ text = $T1; occurred_at = "2024-01-03"; source = $S }),
  (Call "self_event_add" @{ text = $T2; occurred_at = "2024-02-14"; source = $S }),
  (Call "self_belief_revise" @{ key = "indent_style"; stance = "tabs"; confidence = 0.55; caused_by = @($E1) }),
  (Call "self_event_add" @{ text = $T3; occurred_at = "2024-02-20"; source = $S }),
  (Call "self_event_add" @{ text = $T4; occurred_at = "2024-04-09"; source = $S }),
  (Call "self_belief_revise" @{ key = "indent_style"; stance = "spaces"; confidence = 0.8; caused_by = @($E2, $E3, $E4) }),
  (Call "self_sql" @{ sql = "SELECT key, stance, confidence FROM beliefs_current"; as_of_version = 3 }),
  (Call "self_sql" @{ sql = "SELECT key, stance, confidence FROM beliefs_current" }),
  (Call "self_belief_history" @{ key = "indent_style" }),
  (Call "self_versions" @{}),
  (Call "self_verify" @{})
)
function Show($out) {
  foreach ($l in $out) { if ($l) { ($l | ConvertFrom-Json).result.content[0].text; "-----" } }
}
Show ($lines | & $Bin demo 2>$null)

Write-Host "flipping one byte in a segment file, then verifying again"
$seg = Get-ChildItem (Join-Path $Root "demo\log\events") -Filter *.kore | Select-Object -First 1
$b = [IO.File]::ReadAllBytes($seg.FullName); $b[[int]($b.Length / 2)] = $b[[int]($b.Length / 2)] -bxor 0xff
[IO.File]::WriteAllBytes($seg.FullName, $b)
Show ((Call "self_verify" @{}) | & $Bin demo 2>$null)
Remove-Item -Recurse -Force $Root
