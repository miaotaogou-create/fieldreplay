@echo off
setlocal
cd /d "%~dp0"
echo ========================================
echo  E2E-7 FieldReplay replay -^> fake host
echo ========================================
echo.
echo Prereq: E2E-5 .fcap (example Downloads\*_9530.fcap)
echo Host ports (same as E2E-6):
echo   UDP  127.0.0.1:56101
echo   TCP  127.0.0.1:58102  (host listens; replay is client)
echo   MQTT 127.0.0.1:1883 topic e2e/#
echo.
echo [A] Hosts first (keep open; use long window)
echo   1. 04_start_mqtt_broker.bat
echo   2. 06_run_udp_host.bat     (or 07_run_aligned_replay.ps1)
echo   3. 06_run_tcp_host.bat
echo   4. 06_run_mqtt_host_sub.bat  (raise --seconds if session ~46s @1x)
echo.
echo [B] App
echo   pack\FieldReplay\fieldreplay.exe
echo   Replay page: open the E2E-5 .fcap
echo.
echo [C] Multi-target remap (defaults are NOT lab ports)
echo   UDP  ch -^> 127.0.0.1:56101
echo   TCP  ch -^> 127.0.0.1:58102
echo   MQTT ch -^> broker 127.0.0.1:1883  (topic from session / empty prefix)
echo   Video optional; skip for host text check
echo.
echo [D] Start replay (1x or higher). Expect host windows:
echo   STAT,... / LASER,... / SENSOR,...
echo.
echo Automated (no UI): 07_run_aligned_replay.ps1
echo Say: E2E-7 OK when all three show readable lines.
echo.
pause
endlocal
