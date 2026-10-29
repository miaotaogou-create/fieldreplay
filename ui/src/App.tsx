/**
 * @license
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useEffect, useRef, useState } from 'react';
import {
  AppTab,
  ChannelConfig,
  CapturedPacket,
  CaptureSessionMetadata,
} from './types/recorder';
import {
  defaultChannels,
  generateServoAlarmPreset,
} from './utils/mockFieldData';
import { Header } from './components/Header';
import { FieldRecorderView } from './components/FieldRecorderView';
import { DevReplayerView } from './components/DevReplayerView';
import { PacketsView } from './components/PacketsView';
import { QtArchitectureGuide } from './components/QtArchitectureGuide';

const TOAST_MS = 3500;

export default function App() {
  const [currentTab, setCurrentTab] = useState<AppTab>('record');
  const [channels, setChannels] = useState<ChannelConfig[]>(defaultChannels);

  // 初始载入一份可演示的现场样本（mock）
  const initialPreset = generateServoAlarmPreset();
  const [sessionMetadata, setSessionMetadata] = useState<CaptureSessionMetadata>(
    initialPreset.metadata
  );
  const [recordedPackets, setRecordedPackets] = useState<CapturedPacket[]>(
    initialPreset.packets
  );

  const [notification, setNotification] = useState<string | null>(null);
  const toastTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    return () => {
      if (toastTimerRef.current) clearTimeout(toastTimerRef.current);
    };
  }, []);

  const showToast = (msg: string) => {
    if (toastTimerRef.current) clearTimeout(toastTimerRef.current);
    setNotification(msg);
    toastTimerRef.current = setTimeout(() => {
      setNotification(null);
      toastTimerRef.current = null;
    }, TOAST_MS);
  };

  const handleToggleChannel = (id: string) => {
    setChannels((prev) =>
      prev.map((c) => (c.id === id ? { ...c, enabled: !c.enabled } : c))
    );
  };

  const handleAddChannel = (newChan: ChannelConfig) => {
    setChannels((prev) => [...prev, newChan]);
    showToast(`已成功添加通道: ${newChan.name}`);
  };

  const handleAppendPacket = (packet: CapturedPacket) => {
    setRecordedPackets((prev) => [...prev, packet]);
  };

  const handleClearSession = () => {
    setRecordedPackets([]);
    showToast('已清空当前录制缓冲区');
  };

  const handleExportSession = (metadata: CaptureSessionMetadata) => {
    setSessionMetadata(metadata);
    const exportBundle = {
      format: 'FCAP-1.0',
      exportedAt: new Date().toISOString(),
      metadata,
      packets: recordedPackets.map((p) => ({
        id: p.id,
        timestampUs: p.timestampUs,
        absoluteTime: p.absoluteTime,
        channelId: p.channelId,
        channelName: p.channelName,
        channelType: p.channelType,
        direction: p.direction,
        length: p.length,
        hexPreview: p.hexPreview,
        asciiPreview: p.asciiPreview,
        protocolHint: p.protocolHint,
        parsedFields: p.parsedFields,
        hasError: p.hasError,
        errorReason: p.errorReason,
        data: Array.from(p.data),
      })),
    };

    const blob = new Blob([JSON.stringify(exportBundle, null, 2)], {
      type: 'application/json',
    });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = metadata.sessionTitle || `现场数据捕获包_${Date.now()}.fcap`;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);

    showToast('现场数据包已打包下载完成 (.fcap)，可直接发送给研发！');
  };

  const handleLoadServoPreset = () => {
    const preset = generateServoAlarmPreset();
    setSessionMetadata(preset.metadata);
    setRecordedPackets(preset.packets);
    showToast('已载入现场样本：伺服电机突发卡滞过流报警包');
  };

  const handleUploadSession = (
    metadata: CaptureSessionMetadata,
    packets: CapturedPacket[]
  ) => {
    setSessionMetadata(metadata);
    setRecordedPackets(packets);
    showToast(`已成功载入数据包：${metadata.sessionTitle} (${packets.length} 帧)`);
  };

  return (
    <div className="min-h-screen bg-slate-100/70 text-slate-900 flex flex-col font-sans antialiased">
      <Header
        currentTab={currentTab}
        onTabChange={setCurrentTab}
        onLoadPreset={handleLoadServoPreset}
      />

      {/* Toast：连点会重置计时，避免提前消失 */}
      {notification && (
        <div
          role="status"
          aria-live="polite"
          className="fixed bottom-6 right-6 z-50 bg-slate-900 text-white text-xs px-4 py-2.5 rounded-lg shadow-xl border border-slate-800 flex items-center gap-2 max-w-sm"
        >
          <span className="w-2 h-2 rounded-full bg-emerald-400 shrink-0" />
          <span>{notification}</span>
        </div>
      )}

      <main className="flex-1 pb-16">
        {currentTab === 'record' && (
          <FieldRecorderView
            channels={channels}
            onToggleChannel={handleToggleChannel}
            onAddChannel={handleAddChannel}
            recordedPackets={recordedPackets}
            onAppendPacket={handleAppendPacket}
            onClearSession={handleClearSession}
            onExportSession={handleExportSession}
            onGoToReplay={() => setCurrentTab('replay')}
          />
        )}

        {currentTab === 'replay' && (
          <DevReplayerView
            sessionMetadata={sessionMetadata}
            packets={recordedPackets}
            onUploadSession={handleUploadSession}
            onSelectPacketForInspection={() => setCurrentTab('packets')}
          />
        )}

        {currentTab === 'packets' && (
          <PacketsView packets={recordedPackets} onSelectPacket={() => {}} />
        )}

        {currentTab === 'qt_guide' && <QtArchitectureGuide />}
      </main>

      <footer className="py-4 border-t border-slate-200 text-center text-xs text-slate-500 bg-white">
        <span>FieldReplay · 工业上位机现场数据录制与高保真重演工作台</span>
        <span className="mx-2">·</span>
        <span>微秒时基无漂移 · 上位机改指向即可对接</span>
      </footer>
    </div>
  );
}
