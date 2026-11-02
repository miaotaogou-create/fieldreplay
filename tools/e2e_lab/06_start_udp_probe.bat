@echo off
setlocal
cd /d "%~dp0"
echo [E2E-6] UDP probe -> 127.0.0.1:56101 (fake host). Keep open.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp002_udp_send_status.ps1" -HostAddr 127.0.0.1 -Port 56101
echo probe exited: %ERRORLEVEL%
pause
endlocal
