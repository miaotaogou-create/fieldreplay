# 会话 trim + marker（E13）

- 状态：已确认（分阶段推进）
- 日期：2026-10-30
- 依据：E11/E12 非目标「会话 trim / marker」；`.fcap` v1 已定义 `kind=marker`；进度本轨道 A

## 背景

现场一次出差常录很长会话；回实验室只关心其中一段，或需要在时间轴上标「异常出现 / 换料 / 人工干预」。  
格式侧 **`kind=3 marker`（aux=说明文本，payload 空）已定稿**；重放调度已跳过 marker，不往 IO 写。缺的是：可控写入 marker，以及按相对时间窗切出一份可独立重放的新 `.fcap`。

## 目标模型

| 阶段 | ID | 内容 |
|------|-----|------|
| E13a | 本文件 | 语义、验收、非目标拍板 |
| E13b | 引擎 | 控制面插入 `kind=marker` + 自检（读回 aux） |
| E13c | 引擎 | 按相对时间窗 trim 写出新 `.fcap` + 自检 |
| E13d | UI | 重演/报文侧插 marker；trim 另存/导出 |

## 用户故事

1. 作为现场调试人员，我希望在录制或回看时插入带文字的时间点标记，以便同事打开包后立刻看到关键时刻。
2. 作为研发人员，我希望按相对时间窗把长会话裁成短包另存，以便只重放问题片段、减小文件与沟通成本。
3. 作为既有用户，我希望 trim 后的包仍能按通道表配置重放目标，不必重配通道含义。

## 产品约定（已确认）

### Marker

| 项 | 约定 |
|----|------|
| 格式 | `kind=marker`；`aux` = UTF-8 说明（可空字符串，但不建议）；`payload_len=0`；`flags=0` |
| 通道索引 | 默认 `channel_index=0`（会话至少一路时）；可选用参数指定合法 `ch`；无通道表时拒绝 |
| 方向 | `direction=0`（RX）；不参与业务语义 |
| 录制中写入 | 允许；`relative_us` = 当前相对会话起点；热路径追加，与普通帧同一写盘路径 |
| 离线写入 | 允许（已 `session.open`/`load` 且非录制中）；`relative_us` 由调用方给出；须满足文件内 **单调非降**（同戳允许多条：可插在该戳已有帧之后） |
| 重放 | **不**发往任何 sink（与现网一致）；UI/报文列表应可见 |
| 与 trim | 落在时间窗内的 marker **保留**；窗外丢弃 |

### Trim

| 项 | 约定 |
|----|------|
| 时间窗 | 闭区间 `[fromUs, toUs]`，单位与帧一致：相对录制起点的微秒 |
| 保留条件 | `fromUs ≤ relative_us ≤ toUs` 的全部 PacketRecord（含 data / TCP 事件 / marker / 未知 kind 原样拷贝） |
| 非法窗 | `fromUs > toUs` → 失败；窗内 **0 帧** → 失败（禁止写出空会话冒充成功） |
| 相对戳 | **再基**：写出帧的 `relative_us' = relative_us - fromUs`（新包从 0 起） |
| 墙钟起点 | `start_wall_unix_us' = start_wall_unix_us + fromUs`（未知墙钟则保持 0/原策略）；保证 `墙钟 + relative` 与裁前一致 |
| 通道表 | **完整保留**原通道表（含窗内零帧的通道），便于仍按 index 配重放目标 |
| SessionMeta | 拷贝原 meta；`notes`（或等价）追加一行 trim 说明：`trim [fromUs,toUs] → rebased`；`session_title` 可保留或由调用方覆盖 |
| 头字段 | 重算 `packet_count` / `total_payload_bytes` / `duration_us`（再基后末帧 relative，或 `toUs-fromUs` 取能表达时长者；以 finalize 与现写盘规则一致为准） |
| 输出 | **新文件 / 新字节流**；默认不覆盖源文件；源会话内存对象是否替换由控制面参数决定（见下） |
| 与录/放互斥 | 录制中或重放中拒绝 trim（同 `session.export` 门禁思路） |

### 控制面（实现阶段命名，需求锁定语义）

| 命令（建议名） | 要点 |
|----------------|------|
| `session.addMarker` | 参数：`text`（必填可空）、`relativeUs?`（离线必填或与播放头约定）、`ch?`；录制中可省略 `relativeUs` |
| `session.trim` | 参数：`fromUs`、`toUs`；可选 `replaceSession`（true=内存会话换成裁后结果；false/缺省=只回传新 `.fcap` b64，类似 export） |

具体 JSON 字段名在 E13b/c 实现时与现有 `session.*` 风格对齐即可，**不得改变上表语义**。

## 验收

### E13a（本文件）

- [x] 需求入库；trim 时间窗、再基、通道表/meta、marker 时机与非目标写清
- [x] 与 `docs/design/fcap-format.md` 的 `kind=marker` 无矛盾

### E13b（引擎）

- [x] 控制面可插入 marker；导出/载入后帧 `kind=marker` 且 aux 文本一致
- [x] 录制中插入：相对戳落在插入时刻附近；重放不向 sink 发送该帧
- [x] 自检：写 marker → 读回 aux

### E13c（引擎）

- [x] 给定 `[fromUs,toUs]` 写出新 `.fcap`：仅窗内帧；relative 再基；通道表完整；墙钟起点按约定偏移
- [x] 非法窗 / 空窗失败且可读错误；成功响应含保留帧数
- [x] 自检：构造含 marker+多通道帧的会话 → trim → 打开校验计数与首末 relative

### E13d（UI）

- [x] 重演或报文侧可插入 marker（引擎已连）；列表可见标记文案
- [x] 提供 trim 时间窗输入并另存/下载裁后 `.fcap`；失败提示可读
- [x] 未连引擎时不假装本地假写入正式包

## 非目标

- 多文件 merge / 拼接多个 `.fcap`
- 按通道过滤的「通道 trim」（本阶段只做时间窗）
- 改 `.fcap` v1 布局或新增 kind
- 在 trim 时自动插入「裁剪起止」marker（可后置；本阶段不强制）
- pcap 导入、MQTT mTLS、录制暂停（其它轨道）
- 业务协议解包或按载荷内容搜索裁剪

## 风险与体验

- **TCP 事件落在窗外**：裁后可能缺 `tcp_connect` 却仍有 `data`；重放行为与「从中途 seek」类似，属可接受限制，文档/UI 宜一句提示。
- **单调戳**：离线插 marker 若指定早于已有最大戳的「空洞中点」可以；若小于某已写入戳且无法保持非降顺序 → 拒绝并提示。
- **长文 aux**：不做硬上限也可；若实现加上限须在错误中说明，避免静默截断。

## 待确认

- 无（阻塞项已在上表拍板；若现场坚持「trim 不改 relative、只改导出过滤」再开修订）

## 建议下一手

E13 已收口。下一主线现场再挑：E14 pcap 导入 / E15 MQTT mTLS / E16 录制暂停 / R1 回归。
