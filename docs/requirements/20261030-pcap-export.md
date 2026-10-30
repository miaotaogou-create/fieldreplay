# pcap 导出（E10）

- 状态：已收口
- 日期：2026-10-30
- 依据：MVP 待确认项「是否同时提供 pcap 导出」；增强项排序 E10

## 背景

现场主交付物仍是 `.fcap`（多通道 + 相对时间轴，可重放）。研发常用 Wireshark 看 UDP/TCP 载荷时，希望从同一会话再导出一份经典 `.pcap`，避免手工拼包。串口 / MQTT 不是以太网链路层语义，本阶段不硬塞进 pcap。

## 目标模型

| 项 | 约定 |
|----|------|
| 格式 | 经典 libpcap（magic `0xa1b2c3d4`，微秒时间戳，`LINKTYPE_ETHERNET=1`） |
| 可导出通道 | `udp`、`video`（RTP/UDP 与 RTSP 落盘块均按「载荷进 UDP/伪帧」）、`tcp` 的 `kind=data` |
| 跳过 | `serial`、`mqtt`；`tcp_connect` / `tcp_disconnect` / `marker`；未知 kind |
| 封装 | 合成以太网 + IPv4 + UDP 或 TCP；载荷为 `.fcap` 原始 `payload`；对端尽量取自帧 `aux`（`ip:port`）与通道 `config_json` |
| 时间 | `ts = start_wall_unix_us + relative_us`（墙钟未知则仅用相对微秒） |

## 用户故事

1. 作为现场调试人员，我希望在导出 `.fcap` 时可选再下一份 `.pcap`，以便直接发给用 Wireshark 的同事。
2. 作为研发人员，我希望 CLI 能从已有 `.fcap` 转出 `.pcap`，以便脚本批处理。

## 验收

- [x] 引擎模块可把内存会话 / `.fcap` 写成经典 `.pcap`（无新依赖）
- [x] UDP（含视频 RTP/UDP）数据帧进入 pcap，Wireshark 可识别为 UDP；TCP `data` 帧识别为 TCP
- [x] 串口 / MQTT / 连接事件被跳过；响应或 CLI 输出含 `written` / `skipped` 计数
- [x] 控制面 `session.exportPcap`：与 `session.export` 相同门禁（录/放中拒绝、无会话/无帧拒绝）；回传 base64
- [x] CLI：`export-pcap --in <file.fcap> --out <file.pcap>`
- [x] UI：导出弹窗可选「同时导出 .pcap」；引擎已连时在 `.fcap` 之外再下载；未连则提示需引擎
- [x] `self-check` 含：两帧 UDP → pcap 魔数与 `written=2`

## 非目标

- pcapng、导入 `.pcap` 回灌 `.fcap`
- 真实 MAC / 真实 IP 校验和保真（合成头；UDP/TCP checksum 可为 0）
- 串口 / MQTT 的专用 DLT / 用户链路类型
- 替代 `.fcap` 作为重放输入

## 待确认

- 无（本阶段按上表最小切片）

## 下一步

增强项 E8–E10 已收口；后续视需求再定（如 pcap 导入、客户端证书等）。
