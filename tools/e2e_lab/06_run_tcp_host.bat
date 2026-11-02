@echo off
setlocal
cd /d "%~dp0"
echo [E2E-6] fake-host TCP listen 127.0.0.1:58102
echo Start 06_start_tcp_probe.bat AFTER this window shows listening.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp006_tcp_host_recv.ps1"
echo exit: %ERRORLEVEL%
pause
endlocal
