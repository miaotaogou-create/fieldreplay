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
import { channelToEngineConfig, pickRecordChannel } from './engine/channelConfig';
import { defaultEngineUrl, EngineClient } from './engine/client';
import {
  replayConfigToEngine,
  ReplayTargetType,
} from './engine/replayTargetConfig';
import { EngineConnState } from './engine/types';
import {
  emptySessionMetadata,
  mapEnginePacket,
  mapEngineSession,
  metadataToFcapMeta,
} from './engine/sessionMap';
import { ReplayConfig } from './types/recorder';

const TOAST_MS = 3500;

export default function App() {
  const [currentTab, setCurrentTab] = useState<AppTab>('record');
  const [channels, setChannels] = useState<ChannelConfig[]>(defaultChannels);
  const [sessionMetadata, setSessionMetadata] = useState<CaptureSessionMetadata>(
    () => emptySessionMetadata()
  );
  const [recordedPackets, setRecordedPackets] = useState<CapturedPacket[]>([]);
  const [engineState, setEngineState] = useState<EngineConnState>('disconnected');
  const [engineReplayTick, setEngineReplayTick] = useState<{
    sent: number;
    relativeUs: number;
    durationUs: number;
  } | null>(null);
  const [engineReplayDoneSeq, setEngineReplayDoneSeq] = useState(0);

  const [notification, setNotification] = useState<string | null>(null);
  const toastTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const toastFnRef = useRef<(msg: string) => void>(() => {});
  const engineRef = useRef<EngineClient | null>(null);
  const liveChannelRef = useRef<{
    id: string;
    name: string;
    type: ChannelConfig['type'];
  } | null>(null);
  const recordStartWallUsRef = useRef(0);

  const showToast = (msg: string) => {
    if (toastTimerRef.current) clearTimeout(toastTimerRef.current);
    setNotification(msg);
    toastTimerRef.current = setTimeout(() => {
      setNotification(null);
      toastTimerRef.current = null;
    }, TOAST_MS);
  };
  toastFnRef.current = showToast;

  useEffect(() => {
    const client = new EngineClient(defaultEngineUrl());
    engineRef.current = client;
    client.onStateChange = setEngineState;
    client.onEvent = (ev) => {
      if (ev.event === 'record.packet' && ev.packet) {
        const ch = liveChannelRef.current ?? {
          id: `ch_${ev.packet.ch}`,
          name: `通道 ${ev.packet.ch}`,
          type: 'udp' as const,
        };
        const mapped = mapEnginePacket(
          ev.packet,
          ch,
          recordStartWallUsRef.current
        );
        setRecordedPackets((prev) => [...prev, mapped]);
      } else if (ev.event === 'record.error' && ev.error) {
        toastFnRef.current(`录制错误：${ev.error}`);
      } else if (ev.event === 'replay.progress') {
        setEngineReplayTick({
          sent: ev.sent ?? 0,
          relativeUs: ev.relativeUs ?? 0,
          durationUs: ev.durationUs ?? 0,
        });
      } else if (ev.event === 'replay.done') {
        setEngineReplayTick({
          sent: ev.sent ?? 0,
          relativeUs: 0,
          durationUs: 0,
        });
        setEngineReplayDoneSeq((n) => n + 1);
        toastFnRef.current(
          ev.stopped
            ? `重放已停止（已发 ${ev.sent ?? 0} 帧）`
            : `重放完成（已发 ${ev.sent ?? 0} 帧）`
        );
      }
    };
    client.start();
    return () => {
      client.onEvent = null;
      client.stop();
      engineRef.current = null;
      if (toastTimerRef.current) clearTimeout(toastTimerRef.current);
    };
  }, []);

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

  const handleEngineStartRecord = async (channel: ChannelConfig) => {
    const engine = engineRef.current;
    if (!engine?.connected) {
      throw new Error('引擎未连接');
    }
    const config = channelToEngineConfig(channel);
    liveChannelRef.current = {
      id: channel.id,
      name: channel.name,
      type: channel.type,
    };
    recordStartWallUsRef.current = Date.now() * 1000;
    setRecordedPackets([]);
    await engine.startRecord({
      type: channel.type,
      name: channel.name,
      sessionTitle: `现场录制-${channel.name}`,
      config,
    });
    showToast(`已开始真实录制：${channel.name}（${channel.type}）`);
  };

  const handleEngineStopRecord = async () => {
    const engine = engineRef.current;
    if (!engine?.connected) {
      throw new Error('引擎未连接');
    }
    const result = await engine.stopRecord();
    liveChannelRef.current = null;
    showToast(
      `录制已停止：${result.packetCount} 帧已写入 ${result.out.split(/[/\\]/).pop()}`
    );
    return result;
  };

  const handleEngineStartReplay = async (
    tab: ReplayTargetType,
    cfg: ReplayConfig
  ) => {
    const engine = engineRef.current;
    if (!engine?.connected) {
      throw new Error('引擎未连接');
    }
    if (recordedPackets.length === 0) {
      throw new Error('会话无帧，请先载入 .fcap');
    }
    const mapped = replayConfigToEngine(tab, cfg);
    setEngineReplayTick(null);
    await engine.startReplay({
      type: mapped.type,
      config: mapped.config,
      topicPrefix: mapped.topicPrefix,
    });
    showToast(`已开始引擎重放：${mapped.type}`);
  };

  const handleEngineStopReplay = async () => {
    const engine = engineRef.current;
    if (!engine?.connected) {
      throw new Error('引擎未连接');
    }
    const result = await engine.stopReplay();
    showToast(`重放已停止：已发 ${result.sent} 帧`);
    return result;
  };

  const handleClearSession = () => {
    setRecordedPackets([]);
    setSessionMetadata(emptySessionMetadata());
    void engineRef.current?.clearSession().catch(() => undefined);
    showToast('已清空当前录制缓冲区');
  };

  const downloadBlob = (blob: Blob, fileName: string) => {
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = fileName.endsWith('.fcap') ? fileName : `${fileName}.fcap`;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
  };

  const handleExportSession = async (metadata: CaptureSessionMetadata) => {
    setSessionMetadata(metadata);
    const fileName =
      metadata.sessionTitle || `现场数据捕获包_${Date.now()}.fcap`;
    const engine = engineRef.current;

    if (engine?.connected) {
      try {
        const bytes = await engine.exportSessionBytes(metadataToFcapMeta(metadata));
        if (
          bytes.length < 4
          || bytes[0] !== 0x46
          || bytes[1] !== 0x43
          || bytes[2] !== 0x41
          || bytes[3] !== 0x50
        ) {
          throw new Error('引擎返回内容不是二进制 .fcap');
        }
        const ab = new ArrayBuffer(bytes.byteLength);
        new Uint8Array(ab).set(bytes);
        downloadBlob(new Blob([ab], { type: 'application/octet-stream' }), fileName);
        showToast('已导出真实二进制 .fcap，可直接发给研发重放');
        return;
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        showToast(`导出失败：${msg}`);
        return;
      }
    }

    // 引擎未连接：离线演示仍下发 JSON 包（后缀 .fcap，仅供 UI 演示）
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
    downloadBlob(
      new Blob([JSON.stringify(exportBundle, null, 2)], { type: 'application/json' }),
      fileName
    );
    showToast('已下载离线演示包（JSON）；连接引擎后可导出真实二进制 .fcap');
  };

  const handleLoadServoPreset = () => {
    const preset = generateServoAlarmPreset();
    setSessionMetadata(preset.metadata);
    setRecordedPackets(preset.packets);
    showToast('已载入离线演示样本（引擎未参与解析）');
  };

  const handleUploadSession = (
    metadata: CaptureSessionMetadata,
    packets: CapturedPacket[]
  ) => {
    setSessionMetadata(metadata);
    setRecordedPackets(packets);
    showToast(`已成功载入数据包：${metadata.sessionTitle} (${packets.length} 帧)`);
  };

  const handleLoadCaptureFile = async (file: File) => {
    const engine = engineRef.current;
    const lower = file.name.toLowerCase();

    if (engine?.connected && (lower.endsWith('.fcap') || !lower.endsWith('.json'))) {
      try {
        const buf = new Uint8Array(await file.arrayBuffer());
        // 二进制 .fcap 以 FCAP magic 开头；旧 JSON 演示包走下方分支
        const isBinary =
          buf.length >= 4
          && buf[0] === 0x46
          && buf[1] === 0x43
          && buf[2] === 0x41
          && buf[3] === 0x50;
        if (isBinary) {
          const dto = await engine.loadSessionBytes(buf);
          const mapped = mapEngineSession(dto);
          setSessionMetadata(mapped.metadata);
          setRecordedPackets(mapped.packets);
          showToast(
            `已通过引擎载入：${mapped.metadata.sessionTitle}（${mapped.packets.length} 帧）`
          );
          return;
        }
      } catch (e) {
        showToast(
          `引擎载入失败：${e instanceof Error ? e.message : String(e)}`
        );
        return;
      }
    }

    try {
      const text = await file.text();
      const parsed = JSON.parse(text);
      if (parsed.metadata && parsed.packets) {
        const rehydrated = parsed.packets.map(
          (p: CapturedPacket & { data?: number[] | Record<string, number> }) => ({
            ...p,
            data: new Uint8Array(
              p.data
                ? Array.isArray(p.data)
                  ? p.data
                  : Object.values(p.data)
                : []
            ),
          })
        );
        handleUploadSession(parsed.metadata, rehydrated);
        return;
      }
    } catch {
      /* fallthrough */
    }

    if (!engine?.connected) {
      showToast('未连接引擎，且无法按 JSON 演示包解析该文件');
    } else {
      showToast('无法识别为二进制 .fcap 或 JSON 演示包');
    }
  };

  return (
    <div className="min-h-screen bg-slate-100/70 text-slate-900 flex flex-col font-sans antialiased">
      <Header
        currentTab={currentTab}
        onTabChange={setCurrentTab}
        onLoadPreset={handleLoadServoPreset}
        engineState={engineState}
      />

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
            engineConnected={engineState === 'connected'}
            onEngineStartRecord={handleEngineStartRecord}
            onEngineStopRecord={handleEngineStopRecord}
            pickRecordChannel={pickRecordChannel}
          />
        )}

        {currentTab === 'replay' && (
          <DevReplayerView
            sessionMetadata={sessionMetadata}
            packets={recordedPackets}
            onUploadSession={handleUploadSession}
            onLoadCaptureFile={handleLoadCaptureFile}
            onSelectPacketForInspection={() => setCurrentTab('packets')}
            engineConnected={engineState === 'connected'}
            engineReplayTick={engineReplayTick}
            engineReplayDoneSeq={engineReplayDoneSeq}
            onEngineStartReplay={handleEngineStartReplay}
            onEngineStopReplay={handleEngineStopReplay}
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
