/**
 * 壳 IPC：导出字节写入用户选定路径（U11）。
 * 约定：write_file|<id>|<绝对路径>|<base64>
 *
 * ponytail: 大包经 IPC 传 b64；若现场包极大再改引擎直写 out。
 */

import { hasShellIpc, shellRpc } from './shellRpc';
import { pickSaveFcapPath } from './recordPath';

function bytesToBase64(bytes: Uint8Array): string {
  // 分块避免超大 apply 栈溢出
  const chunk = 0x8000;
  let binary = '';
  for (let i = 0; i < bytes.length; i += chunk) {
    const sub = bytes.subarray(i, i + chunk);
    binary += String.fromCharCode(...sub);
  }
  return btoa(binary);
}

/** 同目录同基名 `.pcap`（纯路径变换，不访问磁盘）。 */
export function pcapPathBesideFcap(fcapPath: string): string {
  const trimmed = fcapPath.replace(/[/\\]+$/, '');
  if (/\.fcap$/i.test(trimmed)) {
    return trimmed.replace(/\.fcap$/i, '.pcap');
  }
  return `${trimmed}.pcap`;
}

/**
 * 将字节写入绝对路径；成功回传路径，取消/无壳/失败回传空串。
 */
export async function writeFileBytes(
  absPath: string,
  bytes: Uint8Array
): Promise<string> {
  const path = absPath.trim();
  if (!path || !hasShellIpc()) return '';
  const b64 = bytesToBase64(bytes);
  // 路径在 Windows 不含 `|`；与壳 splitn(4) 对齐
  return shellRpc((id) => `write_file|${id}|${path}|${b64}`, 120_000);
}

/**
 * 另存为选路径并写入 `.fcap`；取消另存为返回空串（不落盘）。
 */
export async function pickAndWriteFcap(
  bytes: Uint8Array,
  suggestedFileName?: string
): Promise<string> {
  const path = await pickSaveFcapPath(suggestedFileName);
  if (!path) return '';
  return writeFileBytes(path, bytes);
}

/**
 * 在已选 `.fcap` 旁写入同基名 `.pcap`；失败返回空串。
 */
export async function writePcapBesideFcap(
  fcapPath: string,
  pcapBytes: Uint8Array
): Promise<string> {
  const pcapPath = pcapPathBesideFcap(fcapPath);
  return writeFileBytes(pcapPath, pcapBytes);
}

export { hasShellIpc, pickSaveFcapPath };
