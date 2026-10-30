@echo off
chcp 65001 >nul
set "FFMPEG=C:\01_Install\bin\ffmpeg.exe"
set "URL=rtsp://127.0.0.1:8554/live/cam1"
echo [E2E-1] ffmpeg 推测试图案到 %URL%
echo 请先已启动 01_start_mediamtx.bat。保持本窗口打开。
"%FFMPEG%" -hide_banner -re -f lavfi -i testsrc=size=640x360:rate=15 -f lavfi -i sine=frequency=1000:sample_rate=44100 -c:v libx264 -pix_fmt yuv420p -preset ultrafast -tune zerolatency -c:a aac -f rtsp "%URL%"
pause
