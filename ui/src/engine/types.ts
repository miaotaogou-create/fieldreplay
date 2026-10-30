/**
 * @license
 * SPDX-License-Identifier: Apache-2.0
 */

export type EngineConnState = 'disconnected' | 'connecting' | 'connected' | 'error';

export interface EngineChannelDto {
  index: number;
  type: string;
  name: string;
  config: Record<string, unknown>;
}

export interface EnginePacketDto {
  i: number;
  us: number;
  ch: number;
  kind: number;
  dir: number;
  flags: number;
  topic: string;
  qos: number;
  retain: boolean;
  auxB64: string;
  payloadB64: string;
}

export interface EngineSessionDto {
  source: string;
  startWallUnixUs: number;
  durationUs: number;
  totalPayloadBytes: number;
  finalized: boolean;
  meta: Record<string, unknown> | null;
  channels: EngineChannelDto[];
  packets: EnginePacketDto[];
}

export interface EngineRpcOk<T> {
  id: string;
  ok: true;
  result: T;
}

export interface EngineRpcErr {
  id: string;
  ok: false;
  error: string;
}

/** 无 id 的服务端推送（如录制预览帧 / 重放进度）。 */
export interface EnginePushEvent {
  event: string;
  packet?: EnginePacketDto;
  error?: string;
  sent?: number;
  relativeUs?: number;
  durationUs?: number;
  stopped?: boolean;
}

export interface EngineRecordStartResult {
  started: boolean;
  out: string;
  channel: EngineChannelDto;
}

export interface EngineRecordStopResult {
  stopped: boolean;
  out: string;
  packetCount: number;
  durationUs: number;
  finalized: boolean;
  warning?: string;
}

export interface EngineReplayStartResult {
  started: boolean;
  type: string;
  config: Record<string, unknown>;
  durationUs: number;
  speed?: number;
  fromUs?: number;
  loop?: boolean;
}

export interface EngineReplayStopResult {
  stopped: boolean;
  sent: number;
}

export interface EngineReplayPauseResult {
  paused: boolean;
  already?: boolean;
}

export interface EngineReplayResumeResult {
  paused: boolean;
  already?: boolean;
}

export interface EngineReplaySeekResult {
  seeked: boolean;
  relativeUs: number;
  clamped?: boolean;
}

export interface EngineSessionExportResult {
  exported: boolean;
  packetCount: number;
  bytes: number;
  b64: string;
}
