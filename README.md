# FieldReplay

上位机现场数据录制与高保真重演工作台。

- **现场**：顶替上位机接收多通道数据并落盘  
- **实验室**：按时间轴将录制包灌回真实上位机  
- **交付**：Windows 单入口客户端（双击 EXE 出窗口，不靠系统浏览器）

当前：界面可开发预览；引擎与 `.fcap` 读写、单 EXE 打包按进度本阶段 E 推进。

## 开发期预览界面（非交付形态）

```bat
cd ui
npm install --legacy-peer-deps
npm run dev
```

浏览器打开终端提示的本地地址（默认 `http://localhost:3000`）。正式交付为内嵌 UI 的桌面客户端，见 `PROGRESS.md` 决策与 E6。

## 文档

- 进度本：`PROGRESS.md`（新会话先读）
- 需求：`docs/requirements/20261029-mvp.md`
- 架构：`docs/design/architecture.md`
- 会话格式：`docs/design/fcap-format.md`

## 许可证

Apache-2.0（见 `LICENSE`）
