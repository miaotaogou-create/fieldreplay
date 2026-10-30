# FieldReplay 进度本

> **新会话请先读本文件**，再读 `docs/requirements/` 与 `docs/design/`。  
> 每次阶段性工作结束请更新「当前状态 / 下一步 / 决策记录」。

## 一句话目标

独立工具：现场顶替上位机 **录** 多通道原始数据；实验室当设备 **放** 回上位机，尽可能保真还原现场时序与载荷。单入口双击运行。

## 当前状态（2026-10-30）

| 项 | 状态 |
|----|------|
| 产品边界与 MVP 通道 | 已确认（见需求文档） |
| 架构选型 | 已确认：原生录放引擎 + 现有 React UI 壳 + 打包为单入口应用 |
| 引擎语言 | **Rust**（仅 std）；见决策 14 |
| 仓库骨架 / docs / 进度本 | 已就绪 |
| **`ui/` 界面** | 三 Tab（录制/重演/报文）；自定义标题栏；引擎已连走控制面，离线仍可 mock；`npm run dev` 仅开发预览 |
| 视觉稿来源 | `C:\Users\49358\Downloads\fieldreplay---上位机现场数据录制与高保真重演工作台 (1)` |
| `engine/` 原生引擎 | **E5 已收口**；`serve --ui-dir` 同端口静态 UI + WS |
| 二进制 `.fcap` 规范 | **E0 已定稿**：`docs/design/fcap-format.md` |
| 二进制 `.fcap` 读写 / 导出 | **已落地**（写读 + `session.export` 回传二进制） |
| 打包单入口 | **E6 已收口**（E6a–E6d） |
| `shell/` 薄壳 | **E6b 已落地**：wry WebView2；启动拉起引擎；`--smoke` 自检通过 |
| 绿色目录 | **E6c+E6d**：`tools/pack_green.bat` → `pack/FieldReplay/`；双击手测通过 |
| GitHub 开源仓 | 已建：https://github.com/miaotaogou-create/fieldreplay |
| **下一步** | **R1 已收口**；备选 **E15 / E16** 现场再选定 |
| IA / 标题栏 | **已改**：去掉「对接说明」Tab；壳无边框 + 自定义标题栏；去掉「x64 · 无边框窗体」 |
| 界面密度 | **已改**：`fr-page` 主区吃满窗口高度；监视/列表面板 `flex-1`；矮屏压间距；最大化优先无整页竖滚 |
| 启动控制台 | **已改**：壳 `windows_subsystem=windows`；双击不再弹黑框；失败 MessageBox；`--smoke` 仍打日志 |

## 已确认决策

1. **不做**透明代理 / 旁路嗅探；现场工具 **顶替上位机接收**。
2. **不做**业务协议深度解析（可后置插件）；先保证原始字节 + 时间戳。
3. MVP 通道：**UDP 多路 + 串口 + TCP**（服务端/客户端均可配）。
4. TCP：按应用层字节流 + 连接事件录放；重放时角色与录制时对调（工具演设备）。
5. **MQTT**：纳入产品范围（紧接 MVP 传输通道之后、视频之前）。录=订阅（顶替上位机），放=按时间轴发布（演设备/传感器）；记录 topic + payload + 时间戳，不解析业务 JSON。
6. **视频**后置。
7. UI：复用 `ui/`（React + Vite + Tailwind），**不** Qt 像素级重绘；热路径在原生引擎，UI 只看统计与预览窗口。
8. 交付：现场 **双击即用的单入口应用**（可为绿色目录 + 主 EXE，不强制磁盘上唯一文件）。
9. 开源：GitHub 公开仓库；文档中文为主。
10. **开发顺序（2026-10-29）**：**先完成界面**（对照 Downloads 原型），再引擎 / `.fcap` / 打包。
11. **交付形态**：最终是双击主程序进入录/放界面；`npm run dev` / 浏览器仅开发期。壳内嵌 UI（WebView / Tauri 等，E6 定），**不以「先开浏览器再连本地服务」为正式用法**。
12. **帧数据检查页**：保留为通用原始帧浏览器（筛选、HEX/ASCII、通道/Topic/时间戳）；**不做**业务协议智能解包。解包仅可后置可选插件。
13. **最终用户体验（再强调）**：交付物就是普通桌面客户端——**单入口 EXE，双击运行出本机窗口**；界面不通过系统浏览器查看。Vite/`localhost` 只给开发用。
14. **引擎语言**：Rust（`engine/`，E1 仅标准库）；后续壳可走 Tauri 等同生态，E6 再定。
15. **E6 壳选型**：不整仓迁 Tauri；引擎 `serve --ui-dir` 托管静态 UI + 既有 WS；薄壳 WebView2（wry）只开窗口。拆 E6a–E6d。

## 仓库布局

```
fieldreplay/
  PROGRESS.md              ← 进度本（接续入口）
  README.md
  LICENSE
  docs/requirements/       ← 需求与验收
  docs/design/             ← 架构与格式
  ui/                      ← React 界面
  engine/                  ← 原生录放引擎
  shell/                   ← 薄壳 WebView2（E6b；双击入口）
  tests/
```

## 小任务板（一次只做一格；做完勾选并更新「当前」）

> **纪律**：大需求先拆再写；每个新对话默认只接 **一个** 未勾选项。视觉对照：`Downloads\fieldreplay---上位机现场数据录制与高保真重演工作台 (1)`。

### 阶段 U — 界面（当前主线，引擎未开始前只做这里）

| ID | 任务 | 状态 | 完成记录 |
|----|------|------|----------|
| U0 | 同步 Downloads 原型 `(1)` 差异（MQTT 重演/报文/类型/mock）进 `ui/` | [x] | 2026-10-29 |
| U1 | 录制页：MQTT 通道展示 + 添加 + mock 发包；通道勾选可开关 | [x] | 2026-10-29 |
| U2 | 去掉 AI Studio 残留：卸 `@google/genai`、删无用 README 横幅/密钥说明；`ui/README.md` 改中文产品预览说明 | [x] | 2026-10-29：卸 genai/express/dotenv/motion；删 `.env.example`/`metadata.json`/未引用 aistudio 图；README 改中文预览；vite 去掉 DISABLE_HMR |
| U3 | 录制页对照原型：导出弹窗 / 空态 / 文案 / 筛选与滚动，缺什么补什么（本步只改 `FieldRecorderView`） | [x] | 2026-10-29：勾选防双触发；筛选空态与计数；滚动离底解锁；空包导出禁用；添加通道文案对齐原型 |
| U4 | 重演页对照原型：TCP/MQTT/UDP/串口目标区与播放条（本步只改 `DevReplayerView`） | [x] | 2026-10-29：空包禁用播放条；MQTT Topic 前缀/Broker 状态；TCP 客户端数；串口同步 virtualComPort；丢包进日志；帧检查跳转报文页 |
| U5 | 报文页对照原型：筛选（含 MQTT）、详情抽屉（本步只改 `PacketsView`） | [x] | 2026-10-29：Topic 进搜索/列表预览；详情 MQTT Topic·QoS·墙钟·方向；筛选计数/空态/清除；CSV 含 Topic 且空列表禁用 |
| U6 | 对接说明页对照原型（本步只改 `QtArchitectureGuide`）；产品文案去「仅 Qt」口吻若需通用化 | [x] | 2026-10-29：文案改「上位机通用 + Qt 示例」；挂载串口/UDP 与 `.fcap` 示意块（原型死代码补齐）；保留 Q1–Q3 结构 |
| U7 | 顶栏 / 四 Tab 切换 / Toast（本步只改 `Header`+`App`）对照原型 | [x] | 2026-10-29：Tab「对接说明」对齐需求 IA；aria-current；Toast 连点重置计时+role=status；页脚去仅 Qt 口吻 |
| U8 | 界面阶段验收：`npm run dev` 手测四页主路径；在进度本记「通过/缺口」清单 | [x] | 2026-10-29：**通过**（见下方 U8 清单）；`npm run lint` 通过 |

### U8 手测清单（2026-10-29，`npm run dev` @ localhost:3000）

对应验收：`docs/requirements/20261029-mvp.md`「界面」条（四 Tab IA、mock 可完成、主路径对齐）；引擎/真实 IO / 单入口打包 **不在本阶段范围**。

| 项 | 结果 | 说明 |
|----|------|------|
| 顶栏四 Tab 切换（录制 / 重演 / 报文 / 对接说明）+ `aria-current` | 通过 | 标签与需求 IA 一致 |
| Toast：载入样本 / 清空 / 添加通道 | 通过 | `role=status`，文案正确 |
| 录制：通道勾选、添加自定义（含类型切换）、开始/暂停/停止并导出弹窗 | 通过 | MQTT 通道可见；弹窗可取消 |
| 录制：空缓存空态；无包时导出入口不可用 | 通过 | 空态文案出现；导出/清空按钮不渲染（防空包） |
| 录制：监视筛选空态 / 计数文案 | 通过 | （本轮抽测清空与信道下拉含自定义口） |
| 重演：有包时可播；进度前进；暂停 | 通过 | 「高保真重演」→「暂停重演」；帧号推进 |
| 重演：TCP / MQTT / UDP / 串口目标区切换 | 通过 | MQTT Broker 状态与 Topic 前缀；UDP IP:port；串口虚拟对 |
| 重演：空会话播放条禁用 + 提示文案 | 通过 | 播放/步进/重置均 `disabled` |
| 重演 → 报文：「报文页查看」跳转 | 通过 | 切到「帧数据检查」 |
| 报文：MQTT 筛选、Topic 列表预览、详情 Topic/QoS/墙钟 | 通过 | |
| 报文：筛选无匹配空态；CSV 空列表禁用；清除筛选 | 通过 | |
| 对接说明：通用上位机口吻 + 串口/UDP / `.fcap` 示意块 | 通过 | |
| `npm run lint`（`tsc --noEmit`） | 通过 | |
| **缺口 G1** | 已消 | 样本/类型注释去「Qt 上位机」口吻；`hostComPort` 替代 `qtTargetPort` |
| **缺口 G2** | 已消 | `docs/design/fcap-format.md` 已定稿（E0） |
| **缺口 G3** | 已消 | 清空与新开引擎录制均复位会话元数据 |
| **缺口 G4** | 未测深 | 「载入其他 .fcap 包」真实文件选择器未做端到端（mock 阶段可后补） |
| **非本阶段** | — | 真实 UDP/串口/TCP/MQTT IO、二进制 `.fcap`、单入口打包 |

**结论**：界面阶段 **通过**，可进入阶段 E；G1/G3 为文案/状态抛光，不阻塞开引擎。

### 阶段 E — 引擎与格式（界面 U8 通过后再拆细做）

| ID | 任务 | 状态 | 完成记录 |
|----|------|------|----------|
| E0 | 定稿 `.fcap` v1 文档（`docs/design/fcap-format.md`） | [x] | 2026-10-29：小端；64B 头 + 通道表 + 可选 JSON meta + 变长帧；TCP 事件/MQTT topic 进 aux；业务协议不入库 |
| E1 | 引擎最小闭环：UDP 录 → 写盘 → 按时间轴重放 | [x] | 2026-10-29：Rust 引擎；`record`/`replay`/`self-check`；单元往返 + UDP 环回通过 |
| E2 | 串口录放 | [x] | 2026-10-29：`serial` Win32 实口 + 内存环回；`record`/`replay --port`；self-check 串口环回通过 |
| E3 | TCP 角色可配录放 | [x] | 2026-10-29：`tcp` std；server/client 录连接事件+RX；重放角色取反；CLI `--tcp-role`/`--tcp`；self-check 双向环回通过 |
| E4 | MQTT 订阅录 / 发布放 | [x] | 2026-10-29：`mqtt` 最小 3.1.1（std）；订阅录/发布放；CLI `--mqtt`/`--topic`/`--topic-prefix`；迷你 Broker 环回 self-check 通过；无账号/TLS |
| E5 | UI 去 mock，接本地 IPC/WebSocket（拆成 E5a–E5d） | [x] | 2026-10-29：E5a–E5d 全部完成 |
| E5a | 引擎 `serve`：本地 WebSocket 控制 API（ping/status/session.load\|open\|clear）；UI 可连上并载入真实 `.fcap` 进报文列表；去掉启动自动 mock 样本 | [x] | 2026-10-29：`control` 模块（std WS）；CLI `serve`；UI `engine/` 客户端；顶栏引擎状态；重演页二进制 `.fcap` 经引擎载入；启动不再自动 mock |
| E5b | 录制页去 mock 发包：经控制 API 启停真实 UDP/串口/TCP/MQTT 录制，预览窗收引擎推送 | [x] | 2026-10-29：`record.start/stop` + `record.packet` 推送；录制钩子；UI 引擎已连时走真实启停；离线仍可 mock；self-check UDP 环回通过 |
| E5c | 重演页去 mock 播放：经控制 API 载入后真实按时间轴重放（目标映射） | [x] | 2026-10-29：`replay.start/stop` + `replay.progress`/`done`；四通道可中断；UI 引擎已连时走真重放（1x，无循环/倍速）；self-check UDP 环回通过 |
| E5d | 导出写真实二进制 `.fcap`；mock 样本仅作引擎未连接时的离线演示；收口 E5 | [x] | 2026-10-29：`session.export` 回传 FCAP 字节；UI 已连→真二进制下载；未连→JSON 演示包；self-check 往返通过 |
| E6 | Windows 单入口打包（普通客户端窗口，非浏览器） | [x] | 2026-10-29：E6a–E6d 全部完成；见下方 E6d 清单 |
| E6a | 引擎 `serve --ui-dir` 同端口托管 `ui/dist`；GET/WS 分流；自检 | [x] | 2026-10-29：`http_static`；CLI `--ui-dir`；self-check HTTP+WS 同端口通过 |
| E6b | 薄壳 WebView2 窗口加载本机 UI；启动时拉起/内嵌引擎 serve | [x] | 2026-10-29：`shell/` + wry/tao；拉起 `serve --ui-dir`；Job 连带回收；`cargo run -- --smoke` 通过 |
| E6c | 绿色目录打包脚本（主 EXE + ui/dist + 引擎） | [x] | 2026-10-29：`tools/pack_green.ps1`（bat 入口）；产物 `pack/FieldReplay/`；脚本内 `--smoke` 通过 |
| E6d | 双击手测验收 + README/进度本收口 E6 | [x] | 2026-10-29：**通过**（见下方 E6d 清单）；README/需求/架构已收口 |
| E7 | 视频通道（拆 E7a–E7d） | [x] | 已收口；需求 `docs/requirements/20261030-video.md` |
| E7a | RTP/UDP 视频通道：录放 + `.fcap` video + 控制面/UI + self-check | [x] | 2026-10-30：复用 UDP 热路径；`media=rtp_udp`；不解编码；UI 可加视频通道；重演 UDP 目标区自动 `video_stream` |
| E7b | 视频 UI 抛光（重演页目标提示、筛选文案、手测清单） | [x] | 2026-10-30：报文筛选「视频 RTP」+ 详情提示；重演页视频目标区/默认 5004；录制文案去任务号；见下方 E7b 清单 |
| E7c | 最小 RTSP 客户端拉流（按块落盘，仍不解 NAL） | [x] | 已收口：`media=rtsp_tcp`；迷你服务端自检；控制面/UI/CLI |
| E7d | 最小 RTSP 服务侧重放 | [x] | 已收口：`mode=rtsp_listen`；PLAY 后按时间轴吐块；多路仍请用单路 |
| E8 | 引擎重放倍速（`replay.start.speed`） | [x] | 2026-10-30：时钟按倍速缩墙钟；四通道重放 + 控制面 + UI 倍速按钮接通；循环/暂停仍后置 |
| E9 | 引擎重放真实暂停（`replay.pause` / `resume`） | [x] | 2026-10-30：时钟冻结时间轴；四通道 + 控制面 + UI 暂停/继续；复位仍走 stop；循环/seek 仍后置 |
| E10 | 引擎重放 seek（`replay.seek` / `fromUs`） | [x] | 2026-10-30：回卷读头 + 校准时钟；播放/暂停中可拖进度条；启动可从当前头起播；循环仍后置 |
| E11 | 引擎重放故障注入（`replay.start.fault`） | [x] | 2026-10-30：丢包率 + 篡改载荷；四通道热路径；控制面/UI 接通；self-check 100% 丢包通过；循环仍后置 |
| E12 | 引擎重放循环（`replay.start.loop`） | [x] | 2026-10-30：EOF 回卷时间轴；四通道 + 控制面/UI 接通；self-check 越过单遍后 stop 通过 |
| E13 | 真实 IO 端到端验收（需求前 4 条勾选） | [x] | 2026-10-30：`io_accept`（UDP+串口会话、高速率 2000 帧、重放间隔）；TCP 沿用既有双向环回；串口=内存环回；录制侧 SO_RCVBUF |

### E6d 手测清单（2026-10-29，`pack\FieldReplay\fieldreplay.exe`）

对应验收：`docs/requirements/20261029-mvp.md`「Windows 下通过双击主程序进入录/放界面」；交付形态见架构「打包」。

| 项 | 结果 | 说明 |
|----|------|------|
| `fieldreplay.exe --smoke` | 通过 | 引擎路径/UI 目录解析正确；HTTP 探活 OK |
| 启动主程序 → 本机窗口 | 通过 | 可见窗口标题 `FieldReplay`；子进程 `fieldreplay-engine` 一并拉起 |
| 同端口静态 UI | 通过 | `http://127.0.0.1:17890/` HTTP 200；壳与引擎同目录布局 |
| WebSocket 控制面 | 通过 | `cmd:ping` / `cmd:status` 正常 |
| 顶栏四 Tab（录制 / 重演 / 报文 / 对接说明） | 通过 | 引擎已连文案：`开始引擎录制` / `引擎重演` |
| 空会话：重演播放条禁用 | 通过 | |
| 「离线演示样本」→ 报文有帧 / 重演可播 | 通过 | 会话标题与故障点可见 |
| README / 进度本 / 需求勾选收口 | 通过 | 本轮文档已更新 |
| **残留（不阻塞 E6）** | — | 故障注入后置；真实 IO 端到端勾选见需求前 4 条 |

### E7b 手测清单（2026-10-30，`ui` / 引擎已连可选）

对应：`docs/requirements/20261030-video.md` E7b 验收。

| 项 | 结果 | 说明 |
|----|------|------|
| 报文页筛选「视频 RTP」 | 通过 | 代码已加 Tab；可与 mock/真会话对照 |
| 报文详情：视频通道提示不解编码 | 通过 | violet 提示条 |
| 重演：视频会话自动切「视频 RTP」目标区 | 通过 | 默认推送口 5004（仅当仍为 8080 时改） |
| 录制：添加「视频 RTP」文案无内部任务号 | 通过 | 「RTP/UDP 监听地址（不解编码；RTSP 后置）」 |
| `npm run lint` | 通过 | `tsc --noEmit` |

### 阶段 X — 备选增强（当前主线：**无**；R1 已收口）

> 增强项编号 E8–E12 = 串口 / MQTT 凭据 TLS / pcap 导出 / 多通道 / 多路 sink（与上方早期「重放 E8–E13」编号不同，读全称）。  
> **选定轨道后**只做该轨道第一格；先需求文档再代码。

| 轨 | 首格 | 内容 | 状态 |
|----|------|------|------|
| A **E13** trim+marker | — | E13a–d 全部完成 | **已收口** |
| B **E14** pcap 导入 | — | E14a–d 全部完成 | **已收口** |
| C **E15** MQTT mTLS | E15a | 需求：客户端证书字段；仍默认不做 skip_verify | 待命 |
| D **E16** 录制暂停 | E16a | 需求：**拍板**相对戳冻结 vs 墙钟空洞 | 待命 |
| E **R1** 回归 | — | R1a 通过；R1b 无缺陷跳过 | **已收口** |

#### E13 会话 trim + marker

| ID | 任务 | 状态 | 完成记录 |
|----|------|------|----------|
| E13a | 需求入库（trim 时间窗、marker 时机、验收/非目标） | [x] | 2026-10-30：`docs/requirements/20261030-session-trim-marker.md`；闭区间再基、整表保留、非目标含 merge |
| E13b | 引擎：控制面插 `kind=marker` + 自检 | [x] | 2026-10-30：`session.addMarker`；录制热写 `LiveRecordSink` / 离线 `relativeUs`；自检离线+热插读回 aux |
| E13c | 引擎：时间窗 trim 写出新 `.fcap` + 自检 | [x] | 2026-10-30：`session.trim` 闭区间再基；整表通道；notes 追加；非法/空窗失败；自检多通道+marker |
| E13d | UI：插 marker；trim 导出/另存 | [x] | 2026-10-30：重演页 `SessionMarkerTrimPanel`；录制热插；报文列表可见 marker；`kind=3` 不再误标故障；未连引擎拒绝假写入 |

#### E14 pcap 导入

| ID | 任务 | 状态 | 完成记录 |
|----|------|------|----------|
| E14a | 需求入库（链路范围、跳过、往返、非目标） | [x] | 2026-10-30：`docs/requirements/20261030-pcap-import.md`；仅以太网经典 pcap + IPv4 UDP/TCP data；与 E10 往返保载荷与相对轴 |
| E14b | 引擎：pcap→fcap + 自检 | [x] | 2026-10-30：`pcap_bytes_to_session` / `self_check_pcap_import`；E10 往返载荷+相对轴；ARP skipped；全跳过/非以太网拒绝 |
| E14c | 控制面 import | [x] | 2026-10-30：`session.importPcap`（b64/path；默认替换会话；统计+FCAP b64）；CLI `import-pcap`；自检 WS 写读 |
| E14d | UI 载入入口 + 手测 | [x] | 2026-10-30：重演页「导入 .pcap」；`session.importPcap`；未连引擎拒绝假导入；`npm run build` 通过 |

#### E15 MQTT mTLS

| ID | 任务 | 状态 |
|----|------|------|
| E15a | 需求入库（证书路径/控制面 JSON） | [ ] |
| E15b | 引擎 mTLS 录放 | [ ] |
| E15c | 控制面 + UI（单路/多路） | [ ] |
| E15d | mTLS 自检夹具 | [ ] |

#### E16 录制暂停

| ID | 任务 | 状态 |
|----|------|------|
| E16a | 需求入库（相对戳策略 + 多路语义） | [ ] |
| E16b | 引擎 `record.pause`/`resume` | [ ] |
| E16c | 控制面 + UI | [ ] |
| E16d | 自检对齐 E16a | [ ] |

#### R1 回归

| ID | 任务 | 状态 | 完成记录 |
|----|------|------|----------|
| R1a | 手测 E12d：UDP+TCP / UDP+串口 / UDP+MQTT | [x] | 2026-10-30：`cargo run -- self-check` 全过（含三组多路对齐）；`multiReplayTargets.selfcheck` ok；UI 表单项/跳过文案静态核对无「多路暂仅 UDP」；无产品缺陷 |
| R1b | 按缺陷修并回写进度本 | [x] | 2026-10-30：无缺陷，跳过。残留风险：控制面 WS 自检未跑 multi `targets[]`（库级已覆盖）；实口对 `FIELDREPLAY_SERIAL_PAIR` 未设故跳过 |

## 新会话接续提示（给协作者 / Agent）

1. 读 `PROGRESS.md` → **无默认主线**（R1 已收口）；先问用户在 **E15 / E16** 中选定一条，再开需求/改代码
2. 选定后只做该轨道 **第一个未勾选** 项；做完勾选、填完成记录、提交推送；不要顺手做下一格，除非用户当轮点名
3. 界面 U / 引擎 E5 / **打包 E6** / 重放增强与 **E7 视频** / 增强项 **E8–E14** / **R1** 已收口；`.fcap` 见 `docs/design/fcap-format.md`；引擎语言 **Rust**（决策 14）；壳选型见决策 15
4. 工作目录：`C:\ZYL\workspace\projects\fieldreplay`
5. **交付用法**：`tools\pack_green.bat` → 双击 `pack\FieldReplay\fieldreplay.exe`（需 WebView2 Runtime）
6. **开发预览**（非交付）：终端 A `cd engine && cargo run -- serve`；终端 B `cd ui && npm run dev`；或 `serve --ui-dir ../ui/dist` 后浏览器开 `http://127.0.0.1:17890/`（仍非交付）
7. 引擎自检：`cd engine && cargo test && cargo run -- self-check`
8. **壳冒烟**：先 `engine` build + `ui` `npm run build`；再 `cd shell && cargo run -- --smoke`；开窗口：`cargo run`（需 WebView2 Runtime）
9. **绿色打包**：`tools\pack_green.bat` → `pack\FieldReplay\`（含 `--smoke`）

## 决策记录（追加）

| 日期 | 决策 | 原因 |
|------|------|------|
| 2026-10-29 | 原生引擎 + React 壳，而非纯 Qt UI 或纯浏览器前后端 | 保真靠引擎；界面复用现稿；交付单入口 |
| 2026-10-29 | TCP 进 MVP；视频后置 | 激光器等设备现场常见 TCP，且上位机 server/client 都有 |
| 2026-10-29 | MQTT 纳入范围（MVP 之后、视频之前） | 现场可能收 MQTT；与「顶替订阅 / 重放发布」模型一致 |
| 2026-10-29 | 录制热路径不进 UI | 高速率下避免丢时序/卡顿 |
| 2026-10-29 | **先完成界面，再做引擎** | 用户明确；视觉稿以 Downloads 原型 `(1)` 为准 |
| 2026-10-29 | 保留「帧数据检查」，去掉协议字段智能解包 UI；时间轴故障点改菱形 | 通用工具不绑业务协议；竖条故障标记易被看成暂停 |
| 2026-10-29 | 正式交付为单入口应用，不以浏览器为最终用法 | 架构已写；用户再确认；开发期可用 Vite 预览 |
| 2026-10-29 | 定稿 `.fcap` v1（E0） | 引擎读写契约；解 G2；对接说明示意与文档对齐 |
| 2026-10-29 | 再强调：单 EXE 双击 = 普通客户端窗口，不经系统浏览器 | 用户新会话明确最终目标 |
| 2026-10-29 | **引擎语言选 Rust**（仅标准库做 E1；无 Tokio/额外 crate） | 本机已有 cargo；热路径原生；利于后续 Tauri/单入口；Winsock 经 std::net 足够 UDP |
| 2026-10-29 | E2 串口仍仅标准库：Windows 手写 Win32 FFI；自检用内存环回 | 不引入 serialport 等 crate；无虚口时仍可验录放路径；实口手测靠 COM/虚拟对 |
| 2026-10-29 | E3 TCP 仍仅标准库：`TcpListener`/`TcpStream`；重放角色相对录制 config 取反 | 与架构/格式文档一致；自检覆盖 server 录→client 放与 client 录→server 放 |
| 2026-10-29 | E4 MQTT 仍仅标准库：手写最小 3.1.1；自检嵌迷你 Broker；账号/TLS 后置 | 与决策「先原始字节」一致；待确认项不阻塞录放闭环 |
| 2026-10-29 | E5 拆为 E5a–E5d；控制面用本机 WebSocket + 手写 JSON（仍仅 std）；E6 再定 Tauri/IPC | 浏览器开发预览只能走 WS/HTTP；热路径仍不经 UI |
| 2026-10-29 | E5b：同时刻一路录制；推送无 `id` 的 `record.packet`；暂停仅离线 mock | 与单通道会话模型一致；真实暂停后置 |
| 2026-10-29 | E5c：同时刻一路重放；与录制互斥；`replay.progress`/`done`；真实暂停/倍速/循环后置 | 对齐 CLI 时间轴；UI 离线仍可 mock 倍速与故障注入 |
| 2026-10-29 | E5d：`session.export` 回传二进制 b64；已连引擎禁止伪装 JSON 为正式 `.fcap`；离线仍可 JSON 演示 | 与载入侧对称；浏览器无法直读引擎 temp 路径 |
| 2026-10-29 | E6 壳：引擎托管静态 UI + 薄壳 WebView2；不整仓 Tauri；拆 E6a–E6d | 保留 std 引擎与 WS；壳只负责窗口；E6a 先通静态托管 |
| 2026-10-29 | E6a：`serve --ui-dir`；同端口 GET 静态 / WS 升级分流 | 交付壳与开发期均可加载同一 UI 构建产物 |
| 2026-10-29 | E6b：独立 `shell/` crate（wry+tao）；子进程引擎 + Job 回收；`--smoke` 不开窗自检 | 双击出本机窗口；热路径仍在引擎 |
| 2026-10-29 | E6c：`tools/pack_green` 组装旁路布局（主 EXE + 引擎 + ui/dist）；产物不进 git | 与 paths.rs 默认查找一致；交付可拷整目录 |
| 2026-10-29 | E6d：绿色包双击手测通过；E6 收口 | 窗口标题 FieldReplay；引擎+UI/WS 同端口；需求「双击进界面」勾选 |
| 2026-10-29 | 去掉「对接说明」Tab；壳无系统边框，UI 自定义标题栏经 IPC 控窗 | 上位机只是重演接收方，不必单独 Tab；窗体对齐 Downloads 原型 |
| 2026-10-29 | 去掉标题栏「x64 · 无边框窗体」；三页主区与页头页脚整体收紧 | 全屏仍出现纵向滚动条；中间区块 padding/说明文案过大 |
| 2026-10-29 | 壳 GUI 子系统 + 主区 flex 自适应窗口高度 | 双击弹控制台不友好；卡片固定高度导致最大化仍整页竖滚 |
| 2026-10-30 | E8：`replay.start` 支持 `speed`；UI 倍速按钮对引擎生效 | 需求写了倍速增强；原先引擎固定 1x |
| 2026-10-30 | 收口 G1/G3（去 Qt 口吻；清空/新录复位会话标题） | U8 残留抛光 |
| 2026-10-30 | E9：`replay.pause`/`resume`；暂停期间墙钟不计入时间轴；UI 接通 | 进度本「真实暂停」；复位仍 `replay.stop` |
| 2026-10-30 | E10：`replay.seek` + `replay.start.fromUs`；进度条/故障点跳转接通引擎 | 播放中可 scrub；TCP seek 会重跑连接事件 |
| 2026-10-30 | E11：`replay.start.fault`（丢包率 + 篡改载荷）；UI 启用后随引擎重放生效 | 热路径扰动原始字节；不解析业务 CRC；循环仍后置 |
| 2026-10-30 | E12：`replay.start.loop`；EOF 回卷；UI 循环开关接通引擎 | 空遍不空转；`loop:true` 仅 stop 才 `replay.done` |
| 2026-10-30 | E13：`io_accept` 挂入 `self-check`；需求 MVP 前 4 条勾选；UDP 录制 SO_RCVBUF | 高速率写盘/间隔重放可自动验；串口仍内存环回；实口/COM 不阻塞勾选 |
| 2026-10-30 | E7 拆 E7a–E7d；E7a=RTP/UDP 原始块（不解编码、不做 RTSP） | 保真原则优先；完整 RTSP 工作量大且易破 std；先交付可验收的视频通道类型 |
| 2026-10-30 | E7a 收口：`record`/`replay` video + 控制面 `video`\|`video_stream` + UI | self-check 环回；重演页沿用 UDP 目标映射 |
| 2026-10-30 | E7b 收口：视频 UI 抛光（筛选/重演目标提示/录制文案）+ 手测清单 | 不改引擎热路径；RTSP 仍 E7c/d |
| 2026-10-30 | 增强项 E10：经典 `.pcap` 导出（合成以太网；UDP/视频/TCP data） | 与表内「E10 seek」编号无关；主交付仍为 `.fcap`；见 `20261030-pcap-export.md` |
| 2026-10-30 | 增强项 E11a–d：多通道并发录制 + UI 一键录 + 控制面/UI 多目标对齐重放 | 见 `20261030-multi-channel.md` |
| 2026-10-30 | 增强项 E12a–d：多路 TCP/串口/MQTT sink + UI 放开 capable | 见 `20261030-multi-sink.md`；多路 RTSP listen 仍单路 |
| 2026-10-30 | E12 后无默认主线；备选拆为 E13 trim+marker / E14 pcap 导入 / E15 MQTT mTLS / E16 录制暂停 / R1 回归 | 现场再挑一条；先需求文档再代码；编号续增强项序列，勿与早期重放 E8–E13 混读 |
| 2026-10-30 | **选定 E13** 为下一主线；E13a 需求入库 | 格式已有 marker；现场裁剪/标注价值高；mTLS/录暂停分别缺场景与未拍板戳策略；pcap 导入更大；R1 有引擎自检不挡增强 |
| 2026-10-30 | E13 全轨收口（含 UI） | 见 `20261030-session-trim-marker.md`；下一主线再次现场选定 |
| 2026-10-30 | **选定 E14** pcap 导入；E14a 需求入库 | 闭环 E10；仅以太网经典 pcap + IPv4 UDP/TCP data；见 `20261030-pcap-import.md` |
| 2026-10-30 | E14 全轨收口（含 UI「导入 .pcap」） | 见 `20261030-pcap-import.md`；下一主线再次现场选定（E15/E16/R1） |
| 2026-10-30 | **选定 R1** 为下一主线（先于 E15/E16） | E11–E14 连堆后先回归；E15 缺客户 mTLS 场景；E16 相对戳策略未拍板 |
| 2026-10-30 | R1 收口（R1a 通过 / R1b 无缺陷跳过） | self-check + UI targets 自检 + 文案核对；下一主线再次现场选定（E15/E16） |
