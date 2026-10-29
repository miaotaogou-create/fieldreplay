# FieldReplay

上位机现场数据录制与高保真重演工作台。

- **现场**：顶替上位机接收多通道数据并落盘  
- **实验室**：按时间轴将录制包灌回真实上位机  
- **交付**：Windows 单入口客户端（双击 EXE 出窗口，不靠系统浏览器）

当前：引擎 **E6a**（录放 + `serve` WS；可选 `--ui-dir` 托管静态 UI）。下一步 **E6b** 薄壳 WebView2 窗口，见 `PROGRESS.md`。

## 开发期预览（非交付形态）

```bat
REM 方式 1：Vite 热更新
REM 终端 A
cd engine
cargo run -- serve

REM 终端 B
cd ui
npm install --legacy-peer-deps
npm run dev
```

浏览器打开 `http://localhost:3000`。

```bat
REM 方式 2：引擎托管已构建 UI（更接近交付）
cd ui && npm run build
cd ..\engine
cargo run -- serve --ui-dir ..\ui\dist
```

浏览器打开 `http://127.0.0.1:17890/`。正式交付为双击 EXE 出本机窗口（E6b+），不经系统浏览器。

## 引擎

```bat
cd engine
cargo test
cargo run -- self-check
cargo run -- serve
cargo run --release -- record --bind 0.0.0.0:9000 --out session.fcap
cargo run --release -- replay --in session.fcap --target 127.0.0.1:9001
```

说明见 [`engine/README.md`](engine/README.md)。

## 文档

- 进度本：`PROGRESS.md`（新会话先读）
- 需求：`docs/requirements/20261029-mvp.md`
- 架构：`docs/design/architecture.md`
- 会话格式：`docs/design/fcap-format.md`

## 许可证

Apache-2.0（见 `LICENSE`）
