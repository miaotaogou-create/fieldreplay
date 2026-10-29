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

export const Header: React.FC<HeaderProps> = ({ currentTab, onTabChange, onLoadPreset }) => {
  return (
    <header className="flex items-center justify-between gap-8 px-6 py-3.5 bg-white border-b border-slate-200 sticky top-0 z-40">
      {/* Zone 1: Single text element wordmark */}
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

      {/* Zone 2: Navigation Links / Mode Switcher */}
      <nav className="flex items-center gap-1 sm:gap-2">
        <button
          onClick={() => onTabChange('record')}
          className={`flex items-center gap-2 px-3.5 py-1.5 text-xs font-medium rounded-lg transition-colors whitespace-nowrap shrink-0 ${
            currentTab === 'record'
              ? 'bg-sky-50 text-sky-700 font-semibold'
              : 'text-slate-600 hover:text-slate-900 hover:bg-slate-50'
          }`}
        >
          <Radio className="w-3.5 h-3.5" />
          <span>现场录制模式</span>
        </button>

        <button
          onClick={() => onTabChange('replay')}
          className={`flex items-center gap-2 px-3.5 py-1.5 text-xs font-medium rounded-lg transition-colors whitespace-nowrap shrink-0 ${
            currentTab === 'replay'
              ? 'bg-sky-50 text-sky-700 font-semibold'
              : 'text-slate-600 hover:text-slate-900 hover:bg-slate-50'
          }`}
        >
          <PlayCircle className="w-3.5 h-3.5" />
          <span>研发重演模式</span>
        </button>

        <button
          onClick={() => onTabChange('packets')}
          className={`flex items-center gap-2 px-3.5 py-1.5 text-xs font-medium rounded-lg transition-colors whitespace-nowrap shrink-0 ${
            currentTab === 'packets'
              ? 'bg-sky-50 text-sky-700 font-semibold'
              : 'text-slate-600 hover:text-slate-900 hover:bg-slate-50'
          }`}
        >
          <Layers className="w-3.5 h-3.5" />
          <span>帧数据检查</span>
        </button>

        <button
          onClick={() => onTabChange('qt_guide')}
          className={`flex items-center gap-2 px-3.5 py-1.5 text-xs font-medium rounded-lg transition-colors whitespace-nowrap shrink-0 ${
            currentTab === 'qt_guide'
              ? 'bg-sky-50 text-sky-700 font-semibold'
              : 'text-slate-600 hover:text-slate-900 hover:bg-slate-50'
          }`}
        >
          <FileCode2 className="w-3.5 h-3.5" />
          <span>Qt上位机对接架构</span>
        </button>
      </nav>

      {/* Zone 3: 1 primary action */}
      <div className="flex items-center gap-2 shrink-0">
        <button
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
