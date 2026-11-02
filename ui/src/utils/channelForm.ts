/**
 * 录制页通道表单：ChannelConfig ↔ 参数行（添加 / 编辑共用）。
 */

import type { ChannelConfig, ChannelType } from '../types/recorder';
import { parseSerialParamLine } from './serialParams';

export interface ChannelFormValues {
  name: string;
  type: ChannelType;
  param: string;
  mqttTls: boolean;
  mqttCa: string;
  mqttSni: string;
}

const TYPE_COLOR: Record<ChannelType, string> = {
  serial: '#0891b2',
  udp: '#0891b2',
  tcp: '#0891b2',
  mqtt: '#0d9488',
  video_stream: '#7c3aed',
};

/** 把已有通道填回表单（编辑预填）。 */
export function channelToFormValues(ch: ChannelConfig): ChannelFormValues {
  const p = ch.params;
  let param = '';
  if (ch.type === 'serial') {
    const baud = p.baudRate ?? 115200;
    const db = p.dataBits ?? 8;
    const parityChar =
      p.parity === 'even' ? 'E' : p.parity === 'odd' ? 'O' : 'N';
    const sb = p.stopBits ?? 1;
    param = `${p.portName ?? 'COM3'}, ${baud}, ${db}${parityChar}${sb}`;
  } else if (ch.type === 'mqtt') {
    const parts = [p.mqttBroker ?? '127.0.0.1:1883', p.mqttTopic ?? '#'];
    if (p.mqttUsername) parts.push(p.mqttUsername);
    if (p.mqttPassword) parts.push(p.mqttPassword);
    param = parts.join(', ');
  } else if (ch.type === 'video_stream' && p.streamUrl) {
    param = p.streamUrl;
  } else {
    const ip = p.ip ?? (ch.type === 'video_stream' ? '0.0.0.0' : '127.0.0.1');
    const port = p.port ?? (ch.type === 'video_stream' ? 5004 : 8888);
    param = `${ip}:${port}`;
  }
  return {
    name: ch.name,
    type: ch.type,
    param,
    mqttTls: !!p.mqttTls,
    mqttCa: p.mqttCaCertPath ?? '',
    mqttSni: p.mqttTlsServerName ?? '',
  };
}

export function defaultFormValuesForType(type: ChannelType): Omit<ChannelFormValues, 'name'> {
  if (type === 'serial') {
    return {
      type,
      param: 'COM4, 921600, 8-N-1',
      mqttTls: false,
      mqttCa: '',
      mqttSni: '',
    };
  }
  if (type === 'mqtt') {
    return {
      type,
      param: '127.0.0.1:1883, factory/+/telemetry',
      mqttTls: false,
      mqttCa: '',
      mqttSni: '',
    };
  }
  if (type === 'video_stream') {
    return {
      type,
      param: '0.0.0.0:5004',
      mqttTls: false,
      mqttCa: '',
      mqttSni: '',
    };
  }
  return {
    type,
    param: '0.0.0.0:8888',
    mqttTls: false,
    mqttCa: '',
    mqttSni: '',
  };
}

/**
 * 由表单组装通道。
 * - 新建：不传 base，生成新 id
 * - 编辑：传 base，保留 id / enabled / color / TCP role 等
 */
export function buildChannelFromForm(
  values: ChannelFormValues,
  base?: ChannelConfig
): ChannelConfig {
  const { name, type, param, mqttTls, mqttCa, mqttSni } = values;
  const isMqtt = type === 'mqtt';
  const isSerial = type === 'serial';
  const isVideo = type === 'video_stream';
  const paramTrim = param.trim();
  const isRtsp = isVideo && paramTrim.toLowerCase().startsWith('rtsp://');
  const hostPort =
    !isRtsp && paramTrim.includes(':')
      ? paramTrim.split(':')
      : [isVideo ? '0.0.0.0' : '127.0.0.1', paramTrim];
  const listenIp = hostPort[0]?.trim() || (isVideo ? '0.0.0.0' : '127.0.0.1');
  const listenPort = Number(hostPort[1]?.trim()) || (isVideo ? 5004 : 8888);
  const serialParsed = isSerial ? parseSerialParamLine(paramTrim) : null;

  return {
    id: base?.id ?? `ch_custom_${Date.now()}`,
    name,
    type,
    enabled: base?.enabled ?? true,
    color: base?.color ?? TYPE_COLOR[type],
    params: {
      portName: serialParsed?.portName,
      baudRate: serialParsed?.baudRate,
      dataBits: serialParsed?.dataBits,
      stopBits: serialParsed?.stopBits,
      parity: serialParsed?.parity,
      ip: !isSerial && !isMqtt ? (isRtsp ? '0.0.0.0' : listenIp) : undefined,
      port: !isSerial && !isMqtt ? (isRtsp ? 0 : listenPort) : undefined,
      role: type === 'tcp' ? (base?.params.role ?? 'server') : undefined,
      streamUrl: isRtsp ? paramTrim : undefined,
      mqttBroker: isMqtt
        ? paramTrim.split(',')[0]?.trim() || '127.0.0.1:1883'
        : undefined,
      mqttTopic: isMqtt
        ? paramTrim.split(',')[1]?.trim() || 'factory/+/telemetry'
        : undefined,
      mqttQoS: isMqtt ? 1 : undefined,
      mqttUsername: isMqtt
        ? paramTrim.split(',')[2]?.trim() || undefined
        : undefined,
      mqttPassword: isMqtt
        ? paramTrim.split(',')[3]?.trim() || undefined
        : undefined,
      mqttTls: isMqtt ? mqttTls : undefined,
      mqttCaCertPath:
        isMqtt && mqttTls && mqttCa.trim() ? mqttCa.trim() : undefined,
      mqttTlsServerName:
        isMqtt && mqttTls && mqttSni.trim() ? mqttSni.trim() : undefined,
    },
    connected: base?.connected ?? true,
    rxBytes: base?.rxBytes ?? 0,
    rxFrames: base?.rxFrames ?? 0,
    currentRateBps: base?.currentRateBps ?? 0,
  };
}
