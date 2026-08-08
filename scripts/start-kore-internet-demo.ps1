# Two KORE nodes on one PC — bootstrap (sai) + peer
# Usage: open TWO terminals, run part 1 in terminal A, part 2 in terminal B
# Or: .\start-kore-internet-demo.ps1 bootstrap | peer

param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("bootstrap", "peer")]
    [string]$Role
)

$KoreRoot = Split-Path -Parent $PSScriptRoot
Set-Location $KoreRoot

$LanIp = (Get-NetIPAddress -AddressFamily IPv4 | Where-Object { $_.IPAddress -like "192.168.*" } | Select-Object -First 1).IPAddress
if (-not $LanIp) { $LanIp = "127.0.0.1" }

if ($Role -eq "bootstrap") {
    $env:KORE_CONTINUOUS = "1"
    $env:KORE_HEARTBEAT_SECS = "1"
    $env:KORE_MESH_PORT = "8980"
    $env:KORE_FEDERATION_PORT = "8979"
    $env:KORE_MESH_RELAY = "1"
    $env:KORE_INTERNET_LAN = "1"
    $env:KORE_MESH_ADVERTISE_HOST = $LanIp
    $env:KORE_DEVICE_KIND = "bootstrap"
    Write-Host "Bootstrap KORE (sai) — LAN $LanIp mesh :8980 live :7979"
    cargo run -p kore-self -- sai live 7979
} else {
    $env:KORE_MESH_PORT = "8981"
    $env:KORE_FEDERATION_PORT = "8982"
    $env:KORE_MESH_BOOTSTRAP = "127.0.0.1:8980"
    $env:KORE_INTERNET_LAN = "1"
    $env:KORE_MESH_ADVERTISE_HOST = $LanIp
    $env:KORE_DEVICE_KIND = "pc"
    Write-Host "Peer KORE (peer) — bootstrap 127.0.0.1:8980 mesh :8981 live :7980"
    cargo run -p kore-self -- peer live 7980
}
