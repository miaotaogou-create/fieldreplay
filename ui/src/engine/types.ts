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

/** 无 id 的服务端推送（如录制预览帧）。 */
export interface EnginePushEvent {
  event: string;
  packet?: EnginePacketDto;
  error?: string;
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
