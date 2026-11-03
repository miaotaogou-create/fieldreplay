/**
 * 壳 IPC 请求/回传桥（`postMessage` → `__frShellResolve`）。
 * 录制路径与导出落盘共用，避免多份 pending 表互相覆盖。
 */

import { hasShellIpc } from './windowControls';

type Resolver = (value: string) => void;

declare global {
  interface Window {
    __frShellResolve?: (id: string, value: string) => void;
  }
}

const pending = new Map<string, Resolver>();
let seq = 0;
let bridgeReady = false;

function ensureBridge(): void {
  if (bridgeReady || typeof window === 'undefined') return;
  window.__frShellResolve = (id: string, value: string) => {
    const resolve = pending.get(id);
    if (!resolve) return;
    pending.delete(id);
    resolve(typeof value === 'string' ? value : '');
  };
  bridgeReady = true;
}

function nextId(): string {
  seq += 1;
  return `r${Date.now().toString(36)}_${seq}`;
}

/**
 * 向壳发消息并等待回传；超时/无壳返回空串。
 * `buildMessage(id)` 须把 id 编进约定格式（如 `pick_save_fcap|${id}|name`）。
 */
export async function shellRpc(
  buildMessage: (id: string) => string,
  timeoutMs: number
): Promise<string> {
  if (!hasShellIpc()) return '';
  try {
    const id = nextId();
    ensureBridge();
    return await new Promise<string>((resolve, reject) => {
      const timer = window.setTimeout(() => {
        pending.delete(id);
        reject(new Error('等待壳 IPC 超时'));
      }, timeoutMs);
      pending.set(id, (value) => {
        window.clearTimeout(timer);
        resolve(value);
      });
      window.ipc!.postMessage(buildMessage(id));
    });
  } catch {
    return '';
  }
}

export { hasShellIpc };
