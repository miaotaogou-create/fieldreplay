# FieldReplay 薄壳

Windows WebView2 窗口：启动时拉起 `fieldreplay-engine serve --ui-dir`，加载本机 `http://127.0.0.1:17890/`。  
不把录放热路径迁入 Tauri；壳只做窗口。

## 开发构建

先备好引擎与 UI 产物：

```bat
cd ..\engine
cargo build
cd ..\ui
npm run build
cd ..\shell
cargo build
cargo run -- --smoke
cargo run
```

可执行文件：`target/debug/fieldreplay.exe`（或 `release`）。

## 参数

| 参数 | 说明 |
|------|------|
| `--engine` | 引擎 EXE；默认同目录 / 仓库 `engine/target/{debug\|release}` |
| `--ui-dir` | 静态 UI；默认 `ui/dist` |
| `--bind` | 控制面地址，默认 `127.0.0.1:17890` |
| `--smoke` | 不开窗口，仅确认引擎托管 UI 可 HTTP 访问 |

环境变量：`FIELDREPLAY_ENGINE`、`FIELDREPLAY_UI_DIR`。

## 依赖

本机需已安装 [WebView2 Runtime](https://developer.microsoft.com/microsoft-edge/webview2/)（Win10/11 通常已有）。
