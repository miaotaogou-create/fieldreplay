@echo off
setlocal
cd /d "%~dp0"
call "%~dp004_resolve_engine.bat" || exit /b 1
echo [E2E-4] start MQTT mini-broker on 127.0.0.1:1883
echo Keep this window open. Ctrl+C to stop.
"%ENGINE%" lab-mqtt-broker --bind 127.0.0.1:1883
echo broker exited: %ERRORLEVEL%
pause
endlocal
