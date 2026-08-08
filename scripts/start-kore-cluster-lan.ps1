# KORE Distribution — LAN multi-machine cluster (Phase 7)
# Machine A (coordinator):  .\start-kore-cluster-lan.ps1 -Role coord -LanIp 192.168.1.98
# Machine B (worker):       .\start-kore-cluster-lan.ps1 -Role worker -LanIp 192.168.1.99 -CoordIp 192.168.1.98

param(
    [ValidateSet("coord", "worker", "all-local")]
    [string]$Role = "all-local",
    [string]$LanIp = "127.0.0.1",
    [string]$CoordIp = "127.0.0.1",
    [int]$CoordPort = 7878,
    [int]$Workers = 2
)

$root = Split-Path $PSScriptRoot -Parent
Set-Location $root
$coord = "${CoordIp}:${CoordPort}"
$exeCoord = Join-Path $root "target\debug\kore-coord.exe"
$exeWorker = Join-Path $root "target\debug\kore-worker.exe"

if (-not (Test-Path $exeCoord)) {
    Write-Host "Building cluster..."
    cargo build -p kore-coord -p kore-worker -p kore-distributed
}

$env:KORE_CLUSTER_LOCAL = "1"

switch ($Role) {
    "coord" {
        $env:KORE_COORD_BIND = "0.0.0.0:${CoordPort}"
        Write-Host "[LAN] Coordinator 0.0.0.0:${CoordPort} (advertise ${LanIp}:${CoordPort})"
        & $exeCoord
    }
    "worker" {
        $env:KORE_WORKER_ADVERTISE = $LanIp
        Write-Host "[LAN] Worker -> coord $coord advertise $LanIp"
        & $exeWorker $coord "worker-$LanIp"
    }
    "all-local" {
        $env:KORE_COORD_BIND = "127.0.0.1:${CoordPort}"
        Start-Process -FilePath $exeCoord -NoNewWindow
        Start-Sleep -Seconds 2
        for ($i = 1; $i -le $Workers; $i++) {
            Start-Process -FilePath $exeWorker -ArgumentList $coord, "worker-$i" -NoNewWindow
        }
        Write-Host "[local] Cluster up: coord=$coord workers=$Workers KORE_CLUSTER_LOCAL=1"
        Write-Host "Query: cluster_query_persistent('$coord', sql, table, data)"
    }
}
