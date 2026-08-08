# KORE Phase-1 Distribution — persistent TCP cluster (kore-net + kore-coord + kore-worker)
# Terminal 1: coordinator
# Terminal 2+: workers
# Terminal N: queries via cluster_query_persistent or kore-self self_distributed_query

param(
    [ValidateSet("coord", "worker", "status")]
    [string]$Role = "coord",
    [string]$CoordAddr = "127.0.0.1:7878",
    [string]$WorkerId = "worker-1"
)

$root = Split-Path $PSScriptRoot -Parent
Set-Location $root

# LAN: $env:KORE_COORD_BIND = "0.0.0.0:7878"
# LAN worker advertise: $env:KORE_WORKER_ADVERTISE = "192.168.1.98"

switch ($Role) {
    "coord" {
        if (-not $env:KORE_COORD_BIND) { $env:KORE_COORD_BIND = $CoordAddr }
        Write-Host "[kore-cluster] Coordinator on $($env:KORE_COORD_BIND)"
        Write-Host "  Workers connect: cargo run -p kore-worker -- $CoordAddr worker-N"
        Write-Host "  Query: cluster_query_persistent('$CoordAddr', sql, table, data)"
        cargo run -p kore-coord
    }
    "worker" {
        Write-Host "[kore-cluster] Worker $WorkerId -> $CoordAddr"
        cargo run -p kore-worker -- $CoordAddr $WorkerId
    }
    "status" {
        Write-Host "Coord bind: $($env:KORE_COORD_BIND ?? $CoordAddr)"
        Write-Host "Worker bind: $($env:KORE_WORKER_BIND ?? '0.0.0.0:0')"
        Write-Host "Worker advertise: $($env:KORE_WORKER_ADVERTISE ?? '(auto 127.0.0.1:port)')"
    }
}
