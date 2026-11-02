@echo off
setlocal
cd /d "%~dp0"
call "%~dp004_resolve_engine.bat" || exit /b 1
echo [E2E-6] fake-host MQTT subscribe e2e/#
echo Need: 04_start_mqtt_broker.bat then 04_start_mqtt_pub.bat (or any pub on e2e/*)
"%ENGINE%" lab-mqtt-sub --mqtt 127.0.0.1:1883 --topic e2e/# --seconds 8
echo sub exited: %ERRORLEVEL%
pause
endlocal
