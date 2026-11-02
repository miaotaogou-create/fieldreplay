@echo off
setlocal
cd /d "%~dp0"
echo ========================================
echo  E2E-6 fake-host receive checklist
echo ========================================
echo.
echo Ports (NOT record-side 56001/58002):
echo   UDP host  127.0.0.1:56101
echo   TCP host  127.0.0.1:58102
echo   MQTT      127.0.0.1:1883 topic e2e/#
echo.
echo [1] UDP
echo   A. 06_run_udp_host.bat
echo   B. 06_start_udp_probe.bat
echo   Expect PASS + STAT text/hex
echo.
echo [2] TCP  (listen first)
echo   A. 06_run_tcp_host.bat
echo   B. 06_start_tcp_probe.bat
echo   Expect PASS + LASER text/hex
echo.
echo [3] MQTT
echo   A. 04_start_mqtt_broker.bat
echo   B. 04_start_mqtt_pub.bat
echo   C. 06_run_mqtt_host_sub.bat
echo   Expect several SENSOR lines + PASS
echo.
echo Say: E2E-6 OK when all three PASS.
echo E2E-7 will point FieldReplay replay targets at these host ports.
echo.
pause
endlocal
