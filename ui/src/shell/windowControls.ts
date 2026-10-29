/**
 * 薄壳窗口控制：有 `window.ipc`（WebView2 / wry）时走原生；
 * 浏览器开发预览则降级（最大化用 Fullscreen API）。
 */

export type ShellWindowCommand =
  | 'minimize'
  | 'maximize'
  | 'close'
  | 'drag_window';

declare global {
  interface Window {
    ipc?: { postMessage: (message: string) => void };
  }
}

export function hasShellIpc(): boolean {
  return typeof window !== 'undefined' && typeof window.ipc?.postMessage === 'function';
}

export function postShellCommand(cmd: ShellWindowCommand): boolean {
  if (!hasShellIpc()) return false;
  window.ipc!.postMessage(cmd);
  return true;
}
