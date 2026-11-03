/**
 * 壳 IPC：原生打开 `.fcap`（U12）。
 * 约定：pick_open_fcap|<id>
 */

import { hasShellIpc, shellRpc } from './shellRpc';

/** 原生打开对话框；取消或无壳返回空串。 */
export async function pickOpenFcapPath(): Promise<string> {
  return shellRpc((id) => `pick_open_fcap|${id}`, 120_000);
}

export { hasShellIpc };
