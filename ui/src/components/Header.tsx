/**
 * @license
 * SPDX-License-Identifier: Apache-2.0
 */

import React from 'react';
import { AppTab } from '../types/recorder';
import { Radio, PlayCircle, Layers, Sparkles } from 'lucide-react';
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
    <header className="flex items-center justify-between gap-3 px-3 sm:px-4 py-1.5 bg-white border-b border-slate-200 shrink-0 z-40">
      <div className="flex items-center gap-2.5 shrink-0">
        <div className="w-7 h-7 rounded-md bg-sky-600 text-white flex items-center justify-center font-bold text-xs shadow-sm">
          FR
        </div>
        <div>
          <span className="text-sm font-bold tracking-tight text-slate-900 whitespace-nowrap">
            FieldReplay
          </span>
          <span className="hidden lg:inline-block ml-2 text-[11px] text-slate-500 font-normal">
            上位机现场数据录制与高保真重演
          </span>
        </div>
      </div>

      <nav className="flex items-center gap-1" aria-label="主功能切换">
        {TABS.map(({ id, label, icon: Icon }) => {
          const active = currentTab === id;
          return (
            <button
              key={id}
              type="button"
              onClick={() => onTabChange(id)}
              aria-current={active ? 'page' : undefined}
              className={`flex items-center gap-1.5 px-2.5 py-1 text-xs font-medium rounded-md transition-colors whitespace-nowrap shrink-0 ${
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

      <div className="flex items-center gap-1.5 shrink-0">
        <span
          className={`px-2 py-0.5 text-[11px] font-medium rounded-md border whitespace-nowrap ${badge.className}`}
        >
          {badge.label}
        </span>
        <button
          type="button"
          onClick={onLoadPreset}
          className="flex items-center gap-1.5 px-2.5 py-1 text-xs font-medium text-slate-700 bg-slate-100 hover:bg-slate-200 rounded-md transition-colors whitespace-nowrap shrink-0"
          title="载入工业现场伺服卡滞过流故障案例数据包（离线演示）"
        >
          <Sparkles className="w-3.5 h-3.5 text-sky-600" />
          <span>离线演示样本</span>
        </button>
      </div>
    </header>
  );
};
