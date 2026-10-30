@echo off
setlocal
cd /d "%~dp0"
echo [E2E-3] listen 127.0.0.1:57002 for ~10s
echo Start THIS first, then 03_start_tcp_send.bat in another window.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp003_tcp_recv_laser.ps1"
echo result: %ERRORLEVEL%
pause
endlocal
