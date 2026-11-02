/**
 * 录制页通道卡片右键菜单（编辑 / 删除）。
 */

import React, { useEffect, useRef } from 'react';

export interface ChannelCardContextMenuState {
  channelId: string;
  channelName: string;
  x: number;
  y: number;
}

interface Props {
  menu: ChannelCardContextMenuState;
  actionsLocked: boolean;
  lockReason: string;
  onEdit: (channelId: string) => void;
  onDelete: (channelId: string, channelName: string) => void;
  onClose: () => void;
}

export const ChannelCardContextMenu: React.FC<Props> = ({
  menu,
  actionsLocked,
  lockReason,
  onEdit,
  onDelete,
  onClose,
}) => {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    };
    document.addEventListener('keydown', onKey);
    document.addEventListener('mousedown', onDown);
    return () => {
      document.removeEventListener('keydown', onKey);
      document.removeEventListener('mousedown', onDown);
    };
  }, [onClose]);

  // 尽量不超出视口
  const left = Math.min(menu.x, window.innerWidth - 160);
  const top = Math.min(menu.y, window.innerHeight - 100);

  return (
    <div
      ref={ref}
      role="menu"
      className="fixed z-[60] min-w-[9rem] rounded-md border border-slate-200 bg-white py-1 shadow-lg text-xs"
      style={{ left, top }}
    >
      <button
        type="button"
        role="menuitem"
        disabled={actionsLocked}
        title={actionsLocked ? lockReason : undefined}
        className="w-full px-3 py-1.5 text-left text-slate-800 hover:bg-slate-50 disabled:cursor-not-allowed disabled:text-slate-400 disabled:hover:bg-white"
        onClick={() => {
          if (actionsLocked) return;
          onEdit(menu.channelId);
          onClose();
        }}
      >
        编辑
      </button>
      <button
        type="button"
        role="menuitem"
        disabled={actionsLocked}
        title={actionsLocked ? lockReason : undefined}
        className="w-full px-3 py-1.5 text-left text-rose-700 hover:bg-rose-50 disabled:cursor-not-allowed disabled:text-slate-400 disabled:hover:bg-white"
        onClick={() => {
          if (actionsLocked) return;
          onDelete(menu.channelId, menu.channelName);
          onClose();
        }}
      >
        删除
      </button>
      {actionsLocked && (
        <div className="px-3 py-1 text-[10px] text-slate-400 border-t border-slate-100 mt-0.5">
          {lockReason}
        </div>
      )}
    </div>
  );
};
