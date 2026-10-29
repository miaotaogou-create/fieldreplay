/**
 * @license
 * SPDX-License-Identifier: Apache-2.0
 */

import React from 'react';
import { AppTab } from '../types/recorder';
import { Radio, PlayCircle, Layers, FileCode2, Sparkles } from 'lucide-react';

interface HeaderProps {
  currentTab: AppTab;
  onTabChange: (tab: AppTab) => void;
  onLoadPreset: () => void;
}

const TABS: { id: AppTab; label: string; icon: React.ElementType }[] = [
  { id: 'record', label: '现场录制模式', icon: Radio },
  { id: 'replay', label: '研发重演模式', icon: PlayCircle },
  { id: 'packets', label: '帧数据检查', icon: Layers },
  { id: 'qt_guide', label: '对接说明', icon: FileCode2 },
];

export const Header: React.FC<HeaderProps> = ({ currentTab, onTabChange, onLoadPreset }) => {
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

      {/* Zone 3: 载入样本 */}
      <div className="flex items-center gap-2 shrink-0">
        <button
          type="button"
          onClick={onLoadPreset}
          className="flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium text-slate-700 bg-slate-100 hover:bg-slate-200 rounded-lg transition-colors whitespace-nowrap shrink-0"
          title="载入工业现场伺服卡滞过流故障案例数据包"
        >
          <Sparkles className="w-3.5 h-3.5 text-sky-600" />
          <span>载入现场故障样本</span>
        </button>
      </div>
    </header>
  );
};
