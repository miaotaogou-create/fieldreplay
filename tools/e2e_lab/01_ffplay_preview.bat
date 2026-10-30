@echo off
setlocal
set "FFPLAY=C:\01_Install\bin\ffplay.exe"
set "URL=rtsp://127.0.0.1:8554/live/cam1"
echo [E2E-1] ffplay preview
echo URL=%URL%
echo Start MediaMTX + push first. Close this window to stop preview.
if not exist "%FFPLAY%" (
  echo ERROR: ffplay not found: %FFPLAY%
  pause
  exit /b 1
)
"%FFPLAY%" -rtsp_transport tcp -i "%URL%"
echo ffplay exited: %ERRORLEVEL%
pause
endlocal
