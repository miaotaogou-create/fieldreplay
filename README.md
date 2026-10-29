# FieldReplay

上位机现场数据录制与高保真重演工作台。

- **现场**：顶替上位机接收多通道数据并落盘  
- **实验室**：按时间轴将录制包灌回真实上位机  

当前优先完成 `ui/` 界面（对照 Downloads 原型）；原生录放引擎随后。

## 快速预览界面

```bat
cd ui
npm install --legacy-peer-deps
npm run dev
```

浏览器打开终端提示的本地地址（默认 `http://localhost:3000`）。

## 文档

- 进度本：`PROGRESS.md`（新会话先读）
- 需求：`docs/requirements/20261029-mvp.md`
- 架构：`docs/design/architecture.md`

## 许可证

Apache-2.0（见 `LICENSE`）
