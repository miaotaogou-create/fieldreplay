# E2E 实验室夹具

配合 `docs/requirements/20261030-e2e-lab.md`。一步只测一项。

## E2E-1：RTSP 假设备

1. 双击或在本目录运行 `01_start_mediamtx.bat`（保持窗口开着）
2. 再开一个窗口运行 `01_push_testpattern.bat`（保持窗口开着）
3. 再开窗口运行 `01_ffplay_preview.bat`，应看到彩条/测试图案
4. 关掉预览即可；**先别关** MediaMTX 与推流窗口（后面录制还要用）

失败时：确认 `8554` 未被占用；ffmpeg 在 `C:\01_Install\bin\`。
