/**
 * 自定义标题栏（对照 Downloads 原型视觉；壳内无系统边框时使用）。
 */

import React from 'react';
import { Minus, Square, Copy, X } from 'lucide-react';

interface DesktopTitleBarProps {
  isMaximized: boolean;
  onToggleMaximize: () => void;
  onMinimize: () => void;
  onRequestClose: () => void;
}

export const DesktopTitleBar: React.FC<DesktopTitleBarProps> = ({
  isMaximized,
  onToggleMaximize,
  onMinimize,
  onRequestClose,
}) => {
  return (
    <div
      data-wry-drag-region
      className="bg-slate-900 text-slate-300 text-xs flex items-center justify-between px-3 h-8 select-none border-b border-slate-800 shrink-0 z-50 fr-titlebar-drag"
      onDoubleClick={(e) => {
        // 按钮区不响应双击最大化
        if ((e.target as HTMLElement).closest('button')) return;
        onToggleMaximize();
      }}
    >
      <div className="flex items-center gap-2 min-w-0 pointer-events-none">
        <div className="w-4 h-4 rounded bg-sky-500 text-white flex items-center justify-center text-[10px] font-bold shrink-0">
          FR
        </div>
        <span className="font-medium text-slate-200 tracking-wide text-[11px] truncate">
          FieldReplay - 现场数据录制与高保真重演工具
        </span>
      </div>

      <div className="flex items-center h-full -mr-3 fr-titlebar-no-drag">
        <button
          type="button"
          onClick={onMinimize}
          title="最小化"
          className="h-8 w-11 flex items-center justify-center text-slate-400 hover:text-white hover:bg-slate-800 transition-colors"
        >
          <Minus className="w-3.5 h-3.5" />
        </button>

        <button
          type="button"
          onClick={onToggleMaximize}
          title={isMaximized ? '还原' : '最大化'}
          className="h-8 w-11 flex items-center justify-center text-slate-400 hover:text-white hover:bg-slate-800 transition-colors"
        >
          {isMaximized ? (
            <Copy className="w-3 h-3 rotate-180" />
          ) : (
            <Square className="w-3 h-3" />
          )}
        </button>

        <button
          type="button"
          onClick={onRequestClose}
          title="关闭"
          className="h-8 w-11 flex items-center justify-center text-slate-400 hover:text-white hover:bg-rose-600 transition-colors"
        >
          <X className="w-3.5 h-3.5" />
        </button>
      </div>
    </div>
  );
};
