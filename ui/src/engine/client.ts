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

  startRecord(args: {
    type: string;
    name?: string;
    out?: string;
    sessionTitle?: string;
    config: Record<string, unknown>;
  }): Promise<EngineRecordStartResult> {
    return this.request<EngineRecordStartResult>('record.start', args);
  }

  stopRecord(): Promise<EngineRecordStopResult> {
    return this.request<EngineRecordStopResult>('record.stop', {});
  }

  startReplay(args: {
    type: string;
    config: Record<string, unknown>;
    topicPrefix?: string;
    /** 墙钟倍速；默认 1.0 */
    speed?: number;
    /** 从该相对微秒起播（跳过更早数据帧） */
    fromUs?: number;
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
}

export function defaultEngineUrl(): string {
  const fromEnv = (import.meta as { env?: Record<string, string> }).env
    ?.VITE_ENGINE_WS;
  return fromEnv || DEFAULT_URL;
}

export type { EnginePacketDto };
