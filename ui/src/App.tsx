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
import { DesktopTitleBar } from './components/DesktopTitleBar';
import { FieldRecorderView } from './components/FieldRecorderView';
import { DevReplayerView } from './components/DevReplayerView';
import { PacketsView } from './components/PacketsView';
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
import { hasShellIpc, postShellCommand } from './shell/windowControls';
import { ReplayConfig } from './types/recorder';
import { AlertTriangle, Download } from 'lucide-react';

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
  const [isMaximized, setIsMaximized] = useState(false);
  const [showCloseModal, setShowCloseModal] = useState(false);

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

  const handleMinimize = () => {
    if (postShellCommand('minimize')) return;
    showToast('开发预览无系统最小化；请用绿色包主程序');
  };

  const handleToggleMaximize = () => {
    if (postShellCommand('maximize')) {
      setIsMaximized((v) => !v);
      return;
    }
    // 浏览器预览：用 Fullscreen 近似最大化
    if (!document.fullscreenElement) {
      document.documentElement.requestFullscreen().catch(() => {});
      setIsMaximized(true);
    } else {
      document.exitFullscreen().catch(() => {});
      setIsMaximized(false);
    }
  };

  const handleConfirmClose = () => {
    setShowCloseModal(false);
    if (postShellCommand('close')) return;
    showToast('开发预览无法退出壳进程；请关闭浏览器标签');
  };

  const handleRequestClose = () => {
    if (recordedPackets.length > 0) {
      setShowCloseModal(true);
      return;
    }
    handleConfirmClose();
  };

  useEffect(() => {
    const onFs = () => setIsMaximized(!!document.fullscreenElement);
    document.addEventListener('fullscreenchange', onFs);
    return () => document.removeEventListener('fullscreenchange', onFs);
  }, []);

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
    // 新开录制时复位会话元数据，避免重演页仍显示上一包标题（G3）
    setSessionMetadata({
      ...emptySessionMetadata(),
      sessionTitle: `现场录制-${channel.name}`,
      recordedBy: '现场录制中',
      startTime: new Date().toLocaleString(),
    });
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
    const speed = Number.isFinite(cfg.speed) && cfg.speed > 0 ? cfg.speed : 1;
    await engine.startReplay({
      type: mapped.type,
      config: mapped.config,
      topicPrefix: mapped.topicPrefix,
      speed,
    });
    showToast(
      speed === 1
        ? `已开始引擎重放：${mapped.type}`
        : `已开始引擎重放：${mapped.type}（${speed}x）`
    );
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

  const handleEnginePauseReplay = async () => {
    const engine = engineRef.current;
    if (!engine?.connected) {
      throw new Error('引擎未连接');
    }
    const result = await engine.pauseReplay();
    showToast(result.already ? '已处于暂停' : '重放已暂停');
    return result;
  };

  const handleEngineResumeReplay = async () => {
    const engine = engineRef.current;
    if (!engine?.connected) {
      throw new Error('引擎未连接');
    }
    const result = await engine.resumeReplay();
    showToast(result.already ? '已在播放' : '重放已继续');
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
    <div className="h-screen overflow-hidden bg-slate-100/70 text-slate-900 flex flex-col font-sans antialiased">
      <DesktopTitleBar
        isMaximized={isMaximized}
        onToggleMaximize={handleToggleMaximize}
        onMinimize={handleMinimize}
        onRequestClose={handleRequestClose}
      />

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

      <main className="flex-1 min-h-0 overflow-hidden">
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
            onEnginePauseReplay={handleEnginePauseReplay}
            onEngineResumeReplay={handleEngineResumeReplay}
          />
        )}

        {currentTab === 'packets' && (
          <PacketsView packets={recordedPackets} onSelectPacket={() => {}} />
        )}
      </main>

      <footer className="py-1 border-t border-slate-200 text-center text-[10px] text-slate-500 bg-white shrink-0">
        <span>FieldReplay · 微秒时基录放 · 上位机改指向即可对接</span>
        {!hasShellIpc() && (
          <>
            <span className="mx-2">·</span>
            <span className="text-amber-600">浏览器预览</span>
          </>
        )}
      </footer>

      {showCloseModal && (
        <div className="fixed inset-0 z-50 bg-slate-950/60 backdrop-blur-xs flex items-center justify-center p-4">
          <div className="bg-white rounded-xl max-w-md w-full p-6 shadow-2xl border border-slate-200 space-y-4">
            <div className="flex items-center gap-3 pb-3 border-b border-slate-100">
              <div className="w-10 h-10 rounded-lg bg-amber-50 text-amber-600 flex items-center justify-center shrink-0">
                <AlertTriangle className="w-5 h-5" />
              </div>
              <div>
                <h3 className="text-base font-bold text-slate-900">退出 FieldReplay 确认</h3>
                <p className="text-xs text-slate-500 mt-0.5">当前缓冲区尚有未关闭会话数据</p>
              </div>
            </div>
            <p className="text-xs text-slate-600 leading-relaxed">
              缓冲区共{' '}
              <strong className="font-mono text-slate-900">{recordedPackets.length}</strong>{' '}
              帧。若尚未导出，建议先保存再退出。
            </p>
            <div className="flex items-center justify-end gap-2 pt-3 border-t border-slate-100">
              <button
                type="button"
                onClick={() => setShowCloseModal(false)}
                className="px-3 py-1.5 text-xs text-slate-600 hover:text-slate-800 rounded-lg"
              >
                取消
              </button>
              <button
                type="button"
                onClick={handleConfirmClose}
                className="px-3.5 py-1.5 text-xs font-medium text-slate-700 bg-slate-100 hover:bg-slate-200 rounded-lg"
              >
                直接退出
              </button>
              <button
                type="button"
                onClick={() => {
                  void handleExportSession(sessionMetadata).finally(handleConfirmClose);
                }}
                className="px-4 py-1.5 text-xs font-semibold text-white bg-sky-600 hover:bg-sky-500 rounded-lg shadow-sm flex items-center gap-1.5"
              >
                <Download className="w-3.5 h-3.5" />
                <span>保存并退出</span>
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
