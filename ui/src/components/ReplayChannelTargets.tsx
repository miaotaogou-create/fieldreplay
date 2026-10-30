/**
 * @license
 * SPDX-License-Identifier: Apache-2.0
 */

import React from 'react';
import {
  ChannelReplayTargetRow,
  isMultiReplayCapable,
} from '../engine/multiReplayTargets';

interface ReplayChannelTargetsProps {
  rows: ChannelReplayTargetRow[];
  disabled?: boolean;
  onChange: (rows: ChannelReplayTargetRow[]) => void;
}

/** E11d：会话内各通道重演目标（多路对齐；本阶段仅 UDP/视频 UDP）。 */
export const ReplayChannelTargets: React.FC<ReplayChannelTargetsProps> = ({
  rows,
  disabled = false,
  onChange,
}) => {
  const patch = (channelId: string, partial: Partial<ChannelReplayTargetRow>) => {
    onChange(
      rows.map((r) => (r.channelId === channelId ? { ...r, ...partial } : r))
    );
  };

  const enabledCapable = rows.filter(
    (r) => r.enabled && isMultiReplayCapable(r.type)
  ).length;
  const willSkip = rows.length - enabledCapable;

  return (
    <div className="space-y-2 text-xs">
      <p className="text-[11px] text-slate-500 leading-snug">
        一键重演按<strong className="font-semibold text-slate-700">整会话时间轴</strong>
        对齐调度；缺目标或本阶段不支持的通道将跳过（
        {enabledCapable} 路启用
        {willSkip > 0 ? ` · ${willSkip} 路跳过` : ''}
        ）。多路 sink 暂仅 UDP / 视频 UDP 推送。
      </p>

      <div className="divide-y divide-slate-100 border border-slate-200 rounded-md overflow-hidden">
        {rows.map((row) => {
          const capable = isMultiReplayCapable(row.type);
          return (
            <div
              key={row.channelId}
              className={`px-2 py-1.5 flex flex-col sm:flex-row sm:items-center gap-1.5 ${
                capable ? 'bg-white' : 'bg-slate-50/80'
              }`}
            >
              <label className="flex items-center gap-1.5 min-w-0 sm:w-44 shrink-0">
                <input
                  type="checkbox"
                  checked={row.enabled && capable}
                  disabled={disabled || !capable}
                  onChange={(e) =>
                    patch(row.channelId, { enabled: e.target.checked })
                  }
                  className="rounded border-slate-300 text-sky-600"
                />
                <span className="truncate font-medium text-slate-800" title={row.name}>
                  [{row.channelIndex}] {row.name}
                </span>
              </label>

              <span className="text-[10px] font-mono text-slate-500 shrink-0 w-24">
                {row.type}
                {row.packetCount > 0 ? ` · ${row.packetCount}帧` : ''}
              </span>

              {capable ? (
                <div className="flex items-center gap-1 font-mono flex-1 min-w-0">
                  <input
                    type="text"
                    value={row.targetIp}
                    disabled={disabled || !row.enabled}
                    onChange={(e) =>
                      patch(row.channelId, { targetIp: e.target.value })
                    }
                    className="bg-slate-50 border border-slate-300 rounded px-1.5 py-0.5 text-xs text-slate-800 w-28 disabled:opacity-50"
                  />
                  <span className="text-slate-400">:</span>
                  <input
                    type="number"
                    value={row.targetPort}
                    disabled={disabled || !row.enabled}
                    onChange={(e) =>
                      patch(row.channelId, {
                        targetPort: parseInt(e.target.value, 10) || 0,
                      })
                    }
                    className="bg-slate-50 border border-slate-300 rounded px-1.5 py-0.5 text-xs text-slate-800 w-20 disabled:opacity-50"
                  />
                </div>
              ) : (
                <span className="text-[10px] text-amber-700/90">
                  多路对齐暂不支持，将跳过（单通道会话可用上方协议页）
                </span>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
};
