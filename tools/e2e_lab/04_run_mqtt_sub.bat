@echo off
setlocal
cd /d "%~dp0"
call "%~dp004_resolve_engine.bat" || exit /b 1
echo [E2E-4] MQTT subscribe check (start broker, then pub, then this)
"%ENGINE%" lab-mqtt-sub --mqtt 127.0.0.1:1883 --topic e2e/# --seconds 8
echo sub exited: %ERRORLEVEL%
pause
endlocal
