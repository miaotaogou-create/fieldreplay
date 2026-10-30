@echo off
setlocal
cd /d "%~dp0"
echo [E2E-2] listen 127.0.0.1:56001 for 8s
echo Start 02_start_udp_send.bat in another window FIRST, then this.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp002_udp_recv_status.ps1"
echo result: %ERRORLEVEL%
pause
endlocal
