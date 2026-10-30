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
  /** 旧单路响应 */
  channel?: EngineChannelDto;
  /** E11a 多路响应 */
  channels?: Array<Pick<EngineChannelDto, 'index' | 'type' | 'name'>>;
}

export interface EngineRecordStopResult {
  stopped: boolean;
  out: string;
  packetCount: number;
  durationUs: number;
  finalized: boolean;
  warning?: string;
}

/** E16c：与 replay.pause 同形 */
export interface EngineRecordPauseResult {
  paused: boolean;
  already?: boolean;
}

export interface EngineRecordResumeResult {
  paused: boolean;
  already?: boolean;
}

export interface EngineReplaySkippedChannel {
  ch: number;
  type: string;
  name: string;
}

export interface EngineReplayTargetDto {
  ch: number;
  type: string;
  config: Record<string, unknown>;
}

export interface EngineReplayStartResult {
  started: boolean;
  /** 单路响应 */
  type?: string;
  config?: Record<string, unknown>;
  /** E11c/E11d 多路响应 */
  targets?: EngineReplayTargetDto[];
  skipped?: EngineReplaySkippedChannel[];
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

/** E10：经典 pcap 导出 */
export interface EngineSessionExportPcapResult {
  exported: boolean;
  written: number;
  skipped: number;
  bytes: number;
  b64: string;
}

/** E13b：插入 marker */
export interface EngineAddMarkerResult {
  added: boolean;
  live: boolean;
  kind: number;
  relativeUs: number;
  ch: number;
  text: string;
}

/** E13c：时间窗 trim */
export interface EngineTrimResult {
  trimmed: boolean;
  fromUs: number;
  toUs: number;
  packetCount: number;
  bytes: number;
  replaced: boolean;
  b64: string;
}

/** E14d：经典 pcap → 会话 */
export interface EngineImportPcapResult {
  imported: boolean;
  written: number;
  skipped: number;
  channelCount: number;
  packetCount: number;
  bytes: number;
  replaced: boolean;
  b64: string;
  session: EngineSessionDto;
}

/** E8a：本机串口枚举 */
export interface EngineSerialListResult {
  ports: string[];
}
