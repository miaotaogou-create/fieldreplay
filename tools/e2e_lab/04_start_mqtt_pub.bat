@echo off
setlocal
cd /d "%~dp0"
call "%~dp004_resolve_engine.bat" || exit /b 1
echo [E2E-4] start MQTT fake device -^> 127.0.0.1:1883 topic e2e/sensor
echo Keep this window open. Ctrl+C to stop.
echo Start 04_start_mqtt_broker.bat first.
"%ENGINE%" lab-mqtt-pub --mqtt 127.0.0.1:1883 --topic e2e/sensor --interval-ms 200
echo publisher exited: %ERRORLEVEL%
pause
endlocal
