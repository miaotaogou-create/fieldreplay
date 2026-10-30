@echo off
setlocal
cd /d "%~dp0"
echo [E2E-2] start UDP fake device -^> 127.0.0.1:56001
echo Keep this window open. Ctrl+C to stop.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp002_udp_send_status.ps1"
echo sender exited: %ERRORLEVEL%
pause
endlocal
