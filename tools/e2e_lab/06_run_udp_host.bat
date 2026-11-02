@echo off
setlocal
cd /d "%~dp0"
echo [E2E-6] fake-host UDP recv on 127.0.0.1:56101
echo Start 06_start_udp_probe.bat in another window.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp006_udp_host_recv.ps1"
echo exit: %ERRORLEVEL%
pause
endlocal
