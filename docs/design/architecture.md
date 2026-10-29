# FieldReplay 架构说明

- 状态：已确认（原则）；实现细节可随引擎选型微调
- 日期：2026-10-29

## 总览

```
┌─────────────────────────────────────────┐
│  单入口应用（Windows 主程序）              │
│  ┌─────────────┐    IPC/WS/本地 RPC     │
│  │  UI 壳       │◄────────────────────►│
│  │  (ui/ React) │    统计·控制·预览      │
│  └─────────────┘                        │
│  ┌─────────────────────────────────────┐│
│  │  engine/ 原生录放引擎（热路径）        ││
│  │  通道 IO · 时间戳 · FCAP 写读 · 调度  ││
│  └─────────────────────────────────────┘│
└─────────────────────────────────────────┘
         ▲ 录：设备指向本机
         ▼ 放：灌向上位机 / 虚拟串口
```

## 引擎实现语言

- **已定（2026-10-29）**：**Rust**，代码在 `engine/`（crate `fieldreplay-engine`）。
- E1 仅用标准库（`std::net` UDP + 文件 IO）；后续通道可按需加最小依赖。
- 与 UI 的 IPC/壳嵌入方式仍在 **E6** 敲定（Tauri / WebView2 等）。

## 原则

1. **热路径在引擎**：收包 → 打戳 → 写盘（或重放发送）不经过 UI 线程/JS。
2. **UI 抽样**：速率、计数、最近 N 条预览；全量数据在文件中。
3. **双模式同一引擎**：RecordSession / ReplaySession，通道配置可序列化进会话元数据。
4. **与业务解耦**：不解析业务帧；只认识通道类型与传输角色。

## 模块划分（逻辑）

| 模块 | 职责 |
|------|------|
| Channel | UDP / Serial / TCP / MQTT 的打开、读写、关闭；TCP 含 role；MQTT 含 broker/topic/QoS |
| Clock | 录制起点、相对微秒时间戳 |
| SessionWriter / Reader | `.fcap`（或确定后的格式）顺序写、顺序/索引读 |
| Scheduler | 重放按 `relative_us` 发事件 |
| Control API | 本机 WebSocket（默认 `127.0.0.1:17890`）；供 UI 调用 |
| UI | `ui/` 现有页面，经控制面载入会话 / 启停录制 |

## TCP 角色

| 场景 | 工具角色 |
|------|----------|
| 录制且上位机原为 server | 工具 listen |
| 录制且上位机原为 client | 工具 connect |
| 重放 | 自动（或显式）取反，演设备侧 |

录制事件至少包括：`connect` / `disconnect` / `data`（带 direction）。

## MQTT 角色

| 场景 | 工具角色 |
|------|----------|
| 录制 | 连接 Broker，**订阅**上位机原订阅的 topic（顶替上位机） |
| 重放 | 连接 Broker（或实验室 Broker），按时间轴 **发布** 已录 payload（演设备/传感器） |

落盘至少包括：`relative_us`、`topic`、`payload` 原始字节；QoS/retain 作为元数据可选保存。不解析 payload 业务含义。

## 控制面命令（E5）

开发期：`cargo run -- serve`；UI 经 `ws://127.0.0.1:17890` 连接。

| cmd | 作用 |
|-----|------|
| `ping` / `status` | 探活；`status.state` 为 `idle` \| `recording` \| `replaying` |
| `session.open` / `session.load` / `session.clear` | 载入/清空内存会话（E5a）；录制/重放中拒绝 |
| `record.start` | 启真实通道录制：`type` + `config`（与 `.fcap` config_json 对齐）；可选 `out` / `name` / `sessionTitle`。同时刻一路。预览帧推送 `{"event":"record.packet","packet":{...}}`（无 `id`） |
| `record.stop` | 停止并 finalize；会话写入控制面内存；响应含 `packetCount` / `out` |
| `replay.start` | 对已载入会话按时间轴重放：`type` + `config`（UDP `target_ip`/`target_port`；串口 `port_name`+波特率；TCP 重放侧 `host`/`port`，角色由文件取反；MQTT `broker`/`port` + 可选 `topicPrefix`）。同时刻一路，与录制互斥。推送 `replay.progress`（`sent`/`relativeUs`/`durationUs`）与结束时 `replay.done` |
| `replay.stop` | 中止重放；响应含 `sent`。自然播完也会推 `replay.done` 并自动回收槽位 |

导出二进制收口属 E5d。真实暂停 / 倍速 / seek / 故障注入仍后置（开发预览页离线 mock 可继续演示）。

## 会话文件

二进制契约见 [`fcap-format.md`](./fcap-format.md)（`.fcap` v1）。

## 打包（最终交付）

- **体验**：普通桌面客户端——双击主程序（单入口 EXE）出现本机窗口；**不**打开系统浏览器看界面。
- 允许：绿色目录内含主 EXE、引擎、内嵌 UI 资源、依赖 DLL（不强制磁盘上物理「唯一一个文件」，但用户感知是「点一个程序就开」）。
- 壳选型（WebView2 / Tauri 等）在 **E6** 敲定；开发期可用 `ui` 的 Vite 预览，**不算交付形态**。
- 避免：要求用户先开浏览器再连本地/远程服务作为正式用法。

## 非目标架构

- 浏览器 ↔ 远程云后端作为现场主路径
- 以「启动浏览器访问 localhost」作为交付给现场的用法
- 用 tcpreplay 重放原始以太网帧作为 TCP 主方案
