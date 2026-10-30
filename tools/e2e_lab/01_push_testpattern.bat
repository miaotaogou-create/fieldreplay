@echo off
setlocal
set "FFMPEG=C:\01_Install\bin\ffmpeg.exe"
set "URL=rtsp://127.0.0.1:8554/live/cam1"
echo [E2E-1] ffmpeg push test pattern
echo URL=%URL%
echo Start 01_start_mediamtx.bat first. Keep this window open.
if not exist "%FFMPEG%" (
  echo ERROR: ffmpeg not found: %FFMPEG%
  pause
  exit /b 1
)
"%FFMPEG%" -hide_banner -re -f lavfi -i testsrc=size=640x360:rate=15 -f lavfi -i sine=frequency=1000:sample_rate=44100 -c:v libx264 -pix_fmt yuv420p -preset ultrafast -tune zerolatency -c:a aac -f rtsp "%URL%"
echo ffmpeg exited: %ERRORLEVEL%
pause
endlocal
