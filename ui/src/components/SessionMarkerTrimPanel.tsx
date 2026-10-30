/**
 * @license
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useState } from 'react';
import { BookmarkPlus, Scissors } from 'lucide-react';

export interface SessionMarkerTrimPanelProps {
  engineConnected: boolean;
  hasPackets: boolean;
  /** 当前播放头相对微秒（离线插 marker 用） */
  currentTimeUs: number;
  /** 会话总时长（微秒），用于填默认 toUs */
  durationUs: number;
  /** 引擎重放进行中时拒绝 marker/trim */
  engineReplayBusy?: boolean;
  onAddMarker: (text: string, relativeUs: number) => void | Promise<void>;
  onTrimDownload: (fromUs: number, toUs: number) => void | Promise<void>;
}

function parseUs(raw: string): number | null {
  const t = raw.trim();
  if (!t) return null;
  const n = Number(t);
  if (!Number.isFinite(n) || n < 0 || !Number.isInteger(n)) return null;
  return n;
}

/** 重演页：插 marker + 时间窗 trim 另存（须连引擎）。 */
export const SessionMarkerTrimPanel: React.FC<SessionMarkerTrimPanelProps> = ({
  engineConnected,
  hasPackets,
  currentTimeUs,
  durationUs,
  engineReplayBusy = false,
  onAddMarker,
  onTrimDownload,
}) => {
  const [markerText, setMarkerText] = useState('');
  const [fromUs, setFromUs] = useState('0');
  const [toUs, setToUs] = useState('');
  const [busy, setBusy] = useState(false);

  const blocked = !engineConnected || !hasPackets || busy || engineReplayBusy;

  const handleMarker = async () => {
    if (!engineConnected) return;
    setBusy(true);
    try {
      await onAddMarker(markerText, currentTimeUs);
      setMarkerText('');
    } finally {
      setBusy(false);
    }
  };

  const handleTrim = async () => {
    if (!engineConnected) return;
    const from = parseUs(fromUs);
    const to = parseUs(toUs !== '' ? toUs : String(durationUs));
    if (from === null || to === null) return;
    setBusy(true);
    try {
      await onTrimDownload(from, to);
    } finally {
      setBusy(false);
    }
  };

  const fillToEnd = () => setToUs(String(durationUs));
  const fillFromPlayhead = () => setFromUs(String(currentTimeUs));
  const fillToPlayhead = () => setToUs(String(currentTimeUs));

  return (
    <div className="bg-white rounded-lg border border-slate-200 px-3 py-2 shadow-xs space-y-2 shrink-0">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h2 className="text-[11px] font-bold text-slate-800 uppercase tracking-wide">
          会话标注与裁剪
        </h2>
        {!engineConnected && (
          <span className="text-[11px] text-amber-700 bg-amber-50 border border-amber-200 px-2 py-0.5 rounded">
            未连引擎：不会本地假写入正式包
          </span>
        )}
        {engineConnected && engineReplayBusy && (
          <span className="text-[11px] text-amber-700 bg-amber-50 border border-amber-200 px-2 py-0.5 rounded">
            重放中请先停止，再插标记或裁剪
          </span>
        )}
      </div>

      <div className="flex flex-col lg:flex-row gap-2">
        <div className="flex-1 flex flex-wrap items-end gap-2 min-w-0">
          <label className="flex flex-col gap-0.5 min-w-[12rem] flex-1">
            <span className="text-[10px] text-slate-500">标记说明（写到 marker.aux）</span>
            <input
              type="text"
              value={markerText}
              onChange={(e) => setMarkerText(e.target.value)}
              placeholder="例如：异常出现 / 换料"
              disabled={!engineConnected || busy || engineReplayBusy}
              className="px-2 py-1 text-xs border border-slate-200 rounded-md bg-slate-50 focus:bg-white focus:outline-none focus:ring-1 focus:ring-sky-400 disabled:opacity-50"
            />
          </label>
          <div className="text-[10px] text-slate-500 font-mono pb-1.5 shrink-0">
            @ {currentTimeUs} µs
          </div>
          <button
            type="button"
            onClick={() => void handleMarker()}
            disabled={blocked}
            className="px-2.5 py-1 text-xs font-medium bg-violet-600 hover:bg-violet-500 text-white rounded-md disabled:opacity-40 disabled:cursor-not-allowed flex items-center gap-1.5 shrink-0"
          >
            <BookmarkPlus className="w-3.5 h-3.5" />
            在播放头插入标记
          </button>
        </div>

        <div className="hidden lg:block w-px bg-slate-200 self-stretch" />

        <div className="flex-1 flex flex-wrap items-end gap-2 min-w-0">
          <label className="flex flex-col gap-0.5 w-28">
            <span className="text-[10px] text-slate-500">fromUs</span>
            <input
              type="text"
              inputMode="numeric"
              value={fromUs}
              onChange={(e) => setFromUs(e.target.value)}
              disabled={!engineConnected || busy || engineReplayBusy}
              className="px-2 py-1 text-xs font-mono border border-slate-200 rounded-md bg-slate-50 focus:bg-white focus:outline-none focus:ring-1 focus:ring-sky-400 disabled:opacity-50"
            />
          </label>
          <label className="flex flex-col gap-0.5 w-28">
            <span className="text-[10px] text-slate-500">toUs（闭区间）</span>
            <input
              type="text"
              inputMode="numeric"
              value={toUs}
              onChange={(e) => setToUs(e.target.value)}
              placeholder={String(durationUs || 0)}
              disabled={!engineConnected || busy || engineReplayBusy}
              className="px-2 py-1 text-xs font-mono border border-slate-200 rounded-md bg-slate-50 focus:bg-white focus:outline-none focus:ring-1 focus:ring-sky-400 disabled:opacity-50"
            />
          </label>
          <div className="flex flex-wrap gap-1 pb-0.5">
            <button
              type="button"
              onClick={fillFromPlayhead}
              disabled={!hasPackets}
              className="px-1.5 py-0.5 text-[10px] text-slate-600 bg-slate-100 hover:bg-slate-200 rounded disabled:opacity-40"
            >
              from=播放头
            </button>
            <button
              type="button"
              onClick={fillToPlayhead}
              disabled={!hasPackets}
              className="px-1.5 py-0.5 text-[10px] text-slate-600 bg-slate-100 hover:bg-slate-200 rounded disabled:opacity-40"
            >
              to=播放头
            </button>
            <button
              type="button"
              onClick={fillToEnd}
              disabled={!hasPackets}
              className="px-1.5 py-0.5 text-[10px] text-slate-600 bg-slate-100 hover:bg-slate-200 rounded disabled:opacity-40"
            >
              to=末帧
            </button>
          </div>
          <button
            type="button"
            onClick={() => void handleTrim()}
            disabled={blocked}
            className="px-2.5 py-1 text-xs font-medium bg-slate-800 hover:bg-slate-700 text-white rounded-md disabled:opacity-40 disabled:cursor-not-allowed flex items-center gap-1.5 shrink-0"
          >
            <Scissors className="w-3.5 h-3.5" />
            另存裁剪 .fcap
          </button>
        </div>
      </div>

      <p className="text-[10px] text-slate-500 leading-snug">
        裁剪为闭区间再基；通道表整表保留。若窗切在 TCP 会话中途，重放表现类似 seek，属可接受限制。
      </p>
    </div>
  );
};
