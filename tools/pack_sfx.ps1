# FieldReplay G2b: green pack -> single self-extracting EXE
# Requires: pack\FieldReplay\ from tools\pack_green (or pass -PackGreen)
# Output: pack\FieldReplay.exe

param(
    [switch]$PackGreen
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
if (-not $Root) {
    $Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
}

$Green = Join-Path $Root "pack\FieldReplay"
$OutExe = Join-Path $Root "pack\FieldReplay.exe"
$Staging = Join-Path $Root "pack\_sfx_staging"
$ZipPath = Join-Path $Staging "payload.zip"
$StubSrc = Join-Path $Root "sfx\target\release\fieldreplay-sfx.exe"

function Step([string]$msg) {
    Write-Host ""
    Write-Host ("=== {0} ===" -f $msg)
}

if ($PackGreen) {
    Step "pack_green first"
    & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "pack_green.ps1")
    if ($LASTEXITCODE -ne 0) { throw "pack_green failed" }
}

$ShellInGreen = Join-Path $Green "fieldreplay.exe"
$EngineInGreen = Join-Path $Green "fieldreplay-engine.exe"
$IndexInGreen = Join-Path $Green "ui\dist\index.html"
if (-not (Test-Path $ShellInGreen)) {
    throw ("missing green pack: {0} (run tools\pack_green.bat first)" -f $ShellInGreen)
}
if (-not (Test-Path $EngineInGreen)) {
    throw ("missing {0}" -f $EngineInGreen)
}
if (-not (Test-Path $IndexInGreen)) {
    throw ("missing {0}" -f $IndexInGreen)
}

Step "[1/4] sfx stub Release"
Push-Location (Join-Path $Root "sfx")
try {
    & cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "sfx release build failed" }
} finally {
    Pop-Location
}

if (-not (Test-Path $StubSrc)) {
    throw ("missing stub {0}" -f $StubSrc)
}

Step "[2/4] zip green dir"
if (Test-Path $Staging) {
    Remove-Item -Recurse -Force $Staging
}
New-Item -ItemType Directory -Force -Path $Staging | Out-Null

Add-Type -AssemblyName System.IO.Compression.FileSystem
if (Test-Path $ZipPath) {
    Remove-Item -Force $ZipPath
}
# Zip root = green pack contents (same layout after extract into FieldReplay)
[System.IO.Compression.ZipFile]::CreateFromDirectory(
    $Green,
    $ZipPath,
    [System.IO.Compression.CompressionLevel]::Optimal,
    $false
)

$ZipBytes = [System.IO.File]::ReadAllBytes($ZipPath)
if ($ZipBytes.Length -lt 22) {
    throw "zip too small"
}

# Version = SHA256 of zip; matching .fr_sfx_version skips extract
$Sha = [System.Security.Cryptography.SHA256]::Create()
try {
    $Hash = ($Sha.ComputeHash($ZipBytes) | ForEach-Object { $_.ToString("x2") }) -join ""
} finally {
    $Sha.Dispose()
}
$Version = $Hash
Write-Host ("version: {0}" -f $Version)

Step ("[3/4] append payload -> {0}" -f $OutExe)
$StubBytes = [System.IO.File]::ReadAllBytes($StubSrc)
$VerBytes = [System.Text.Encoding]::UTF8.GetBytes($Version)
$VerLen = [BitConverter]::GetBytes([uint32]$VerBytes.Length)
$ZipLen = [BitConverter]::GetBytes([uint64]$ZipBytes.Length)
$Magic = [System.Text.Encoding]::ASCII.GetBytes("FRSFX001")
if (-not [BitConverter]::IsLittleEndian) {
    throw "pack_sfx requires little-endian host"
}

$OutDir = Split-Path -Parent $OutExe
if (-not (Test-Path $OutDir)) {
    New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
}
$fs = [System.IO.File]::Create($OutExe)
try {
    $fs.Write($StubBytes, 0, $StubBytes.Length)
    $fs.Write($ZipBytes, 0, $ZipBytes.Length)
    $fs.Write($VerBytes, 0, $VerBytes.Length)
    $fs.Write($VerLen, 0, $VerLen.Length)
    $fs.Write($ZipLen, 0, $ZipLen.Length)
    $fs.Write($Magic, 0, $Magic.Length)
} finally {
    $fs.Dispose()
}

Step "[4/4] self-check"
& $OutExe --self-check
if ($LASTEXITCODE -ne 0) {
    throw "sfx self-check failed"
}

Remove-Item -Recurse -Force $Staging -ErrorAction SilentlyContinue

Write-Host ""
Write-Host ("DONE: {0}" -f $OutExe)
Write-Host "Double-click the single EXE; it extracts to sibling FieldReplay when writable."
exit 0
