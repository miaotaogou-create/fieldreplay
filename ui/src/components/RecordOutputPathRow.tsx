/**
 * U10c：录制落盘路径行（展示 / 手改 / 浏览 / 恢复默认；录中只读）。
 */

import React from 'react';
import { FolderOpen, RotateCcw } from 'lucide-react';
import {
  hasShellIpc,
  pickSaveFcapPath,
  requestDefaultRecordPath,
} from '../shell/recordPath';

export interface RecordOutputPathRowProps {
  value: string;
  onChange: (path: string) => void;
  /** 录制中 / 暂停时只读 */
  readOnly: boolean;
  /** 父级 busy 时禁用浏览与恢复 */
  disabled?: boolean;
  onHint?: (msg: string | null) => void;
}

function basenameHint(path: string): string | undefined {
  const name = path.trim().split(/[/\\]/).pop();
  return name || undefined;
}

export const RecordOutputPathRow: React.FC<RecordOutputPathRowProps> = ({
  value,
  onChange,
  readOnly,
  disabled = false,
  onHint,
}) => {
  const shell = hasShellIpc();
  const controlsOff = readOnly || disabled;

  const handleBrowse = async () => {
    if (controlsOff || !shell) return;
    onHint?.(null);
    const picked = await pickSaveFcapPath(basenameHint(value));
    if (picked) onChange(picked);
  };

  const handleRestoreDefault = async () => {
    if (controlsOff) return;
    onHint?.(null);
    const path = await requestDefaultRecordPath();
    if (path) {
      onChange(path);
      return;
    }
    onHint?.(
      shell
        ? '未能取得默认路径'
        : '无壳预览：请手填绝对路径；正式用法请双击壳 EXE'
    );
  };

  return (
    <div className="flex flex-col sm:flex-row sm:items-center gap-1.5 w-full min-w-0">
      <label className="text-[10px] text-slate-400 uppercase tracking-wider shrink-0 w-14">
        保存路径
      </label>
      <div className="flex flex-1 items-center gap-1.5 min-w-0">
        <input
          type="text"
          value={value}
          readOnly={readOnly}
          disabled={disabled && !readOnly}
          onChange={(e) => {
            if (!readOnly) onChange(e.target.value);
          }}
          placeholder={
            shell
              ? '正在解析默认路径…'
              : '手填绝对路径，例如 D:\\capture\\demo.fcap'
          }
          title={value || undefined}
          className={`flex-1 min-w-0 px-2 py-1 text-[11px] font-mono rounded-md border focus:outline-none focus:ring-1 focus:ring-sky-400 ${
            readOnly
              ? 'bg-slate-950 border-slate-700 text-slate-300 cursor-default'
              : 'bg-slate-800 border-slate-600 text-slate-100 placeholder:text-slate-500'
          }`}
        />
        <button
          type="button"
          disabled={controlsOff || !shell}
          onClick={() => void handleBrowse()}
          title={
            shell
              ? '浏览另存为'
              : '无壳预览：浏览不可用，请手填绝对路径'
          }
          className="shrink-0 flex items-center gap-1 px-2 py-1 rounded-md text-[11px] font-medium bg-slate-800 border border-slate-600 text-slate-200 hover:bg-slate-700 disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
        >
          <FolderOpen className="w-3.5 h-3.5" />
          <span>浏览</span>
        </button>
        <button
          type="button"
          disabled={controlsOff}
          onClick={() => void handleRestoreDefault()}
          title="恢复为 EXE\\record\\当天 建议路径"
          className="shrink-0 flex items-center gap-1 px-2 py-1 rounded-md text-[11px] font-medium bg-slate-800 border border-slate-600 text-slate-200 hover:bg-slate-700 disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
        >
          <RotateCcw className="w-3.5 h-3.5" />
          <span>默认</span>
        </button>
      </div>
    </div>
  );
};
