@echo off
setlocal
cd /d "%~dp0"
echo [E2E-6] TCP probe -> 127.0.0.1:58102 (fake host). Keep open.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp003_tcp_send_laser.ps1" -HostAddr 127.0.0.1 -Port 58102
echo probe exited: %ERRORLEVEL%
pause
endlocal
