@echo off
setlocal
cd /d "%~dp0"
echo [E2E-7] aligned replay via control WS -^> fake hosts
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp007_run_aligned_replay.ps1" %*
echo exit: %ERRORLEVEL%
pause
endlocal
