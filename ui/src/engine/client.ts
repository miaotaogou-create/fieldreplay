/**
 * @license
 * SPDX-License-Identifier: Apache-2.0
 */

import { base64ToBytes, bytesToBase64 } from './bytes';
import {
  EngineConnState,
  EnginePacketDto,
  EnginePushEvent,
  EngineRecordStartResult,
  EngineRecordStopResult,
  EngineReplayPauseResult,
  EngineReplayResumeResult,
  EngineReplaySeekResult,
  EngineReplayStartResult,
  EngineReplayStopResult,
  EngineRpcErr,
  EngineRpcOk,
  EngineSessionDto,
  EngineSessionExportResult,
  EngineSessionExportPcapResult,
  EngineAddMarkerResult,
  EngineTrimResult,
  EngineSerialListResult,
} from './types';

const DEFAULT_URL = 'ws://127.0.0.1:17890';

type Pending = {
  resolve: (v: unknown) => void;
  reject: (e: Error) => void;
};

export class EngineClient {
  private url: string;
  private ws: WebSocket | null = null;
  private seq = 1;
  private pending = new Map<string, Pending>();
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private stopped = false;

  state: EngineConnState = 'disconnected';
  lastError: string | null = null;
  onStateChange: ((s: EngineConnState) => void) | null = null;
  /** 引擎主动推送（无 id），如 record.packet */
  onEvent: ((ev: EnginePushEvent) => void) | null = null;

  constructor(url = DEFAULT_URL) {
    this.url = url;
  }

  start() {
    this.stopped = false;
    this.connect();
  }

  stop() {
    this.stopped = true;
    if (this.reconnectTimer) clearTimeout(this.reconnectTimer);
    this.reconnectTimer = null;
    this.ws?.close();
    this.ws = null;
    this.setState('disconnected');
  }

  get connected() {
    return this.state === 'connected';
  }

  private setState(s: EngineConnState) {
    this.state = s;
    this.onStateChange?.(s);
  }

  private connect() {
    if (this.stopped) return;
    this.setState('connecting');
    try {
      const ws = new WebSocket(this.url);
      this.ws = ws;
      ws.onopen = () => {
        this.lastError = null;
        this.setState('connected');
        void this.request('ping', {}).catch(() => {
          /* 握手后探活失败会走 onclose */
        });
      };
      ws.onmessage = (ev) => {
        if (typeof ev.data !== 'string') return;
        this.onMessage(ev.data);
      };
      ws.onerror = () => {
        this.lastError = 'WebSocket 错误';
      };
      ws.onclose = () => {
        this.failAllPending('引擎连接已断开');
        this.ws = null;
        this.setState('disconnected');
        if (!this.stopped) {
          this.reconnectTimer = setTimeout(() => this.connect(), 2000);
        }
      };
    } catch (e) {
      this.lastError = e instanceof Error ? e.message : String(e);
      this.setState('error');
      if (!this.stopped) {
        this.reconnectTimer = setTimeout(() => this.connect(), 2000);
      }
    }
  }

  private onMessage(raw: string) {
    let msg: (EngineRpcOk<unknown> | EngineRpcErr | EnginePushEvent) & {
      id?: string;
      event?: string;
    };
    try {
      msg = JSON.parse(raw);
    } catch {
      return;
    }

    // 无 id 的推送事件
    if (msg.event && !msg.id) {
      this.onEvent?.(msg as EnginePushEvent);
      return;
    }

    const id = msg.id;
    if (!id) return;
    const p = this.pending.get(id);
    if (!p) return;
    this.pending.delete(id);
    const rpc = msg as EngineRpcOk<unknown> | EngineRpcErr;
    if (rpc.ok) p.resolve(rpc.result);
    else p.reject(new Error(rpc.error || '引擎返回错误'));
  }

  private failAllPending(reason: string) {
    for (const [, p] of this.pending) p.reject(new Error(reason));
    this.pending.clear();
  }

  request<T = unknown>(cmd: string, fields: Record<string, unknown> = {}): Promise<T> {
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
      return Promise.reject(new Error('引擎未连接'));
    }
    const id = String(this.seq++);
    const payload = JSON.stringify({ id, cmd, ...fields });
    return new Promise<T>((resolve, reject) => {
      this.pending.set(id, {
        resolve: (v) => resolve(v as T),
        reject,
      });
      this.ws!.send(payload);
    });
  }

  loadSessionBytes(bytes: Uint8Array): Promise<EngineSessionDto> {
    return this.request<EngineSessionDto>('session.load', {
      b64: bytesToBase64(bytes),
    });
  }

  clearSession(): Promise<void> {
    return this.request('session.clear', {}).then(() => undefined);
  }

  /** 导出内存会话为二进制 `.fcap`（base64）；可选覆盖 SessionMeta。 */
  exportSession(meta?: Record<string, unknown>): Promise<EngineSessionExportResult> {
    const fields: Record<string, unknown> = {};
    if (meta) fields.meta = meta;
    return this.request<EngineSessionExportResult>('session.export', fields);
  }

  /** 导出并解码为字节，供浏览器下载。 */
  async exportSessionBytes(meta?: Record<string, unknown>): Promise<Uint8Array> {
    const result = await this.exportSession(meta);
    return base64ToBytes(result.b64);
  }

  /** 导出内存会话为经典 `.pcap`（base64）。 */
  exportSessionPcap(): Promise<EngineSessionExportPcapResult> {
    return this.request<EngineSessionExportPcapResult>('session.exportPcap', {});
  }

  async exportSessionPcapBytes(): Promise<{
    bytes: Uint8Array;
    written: number;
    skipped: number;
  }> {
    const result = await this.exportSessionPcap();
    return {
      bytes: base64ToBytes(result.b64),
      written: result.written,
      skipped: result.skipped,
    };
  }

  startRecord(args: {
    /** 旧单路；有 `channels` 时引擎优先走多路 */
    type?: string;
    name?: string;
    out?: string;
    sessionTitle?: string;
    config?: Record<string, unknown>;
    channels?: Array<{
      type: string;
      name?: string;
      config: Record<string, unknown>;
    }>;
  }): Promise<EngineRecordStartResult> {
    return this.request<EngineRecordStartResult>('record.start', args);
  }

  stopRecord(): Promise<EngineRecordStopResult> {
    return this.request<EngineRecordStopResult>('record.stop', {});
  }

  startReplay(args: {
    /** 单路；与 targets 二选一（引擎：非空 targets 优先） */
    type?: string;
    config?: Record<string, unknown>;
    topicPrefix?: string;
    /** E11c/E11d 多路目标（按通道 index 绑定） */
    targets?: Array<{
      ch: number;
      type: string;
      config: Record<string, unknown>;
    }>;
    /** 墙钟倍速；默认 1.0 */
    speed?: number;
    /** 从该相对微秒起播（跳过更早数据帧） */
    fromUs?: number;
    /** 播完后回卷循环，直到 stop */
    loop?: boolean;
    /** 引擎侧故障注入（丢包 / 篡改载荷） */
    fault?: {
      enabled: boolean;
      lossRatePercent?: number;
      corruptCrc?: boolean;
    };
  }): Promise<EngineReplayStartResult> {
    return this.request<EngineReplayStartResult>('replay.start', args);
  }

  stopReplay(): Promise<EngineReplayStopResult> {
    return this.request<EngineReplayStopResult>('replay.stop', {});
  }

  pauseReplay(): Promise<EngineReplayPauseResult> {
    return this.request<EngineReplayPauseResult>('replay.pause', {});
  }

  resumeReplay(): Promise<EngineReplayResumeResult> {
    return this.request<EngineReplayResumeResult>('replay.resume', {});
  }

  seekReplay(relativeUs: number): Promise<EngineReplaySeekResult> {
    return this.request<EngineReplaySeekResult>('replay.seek', { relativeUs });
  }

  /** E8a：枚举本机串口（Windows COM） */
  listSerialPorts(): Promise<EngineSerialListResult> {
    return this.request<EngineSerialListResult>('serial.list', {});
  }

  /** E13b：插入 marker；录制中省略 relativeUs，离线必填 */
  addMarker(args: {
    text: string;
    relativeUs?: number;
    ch?: number;
  }): Promise<EngineAddMarkerResult> {
    const fields: Record<string, unknown> = { text: args.text };
    if (args.relativeUs !== undefined) fields.relativeUs = args.relativeUs;
    if (args.ch !== undefined) fields.ch = args.ch;
    return this.request<EngineAddMarkerResult>('session.addMarker', fields);
  }

  /** E13c：按相对时间窗裁出新 `.fcap`（默认不替换内存会话） */
  trimSession(args: {
    fromUs: number;
    toUs: number;
    replaceSession?: boolean;
  }): Promise<EngineTrimResult> {
    const fields: Record<string, unknown> = {
      fromUs: args.fromUs,
      toUs: args.toUs,
    };
    if (args.replaceSession !== undefined) {
      fields.replaceSession = args.replaceSession;
    }
    return this.request<EngineTrimResult>('session.trim', fields);
  }

  async trimSessionBytes(args: {
    fromUs: number;
    toUs: number;
    replaceSession?: boolean;
  }): Promise<{ bytes: Uint8Array; packetCount: number; replaced: boolean }> {
    const result = await this.trimSession(args);
    return {
      bytes: base64ToBytes(result.b64),
      packetCount: result.packetCount,
      replaced: result.replaced,
    };
  }
}

export function defaultEngineUrl(): string {
  const fromEnv = (import.meta as { env?: Record<string, string> }).env
    ?.VITE_ENGINE_WS;
  return fromEnv || DEFAULT_URL;
}

export type { EnginePacketDto };
