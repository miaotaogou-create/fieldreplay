@echo off
chcp 65001 >nul
set "FFPLAY=C:\01_Install\bin\ffplay.exe"
set "URL=rtsp://127.0.0.1:8554/live/cam1"
echo [E2E-1] ffplay 预览 %URL%
echo 请先已启动 MediaMTX 与推流。关闭本窗口即停预览。
"%FFPLAY%" -rtsp_transport tcp -i "%URL%"
pause
