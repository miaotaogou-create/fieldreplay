# FieldReplay 进度本

> **新会话请先读本文件**，再读 `docs/requirements/` 与 `docs/design/`。  
> 每次阶段性工作结束请更新「当前状态 / 下一步 / 决策记录」。

## 一句话目标

独立工具：现场顶替上位机 **录** 多通道原始数据；实验室当设备 **放** 回上位机，尽可能保真还原现场时序与载荷。单入口双击运行。

## 当前状态（2026-10-29）

| 项 | 状态 |
|----|------|
| 产品边界与 MVP 通道 | 已确认（见需求文档） |
| 架构选型 | 已确认：原生录放引擎 + 现有 React UI 壳 + 打包为单入口应用 |
| 引擎语言 | **Rust**（仅 std）；见决策 14 |
| 仓库骨架 / docs / 进度本 | 已就绪 |
| **`ui/` 界面** | U8 通过；引擎已连时录制/重演走控制面，离线仍可 mock；`npm run dev` 仅开发预览 |
| 视觉稿来源 | `C:\Users\49358\Downloads\fieldreplay---上位机现场数据录制与高保真重演工作台 (1)` |
| `engine/` 原生引擎 | **E5 已收口**；**E6a**：`serve --ui-dir` 同端口静态 UI + WS |
| 二进制 `.fcap` 规范 | **E0 已定稿**：`docs/design/fcap-format.md` |
| 二进制 `.fcap` 读写 / 导出 | **已落地**（写读 + `session.export` 回传二进制） |
| 打包单入口 | **进行中 E6**：E6a/E6b 完成；E6c 绿色目录打包待做 |
| `shell/` 薄壳 | **E6b 已落地**：wry WebView2；启动拉起引擎；`--smoke` 自检通过 |
| GitHub 开源仓 | 已建：https://github.com/miaotaogou-create/fieldreplay |
| **下一步** | **E6c** 绿色目录打包脚本（主 EXE + ui/dist + 引擎） |

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
| **缺口 G1** | 残留 | 重演页多处仍写「Qt 上位机」（对接说明/页脚已通用化；重演页未清） |
| **缺口 G2** | 已消 | `docs/design/fcap-format.md` 已定稿（E0） |
| **缺口 G3** | 残留 | 清空报文后重演页会话标题仍保留旧 `sessionTitle`（元数据未随空包复位） |
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
| E6 | Windows 单入口打包（普通客户端窗口，非浏览器） | [~] | 拆 E6a–E6d；壳选型见决策 15 |
| E6a | 引擎 `serve --ui-dir` 同端口托管 `ui/dist`；GET/WS 分流；自检 | [x] | 2026-10-29：`http_static`；CLI `--ui-dir`；self-check HTTP+WS 同端口通过 |
| E6b | 薄壳 WebView2 窗口加载本机 UI；启动时拉起/内嵌引擎 serve | [x] | 2026-10-29：`shell/` + wry/tao；拉起 `serve --ui-dir`；Job 连带回收；`cargo run -- --smoke` 通过 |
| E6c | 绿色目录打包脚本（主 EXE + ui/dist + 引擎） | [ ] ← **当前下一步** | |
| E6d | 双击手测验收 + README/进度本收口 E6 | [ ] | |
| E7 | （后置）视频通道 | [ ] | |

## 新会话接续提示（给协作者 / Agent）

1. 读 `PROGRESS.md` → 只做小任务板里 **第一个未勾选** 项（现在是 **E6c**）
2. 做完勾选、填完成记录、提交推送；不要顺手做下一格，除非用户当轮点名
3. 界面 U / 引擎 E5 / E6a / E6b 已收口；`.fcap` 见 `docs/design/fcap-format.md`；引擎语言 **Rust**（决策 14）；壳选型见决策 15
4. 工作目录：`C:\ZYL\workspace\projects\fieldreplay`
5. **开发预览**（非交付）：终端 A `cd engine && cargo run -- serve`；终端 B `cd ui && npm run dev`；或 `serve --ui-dir ../ui/dist` 后浏览器开 `http://127.0.0.1:17890/`（仍非交付）
6. 引擎自检：`cd engine && cargo test && cargo run -- self-check`
7. **壳冒烟**：先 `engine` build + `ui` `npm run build`；再 `cd shell && cargo run -- --smoke`；开窗口：`cargo run`（需 WebView2 Runtime）
8. **E6c**：绿色目录打包（主 `fieldreplay.exe` + 引擎 + `ui/dist`）；脚本收口，勿整仓 Tauri

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
