$total = 0
$passed = 0

function Test-Check ($name, $condition) {
    $global:total++
    Write-Host -NoNewline "Testing: $name ... "
    if ($condition) {
        Write-Host "PASS" -ForegroundColor Green
        $global:passed++
    } else {
        Write-Host "FAIL" -ForegroundColor Red
    }
}

Test-Check "Reader File" (Test-Path "C:\Users\ksak_\OneDrive\Desktop\dbt_prep\Kore\python\kore\reader.py")
Test-Check "Reader Logic" ((Get-Content "C:\Users\ksak_\OneDrive\Desktop\dbt_prep\Kore\python\kore\reader.py" -Raw).Contains("_parse_chunk"))
Test-Check "Reader Decomp" ((Get-Content "C:\Users\ksak_\OneDrive\Desktop\dbt_prep\Kore\python\kore\reader.py" -Raw).Contains("zlib"))
Test-Check "Reader BytesIO" ((Get-Content "C:\Users\ksak_\OneDrive\Desktop\dbt_prep\Kore\python\kore\reader.py" -Raw).Contains("BytesIO"))
Test-Check "Spark DS File" (Test-Path "C:\Users\ksak_\OneDrive\Desktop\dbt_prep\Kore\python\kore\spark_datasource.py")
Test-Check "Spark Class" ((Get-Content "C:\Users\ksak_\OneDrive\Desktop\dbt_prep\Kore\python\kore\spark_datasource.py" -Raw).Contains("KoreDataSource"))
Test-Check "Release Plan" (Test-Path "C:\Users\ksak_\OneDrive\Desktop\dbt_prep\Kore\KORE_v1.0.0_RELEASE_PLAN.md")
Test-Check "Benchmark Report" (Test-Path "C:\Users\ksak_\OneDrive\Desktop\dbt_prep\Kore\KORE_BENCHMARK_CERTIFIED_REPORT.md")
Test-Check "Deployment Guide" (Test-Path "C:\Users\ksak_\OneDrive\Desktop\dbt_prep\Kore\KORE_ENTERPRISE_DEPLOYMENT_GUIDE.md")
Test-Check "Complete Summary" (Test-Path "C:\Users\ksak_\OneDrive\Desktop\dbt_prep\Kore\KORE_v1.0.0_COMPLETE_SUMMARY.md")
Test-Check "Doc Index" (Test-Path "C:\Users\ksak_\OneDrive\Desktop\dbt_prep\Kore\KORE_DOCUMENTATION_INDEX.md")
Test-Check "Phase 8 Validation" (Test-Path "C:\Users\ksak_\OneDrive\Desktop\dbt_prep\Kore\PHASE_8_MISSION_VALIDATION.md")
Test-Check "Pass Rate Claim" ((Get-Content "C:\Users\ksak_\OneDrive\Desktop\dbt_prep\Kore\PHASE_8_MISSION_VALIDATION.md" -Raw).Contains("100%"))

$pct = ($passed / $total) * 100
Write-Host "Total: $total"
Write-Host "Passed: $passed"
Write-Host "Failed: $($total - $passed)"
Write-Host "Pass Rate: $pct%"
if ($pct -eq 100) { Write-Host "FINAL STATUS: SUCCESS" } else { Write-Host "FINAL STATUS: FAILURE" }
