/**
 * 壳 IPC：默认录制路径 / 另存为（需 window.ipc + __frShellResolve）。
 * U10b 约定：default_record_path|<id> / pick_save_fcap|<id>|<建议名?>
 */

import { hasShellIpc } from './windowControls';

type Resolver = (path: string) => void;

declare global {
  interface Window {
    __frShellResolve?: (id: string, path: string) => void;
  }
}

const pending = new Map<string, Resolver>();
let seq = 0;
let bridgeReady = false;

function ensureBridge(): void {
  if (bridgeReady || typeof window === 'undefined') return;
  window.__frShellResolve = (id: string, path: string) => {
    const resolve = pending.get(id);
    if (!resolve) return;
    pending.delete(id);
    resolve(typeof path === 'string' ? path : '');
  };
  bridgeReady = true;
}

function nextId(): string {
  seq += 1;
  return `r${Date.now().toString(36)}_${seq}`;
}

/** 向壳请求 `{EXE}\record\日期\建议名.fcap`；无壳或失败返回空串。 */
export async function requestDefaultRecordPath(): Promise<string> {
  if (!hasShellIpc()) return '';
  try {
    const id = nextId();
    ensureBridge();
    return await new Promise<string>((resolve, reject) => {
      const timer = window.setTimeout(() => {
        pending.delete(id);
        reject(new Error('等待默认路径超时'));
      }, 15_000);
      pending.set(id, (path) => {
        window.clearTimeout(timer);
        resolve(path);
      });
      window.ipc!.postMessage(`default_record_path|${id}`);
    });
  } catch {
    return '';
  }
}

/** 原生另存为；取消或无壳返回空串。 */
export async function pickSaveFcapPath(suggestedFileName?: string): Promise<string> {
  if (!hasShellIpc()) return '';
  const id = nextId();
  ensureBridge();
  const name = (suggestedFileName ?? '').trim();
  const msg = name
    ? `pick_save_fcap|${id}|${name}`
    : `pick_save_fcap|${id}`;
  return new Promise<string>((resolve, reject) => {
    const timer = window.setTimeout(() => {
      pending.delete(id);
      reject(new Error('等待另存为超时'));
    }, 120_000);
    pending.set(id, (path) => {
      window.clearTimeout(timer);
      resolve(path);
    });
    window.ipc!.postMessage(msg);
  }).catch(() => '');
}

export { hasShellIpc };
