# FieldReplay 引擎

原生录放热路径（Rust，仅标准库）。当前完成 **E1**：UDP 录制 → 二进制 `.fcap` → 按 `relative_us` 重放。

格式契约：[`../docs/design/fcap-format.md`](../docs/design/fcap-format.md)。

## 构建

```bat
cd engine
cargo build --release
```

可执行文件：`target/release/fieldreplay-engine.exe`。

## 命令

```bat
REM 写两帧 + 本机 UDP 录放环回
fieldreplay-engine self-check

REM 录制（按 Enter 停止；或加 --duration 秒）
fieldreplay-engine record --bind 0.0.0.0:9000 --out session.fcap
fieldreplay-engine record --bind 127.0.0.1:9000 --out session.fcap --duration 5

REM 按时间轴重放到目标
fieldreplay-engine replay --in session.fcap --target 127.0.0.1:9001
```

## 模块

| 模块 | 职责 |
|------|------|
| `fcap` | `.fcap` v1 头 / 通道表 / 帧 写读 |
| `clock` | 录制相对微秒、重放等待 |
| `udp` | bind 收 / 发 |
| `record` / `replay` | UDP 会话闭环 |
| `main` | CLI |

## 自检

```bat
cargo test
cargo run -- self-check
```
