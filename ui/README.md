# FieldReplay 界面预览

本目录是 FieldReplay 的 React 界面壳（Vite + Tailwind），用于本地预览录制 / 重演 / 报文三页；正式交付由薄壳 WebView2 加载（自定义标题栏）。

**E5a**：可连接本机引擎控制面，载入真实二进制 `.fcap`；未连引擎时仍可用「离线演示样本」与 JSON 演示包。

## 本地运行

前提：已安装 Node.js；另开终端跑引擎控制面。

```bat
REM 终端 A：引擎
cd engine
cargo run -- serve

REM 终端 B：界面
cd ui
npm install
npm run dev
```

浏览器打开终端提示的地址（默认 `http://localhost:3000`）。  
默认引擎地址 `ws://127.0.0.1:17890`，可用环境变量 `VITE_ENGINE_WS` 覆盖。

重演页「载入其他 .fcap 包」：选引擎录制的二进制 `.fcap`（magic=`FCAP`）时走引擎解析；旧 JSON 演示包仍可离线解析。

## 说明

- 无需 API Key 或云端密钥。
- 产品目标与验收见仓库根目录 `docs/requirements/`、`PROGRESS.md`。
- 录制/重放真实 IO 见进度本 E5b / E5c；单入口打包见 E6。
