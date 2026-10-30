@echo off
setlocal
set "MTX=C:\ZYL\software\RTSP\mediamtx_v1.15.4_windows_amd64"
echo [E2E-1] start MediaMTX on :8554
echo Keep this window open. Ctrl+C to stop.
if not exist "%MTX%\mediamtx.exe" (
  echo ERROR: mediamtx not found: %MTX%\mediamtx.exe
  pause
  exit /b 1
)
cd /d "%MTX%"
"%MTX%\mediamtx.exe" "%MTX%\mediamtx.yml"
echo mediamtx exited: %ERRORLEVEL%
pause
endlocal
