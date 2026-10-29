/**
 * @license
 * SPDX-License-Identifier: Apache-2.0
 */

import {
  CapturedPacket,
  CaptureSessionMetadata,
  ChannelType,
} from '../types/recorder';
import { bytesToAscii, bytesToHex } from '../utils/mockFieldData';
import { base64ToBytes } from './bytes';
import { EngineSessionDto } from './types';

const CHANNEL_COLORS = ['#0284c7', '#059669', '#d97706', '#0d9488', '#7c3aed'];

function mapChannelType(t: string): ChannelType {
  switch (t) {
    case 'udp':
      return 'udp';
    case 'serial':
      return 'serial';
    case 'tcp':
      return 'tcp';
    case 'mqtt':
      return 'mqtt';
    case 'video_stream':
    case 'video':
      return 'video_stream';
    default:
      return 'udp';
  }
}

function wallUsToIso(us: number): string {
  if (!us) return new Date().toISOString();
  return new Date(Math.floor(us / 1000)).toISOString();
}

export function mapEngineSession(dto: EngineSessionDto): {
  metadata: CaptureSessionMetadata;
  packets: CapturedPacket[];
} {
  const channels = dto.channels.map((c, i) => ({
    id: `ch_${c.index}`,
    name: c.name || `通道 ${c.index}`,
    type: mapChannelType(c.type),
    packetCount: dto.packets.filter((p) => p.ch === c.index).length,
    color: CHANNEL_COLORS[i % CHANNEL_COLORS.length],
  }));

  const packets: CapturedPacket[] = dto.packets.map((p) => {
    const ch = channels.find((c) => c.id === `ch_${p.ch}`) ?? {
      id: `ch_${p.ch}`,
      name: `通道 ${p.ch}`,
      type: 'udp' as ChannelType,
    };
    const data = base64ToBytes(p.payloadB64 || '');
    const absMs = dto.startWallUnixUs
      ? Math.floor(dto.startWallUnixUs / 1000) + Math.floor(p.us / 1000)
      : Date.now();
    const pkt: CapturedPacket = {
      id: p.i + 1,
      timestampUs: p.us,
      absoluteTime: new Date(absMs).toISOString(),
      channelId: ch.id,
      channelName: ch.name,
      channelType: ch.type,
      direction: p.dir === 1 ? 'TX' : 'RX',
      length: data.length,
      data,
      hexPreview: bytesToHex(data),
      asciiPreview: bytesToAscii(data),
      hasError: p.kind === 3,
    };
    if (ch.type === 'mqtt') {
      pkt.mqttTopic = p.topic || undefined;
      pkt.mqttQoS = p.qos;
      pkt.protocolHint = 'MQTT-Message';
    }
    return pkt;
  });

  const meta = dto.meta && typeof dto.meta === 'object' ? dto.meta : {};
  const title =
    (typeof meta.sessionTitle === 'string' && meta.sessionTitle)
    || (typeof meta.title === 'string' && meta.title)
    || (dto.source === 'upload' ? '已载入会话' : dto.source);

  const metadata: CaptureSessionMetadata = {
    sessionTitle: title,
    recordedBy:
      typeof meta.recordedBy === 'string' ? meta.recordedBy : '引擎载入',
    siteName: typeof meta.siteName === 'string' ? meta.siteName : '—',
    deviceModel:
      typeof meta.deviceModel === 'string' ? meta.deviceModel : '—',
    startTime: wallUsToIso(dto.startWallUnixUs),
    durationMs: Math.floor(dto.durationUs / 1000),
    totalPackets: packets.length,
    totalBytes: dto.totalPayloadBytes || packets.reduce((n, p) => n + p.length, 0),
    channels: channels.map(({ id, name, type, packetCount }) => ({
      id,
      name,
      type,
      packetCount,
    })),
    notes:
      typeof meta.notes === 'string'
        ? meta.notes
        : dto.finalized
          ? undefined
          : '会话可能未正常收尾',
  };

  return { metadata, packets };
}

export function emptySessionMetadata(): CaptureSessionMetadata {
  return {
    sessionTitle: '尚未载入会话',
    recordedBy: '—',
    siteName: '—',
    deviceModel: '—',
    startTime: '—',
    durationMs: 0,
    totalPackets: 0,
    totalBytes: 0,
    channels: [],
  };
}
