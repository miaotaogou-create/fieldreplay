/**
 * @license
 * SPDX-License-Identifier: Apache-2.0
 */

import { bytesToBase64 } from './bytes';
import {
  EngineConnState,
  EngineRpcErr,
  EngineRpcOk,
  EngineSessionDto,
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
    let msg: EngineRpcOk<unknown> | EngineRpcErr;
    try {
      msg = JSON.parse(raw);
    } catch {
      return;
    }
    const id = msg.id;
    const p = this.pending.get(id);
    if (!p) return;
    this.pending.delete(id);
    if (msg.ok) p.resolve(msg.result);
    else p.reject(new Error(msg.error || '引擎返回错误'));
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
}

export function defaultEngineUrl(): string {
  const fromEnv = (import.meta as { env?: Record<string, string> }).env
    ?.VITE_ENGINE_WS;
  return fromEnv || DEFAULT_URL;
}
