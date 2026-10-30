@echo off
rem Sets ENGINE to release or debug fieldreplay-engine.exe (ASCII-only).
set "ENGINE=%~dp0..\..\engine\target\release\fieldreplay-engine.exe"
if exist "%ENGINE%" goto :eof
set "ENGINE=%~dp0..\..\engine\target\debug\fieldreplay-engine.exe"
if exist "%ENGINE%" goto :eof
echo FAIL: fieldreplay-engine.exe not found. Build: cd engine ^&^& cargo build
exit /b 1
