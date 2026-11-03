/**
 * 壳 IPC：默认录制路径 / 另存为（U10）。
 * 约定：default_record_path|<id> / pick_save_fcap|<id>|<建议名?>
 */

import { hasShellIpc, shellRpc } from './shellRpc';

/** 向壳请求 `{EXE}\record\日期\建议名.fcap`；无壳或失败返回空串。 */
export async function requestDefaultRecordPath(): Promise<string> {
  return shellRpc((id) => `default_record_path|${id}`, 15_000);
}

/** 原生另存为；取消或无壳返回空串。 */
export async function pickSaveFcapPath(suggestedFileName?: string): Promise<string> {
  const name = (suggestedFileName ?? '').trim();
  return shellRpc((id) => {
    return name ? `pick_save_fcap|${id}|${name}` : `pick_save_fcap|${id}`;
  }, 120_000);
}

export { hasShellIpc };
