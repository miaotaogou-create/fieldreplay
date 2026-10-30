/**
 * @license
 * SPDX-License-Identifier: Apache-2.0
 */

import { CapturedPacket, ChannelConfig, CaptureSessionMetadata } from '../types/recorder';

// Convert Uint8Array to formatted Hex string (e.g. "01 03 00 00 00 0A C5 CD")
export function bytesToHex(bytes: Uint8Array, maxBytes = 32): string {
  const slice = bytes.slice(0, maxBytes);
  const hex = Array.from(slice)
    .map(b => b.toString(16).padStart(2, '0').toUpperCase())
    .join(' ');
  return bytes.length > maxBytes ? `${hex} …` : hex;
}

// Convert Uint8Array to safe ASCII display
export function bytesToAscii(bytes: Uint8Array, maxBytes = 32): string {
  const slice = bytes.slice(0, maxBytes);
  const ascii = Array.from(slice)
    .map(b => (b >= 32 && b <= 126 ? String.fromCharCode(b) : '·'))
    .join('');
  return bytes.length > maxBytes ? `${ascii}…` : ascii;
}

// Modbus CRC16 Calculation
export function calcModbusCRC16(data: Uint8Array): number {
  let crc = 0xffff;
  for (let i = 0; i < data.length; i++) {
    crc ^= data[i];
    for (let j = 0; j < 8; j++) {
      if ((crc & 0x0001) !== 0) {
        crc = (crc >> 1) ^ 0xa001;
      } else {
        crc = crc >> 1;
      }
    }
  }
  return crc;
}

// Default standard channels configuration
export const defaultChannels: ChannelConfig[] = [
  {
    id: 'ch_serial_1',
    name: 'COM3 (RS485/Modbus)',
    type: 'serial',
    enabled: true,
    color: '#0284c7', // Sky-600
    params: {
      portName: 'COM3',
      baudRate: 115200,
      dataBits: 8,
      stopBits: 1,
      parity: 'none',
    },
    connected: true,
    rxBytes: 14820,
    rxFrames: 312,
    currentRateBps: 2400,
  },
  {
    id: 'ch_udp_1',
    name: 'UDP :8080 (雷达点云/PLC)',
    type: 'udp',
    enabled: true,
    color: '#059669', // Emerald-600
    params: {
      ip: '0.0.0.0',
      port: 8080,
    },
    connected: true,
    rxBytes: 94812,
    rxFrames: 856,
    currentRateBps: 18500,
  },
  {
    id: 'ch_tcp_1',
    name: 'TCP :9000 (工控视觉检测)',
    type: 'tcp',
    enabled: true,
    color: '#d97706', // Amber-600
    params: {
      ip: '192.168.1.120',
      port: 9000,
      role: 'server',
    },
    connected: true,
    rxBytes: 42100,
    rxFrames: 144,
    currentRateBps: 5200,
  },
  {
    id: 'ch_mqtt_1',
    name: 'MQTT (网关遥测/产线状态)',
    type: 'mqtt',
    enabled: true,
    color: '#0d9488', // Teal-600
    params: {
      mqttBroker: '127.0.0.1:1883',
      mqttTopic: 'factory/line3/+/telemetry',
      mqttQoS: 1,
    },
    connected: true,
    rxBytes: 31200,
    rxFrames: 96,
    currentRateBps: 3400,
  },
  {
    id: 'ch_video_1',
    name: 'RTSP 相机流 (H.264 Keyframe)',
    type: 'video_stream',
    enabled: false,
    color: '#7c3aed', // Violet-600
    params: {
      streamUrl: 'rtsp://192.168.1.200:554/live/ch0',
      resolution: '1920x1080@30fps',
    },
    connected: false,
    rxBytes: 0,
    rxFrames: 0,
    currentRateBps: 0,
  },
];

// Helper to generate a realistic MQTT Publish packet
export function createMqttPacket(
  id: number,
  timestampUs: number,
  topic = 'factory/line3/robot/status',
  payloadObj: Record<string, any> = { state: 'RUNNING', speed_mm_s: 450, load_kg: 3.2 }
): CapturedPacket {
  const jsonStr = JSON.stringify(payloadObj);
  const raw = new TextEncoder().encode(jsonStr);
  return {
    id,
    timestampUs,
    absoluteTime: new Date(Date.now() - 30000 + Math.floor(timestampUs / 1000)).toLocaleTimeString(),
    channelId: 'ch_mqtt_1',
    channelName: 'MQTT (网关遥测/产线状态)',
    channelType: 'mqtt',
    direction: 'RX',
    length: raw.length,
    data: raw,
    hexPreview: bytesToHex(raw),
    asciiPreview: bytesToAscii(raw),
    protocolHint: 'MQTT-Message',
    mqttTopic: topic,
    mqttQoS: 1,
    parsedFields: {
      'MQTT 主题 (Topic)': topic,
      '服务质量 (QoS)': 'QoS 1',
      '载荷类型': 'JSON Telemetry',
      ...payloadObj,
    },
  };
}

// Helper to generate a realistic Modbus-RTU packet
export function createModbusPacket(
  id: number,
  timestampUs: number,
  slaveId = 1,
  funcCode = 3,
  registerValues: number[] = [1250, 480, 220, 100]
): CapturedPacket {
  const byteCount = registerValues.length * 2;
  const raw = new Uint8Array(3 + byteCount + 2);
  raw[0] = slaveId;
  raw[1] = funcCode;
  raw[2] = byteCount;

  for (let i = 0; i < registerValues.length; i++) {
    const val = registerValues[i];
    raw[3 + i * 2] = (val >> 8) & 0xff;
    raw[3 + i * 2 + 1] = val & 0xff;
  }

  const crc = calcModbusCRC16(raw.slice(0, 3 + byteCount));
  raw[3 + byteCount] = crc & 0xff;
  raw[3 + byteCount + 1] = (crc >> 8) & 0xff;

  const hex = bytesToHex(raw);
  const ascii = bytesToAscii(raw);

  return {
    id,
    timestampUs,
    absoluteTime: new Date(Date.now() - 30000 + Math.floor(timestampUs / 1000)).toLocaleTimeString(),
    channelId: 'ch_serial_1',
    channelName: 'COM3 (RS485/Modbus)',
    channelType: 'serial',
    direction: 'RX',
    length: raw.length,
    data: raw,
    hexPreview: hex,
    asciiPreview: ascii,
    protocolHint: 'Modbus-RTU',
    parsedFields: {
      '从机地址 (Slave)': slaveId,
      '功能码 (Function)': funcCode === 3 ? '0x03 读保持寄存器' : `0x0${funcCode}`,
      '字节数 (ByteCount)': byteCount,
      '电机转速 (RPM)': registerValues[0] ?? 0,
      '母线电压 (V)': ((registerValues[1] ?? 0) / 10).toFixed(1) + ' V',
      '相电流 (A)': ((registerValues[2] ?? 0) / 100).toFixed(2) + ' A',
      '电机温度 (°C)': ((registerValues[3] ?? 0) / 10).toFixed(1) + ' °C',
      'CRC16': `0x${crc.toString(16).toUpperCase().padStart(4, '0')}`,
    },
  };
}

// Helper to generate a realistic UDP sensor/lidar packet
export function createUdpLidarPacket(id: number, timestampUs: number, distanceMm: number): CapturedPacket {
  // Protocol: [Header 0xAA 0x55] [Seq 2B] [Dist 2B] [Signal 1B] [Status 1B] [Checksum 1B]
  const raw = new Uint8Array(8);
  raw[0] = 0xaa;
  raw[1] = 0x55;
  raw[2] = (id >> 8) & 0xff;
  raw[3] = id & 0xff;
  raw[4] = (distanceMm >> 8) & 0xff;
  raw[5] = distanceMm & 0xff;
  raw[6] = 98; // signal quality
  raw[7] = (raw[0] + raw[1] + raw[2] + raw[3] + raw[4] + raw[5] + raw[6]) & 0xff;

  return {
    id,
    timestampUs,
    absoluteTime: new Date(Date.now() - 30000 + Math.floor(timestampUs / 1000)).toLocaleTimeString(),
    channelId: 'ch_udp_1',
    channelName: 'UDP :8080 (雷达点云/PLC)',
    channelType: 'udp',
    direction: 'RX',
    length: raw.length,
    data: raw,
    hexPreview: bytesToHex(raw),
    asciiPreview: bytesToAscii(raw),
    protocolHint: 'IMU-Telemetry',
    parsedFields: {
      '帧头': '0xAA 0x55',
      '序号': id,
      '激光测距': `${distanceMm} mm`,
      '信号强度': '98%',
      '校验和': `0x${raw[7].toString(16).toUpperCase().padStart(2, '0')}`,
    },
  };
}

// Helper to generate a TCP Vision Trigger/Result packet
export function createTcpVisionPacket(
  id: number,
  timestampUs: number,
  isOk = true,
  partId = 'PART-2026-A1'
): CapturedPacket {
  const jsonPayload = JSON.stringify({
    cmd: 'INSPECT_RESULT',
    part: partId,
    passed: isOk,
    score: isOk ? 0.985 : 0.42,
    defect: isOk ? 'None' : 'Scratch_Depth_0.8mm',
    timestamp: Date.now(),
  });
  const encoder = new TextEncoder();
  const raw = encoder.encode(jsonPayload);

  return {
    id,
    timestampUs,
    absoluteTime: new Date(Date.now() - 30000 + Math.floor(timestampUs / 1000)).toLocaleTimeString(),
    channelId: 'ch_tcp_1',
    channelName: 'TCP :9000 (工控视觉检测)',
    channelType: 'tcp',
    direction: 'RX',
    length: raw.length,
    data: raw,
    hexPreview: bytesToHex(raw),
    asciiPreview: bytesToAscii(raw),
    protocolHint: 'Vision-Trigger',
    hasError: !isOk,
    errorReason: !isOk ? '缺陷检测不合格 (Scratch)' : undefined,
    parsedFields: {
      '命令': 'INSPECT_RESULT',
      '工件条码': partId,
      '判定结果': isOk ? 'PASS (合格)' : 'FAIL (不合格缺陷)',
      '置信度': isOk ? '98.5%' : '42.0%',
      '缺陷类型': isOk ? '无' : '划痕超标 (>0.8mm)',
    },
  };
}

// Realistic Preset Capture Scenario 1: 现场伺服电机急停报警与振动异常
export function generateServoAlarmPreset(): {
  metadata: CaptureSessionMetadata;
  packets: CapturedPacket[];
} {
  const packets: CapturedPacket[] = [];
  let tUs = 0;
  let pktId = 1;

  // 1. Normal run for 3 seconds
  for (let i = 0; i < 30; i++) {
    tUs += 100000; // 100ms per packet
    const rpm = 1500 + Math.floor(Math.sin(i / 5) * 40);
    const temp = 450 + Math.floor(i / 2);
    packets.push(createModbusPacket(pktId++, tUs, 1, 3, [rpm, 480, 210, temp]));

    // UDP lidar reading
    const dist = 850 + Math.floor(Math.random() * 6);
    packets.push(createUdpLidarPacket(pktId++, tUs + 25000, dist));

    // Every 500ms, camera inspection
    if (i % 5 === 0) {
      packets.push(createTcpVisionPacket(pktId++, tUs + 50000, true, `SN-${1000 + i}`));
    }

    // Every 600ms, MQTT telemetry
    if (i % 6 === 0) {
      packets.push(createMqttPacket(pktId++, tUs + 70000, 'factory/line3/servo/telemetry', {
        axis: 'Axis_Z',
        load_pct: 42.5 + (i % 3),
        state: 'NOMINAL',
      }));
    }
  }

  // 2. Anomaly occurs at t=3.2s: Sudden motor stall / current surge
  tUs += 120000;
  // Overcurrent surge
  const faultPkt = createModbusPacket(pktId++, tUs, 1, 3, [200, 440, 850, 680]);
  faultPkt.hasError = true;
  faultPkt.errorReason = '过流突发报警: 电流超过临界值 (8.5A)';
  packets.push(faultPkt);

  // Vision rejects part
  tUs += 40000;
  packets.push(createTcpVisionPacket(pktId++, tUs, false, 'SN-1035-DEFECT'));

  // MQTT Emergency Topic alert
  tUs += 15000;
  packets.push(createMqttPacket(pktId++, tUs, 'factory/line3/alarm/emergency', {
    level: 'CRITICAL',
    event: 'SERVO_OVERLOAD_STALL',
    current_amp: 8.5,
    timestamp: Date.now(),
  }));

  // 3. Emergency stop response (RPM -> 0, Voltage drops)
  for (let i = 0; i < 20; i++) {
    tUs += 100000;
    const rpm = Math.max(0, 200 - i * 40);
    packets.push(createModbusPacket(pktId++, tUs, 1, 3, [rpm, 240, 20, 690 - i]));
    packets.push(createUdpLidarPacket(pktId++, tUs + 15000, 320));
  }

  const totalBytes = packets.reduce((acc, p) => acc + p.length, 0);

  const metadata: CaptureSessionMetadata = {
    sessionTitle: '现场3号产线_伺服轴突发卡滞过流报警_复现包.fcap',
    recordedBy: '现场调试员-张工',
    siteName: '苏州智能制造二厂 · 3号线冲压工作站',
    deviceModel: '工控上位机主机-Advantech-IPC610 (Windows 10 LTSC)',
    startTime: '2026-10-28 14:22:18',
    durationMs: Math.round(tUs / 1000),
    totalPackets: packets.length,
    totalBytes,
    channels: [
      { id: 'ch_serial_1', name: 'COM3 (RS485/Modbus)', type: 'serial', packetCount: 51 },
      { id: 'ch_udp_1', name: 'UDP :8080 (雷达点云/PLC)', type: 'udp', packetCount: 50 },
      { id: 'ch_tcp_1', name: 'TCP :9000 (工控视觉检测)', type: 'tcp', packetCount: 7 },
    ],
    notes: '现场偶现伺服电机突然停转，上位机界面卡死报错。已用该工具完整捕获了卡滞前100ms及卡滞后急停的全量总线帧，研发人员可用127.0.0.1或虚拟串口直接重演上位机！',
  };

  return { metadata, packets };
}

// Realistic Preset Scenario 2: 高速激光AGV导航定位与雷达心跳
export function generateAgvNavigationPreset(): {
  metadata: CaptureSessionMetadata;
  packets: CapturedPacket[];
} {
  const packets: CapturedPacket[] = [];
  let tUs = 0;
  let pktId = 1;

  for (let i = 0; i < 60; i++) {
    tUs += 50000; // 50ms intervals = 20Hz
    const dist = 1200 - i * 15;
    packets.push(createUdpLidarPacket(pktId++, tUs, Math.max(100, dist)));

    if (i % 2 === 0) {
      packets.push(createModbusPacket(pktId++, tUs + 12000, 2, 3, [800, 360, 150, 320]));
    }
  }

  const totalBytes = packets.reduce((acc, p) => acc + p.length, 0);

  return {
    metadata: {
      sessionTitle: 'AGV室内激光导航_避障雷达高频采样包.fcap',
      recordedBy: '现场工程师-李工',
      siteName: '华东物流枢纽无人仓',
      deviceModel: 'AGV车载工控机 (Linux Ubuntu 22.04)',
      startTime: '2026-10-28 16:05:00',
      durationMs: Math.round(tUs / 1000),
      totalPackets: packets.length,
      totalBytes,
      channels: [
        { id: 'ch_udp_1', name: 'UDP :8080 (雷达点云/PLC)', type: 'udp', packetCount: 60 },
        { id: 'ch_serial_1', name: 'COM3 (RS485/Modbus)', type: 'serial', packetCount: 30 },
      ],
      notes: '测试AGV以1.2m/s速度接近障碍物时的实时制动点云数据，验证上位机避障算法判定延时。',
    },
    packets,
  };
}
