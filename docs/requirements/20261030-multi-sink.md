# 多路重放扩展 sink（E12）

- 状态：已确认（分阶段推进）
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

E12a 状态：**已完成**（引擎 + 控制面；UI 仍见 E12d）。

## 非目标（本增强整轮）

- 多路 RTSP 服务端
- MQTT mTLS（另立项）
- 会话 trim/marker、pcap 导入、录制暂停

## 建议下一手

E12a 收口后做 E12b 串口 → E12c MQTT → E12d UI。
