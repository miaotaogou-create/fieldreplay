/**
 * @license
 * SPDX-License-Identifier: Apache-2.0
 */

import { ChannelType } from '../types/recorder';

/** 多路对齐重放可绑定的 sink（与引擎 E12a–c 一致；RTSP listen 仍仅单路）。 */
export function isMultiReplayCapable(type: ChannelType | string): boolean {
  const t = String(type).toLowerCase();
  return (
    t === 'udp' ||
    t === 'video_stream' ||
    t === 'video' ||
    t === 'tcp' ||
    t === 'serial' ||
    t === 'mqtt'
  );
}

/** 会话通道 id → .fcap 通道 index（`ch_${index}`）。 */
export function channelIndexFromId(id: string): number {
  const m = /^ch_(\d+)$/.exec(id);
  if (m) return parseInt(m[1], 10);
  return 0;
}

export interface ChannelReplayTargetRow {
  channelId: string;
  channelIndex: number;
  name: string;
  type: ChannelType;
  packetCount: number;
  /** 未勾选则进引擎 skipped */
  enabled: boolean;
  /** UDP / 视频 UDP 推送 */
  targetIp: string;
  targetPort: number;
  /** 串口 */
  serialPort: string;
  baud: number;
  dataBits: 7 | 8;
  stopBits: 1 | 2;
  parity: 'none' | 'even' | 'odd';
  /** TCP 重放侧地址（角色由会话 config 取反） */
  tcpHost: string;
  tcpPort: number;
  /** MQTT */
  mqttBroker: string;
  mqttPort: number;
  mqttTopicPrefix: string;
  mqttUsername: string;
  mqttPassword: string;
  mqttTls: boolean;
  mqttCaCertPath: string;
  mqttTlsServerName: string;
}

export function defaultPortForChannel(
  type: ChannelType,
  channelIndex: number
): number {
  if (type === 'video_stream') return 5004 + channelIndex;
  if (type === 'tcp') return 9000 + channelIndex;
  if (type === 'mqtt') return 1883;
  return 8080 + channelIndex;
}

function emptyRowDefaults(
  type: ChannelType,
  channelIndex: number
): Omit<
  ChannelReplayTargetRow,
  'channelId' | 'channelIndex' | 'name' | 'type' | 'packetCount' | 'enabled'
> {
  return {
    targetIp: '127.0.0.1',
    targetPort: defaultPortForChannel(type, channelIndex),
    serialPort: 'COM2',
    baud: 115200,
    dataBits: 8,
    stopBits: 1,
    parity: 'none',
    tcpHost: '127.0.0.1',
    tcpPort: defaultPortForChannel('tcp', channelIndex),
    mqttBroker: '127.0.0.1',
    mqttPort: 1883,
    mqttTopicPrefix: '',
    mqttUsername: '',
    mqttPassword: '',
    mqttTls: false,
    mqttCaCertPath: '',
    mqttTlsServerName: '',
  };
}

export function buildDefaultChannelTargets(
  channels: Array<{
    id: string;
    name: string;
    type: ChannelType;
    packetCount: number;
  }>
): ChannelReplayTargetRow[] {
  return channels.map((c) => {
    const channelIndex = channelIndexFromId(c.id);
    const capable = isMultiReplayCapable(c.type);
    return {
      channelId: c.id,
      channelIndex,
      name: c.name,
      type: c.type,
      packetCount: c.packetCount,
      enabled: capable,
      ...emptyRowDefaults(c.type, channelIndex),
    };
  });
}

function engineTypeFor(type: ChannelType): string {
  if (type === 'video_stream') return 'video_stream';
  return type;
}

function configForRow(row: ChannelReplayTargetRow): Record<string, unknown> {
  switch (row.type) {
    case 'serial':
      return {
        port_name: row.serialPort.trim() || 'COM2',
        baud: row.baud > 0 ? row.baud : 115200,
        data_bits: row.dataBits,
        stop_bits: row.stopBits,
        parity: row.parity || 'none',
      };
    case 'tcp':
      return {
        host: row.tcpHost.trim() || '127.0.0.1',
        port: row.tcpPort > 0 ? row.tcpPort : defaultPortForChannel('tcp', row.channelIndex),
      };
    case 'mqtt': {
      const config: Record<string, unknown> = {
        broker: row.mqttBroker.trim() || '127.0.0.1',
        port: row.mqttPort > 0 ? row.mqttPort : 1883,
        topic_prefix: row.mqttTopicPrefix || '',
      };
      if (row.mqttUsername) config.username = row.mqttUsername;
      if (row.mqttPassword) config.password = row.mqttPassword;
      if (row.mqttTls) {
        config.tls = true;
        if (row.mqttCaCertPath) config.ca_cert_path = row.mqttCaCertPath;
        if (row.mqttTlsServerName) config.tls_server_name = row.mqttTlsServerName;
      }
      return config;
    }
    case 'udp':
    case 'video_stream':
    default:
      return {
        target_ip: row.targetIp.trim() || '127.0.0.1',
        target_port:
          row.targetPort > 0
            ? row.targetPort
            : defaultPortForChannel(row.type, row.channelIndex),
      };
  }
}

/** 组装 `replay.start` 的 targets[]；跳过未启用或本阶段不支持的通道。 */
export function listEnabledMultiTargets(
  rows: ChannelReplayTargetRow[]
): Array<{ ch: number; type: string; config: Record<string, unknown> }> {
  const out: Array<{ ch: number; type: string; config: Record<string, unknown> }> =
    [];
  for (const row of rows) {
    if (!row.enabled || !isMultiReplayCapable(row.type)) continue;
    out.push({
      ch: row.channelIndex,
      type: engineTypeFor(row.type),
      config: configForRow(row),
    });
  }
  return out;
}
