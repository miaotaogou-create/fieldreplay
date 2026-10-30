# pcap 导入（E14）

- 状态：已确认（分阶段推进）
- 日期：2026-10-30
- 依据：E10 非目标「导入 `.pcap` 回灌 `.fcap`」；D-008「导入另立项」；进度本轨道 B；DECISIONS 待确认「仅 UDP/TCP data 还是尽力映射更多链路层」

## 背景

同事常用 Wireshark / tcpdump 抓到经典 `.pcap`；现场主交付与高保真重放仍靠 `.fcap`。  
E10 已能 **fcap → pcap**（合成以太网，仅 UDP/视频/TCP data）。本轨补 **pcap → fcap**：把「只有网口抓包」的材料灌进本工具时间轴，便于按通道配目标重放。  
主交付仍是 `.fcap`；导入是增强入口，不是替代格式。

## 目标模型

| 阶段 | ID | 内容 |
|------|-----|------|
| E14a | 本文件 | 链路范围、跳过、通道映射、与 E10 往返期望、非目标 |
| E14b | 引擎 | pcap 字节 → 会话（通道表 + 帧）+ 自检 |
| E14c | 控制面 | `session.importPcap`（或等价）+ CLI |
| E14d | UI | 载入入口 + 手测 |

## 用户故事

1. 作为研发人员，我希望把现场同事给的经典 `.pcap` 转成 `.fcap`，以便在 FieldReplay 里按时间轴重放 UDP/TCP 载荷。
2. 作为已用 E10 导出 pcap 的用户，我希望「本工具导出的 pcap 再导入」能找回 UDP/TCP **数据载荷与相对时序**，以便闭环自检与协作。
3. 作为操作者，我希望导入结果里能看到跳过了多少帧、建成了几路通道，失败时有可读原因（魔数不对、无可导入帧等）。

## 产品约定（已确认）

### 输入格式

| 项 | 约定 |
|----|------|
| 支持 | **经典 libpcap**；魔数小端微秒 `0xa1b2c3d4` 与纳秒 `0xa1b23c4d`（时间统一落到微秒相对轴；纳秒截断到 µs） |
| 链路类型 | **`LINKTYPE_ETHERNET`（1）** 为主；可选 **802.1Q VLAN**：剥一层 tag 后再当以太网解析 |
| 不支持（失败或整文件拒绝） | **pcapng**；其它 DLT（含 Linux SLL、Raw IP 等）本阶段 **整文件拒绝** 并提示「仅支持以太网经典 pcap」 |
| 字节序 | 按文件头 `magic` 判定；与 E10 写出小端一致时可直接读 |

> **链路范围拍板（回应待确认）**：本阶段 **仅** 以太网经典 pcap 上的 **IPv4 + UDP/TCP data**；不做「尽力映射更多链路层」。

### 帧级解析与跳过

| 纳入 | 条件 |
|------|------|
| UDP data | IPv4 + UDP；`payload` = UDP 载荷（可为空载荷：仍可计入或跳过，见下） |
| TCP data | IPv4 + TCP；**仅** `payload_len > 0` 的段；`kind=data` |

| 跳过（计入 `skipped`，不失败） | 说明 |
|--------------------------------|------|
| 非 IPv4 | ARP / IPv6 / 其它 Ethertype |
| ICMP 等非 UDP/TCP | |
| TCP 纯 ACK / 无载荷段 | 不合成 `tcp_connect` / `tcp_disconnect` |
| 截断帧（`incl_len` 不足完整 L3/L4 头） | |
| IP 分片（MF 置位或 offset≠0） | 本阶段不重组 |
| 无法解析的畸形帧 | |

空 UDP 载荷：默认 **跳过**（与「无可重放字节」一致）；若实现选择保留须在统计中可区分，本需求验收以「有载荷帧」为准。

### 通道映射

| 项 | 约定 |
|----|------|
| 通道类型 | UDP 流 → `udp`；TCP 流 → `tcp`；**不**自动标 `video`（RTP 仍当普通 UDP；用户可另用真 `.fcap` 视频通道） |
| 流键 | `(ip_proto, min(src,dst) 端点对的规范化五元组)`：同一双向对话归一路；`channel_index` 按首次出现顺序从 0 递增 |
| `config_json` | UDP：`bind_ip`/`bind_port` = 该流「本机侧」；TCP：`host`/`port` = 本机侧；本机侧规则见下 |
| 本机侧 | **该流首包的目的 IPv4:port** 视为本机（监听/服务侧启发式）；源为对端 |
| 帧 `aux` | 对端 `"ip:port"`（UTF-8），与 E10 导出习惯对齐，便于再导出 |
| `direction` | 目的=本机 → `RX(0)`；源=本机 → `TX(1)` |
| 串口 / MQTT | pcap 中不存在；导入结果 **无** 这两类通道 |

### 时间轴与会话元数据

| 项 | 约定 |
|----|------|
| 相对戳 | 首个**纳入**帧的 pcap 时间戳为 0；`relative_us = ts - t0`（同一 µs 允许多帧，保持文件顺序） |
| 墙钟 | `start_wall_unix_us` = 首个纳入帧的绝对时间（若 pcap 时间为 0 起相对抓包，则墙钟可为该绝对值；全 0 则墙钟写 0） |
| SessionMeta | `session_title` 默认含来源提示（如文件名或 `imported from pcap`）；`notes` 追加一行导入摘要：`imported N written, M skipped, C channels` |
| 通道名 | 默认可生成 `udp_<port>` / `tcp_<port>` 或 `ch0`…；不要求用户命名 |

### 与 E10 往返期望

| 期望 | 不期望 |
|------|--------|
| **本工具 E10 导出的** UDP/TCP data pcap → 再导入：纳入帧的 **payload 字节一致**；相对间隔与导出所用 `start_wall+relative` 一致（允许 µs 量化） | 通道数 / `channel_index` / 通道名与导出前 `.fcap` **逐项相同**（导出已丢串口/MQTT/marker/连接事件；导入按流重建） |
| `written` ≥ 1 时得到可 `session.export` 的合法 `.fcap` | 视频通道类型自动恢复为 `video` |
| 导入后再 `exportPcap`：UDP/TCP 载荷仍可被 Wireshark 看见 | 合成 MAC / 校验和 / TCP seq 与导出前 bit 级一致 |
| | 第三方任意 pcap 的完整语义保真（仅尽本阶段规则） |

### 门禁与输出

| 项 | 约定 |
|----|------|
| 录/放中 | 拒绝导入（同 `session.export` / `session.load` 思路） |
| 无可纳入帧 | **失败**（禁止空会话冒充成功） |
| 成功结果 | 替换或返回新会话（控制面参数：默认替换内存会话并回传统计；可选只回传 `.fcap` b64 不换内存——实现阶段与 `session.trim`/`load` 风格对齐即可） |
| 统计 | 响应含 `written`、`skipped`、`channelCount`（命名实现时对齐现有 JSON 风格） |

### 控制面 / CLI（实现阶段命名，需求锁定语义）

| 入口 | 要点 |
|------|------|
| `session.importPcap` | 参数：pcap `b64` 或引擎可读路径（与现有 load/export 能力对齐；浏览器侧优先 b64） |
| CLI | 建议：`import-pcap --in <file.pcap> --out <file.fcap>` |

## 验收

### E14a（本文件）

- [x] 需求入库；链路范围、跳过、通道映射、往返期望、非目标写清
- [x] 与 E10 / `.fcap` v1 无矛盾（不新增 kind；不改主交付格式）

### E14b（引擎）

- [ ] 经典以太网 pcap（含本工具 E10 产物）→ 会话；UDP/TCP 有载荷帧 `written` 正确
- [ ] 串口式无关帧（ARP 等）进 `skipped`；全跳过则失败
- [ ] 自检：构造/导出 pcap → 导入 → payload 与相对轴对齐

### E14c（控制面）

- [ ] `session.importPcap`（或等价）门禁正确；统计字段可读
- [ ] CLI `import-pcap` 写出可 `open` 的 `.fcap`

### E14d（UI）

- [ ] 提供载入 `.pcap` 入口（引擎已连）；成功进入报文/重演列表；失败提示可读
- [ ] 未连引擎不假装本地假导入正式包

## 非目标

- pcapng、Linux SLL、Raw IP、USB/蓝牙等 DLT
- IPv6、IP 分片重组、TCP 流重组（按段逐条即可）
- 从 TCP 握手合成 `tcp_connect` / `tcp_disconnect`
- 自动识别 RTP → `video` / RTSP
- 串口、MQTT 的伪链路导入
- 改 `.fcap` v1 布局或替代 `.fcap` 为主交付
- 真实 MAC/校验和保真；业务协议解包
- 多文件 merge；导入时自动 trim

## 风险与体验

- **本机侧启发式**：抓包机不是「设备本机」时，RX/TX 与 bind 可能对调；重放配目标时用户需按报文页核对。UI/文档一句提示即可。
- **大文件**：浏览器 b64 有内存上限；超大包优先 CLI / 引擎路径（实现阶段可提示）。
- **双向多端口**：规范化五元组把双向归一路，符合「一路通道」模型；若现场要按单边拆通道，后置增强。

## 待确认

- 无（阻塞项已在上表拍板；若现场强要 SLL/pcapng，另开修订）

## 建议下一手

资深软件工程师：E14b（引擎 pcap→会话 + 自检）。新开对话可贴：`新对话读一下进度本，接着做下一步。`
