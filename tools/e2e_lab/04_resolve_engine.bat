@echo off
rem Pick newer of release/debug fieldreplay-engine.exe (ASCII-only).
set "ENGINE="
powershell -NoProfile -Command ^
  "$r='%~dp0..\..\engine\target\release\fieldreplay-engine.exe';" ^
  "$d='%~dp0..\..\engine\target\debug\fieldreplay-engine.exe';" ^
  "$c=@(); if (Test-Path $r) { $c += Get-Item $r }; if (Test-Path $d) { $c += Get-Item $d };" ^
  "if (-not $c) { exit 1 };" ^
  "$best = $c | Sort-Object LastWriteTime -Descending | Select-Object -First 1;" ^
  "Write-Output $best.FullName" > "%TEMP%\fr_e2e_engine.txt"
if errorlevel 1 (
  echo FAIL: fieldreplay-engine.exe not found. Build: cd engine ^&^& cargo build
  exit /b 1
)
set /p ENGINE=<"%TEMP%\fr_e2e_engine.txt"
if not exist "%ENGINE%" (
  echo FAIL: fieldreplay-engine.exe not found. Build: cd engine ^&^& cargo build
  exit /b 1
)
echo using: %ENGINE%
