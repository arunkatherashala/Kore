#!/usr/bin/env pwsh
# Automated GPG Key Generation for Maven Central Signing
# Uses GPG from Git for Windows (included in Git installation)

# Create an alias for GPG from Git
$gpgPath = "C:\Program Files\Git\usr\bin\gpg.exe"
if (Test-Path $gpgPath) {
    Set-Alias -Name gpg -Value $gpgPath -Scope Script
} else {
    Write-Host "[ERROR] Could not find GPG. Please install Git for Windows with GPG support." -ForegroundColor Red
    exit 1
}

Write-Host "[KORE] Generating GPG Key for Maven Central..." -ForegroundColor Cyan

# Check if GPG is installed
$gpgCheck = gpg --version 2>&1
if ($LASTEXITCODE -ne 0) {
    Write-Host "[ERROR] GPG is not working properly." -ForegroundColor Red
    exit 1
}

Write-Host "[OK] GPG found: $($gpgCheck[0])" -ForegroundColor Green

# Create a temporary batch file for key generation
# Use a simple name in the current directory to avoid path escaping issues
$batchFile = "gpg_key_batch.txt"
$batchContent = "%echo Generating GPG key for Maven Central`n%no-ask-passphrase`nKey-Type: RSA`nKey-Length: 4096`nSubkey-Type: RSA`nSubkey-Length: 4096`nName-Real: Arun Kather Ashala`nName-Email: arunkatherashala@gmail.com`nExpire-Date: 0`n%no-protection`n%commit`n%echo done"

# Write the batch file
@"
%echo Generating GPG key for Maven Central
%no-ask-passphrase
Key-Type: RSA
Key-Length: 4096
Subkey-Type: RSA
Subkey-Length: 4096
Name-Real: Arun Kather Ashala
Name-Email: arunkatherashala@gmail.com
Expire-Date: 0
%no-protection
%commit
%echo done
"@ | Out-File -FilePath $batchFile -Encoding ASCII

Write-Host "[WORKING] Generating key (this may take a minute)..." -ForegroundColor Cyan

# Try quick-gen-key mode first (simpler, doesn't need keybox daemon)
& $gpgPath --quick-gen-key "Arun Kather Ashala <arunkatherashala@gmail.com>" rsa4096 never 2>&1

if ($LASTEXITCODE -ne 0) {
    Write-Host "[ERROR] Key generation failed!" -ForegroundColor Red
    if (Test-Path $batchFile) { Remove-Item $batchFile -Force -ErrorAction SilentlyContinue }
    exit 1
}

Write-Host "[OK] Key generated successfully!" -ForegroundColor Green

# List keys to find the key ID
Write-Host "`n[INFO] Your GPG Keys:" -ForegroundColor Cyan
$keyList = & $gpgPath --list-secret-keys --keyid-format LONG

Write-Host $keyList

# Extract the key ID (the part after the /)
$keyLine = $keyList | Select-String "sec.*rsa4096" | Select-Object -First 1
if ($keyLine) {
    $keyID = ($keyLine.Line -split '/')[1].Split(' ')[0]
    Write-Host "`n[KEY_ID] $keyID" -ForegroundColor Yellow
} else {
    Write-Host "[ERROR] Could not find key ID!" -ForegroundColor Red
    if (Test-Path $batchFile) { Remove-Item $batchFile -Force -ErrorAction SilentlyContinue }
    exit 1
}

# Export private key
$privateKeyFile = "$PWD\kore-private-key.asc"
Write-Host "`n[EXPORT] Exporting private key to: $privateKeyFile" -ForegroundColor Cyan

& $gpgPath --export-secret-keys --armor $keyID | Out-File -FilePath $privateKeyFile -Encoding UTF8

if ((Test-Path $privateKeyFile) -and ((Get-Item $privateKeyFile).Length -gt 0)) {
    Write-Host "[OK] Private key exported!" -ForegroundColor Green
    Write-Host "     Location: $privateKeyFile" -ForegroundColor Green
} else {
    Write-Host "[ERROR] Failed to export private key!" -ForegroundColor Red
    Remove-Item $batchFile -Force -ErrorAction SilentlyContinue
    exit 1
}

# Export public key for reference
$publicKeyFile = "$PWD\kore-public-key.asc"
& $gpgPath --export --armor $keyID | Out-File -FilePath $publicKeyFile -Encoding UTF8
Write-Host "[OK] Public key exported to: $publicKeyFile" -ForegroundColor Green

# Cleanup
if (Test-Path $batchFile) { Remove-Item $batchFile -Force -ErrorAction SilentlyContinue }

# Display instructions
Write-Host "`n========================================================" -ForegroundColor Cyan
Write-Host "[SUCCESS] Next Steps:" -ForegroundColor Green
Write-Host "========================================================" -ForegroundColor Cyan

Write-Host @"

1. Go to: https://github.com/arunkatherashala/Kore/settings/secrets/actions

2. Click "New repository secret" and add:
   Name: GPG_PRIVATE_KEY
   Value: [COPY ENTIRE CONTENTS OF: $privateKeyFile]

3. Click "New repository secret" again:
   Name: GPG_PASSPHRASE
   Value: (leave empty - key has no passphrase)

4. After adding secrets, trigger deployment:
   gh workflow run publish-maven.yml --ref release/v0.1.0

========================================================

To copy the private key contents:
   Get-Content $privateKeyFile | Set-Clipboard

Or open the file in notepad and copy all text.

========================================================
"@

# Copy private key to clipboard
Write-Host "[CLIPBOARD] Copying private key to clipboard..." -ForegroundColor Cyan
Get-Content $privateKeyFile | Set-Clipboard
Write-Host "[OK] Private key copied to clipboard!" -ForegroundColor Green

Write-Host "`n[READY] You're ready to deploy to Maven Central!" -ForegroundColor Green
Write-Host "`n[NEXT] Complete these steps:" -ForegroundColor Cyan
Write-Host "1. Go to: https://github.com/arunkatherashala/Kore/settings/secrets/actions" -ForegroundColor White
Write-Host "2. Click 'New repository secret' and add:" -ForegroundColor White
Write-Host "   - Name: GPG_PRIVATE_KEY" -ForegroundColor White
Write-Host "   - Value: (Ctrl+V to paste from clipboard)" -ForegroundColor White
Write-Host "3. Click 'New repository secret' again:" -ForegroundColor White
Write-Host "   - Name: GPG_PASSPHRASE" -ForegroundColor White
Write-Host "   - Value: (leave empty)" -ForegroundColor White
Write-Host "4. Run: gh workflow run publish-maven.yml --ref release/v0.1.0" -ForegroundColor White
Write-Host "`n" -ForegroundColor Cyan

