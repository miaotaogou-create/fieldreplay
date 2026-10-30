@echo off
chcp 65001 >nul
set "MTX=C:\ZYL\software\RTSP\mediamtx_v1.15.4_windows_amd64"
echo [E2E-1] 启动 MediaMTX  RTSP :8554
echo 保持本窗口打开。Ctrl+C 结束。
cd /d "%MTX%"
"%MTX%\mediamtx.exe" "%MTX%\mediamtx.yml"
pause
