# FieldReplay 引擎

原生录放热路径（Rust，仅标准库）。当前完成 **E2**：UDP + 串口录制 → 二进制 `.fcap` → 按 `relative_us` 重放。

格式契约：[`../docs/design/fcap-format.md`](../docs/design/fcap-format.md)。

## 构建

```bat
cd engine
cargo build --release
```

可执行文件：`target/release/fieldreplay-engine.exe`。

## 命令

```bat
REM 写帧往返 + UDP 环回 + 串口内存环回
fieldreplay-engine self-check

REM UDP 录制（按 Enter 停止；或加 --duration 秒）
fieldreplay-engine record --bind 0.0.0.0:9000 --out session.fcap
fieldreplay-engine record --bind 127.0.0.1:9000 --out session.fcap --duration 5

REM 串口录制（Windows 实口；虚拟串口对亦可）
fieldreplay-engine record --port COM3 --baud 115200 --out session.fcap
fieldreplay-engine record --port COM3 --baud 9600 --parity even --out session.fcap --duration 10

REM 按时间轴重放
fieldreplay-engine replay --in session.fcap --target 127.0.0.1:9001
fieldreplay-engine replay --in session.fcap --port COM4 --baud 115200
```

## 模块

| 模块 | 职责 |
|------|------|
| `fcap` | `.fcap` v1 头 / 通道表 / 帧 写读 |
| `clock` | 录制相对微秒、重放等待 |
| `udp` | bind 收 / 发 |
| `serial` | Windows 实口 Win32；自检内存环回 |
| `record` / `replay` | UDP / 串口会话闭环 |
| `main` | CLI |

## 自检

```bat
cargo test
cargo run -- self-check
```

实口手测需本机串口或虚拟串口对（如 com0com）：一端录、一端灌数据；重放写到对端口。
