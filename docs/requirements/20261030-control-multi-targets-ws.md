# 控制面多路 targets[] WS 自检（R2）

- 状态：已确认（R2 全轨收口）
- 日期：2026-10-30
- 依据：R1b 残留风险「控制面 WS 自检未跑 multi `targets[]`（库级已覆盖）」；E16 收口后无默认主线；E15 缺客户 mTLS 场景

## 背景

E11c/E12d 已让 `replay.start` 接受 `targets[]`（UDP/TCP/串口/MQTT），库级 `multi_replay` 自检与 UI `multiReplayTargets.selfcheck` 已覆盖。  
控制面 WebSocket 路径仍只验单路 `type`+`config` 重放，**控制面 JSON 解析 → 多路绑定 → 推送进度** 这条链路缺少环回自检。  
E15（MQTT mTLS）仍缺要求客户端证书的现场 Broker；E9b 已覆盖常见 mqtts。相较之下，补 R1 测试洞更划算、可立即验收。

## 选定理由

| 维度 | R2 控制面 multi targets WS | E15 MQTT mTLS |
|------|---------------------------|---------------|
| 价值 | 堵住已知回归洞；UI↔引擎主路径 | 少数 Broker 双向证书 |
| 依赖 | 本机环回即可 | 客户证书材料 / 夹具扩展 |
| 产品面 | 不扩功能，只加固验收 | 新字段 + Broker 夹具 |
| 与既有 | 对齐库级 UDP+TCP 多路自检语义 | E9b 非目标里已声明另立 |

**结论：本轮主线为 R2。** E15 继续待命，待出现明确 mTLS 现场再开。

## 目标

| ID | 内容 |
|----|------|
| R2a | 本文件：范围、验收、非目标 |
| R2b | 控制面 WS：`session.open` 多通道会话 → `replay.start` 带 `targets[]`（至少 UDP+TCP）→ 环回收到载荷；`started` 回含 `targets` |

## 用户故事

1. 作为研发人员，我希望 `cargo run -- self-check` 覆盖控制面多路 `targets[]`，以便改解析/绑定时尽早失败。
2. 作为协作者，我希望 R1 写明的残留风险被显式关掉或降级，以免下轮再被当成未决债。

## 验收标准

### R2a（本文件）

- [x] 写清相对 E15 的选定理由；E15 继续待命
- [x] 验收与非目标可勾选

### R2b（自检）

- [x] 控制面 WS 自检：会话含 ≥2 通道类型（UDP+TCP）
- [x] `replay.start` 使用非空 `targets[]`；响应含 `"targets"` 且 `"ok":true`
- [x] 高倍速重放后，UDP 与 TCP 接收端均收到对应 data 载荷
- [x] 挂入 `self-check`；全套通过

## 非目标

- 新产品功能或 UI 改动
- 串口实口对 / MQTT Broker 的控制面多路专项（库级已有；本轨以 UDP+TCP 为代表即可）
- MQTT mTLS、`insecure_skip_verify`
- 多路 RTSP listen

## 下一步

**R2 已收口。** E15 仍搁置；无默认主线时现场再选定（或等 mTLS 客户场景）。
