/**
 * @license
 * SPDX-License-Identifier: Apache-2.0
 */

import React from 'react';
import { AppTab } from '../types/recorder';
import { Radio, PlayCircle, Layers, FileCode2, Sparkles } from 'lucide-react';
import { EngineConnState } from '../engine/types';

interface HeaderProps {
  currentTab: AppTab;
  onTabChange: (tab: AppTab) => void;
  onLoadPreset: () => void;
  engineState: EngineConnState;
}

const TABS: { id: AppTab; label: string; icon: React.ElementType }[] = [
  { id: 'record', label: '现场录制模式', icon: Radio },
  { id: 'replay', label: '研发重演模式', icon: PlayCircle },
  { id: 'packets', label: '帧数据检查', icon: Layers },
  { id: 'qt_guide', label: '对接说明', icon: FileCode2 },
];

function engineBadge(state: EngineConnState): { label: string; className: string } {
  switch (state) {
    case 'connected':
      return {
        label: '引擎已连接',
        className: 'bg-emerald-50 text-emerald-700 border-emerald-200',
      };
    case 'connecting':
      return {
        label: '引擎连接中…',
        className: 'bg-amber-50 text-amber-700 border-amber-200',
      };
    case 'error':
      return {
        label: '引擎异常',
        className: 'bg-rose-50 text-rose-700 border-rose-200',
      };
    default:
      return {
        label: '引擎未连接',
        className: 'bg-slate-50 text-slate-500 border-slate-200',
      };
  }
}

export const Header: React.FC<HeaderProps> = ({
  currentTab,
  onTabChange,
  onLoadPreset,
  engineState,
}) => {
  const badge = engineBadge(engineState);
  return (
    <header className="flex items-center justify-between gap-8 px-6 py-3.5 bg-white border-b border-slate-200 sticky top-0 z-40">
      {/* Zone 1: wordmark */}
      <div className="flex items-center gap-3 shrink-0">
        <div className="w-8 h-8 rounded-lg bg-sky-600 text-white flex items-center justify-center font-bold text-sm shadow-sm">
          FR
        </div>
        <div>
          <span className="text-base font-bold tracking-tight text-slate-900 whitespace-nowrap">
            FieldReplay
          </span>
          <span className="hidden sm:inline-block ml-2 text-xs text-slate-500 font-normal">
            上位机现场数据录制与高保真重演
          </span>
        </div>
      </div>

      {/* Zone 2: 四 Tab */}
      <nav className="flex items-center gap-1 sm:gap-2" aria-label="主功能切换">
        {TABS.map(({ id, label, icon: Icon }) => {
          const active = currentTab === id;
          return (
            <button
              key={id}
              type="button"
              onClick={() => onTabChange(id)}
              aria-current={active ? 'page' : undefined}
              className={`flex items-center gap-2 px-3.5 py-1.5 text-xs font-medium rounded-lg transition-colors whitespace-nowrap shrink-0 ${
                active
                  ? 'bg-sky-50 text-sky-700 font-semibold'
                  : 'text-slate-600 hover:text-slate-900 hover:bg-slate-50'
              }`}
            >
              <Icon className="w-3.5 h-3.5" />
              <span>{label}</span>
            </button>
          );
        })}
      </nav>

      {/* Zone 3: 引擎状态 + 离线样本 */}
      <div className="flex items-center gap-2 shrink-0">
        <span
          className={`px-2 py-1 text-[11px] font-medium rounded-md border whitespace-nowrap ${badge.className}`}
          title="开发预览：请另开终端运行 fieldreplay-engine serve"
        >
          {badge.label}
        </span>
        <button
          type="button"
          onClick={onLoadPreset}
          className="flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium text-slate-700 bg-slate-100 hover:bg-slate-200 rounded-lg transition-colors whitespace-nowrap shrink-0"
          title="离线演示样本（不经引擎；引擎连上后请载入真实 .fcap）"
        >
          <Sparkles className="w-3.5 h-3.5 text-sky-600" />
          <span>离线演示样本</span>
        </button>
      </div>
    </header>
  );
};
