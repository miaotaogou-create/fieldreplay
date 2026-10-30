/**
 * @license
 * SPDX-License-Identifier: Apache-2.0
 */

import { ChannelType } from '../types/recorder';

/** 多路对齐重放本阶段可绑定的 sink（与引擎 E11c 一致）。 */
export function isMultiReplayCapable(type: ChannelType | string): boolean {
  const t = String(type).toLowerCase();
  return t === 'udp' || t === 'video_stream' || t === 'video';
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
  targetIp: string;
  targetPort: number;
}

export function defaultPortForChannel(
  type: ChannelType,
  channelIndex: number
): number {
  const base = type === 'video_stream' ? 5004 : 8080;
  return base + channelIndex;
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
      targetIp: '127.0.0.1',
      targetPort: defaultPortForChannel(c.type, channelIndex),
    };
  });
}

/** 组装 `replay.start` 的 targets[]；跳过未启用或本阶段不支持的通道。 */
export function listEnabledMultiTargets(
  rows: ChannelReplayTargetRow[]
): Array<{ ch: number; type: string; config: Record<string, unknown> }> {
  const out: Array<{ ch: number; type: string; config: Record<string, unknown> }> =
    [];
  for (const row of rows) {
    if (!row.enabled || !isMultiReplayCapable(row.type)) continue;
    const type =
      row.type === 'video_stream' ? 'video_stream' : 'udp';
    out.push({
      ch: row.channelIndex,
      type,
      config: {
        target_ip: row.targetIp.trim() || '127.0.0.1',
        target_port: row.targetPort > 0 ? row.targetPort : defaultPortForChannel(row.type, row.channelIndex),
      },
    });
  }
  return out;
}
