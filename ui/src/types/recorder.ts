/**
 * @license
 * SPDX-License-Identifier: Apache-2.0
 */

export type ChannelType = 'serial' | 'udp' | 'tcp' | 'mqtt' | 'video_stream';

export interface ChannelConfig {
  id: string;
  name: string;
  type: ChannelType;
  enabled: boolean;
  color: string;
  // Parameters
  params: {
    portName?: string; // e.g. COM3 / /dev/ttyUSB0
    baudRate?: number; // 9600, 115200, 921600
    dataBits?: 7 | 8;
    stopBits?: 1 | 2;
    parity?: 'none' | 'even' | 'odd';
    ip?: string; // 192.168.1.100 or 0.0.0.0
    port?: number; // 8080
    role?: 'server' | 'client';
    streamUrl?: string; // rtsp://192.168.1.200/live
    resolution?: string;
    // MQTT specific
    mqttTopic?: string;
    mqttBroker?: string;
    mqttQoS?: 0 | 1 | 2;
  };
  // Status
  connected: boolean;
  rxBytes: number;
  rxFrames: number;
  currentRateBps: number;
}

export interface CapturedPacket {
  id: number;
  timestampUs: number; // Microsecond precision relative to start
  absoluteTime: string; // ISO or local time string
  channelId: string;
  channelName: string;
  channelType: ChannelType;
  direction: 'RX' | 'TX';
  length: number;
  data: Uint8Array;
  hexPreview: string;
  asciiPreview: string;
  protocolHint?: 'Modbus-RTU' | 'Modbus-TCP' | 'CAN-over-Serial' | 'Vision-Trigger' | 'IMU-Telemetry' | 'MQTT-Message' | 'Raw-Binary';
  parsedFields?: Record<string, string | number | boolean>;
  hasError?: boolean;
  errorReason?: string;
  mqttTopic?: string;
  mqttQoS?: number;
}

export interface CaptureSessionMetadata {
  sessionTitle: string;
  recordedBy: string;
  siteName: string;
  deviceModel: string;
  startTime: string;
  durationMs: number;
  totalPackets: number;
  totalBytes: number;
  channels: {
    id: string;
    name: string;
    type: ChannelType;
    packetCount: number;
  }[];
  notes?: string;
}

export interface ReplayConfig {
  speed: number; // 0.1x to 10x
  loop: boolean;
  targetIp: string;
  targetUdpPort: number;
  virtualComPort: string;
  // Serial specific configuration
  serial: {
    selectedPort: string; // 重演工具写入的端口 (如 COM2)
    qtTargetPort: string; // 开发者上位机读取的配对端口 (如 COM1)
    baudRate: number;
    isPortOpen: boolean;
    availablePorts: string[];
  };
  // TCP Specific configuration
  tcp: {
    mode: 'tool_is_server' | 'tool_is_client'; // tool_is_server: Qt上位机是客户端主动连过来; tool_is_client: Qt上位机是服务端重演工具主动连过去
    serverPort: number; // 当重演工具是Server时监听的端口 (如 9000 或 502)
    clientTargetHost: string; // 当重演工具是Client时连接的Qt上位机IP (如 127.0.0.1)
    clientTargetPort: number; // 当重演工具是Client时连接的Qt上位机端口
    connectedClientsCount: number;
  };
  // MQTT Specific configuration
  mqtt: {
    enabled: boolean;
    mode: 'virtual_publisher' | 'embedded_broker';
    brokerHost: string; // 127.0.0.1
    brokerPort: number; // 1883
    topicPrefix: string;
    connectedToBroker: boolean;
  };
  simulateOutput: boolean; // True to physically send via WebSockets / local agent
  faultInjection: {
    enabled: boolean;
    lossRatePercent: number; // 0 - 50%
    jitterMs: number; // 0 - 100ms
    corruptCrc: boolean;
    latencyMultiplier: number; // 1x to 5x
  };
}

export type AppTab = 'record' | 'replay' | 'packets' | 'qt_guide';
