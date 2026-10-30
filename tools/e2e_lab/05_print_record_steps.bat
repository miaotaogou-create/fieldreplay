@echo off
setlocal
cd /d "%~dp0"
echo ========================================
echo  E2E-5 FieldReplay multi-record checklist
echo ========================================
echo.
echo [A] Fake sources (keep open)
echo   1. 01_start_mediamtx.bat
echo   2. 01_push_testpattern.bat
echo   3. 04_start_mqtt_broker.bat
echo   4. 04_start_mqtt_pub.bat
echo   5. 02_start_udp_send.bat
echo   DO NOT run 02/03/04 recv-or-sub windows
echo.
echo [B] App
echo   tools\pack_green.bat  (if pack is stale)
echo   pack\FieldReplay\fieldreplay.exe
echo.
echo [C] UI channels - start empty, then ADD:
echo   UDP   0.0.0.0:56001
echo   TCP   0.0.0.0:57002   (server listen)
echo   MQTT  127.0.0.1:1883, e2e/sensor
echo   VIDEO rtsp://127.0.0.1:8554/live/cam1  (optional)
echo   (no serial unless you have COM)
echo.
echo [D] Record
echo   1. engine connected
echo   2. start multi record
echo   3. THEN 03_start_tcp_send.bat
echo   4. watch STAT / LASER / SENSOR ~10-20s
echo   5. stop + export real .fcap
echo   6. say: E2E-5 OK + channels + packets + size
echo.
echo See README.md E2E-5 section for fail checks.
echo.
pause
endlocal
