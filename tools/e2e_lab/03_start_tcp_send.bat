@echo off
setlocal
cd /d "%~dp0"
echo [E2E-3] start TCP fake device -^> 127.0.0.1:57002
echo Start 03_run_tcp_recv.bat FIRST, then keep this window open. Ctrl+C to stop.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp003_tcp_send_laser.ps1"
echo sender exited: %ERRORLEVEL%
pause
endlocal
