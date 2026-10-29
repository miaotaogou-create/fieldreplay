# FieldReplay 会话文件格式 `.fcap` v1

- 状态：已定稿（v1）；实现以本文为准
- 日期：2026-10-29
- 相关：`architecture.md`、`docs/requirements/20261029-mvp.md`

## 目标

现场多通道原始载荷 + 统一相对时间基，可拷贝、可按序重放。  
**不**嵌入业务协议语义；引擎与 UI 只认识通道类型、传输事件与字节。

## 非目标

- 替代 pcap/pcapng 做以太网层分析
- 在文件内做 Modbus 等字段解包
- 强制整文件一次载入内存（实现可流式读）

## 总原则

| 项 | 约定 |
|----|------|
| 字节序 | 小端（little-endian） |
| 扩展名 | `.fcap` |
| MIME / 魔数 | 文件头 magic = ASCII `FCAP`（`46 43 41 50`） |
| 时间 | 相对时间：自录制起点的微秒 `relative_us`（`uint64`）；墙钟起点写在文件头 |
| 写盘 | 热路径顺序追加帧记录；结束时回写头中的计数（可 seek） |
| 读盘 | v1 至少支持顺序扫描；可选索引为增强项，不进 v1 必选 |

> 开发期 UI 曾用 JSON 冒充 `.fcap` 做演示；**正式引擎读写必须是本文二进制布局**。旧 JSON 演示包不保证兼容。

## 文件布局

```
┌─────────────────────────┐
│ FileHeader（固定 64 B）   │
├─────────────────────────┤
│ ChannelTable（变长）      │  ← 由 header.channel_table_bytes 给出长度
├─────────────────────────┤
│ SessionMeta（变长，可 0）  │  ← UTF-8 JSON；由 header.meta_bytes 给出
├─────────────────────────┤
│ PacketRecord × N         │  ← 顺序追加；N = header.packet_count（收尾写回）
└─────────────────────────┘
```

无独立文件尾；`packet_count == 0` 且已有帧时视为「未收尾/损坏」，读端应拒绝或只读到 EOF 并告警（实现自定，须有明确行为）。

---

## 1. FileHeader（64 字节）

| 偏移 | 类型 | 字段 | 说明 |
|------|------|------|------|
| 0 | `char[4]` | `magic` | `"FCAP"` |
| 4 | `uint16` | `version` | **`0x0001`** = v1 |
| 6 | `uint16` | `header_bytes` | 本头长度，v1 固定 **64** |
| 8 | `uint32` | `flags` | 见下表 |
| 12 | `uint64` | `start_wall_unix_us` | 录制起点 UTC Unix 微秒；未知填 0 |
| 20 | `uint16` | `channel_count` | 通道条数 |
| 22 | `uint16` | `reserved0` | 写 0 |
| 24 | `uint32` | `channel_table_bytes` | 紧随头的通道表总字节数 |
| 28 | `uint32` | `meta_bytes` | 会话元数据 JSON 字节数，可为 0 |
| 32 | `uint64` | `packet_count` | 帧记录条数；录制中可暂为 0，**停止时回写** |
| 40 | `uint64` | `total_payload_bytes` | 所有 `payload_len` 之和（不含 aux）；停止时回写 |
| 48 | `uint64` | `duration_us` | 最后一帧 `relative_us`（无帧为 0）；停止时回写 |
| 56 | `uint32` | `reserved1` | 写 0 |
| 60 | `uint32` | `crc32_header` | 可选：对偏移 0..59 的 CRC-32（IEEE）；未用填 0 |

### `flags`（文件级）

| Bit | 名 | 含义 |
|-----|-----|------|
| 0 | `FINALIZED` | 1 = 已正常收尾（计数已回写） |
| 1 | `HAS_META` | 1 = `meta_bytes > 0`（冗余，便于快速判断） |
| 2..31 | — | 保留，写 0 |

---

## 2. ChannelTable

连续 `channel_count` 条 **ChannelEntry**，总长度 = `channel_table_bytes`。

### ChannelEntry

| 类型 | 字段 | 说明 |
|------|------|------|
| `uint16` | `channel_index` | 本会话内通道号，从 0 起；帧记录引用此值 |
| `uint8` | `type` | 见通道类型枚举 |
| `uint8` | `reserved` | 0 |
| `uint16` | `name_len` | 通道显示名 UTF-8 字节数 |
| `uint16` | `config_len` | 配置 UTF-8 JSON 字节数，可为 0 |
| `uint8[name_len]` | `name` | 无 `\0` 终止 |
| `uint8[config_len]` | `config_json` | 见下 |

### 通道类型 `type`

| 值 | 名 | MVP |
|----|-----|-----|
| 1 | `udp` | 是 |
| 2 | `serial` | 是 |
| 3 | `tcp` | 是 |
| 4 | `mqtt` | 是（紧接 MVP） |
| 5 | `video` | 后置；v1 可读但可忽略重放 |
| 0, 6..255 | 保留 | 读到未知类型：跳过该通道帧或整文件报错（实现选一，须文档化） |

### `config_json`（建议键，均可选）

只描述**传输配置**，不写业务协议名。

**UDP**

```json
{ "bind_ip": "0.0.0.0", "bind_port": 8080, "peer_ip": "", "peer_port": 0 }
```

**串口**

```json
{ "port_name": "COM3", "baud": 115200, "data_bits": 8, "stop_bits": 1, "parity": "none" }
```

**TCP**

```json
{ "role": "server", "host": "0.0.0.0", "port": 9000 }
```

`role`：`server` | `client`（**录制时**工具扮演的角色；重放时引擎按架构约定取反）。

**MQTT**

```json
{ "broker": "192.168.1.10", "port": 1883, "topic": "factory/line3/#", "qos": 0 }
```

> ponytail: 配置用 JSON 避免 v1 堆一堆 TLV；上限是解析略重、非零拷贝。若热路径证明不够，再升 typed TLV，版本号升 v2。

---

## 3. SessionMeta（可选）

`meta_bytes` 长度的 **UTF-8 JSON 对象**（无 BOM）。建议键：

```json
{
  "session_title": "现场3号产线_伺服报警.fcap",
  "recorded_by": "",
  "site_name": "",
  "device_model": "",
  "notes": ""
}
```

缺省键不影响引擎读写；UI 可用来展示。

---

## 4. PacketRecord（变长，顺序追加）

每条记录布局：

| 类型 | 字段 | 说明 |
|------|------|------|
| `uint64` | `relative_us` | 相对录制起点的微秒；须对文件内单调非降（同戳允许多条） |
| `uint16` | `channel_index` | 对应 ChannelEntry |
| `uint8` | `kind` | 记录种类，见下 |
| `uint8` | `direction` | `0` = RX（流入工具），`1` = TX（工具发出） |
| `uint32` | `flags` | 见下 |
| `uint32` | `aux_len` | 辅助 UTF-8/二进制长度，紧随头后 |
| `uint32` | `payload_len` | 原始载荷长度 |
| `uint8[aux_len]` | `aux` | 见各 kind 约定 |
| `uint8[payload_len]` | `payload` | 原始字节；业务不解 |

固定头部分长度：**24 字节**（至 `payload_len` 止），其后为 aux + payload。

### `kind`

| 值 | 名 | 含义 | `aux` 建议 | `payload` |
|----|-----|------|------------|-----------|
| 0 | `data` | 普通数据 | UDP/TCP 可选对端 `"ip:port"`；串口通常空；MQTT 见下 | 原始载荷 |
| 1 | `tcp_connect` | TCP 连接建立 | 对端 `"ip:port"` 或空 | 通常 0 |
| 2 | `tcp_disconnect` | TCP 断开 | 可选原因短文本 | 通常 0 |
| 3 | `marker` | 人工/工具标记（非载荷） | 说明文本 | 0 |
| 4..255 | 保留 | 未知 kind：跳过 `aux_len+payload_len` 字节继续扫 | — | — |

### MQTT（`type=mqtt` 且 `kind=data`）

| 段 | 内容 |
|----|------|
| `aux` | UTF-8 **topic**（无 `\0`） |
| `payload` | 消息体原始字节 |
| `flags` 低 2 bit | QoS（0/1/2）；bit2 = retain（若录到） |

不解析 JSON/业务字段。

### `flags`（帧级，除 MQTT QoS 占用外）

| Bit | 名 | 含义 |
|-----|-----|------|
| 0..1 | `mqtt_qos` | 仅 MQTT |
| 2 | `mqtt_retain` | 仅 MQTT |
| 3 | `truncated_error` | 工具侧标记异常（截断、写盘失败旁路等），**非**业务协议校验 |
| 4..31 | 保留 | 0 |

---

## 5. 录制 / 重放约定（与格式相关）

1. **打开文件**：写 FileHeader（`FINALIZED=0`，计数暂 0）→ ChannelTable → SessionMeta → 循环追加 PacketRecord。  
2. **停止**：回写 `packet_count` / `total_payload_bytes` / `duration_us`，置 `FINALIZED=1`。  
3. **重放**：按 `relative_us` 调度；只根据 `channel_index` + `type` + `kind` 决定发往何处；**不**读业务含义。  
4. **TCP**：重放侧角色相对录制配置取反（见 `architecture.md`）；`tcp_connect` / `tcp_disconnect` 参与连接态，不把其当应用数据写 socket（除非实现明确约定）。  
5. **MQTT**：重放按 `aux` topic（可被 UI 前缀改写）发布 `payload`。

---

## 6. 校验与兼容

- 读端：`magic` 不符或 `version` 主版本不认识 → 拒绝打开并提示。  
- `version` 次高字节将来可表示破坏性变更；v1 内只增保留字段、不改偏移语义。  
- `header_bytes > 64` 的未来版本：读端跳过多余头字节再读表（若实现支持）；v1 写端必须写 64。  
- 可选 `crc32_header`：非 0 则校验；为 0 则跳过（现场性能优先）。

---

## 7. 与 UI 模型的对应（实现对照）

| UI / 类型 | `.fcap` |
|-----------|---------|
| `timestampUs` | `relative_us` |
| `channelId` 字符串 | 会话内映射到 `channel_index`；名称在 ChannelEntry |
| `direction` RX/TX | `direction` 0/1 |
| `data` / HEX 预览 | `payload`（预览由读端生成，不入库） |
| `mqttTopic` / QoS | `aux` + `flags` |
| `CaptureSessionMetadata` | SessionMeta JSON + 头中统计字段 |
| `protocolHint` / `parsedFields` | **不写入**正式文件 |

---

## 8. 最小自检（实现阶段）

引擎落地时至少保留其一：

- 写 1 路 UDP 两帧 → 读回 `relative_us` 与 payload 一致；或  
- 单元测试：编码/解码一帧 `PacketRecord` 往返。

**E1 已落地**：`engine` 中 `cargo test`（往返）与 `cargo run -- self-check`（往返 + UDP 环回录放）。

## 修订记录

| 日期 | 说明 |
|------|------|
| 2026-10-29 | v1 初稿定稿（E0） |
| 2026-10-29 | E1 读写实现于 `engine/`（Rust） |
