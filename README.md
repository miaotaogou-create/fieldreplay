# FieldReplay

上位机现场数据录制与高保真重演工作台。

- **现场**：顶替上位机接收多通道数据并落盘  
- **实验室**：按时间轴将录制包灌回真实上位机  
- **交付**：Windows 单入口客户端（双击 EXE 出窗口，不靠系统浏览器）

当前：**E6 / E8–E12（倍速 + 暂停 + seek + 故障注入 + 循环）已收口**——绿色目录双击即可用。下一项见 `PROGRESS.md`（E7 视频后置；真实 IO 端到端验收等）。

## 绿色目录打包（交付形态）

```bat
tools\pack_green.bat
```

产物：`pack\FieldReplay\`（主 `fieldreplay.exe` + `fieldreplay-engine.exe` + `ui\dist`）。脚本末尾会跑 `--smoke`。

**用法**：保持该目录完整，双击 `fieldreplay.exe` 出本机窗口（需 WebView2 Runtime）。自检不开窗：`fieldreplay.exe --smoke`。

## 本机窗口（开发期）

```bat
cd engine && cargo build
cd ..\ui && npm run build
cd ..\shell
cargo run -- --smoke
cargo run
```

双击/运行 `shell\target\debug\fieldreplay.exe` 出本机窗口（需 WebView2 Runtime）。`--smoke` 只探活、不开窗。

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
REM 方式 2：引擎托管已构建 UI（无壳）
cd ui && npm run build
cd ..\engine
cargo run -- serve --ui-dir ..\ui\dist
```

浏览器打开 `http://127.0.0.1:17890/`。正式交付为双击壳 EXE 出本机窗口，不经系统浏览器。

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
