# FieldReplay E6c: green-dir pack (Release engine + ui/dist + shell)
# Output: <repo>\pack\FieldReplay\
#   fieldreplay.exe
#   fieldreplay-engine.exe
#   ui\dist\...

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
if (-not $Root) {
    $Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
}
$Out = Join-Path $Root "pack\FieldReplay"

function Step([string]$msg) {
    Write-Host ""
    Write-Host "=== $msg ==="
}

Step "[1/4] engine Release"
Push-Location (Join-Path $Root "engine")
try {
    & cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "engine release build failed" }
} finally { Pop-Location }

Step "[2/4] ui npm build"
Push-Location (Join-Path $Root "ui")
try {
    & npm run build
    if ($LASTEXITCODE -ne 0) { throw "ui build failed" }
} finally { Pop-Location }

$IndexHtml = Join-Path $Root "ui\dist\index.html"
if (-not (Test-Path $IndexHtml)) {
    throw "missing ui\dist\index.html"
}

Step "[3/4] shell Release"
Push-Location (Join-Path $Root "shell")
try {
    & cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "shell release build failed" }
} finally { Pop-Location }

$EngineExe = Join-Path $Root "engine\target\release\fieldreplay-engine.exe"
$ShellExe = Join-Path $Root "shell\target\release\fieldreplay.exe"
if (-not (Test-Path $EngineExe)) { throw "missing $EngineExe" }
if (-not (Test-Path $ShellExe)) { throw "missing $ShellExe" }

Step "[4/4] assemble $Out"
if (Test-Path $Out) {
    Remove-Item -Recurse -Force $Out
}
New-Item -ItemType Directory -Force -Path (Join-Path $Out "ui\dist") | Out-Null

Copy-Item $ShellExe (Join-Path $Out "fieldreplay.exe")
Copy-Item $EngineExe (Join-Path $Out "fieldreplay-engine.exe")
Copy-Item -Recurse -Force (Join-Path $Root "ui\dist\*") (Join-Path $Out "ui\dist")

$ReadmeSrc = Join-Path $PSScriptRoot "pack_readme.txt"
Copy-Item $ReadmeSrc (Join-Path $Out "README.txt")

Step "smoke: pack\FieldReplay\fieldreplay.exe --smoke"
$SmokeExe = Join-Path $Out "fieldreplay.exe"
& $SmokeExe --smoke
if ($LASTEXITCODE -ne 0) { throw "smoke failed" }

Write-Host ""
Write-Host "DONE: $Out"
Write-Host "Double-click fieldreplay.exe in that folder (E6d hand-test next)."
exit 0
