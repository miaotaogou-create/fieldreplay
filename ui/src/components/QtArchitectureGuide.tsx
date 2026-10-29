/**
 * @license
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useState } from 'react';
import {
  FileCode2,
  Copy,
  Check,
  CheckCircle2,
  HelpCircle,
  Cpu,
  Layers,
  ArrowRight,
  ShieldCheck,
  Zap,
} from 'lucide-react';

export const QtArchitectureGuide: React.FC = () => {
  const [copiedSection, setCopiedSection] = useState<string | null>(null);

  const handleCopy = (code: string, id: string) => {
    navigator.clipboard.writeText(code);
    setCopiedSection(id);
    setTimeout(() => setCopiedSection(null), 2000);
  };

  const binaryFormatCode = `// -------------------------------------------------------------
// 推荐的统一现场捕获文件格式规范 (.fcap 二进制结构体定义)
// -------------------------------------------------------------

#pragma pack(push, 1)

// 1. 全局文件头 (32 字节)
struct FcapFileHeader {
    char     magic[4];       // "FCAP" 标识
    uint16_t version;        // 格式版本: 0x0100 (v1.0)
    uint16_t channel_count;  // 记录的信道总数
    uint64_t start_time_us;  // 录制开始绝对时间戳 (UTC Unix微秒)
    uint64_t total_packets;  // 包总数
    uint32_t reserved[2];    // 预留对齐
};

// 2. 每一帧数据的前置元数据头 (24 字节)
struct FcapPacketHeader {
    uint64_t relative_us;    // 距离录制起点的相对时间微秒 (微秒时基 Δt)
    uint8_t  channel_id;     // 信道标识 (0:COM3, 1:UDP_8080, 2:TCP_9000, 3:Video)
    uint8_t  direction;      // 0: RX (设备流入), 1: TX (上位机下发)
    uint16_t flags;          // 标志位 (例如 bit 0: 校验错误, bit 1: 关键帧)
    uint32_t payload_len;    // 紧随其后的二进制负载字节长度
    uint64_t sequence_id;    // 全局自增帧序列号
    // 紧接着就是 payload_len 长度的原始二进制数据 (Zero-Copy)
};

#pragma pack(pop)`;

  const qtAdapterCode = `// -------------------------------------------------------------
// Qt C++ 上位机无需侵入业务逻辑的重演对接适配方案
// -------------------------------------------------------------

#include <QObject>
#include <QSerialPort>
#include <QUdpSocket>
#include <QTcpSocket>

class ReplayStreamAdapter : public QObject {
    Q_OBJECT
public:
    explicit ReplayStreamAdapter(QObject *parent = nullptr) : QObject(parent) {}

    // 方案 A: 串口对接 (零侵入模式)
    // 借助 Windows com0com 或 Linux socat 虚拟串口对 (COM1 <-> COM2)
    // 重演工具往 COM2 发送现场数据，Qt 像连接真实硬件一样打开 COM1
    static void setupVirtualSerial(QSerialPort &port, const QString &portName = "COM1") {
        port.setPortName(portName);
        port.setBaudRate(QSerialPort::Baud115200);
        port.setDataBits(QSerialPort::Data8);
        port.setParity(QSerialPort::NoParity);
        port.setStopBits(QSerialPort::OneStop);
        port.open(QIODevice::ReadWrite);
    }

    // 方案 B: 网络对接 (回环重放模式)
    // Qt 上位机监听本地 127.0.0.1:8080，重演工具把网络数据包重演打到该端口
    static void setupLoopbackUdp(QUdpSocket &socket, quint16 port = 8080) {
        socket.bind(QHostAddress::LocalHost, port, QUdpSocket::ShareAddress);
    }
};`;

  const tcpMqttCode = `// =============================================================
// Qt C++ TCP 与 MQTT 现场数据重演对接核心实现
// =============================================================

#include <QTcpServer>
#include <QTcpSocket>
#include <QtMqtt/QMqttClient> // Qt官方 qtmqtt 库

// -------------------------------------------------------------
// 1. TCP 重演服务 (重演工具端实现: 模拟现场设备供 Qt 上位机连接)
// -------------------------------------------------------------
class ReplayTcpServer : public QTcpServer {
    Q_OBJECT
private:
    QTcpSocket *m_clientSocket = nullptr;

protected:
    void incomingConnection(qintptr socketDescriptor) override {
        m_clientSocket = new QTcpSocket(this);
        m_clientSocket->setSocketDescriptor(socketDescriptor);

        // 连上 Qt 上位机后，重演引擎开始按录制时间戳向其推送数据
        connect(m_clientSocket, &QTcpSocket::readyRead, this, [this]() {
            // 如果上位机下发了请求 (如握手或查询指令)，也可以进行校验
            QByteArray req = m_clientSocket->readAll();
        });
    }

public:
    // 按照录制帧的微秒时钟，将字节注入 TCP 链路
    void replayFrame(const QByteArray &payload) {
        if (m_clientSocket && m_clientSocket->isOpen()) {
            m_clientSocket->write(payload);
            m_clientSocket->flush();
        }
    }
};

// -------------------------------------------------------------
// 2. MQTT 重演发布者 (重演工具端实现: 模拟现场网关 Publish)
// -------------------------------------------------------------
class ReplayMqttPublisher : public QObject {
    Q_OBJECT
    QMqttClient *m_client;

public:
    explicit ReplayMqttPublisher(QObject *parent = nullptr) : QObject(parent) {
        m_client = new QMqttClient(this);
        m_client->setHostname("127.0.0.1"); // 本地 Broker 地址
        m_client->setPort(1883);
        m_client->connectToHost();
    }

    // 重演引擎按时序发布录制的 MQTT 消息
    void replayMqttMessage(const QString &topic, const QByteArray &payload, quint8 qos = 1) {
        if (m_client->state() == QMqttClient::Connected) {
            // Qt 上位机订阅的正是原 Topic，上位机端 messageReceived 会原样触发!
            m_client->publish(QMqttTopicName(topic), payload, qos);
        }
    }
};`;

  return (
    <div className="max-w-6xl mx-auto px-4 sm:px-6 py-8 space-y-8">
      {/* Question 1: Is this convenient? */}
      <div className="bg-white rounded-xl border border-slate-200 p-6 shadow-xs space-y-4">
        <div className="flex items-center gap-2.5 pb-3 border-b border-slate-100">
          <div className="w-8 h-8 rounded-lg bg-emerald-50 text-emerald-600 flex items-center justify-center font-bold text-sm">
            Q1
          </div>
          <h2 className="text-base font-bold text-slate-900">
            你说这个工具方便不方便？
          </h2>
        </div>

        <p className="text-sm text-slate-700 leading-relaxed">
          <strong className="text-emerald-700 font-semibold">答案是：极其方便，可以说是 Qt / 嵌入式工控上位机开发的“护身符”与“提效神器”！</strong>
        </p>

        <div className="grid grid-cols-1 md:grid-cols-3 gap-4 pt-2">
          <div className="p-4 bg-slate-50 rounded-lg border border-slate-100 space-y-1.5">
            <div className="flex items-center gap-1.5 text-xs font-bold text-slate-900">
              <CheckCircle2 className="w-4 h-4 text-emerald-600" />
              <span>彻底告别“现场偶现、回厂无法复现”</span>
            </div>
            <p className="text-xs text-slate-600 leading-relaxed">
              现场环境复杂（机械抖动、传感器微弱温漂、总线突发丢包）。现场人员只需录制几分钟，抓到的不仅是静态报文，更保留了<strong>微秒级的时间差与时序</strong>。研发人员在后方就能 100% 还原现场！
            </p>
          </div>

          <div className="p-4 bg-slate-50 rounded-lg border border-slate-100 space-y-1.5">
            <div className="flex items-center gap-1.5 text-xs font-bold text-slate-900">
              <CheckCircle2 className="w-4 h-4 text-emerald-600" />
              <span>上位机业务代码“零侵入”</span>
            </div>
            <p className="text-xs text-slate-600 leading-relaxed">
              工具独立运行，上位机不用为了测试在代码里写各种复杂的 Mock/桩代码。上位机在开发机上依然正常监听串口和网口，重演工具把数据灌进去，上位机以为自己真在现场！
            </p>
          </div>

          <div className="p-4 bg-slate-50 rounded-lg border border-slate-100 space-y-1.5">
            <div className="flex items-center gap-1.5 text-xs font-bold text-slate-900">
              <CheckCircle2 className="w-4 h-4 text-emerald-600" />
              <span>时间机器般的调试能力</span>
            </div>
            <p className="text-xs text-slate-600 leading-relaxed">
              真机现场无法暂停，而重演工具支持<strong>原速重放、0.1x 慢动作、逐帧单步步进(Step)、循环回放</strong>，甚至可以人为制造丢包和损坏CRC，验证Qt上位机的容错健壮性。
            </p>
          </div>
        </div>
      </div>

      {/* Question 2: How should the UI be designed? */}
      <div className="bg-white rounded-xl border border-slate-200 p-6 shadow-xs space-y-4">
        <div className="flex items-center gap-2.5 pb-3 border-b border-slate-100">
          <div className="w-8 h-8 rounded-lg bg-sky-50 text-sky-600 flex items-center justify-center font-bold text-sm">
            Q2
          </div>
          <h2 className="text-base font-bold text-slate-900">
            界面应该设计成什么样？“要求不要复杂，尽可能简单”
          </h2>
        </div>

        <p className="text-sm text-slate-700 leading-relaxed">
          针对您的要求<strong>“不要复杂，尽可能简单”</strong>，我们采用<strong>【双模态极简设计】</strong>：
        </p>

        <div className="grid grid-cols-1 md:grid-cols-2 gap-5 pt-2">
          {/* Mode 1: Field Mode */}
          <div className="p-5 bg-sky-50/50 rounded-xl border border-sky-100 space-y-3">
            <div className="flex items-center justify-between">
              <span className="text-xs font-bold text-sky-900 uppercase">
                ① 现场录制端 (面对现场非研发人员)
              </span>
              <span className="text-[11px] bg-sky-100 text-sky-800 px-2 py-0.5 rounded font-medium">
                傻瓜化 3 步
              </span>
            </div>
            <ul className="text-xs text-slate-700 space-y-2 list-disc list-inside">
              <li>
                <strong>第一步：选择信道</strong>：界面只放几个大勾选框（串口COM3、UDP 8080、相机流），不用配复杂的过滤规则。
              </li>
              <li>
                <strong>第二步：大红按钮录制</strong>：一个醒目的【● 开始录制】，中间大字显示录制秒数和已捕获大小，状态一眼能确认。
              </li>
              <li>
                <strong>第三步：一键打包</strong>：点【停止并打包】，自动生成一个带时间戳的现场文件（如 <code>现场数据包_20261028.fcap</code>），调试员直接微信/邮件发给您。
              </li>
            </ul>
          </div>

          {/* Mode 2: Replay Mode */}
          <div className="p-5 bg-slate-50 rounded-xl border border-slate-200 space-y-3">
            <div className="flex items-center justify-between">
              <span className="text-xs font-bold text-slate-900 uppercase">
                ② 研发重演端 (面对后方 Qt 工程师)
              </span>
              <span className="text-[11px] bg-slate-200 text-slate-800 px-2 py-0.5 rounded font-medium">
                视听播放器体验
              </span>
            </div>
            <ul className="text-xs text-slate-700 space-y-2 list-disc list-inside">
              <li>
                <strong>拖入即载入</strong>：把 <code>.fcap</code> 文件往窗口一拖，立即显示总帧数、波形与时间轴。
              </li>
              <li>
                <strong>媒体播放器操控条</strong>：播放、暂停、进度条拖拽、0.1x~10x 倍速、逐帧微秒级单步（⏭️）。
              </li>
              <li>
                <strong>目标端口重映射</strong>：设置把串口重放到本机的虚拟串口，把UDP重放到 <code>127.0.0.1</code>，启动重演，上位机直接接收。
              </li>
            </ul>
          </div>
        </div>
      </div>

      {/* Question 3: How to handle TCP and MQTT during Replay? */}
      <div className="bg-white rounded-xl border border-slate-200 p-6 shadow-xs space-y-6">
        <div className="flex items-center gap-2.5 pb-3 border-b border-slate-100">
          <div className="w-8 h-8 rounded-lg bg-sky-50 text-sky-600 flex items-center justify-center font-bold text-sm">
            Q3
          </div>
          <div>
            <h2 className="text-base font-bold text-slate-900">
              重演的时候，发给本机上位机，TCP 咋办呢？MQTT 咋办呢？
            </h2>
            <p className="text-xs text-slate-500 mt-0.5">
              深入剖析面向连接协议 (TCP) 与发布/订阅协议 (MQTT) 在回放时的角色转换与零侵入实现
            </p>
          </div>
        </div>

        {/* Part 1: TCP Solution */}
        <div className="space-y-3">
          <div className="flex items-center gap-2 text-sm font-bold text-slate-900">
            <span className="w-2 h-2 rounded-full bg-sky-600" />
            <span>一、TCP 重演咋办？（根据上位机角色自动匹配）</span>
          </div>

          <p className="text-xs text-slate-600 leading-relaxed">
            TCP 是<strong>面向连接</strong>的流式协议，必然存在一端是 Server、另一端是 Client。工业现场上位机通常有以下两种拓扑，重演工具完美支持双向自适应：
          </p>

          <div className="grid grid-cols-1 md:grid-cols-2 gap-4 pt-1">
            <div className="p-4 bg-sky-50/50 rounded-lg border border-sky-100 space-y-2 text-xs">
              <div className="font-bold text-sky-950 flex items-center justify-between">
                <span>场景 A：Qt 上位机是 Client (最常见，占 90%)</span>
                <span className="bg-sky-100 text-sky-800 text-[10px] px-1.5 py-0.5 rounded">标准模式</span>
              </div>
              <p className="text-slate-600">
                <strong>现场情况：</strong>PLC (Modbus-TCP)、工控机、机械臂或视觉相机是 TCP Server（如 <code>192.168.1.10:502</code>），Qt 上位机主动 <code>connectToHost()</code> 连接它们。
              </p>
              <div className="p-2.5 bg-white rounded border border-sky-200 text-slate-700 leading-relaxed">
                <strong>💡 重演工具解法：</strong>
                <ol className="list-decimal list-inside space-y-1 mt-1 text-[11px]">
                  <li>重演工具在本地启动一个 <code>QTcpServer</code> 监听对应端口（如 <code>127.0.0.1:502</code>）；</li>
                  <li>Qt 上位机仅需将目标 IP 改为 <code>127.0.0.1</code>（或通过 hosts 文件重定向）；</li>
                  <li>Qt 上位机握手连上重演工具后，重演工具按照录制的<strong>微秒时序</strong>将报文 <code>write()</code> 给连接上的 socket，Qt 端 <code>readyRead()</code> 像在现场一样触发！</li>
                </ol>
              </div>
            </div>

            <div className="p-4 bg-slate-50 rounded-lg border border-slate-200 space-y-2 text-xs">
              <div className="font-bold text-slate-900 flex items-center justify-between">
                <span>场景 B：Qt 上位机是 Server (上位机监听)</span>
                <span className="bg-slate-200 text-slate-800 text-[10px] px-1.5 py-0.5 rounded">反向模式</span>
              </div>
              <p className="text-slate-600">
                <strong>现场情况：</strong>Qt 上位机启动了 <code>QTcpServer::listen(9000)</code>，现场手持终端、PDA 或外部工件检测设备主动连接上位机。
              </p>
              <div className="p-2.5 bg-white rounded border border-slate-200 text-slate-700 leading-relaxed">
                <strong>💡 重演工具解法：</strong>
                <ol className="list-decimal list-inside space-y-1 mt-1 text-[11px]">
                  <li>Qt 上位机正常启动并监听本地端口（如 <code>127.0.0.1:9000</code>）；</li>
                  <li>重演工具作为 TCP Client，点击播放时主动执行 <code>connectToHost("127.0.0.1", 9000)</code> 连上 Qt；</li>
                  <li>连接建立后，重演工具持续把现场录制数据按时钟节奏灌进去。</li>
                </ol>
              </div>
            </div>
          </div>
        </div>

        {/* Part 2: MQTT Solution */}
        <div className="space-y-3 pt-3 border-t border-slate-100">
          <div className="flex items-center gap-2 text-sm font-bold text-slate-900">
            <span className="w-2 h-2 rounded-full bg-teal-600" />
            <span>二、MQTT 咋办？（发布/订阅解耦，重演更简单！）</span>
          </div>

          <p className="text-xs text-slate-600 leading-relaxed">
            MQTT 不像 TCP 那样点对点直连，它是<strong>基于代理（Broker）的发布/订阅（Pub/Sub）</strong>。Qt 上位机（通常使用 Qt 官方的 <code>qtmqtt</code> 库）通过订阅 Topic（例如 <code>factory/line3/+/telemetry</code>）来接收现场传感网关的消息。
          </p>

          <div className="p-4 bg-teal-50/50 rounded-lg border border-teal-100 space-y-3 text-xs">
            <div className="font-bold text-teal-950 flex items-center justify-between">
              <span>核心方案：重演工具充当“虚拟现场发布者 (Virtual Field Publisher)”</span>
              <span className="bg-teal-100 text-teal-800 text-[10px] px-1.5 py-0.5 rounded font-semibold">推荐做法</span>
            </div>

            <div className="grid grid-cols-1 md:grid-cols-3 gap-3">
              <div className="p-3 bg-white rounded border border-teal-200">
                <div className="font-semibold text-slate-900 mb-1">1. 本地 Broker 桥接</div>
                <p className="text-slate-600 text-[11px]">
                  研发机器运行轻量本地 Broker（如开源的 Mosquitto，或重演工具内嵌的微型 Broker 监听 <code>127.0.0.1:1883</code>）。Qt 上位机直接连到 127.0.0.1。
                </p>
              </div>

              <div className="p-3 bg-white rounded border border-teal-200">
                <div className="font-semibold text-slate-900 mb-1">2. 原始主题原样推送</div>
                <p className="text-slate-600 text-[11px]">
                  录制时抓包不仅抓 Payload，还记录了当时的 <strong>Topic</strong>、<strong>QoS (0/1/2)</strong> 和 <strong>Retain</strong> 标志。重演工具直接调用 <code>publish(topic, payload, qos)</code>。
                </p>
              </div>

              <div className="p-3 bg-white rounded border border-teal-200">
                <div className="font-semibold text-slate-900 mb-1">3. Qt 上位机 100% 真实</div>
                <p className="text-slate-600 text-[11px]">
                  Qt 的 <code>QMqttClient::messageReceived</code> 会在正确的时间毫秒被唤醒，收到的主题和载荷与现场别无二致，上位机业务解析代码<strong>完全无需改动</strong>！
                </p>
              </div>
            </div>
          </div>
        </div>

        {/* Code Block: C++ Code for TCP & MQTT replay adapter */}
        <div className="pt-2">
          <div className="flex items-center justify-between mb-2">
            <span className="text-xs font-bold text-slate-800">
              ③ TCP / MQTT 重演对接核心 C++ 逻辑实现 (Qt5 / Qt6 适用)
            </span>
            <button
              onClick={() => handleCopy(tcpMqttCode, 'tcpmqtt')}
              className="flex items-center gap-1 text-xs text-sky-600 hover:text-sky-700 font-medium"
            >
              {copiedSection === 'tcpmqtt' ? <Check className="w-3.5 h-3.5 text-emerald-600" /> : <Copy className="w-3.5 h-3.5" />}
              <span>{copiedSection === 'tcpmqtt' ? '已复制 TCP/MQTT 代码' : '复制 C++ 实现'}</span>
            </button>
          </div>
          <div className="p-4 bg-slate-900 rounded-lg font-mono text-xs text-slate-200 overflow-x-auto border border-slate-800 leading-relaxed">
            <pre>{tcpMqttCode}</pre>
          </div>
        </div>
      </div>
    </div>
  );
};
