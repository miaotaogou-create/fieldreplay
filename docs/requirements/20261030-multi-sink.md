# 多路重放扩展 sink（E12）

- 状态：已完成（E12a–d）
- 日期：2026-10-30
- 依据：E11c/d 已知限制「多路仅 UDP/视频 UDP」；现场多协议同轴对照需串口/TCP/MQTT 也能进 `targets[]`

## 背景

E11 已打通多通道共享时间轴录制与对齐重放，但多路 `targets[]` 硬拒串口/TCP/MQTT，混合会话只能部分重放。  
现场偶现问题常需 **UDP + TCP（及串口/MQTT）交错** 才能复现；补齐 sink 才能兑现 E11 的对照价值。

## 目标模型

| 阶段 | ID | 内容 | 非目标 |
|------|-----|------|--------|
| E12a | 本阶段优先 | 多路调度器接入 **TCP**（与 UDP/视频 UDP 可同会话）；控制面允许 `type=tcp`；自检 UDP+TCP 对齐 | 串口/MQTT；RTSP listen 多路 |
| E12b | 紧随 | 多路 **串口** sink + 虚口/环回自检（有环境则跑） | 录制中改串口参数 |
| E12c | 紧随 | 多路 **MQTT** sink（含 topic/aux、可选凭据）+ MiniBroker 自检 | mTLS 客户端证书 |
| E12d | 随引擎 | UI：放开 capable；串口/TCP/MQTT 目标表单项；跳过文案同步 | 每通道独立倍速 |

## 产品约定

| 项 | 约定 |
|----|------|
| 时间轴 | 仍整会话统一倍速/暂停/seek/loop（同 E11c） |
| TCP 角色 | 与单路一致：按会话通道 `config_json` **取反**；`targets[].config` 只给重放侧地址 |
| 启动失败 | 任一路打开失败 → 整次 `replay.start` 失败 |
| 缺目标 | 仍进 `skipped[]`；有目标通道不得串发到其它 sink |
| RTSP | 多路仍不支持 `rtsp_listen`（会阻塞整轴）；单路照旧 |

## E12a 验收

- [x] `multi_replay` 支持同会话 UDP（或视频 UDP）与 TCP 目标并存，按相对时间轴调度
- [x] TCP：处理 `TcpConnect` / `Data` / `TcpDisconnect`；数据方向语义与单路一致（仅 direction=0 写出）
- [x] 控制面：`targets[]` 允许 `tcp`；错误文案不再「本阶段仅支持 udp」
- [x] `self-check`：UDP+TCP 同会话对齐重放，两路载荷抽查正确（`self_check_multi_udp_tcp_replay`）
- [x] 既有多路 UDP 自检与单路 TCP 行为不回归（`self-check` 全部通过）

E12a 状态：**已完成**（引擎 + 控制面；UI 见 E12d）。

## E12b 验收

- [x] `multi_replay` 支持同会话串口目标（可与 UDP/视频 UDP/TCP 并存），按相对时间轴写 Data
- [x] 控制面：`targets[]` 允许 `serial`；打开失败 → 整次 `replay.start` 失败
- [x] `self-check`：UDP+串口对齐重放，两路载荷抽查正确（`self_check_multi_udp_serial_replay`，内存环回）
- [x] 既有多路 UDP/UDP+TCP 与单路串口行为不回归

E12b 状态：**已完成**（引擎 + 控制面；UI 见 E12d）。

## E12c 验收

- [x] `multi_replay` 支持同会话 MQTT 目标（可与 UDP/视频 UDP/TCP/串口并存），按相对时间轴 PUBLISH（topic 取帧 `aux`，可选 `topic_prefix`；QoS/retain 取帧 flags）
- [x] 控制面：`targets[]` 允许 `mqtt`；可选 username/password / TLS 字段；打开失败 → 整次 `replay.start` 失败
- [x] `self-check`：UDP+MQTT 对齐重放，载荷与 topic 抽查正确（`self_check_multi_udp_mqtt_replay`，MiniBroker）
- [x] 既有多路与单路 MQTT 行为不回归

E12c 状态：**已完成**（引擎 + 控制面；UI 见 E12d）。非目标：mTLS 客户端证书。

## E12d 验收

- [x] `isMultiReplayCapable` 含 UDP / 视频 / TCP / 串口 / MQTT（RTSP listen 仍非多路）
- [x] 多路目标表：按类型展示表单项（UDP/视频 IP:port；TCP host:port；串口口名/波特率/帧格式；MQTT broker/凭据/TLS）
- [x] 跳过文案与提示同步：未勾选跳过；不再写「多路暂仅 UDP」
- [x] `listEnabledMultiTargets` 下发与控制面字段对齐（`multiReplayTargets.selfcheck`）

E12d 状态：**已完成**。

## 非目标（本增强整轮）

- 多路 RTSP 服务端
- MQTT mTLS（另立项）
- 会话 trim/marker、pcap 导入、录制暂停

## 建议下一手

下一主线：会话 trim+marker 见 `20261030-session-trim-marker.md`（E13）。其余备选：pcap 导入 / MQTT mTLS / 录制暂停。
