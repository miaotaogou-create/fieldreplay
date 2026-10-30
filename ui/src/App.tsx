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
import { channelToEngineConfig } from './engine/channelConfig';
import { defaultEngineUrl, EngineClient } from './engine/client';
import { replayConfigToEngine } from './engine/replayTargetConfig';
import { EngineConnState } from './engine/types';
import {
  emptySessionMetadata,
  mapEnginePacket,
  mapEngineSession,
  metadataToFcapMeta,
} from './engine/sessionMap';
import { hasShellIpc, postShellCommand } from './shell/windowControls';
import { AlertTriangle, Download } from 'lucide-react';
import type { EngineStartReplayRequest } from './components/DevReplayerView';

const TOAST_MS = 3500;

/** 经典 libpcap 魔数（小端/大端 × 微秒/纳秒）。 */
function isClassicPcapMagic(buf: Uint8Array): boolean {
  if (buf.length < 4) return false;
  const a = buf[0];
  const b = buf[1];
  const c = buf[2];
  const d = buf[3];
  return (
    (a === 0xd4 && b === 0xc3 && c === 0xb2 && d === 0xa1)
    || (a === 0x4d && b === 0x3c && c === 0xb2 && d === 0xa1)
    || (a === 0xa1 && b === 0xb2 && c === 0xc3 && d === 0xd4)
    || (a === 0xa1 && b === 0xb2 && c === 0x3c && d === 0x4d)
  );
}

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
  /** 当前录制会话：引擎 channel index → UI 通道元数据 */
  const liveChannelsRef = useRef<
    Array<{
      id: string;
      name: string;
      type: ChannelConfig['type'];
    }>
  >([]);
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
        const idx = ev.packet.ch;
        const ch = liveChannelsRef.current[idx] ?? {
          id: `ch_${idx}`,
          name: `通道 ${idx}`,
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
        if (ev.error) {
          toastFnRef.current(`重放失败：${ev.error}`);
        } else {
          toastFnRef.current(
            ev.stopped
              ? `重放已停止（已发 ${ev.sent ?? 0} 帧）`
              : `重放完成（已发 ${ev.sent ?? 0} 帧）`
          );
        }
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

  const handleEngineStartRecord = async (selected: ChannelConfig[]) => {
    const engine = engineRef.current;
    if (!engine?.connected) {
      throw new Error('引擎未连接');
    }
    if (selected.length === 0) {
      throw new Error('请至少启用一个可录制通道');
    }
    liveChannelsRef.current = selected.map((c) => ({
      id: c.id,
      name: c.name,
      type: c.type,
    }));
    recordStartWallUsRef.current = Date.now() * 1000;
    setRecordedPackets([]);
    const title =
      selected.length === 1
        ? `现场录制-${selected[0].name}`
        : `现场录制-${selected.length}路`;
    // 新开录制时复位会话元数据，避免重演页仍显示上一包标题（G3）
    setSessionMetadata({
      ...emptySessionMetadata(),
      sessionTitle: title,
      recordedBy: '现场录制中',
      startTime: new Date().toLocaleString(),
    });
    const channelPayloads = selected.map((c) => ({
      type: c.type,
      name: c.name,
      config: channelToEngineConfig(c),
    }));
    if (channelPayloads.length === 1) {
      const only = channelPayloads[0];
      await engine.startRecord({
        type: only.type,
        name: only.name,
        sessionTitle: title,
        config: only.config,
      });
    } else {
      await engine.startRecord({
        sessionTitle: title,
        channels: channelPayloads,
      });
    }
    const summary = selected.map((c) => `${c.name}(${c.type})`).join('、');
    showToast(`已开始真实录制：${summary}`);
  };

  const handleEngineStopRecord = async () => {
    const engine = engineRef.current;
    if (!engine?.connected) {
      throw new Error('引擎未连接');
    }
    const result = await engine.stopRecord();
    liveChannelsRef.current = [];
    showToast(
      `录制已停止：${result.packetCount} 帧已写入 ${result.out.split(/[/\\]/).pop()}`
    );
    return result;
  };

  const handleEngineStartReplay = async (req: EngineStartReplayRequest) => {
    const engine = engineRef.current;
    if (!engine?.connected) {
      throw new Error('引擎未连接');
    }
    if (recordedPackets.length === 0) {
      throw new Error('会话无帧，请先载入 .fcap');
    }
    const { cfg, fromUs } = req;
    setEngineReplayTick(null);
    const speed = Number.isFinite(cfg.speed) && cfg.speed > 0 ? cfg.speed : 1;
    const startUs = Number.isFinite(fromUs) && fromUs > 0 ? Math.floor(fromUs) : 0;
    const faultCfg = cfg.faultInjection;
    const fault =
      faultCfg?.enabled
        ? {
            enabled: true,
            lossRatePercent: Math.max(0, Math.min(100, Math.floor(faultCfg.lossRatePercent || 0))),
            corruptCrc: !!faultCfg.corruptCrc,
          }
        : undefined;
    const loop = !!cfg.loop;
    const faultHint =
      fault && (fault.lossRatePercent > 0 || fault.corruptCrc)
        ? ` · 故障注入${fault.lossRatePercent > 0 ? ` 丢包${fault.lossRatePercent}%` : ''}${
            fault.corruptCrc ? ' 篡改' : ''
          }`
        : '';
    const loopHint = loop ? ' · 循环' : '';
    const speedHint = speed === 1 ? '' : `（${speed}x）`;

    if (req.mode === 'multi') {
      const result = await engine.startReplay({
        targets: req.targets,
        speed,
        fromUs: startUs,
        loop,
        fault,
      });
      const n = result.targets?.length ?? req.targets.length;
      const skipped = result.skipped?.length ?? 0;
      const skipHint = skipped > 0 ? ` · 跳过 ${skipped} 路` : '';
      showToast(
        `已开始多路对齐重放：${n} 路目标${speedHint}${faultHint}${loopHint}${skipHint}`
      );
      return;
    }

    const { tab } = req;
    const mapped = replayConfigToEngine(tab, cfg);
    // 视频会话：type=video_stream；E7d 可选 RTSP 服务端，否则 UDP 推送
    const sessionIsVideo =
      recordedPackets.some((p) => p.channelType === 'video_stream')
      || sessionMetadata.channels.some((c) => c.type === 'video_stream');
    const replayType =
      tab === 'udp' && sessionIsVideo ? 'video_stream' : mapped.type;
    const config =
      tab === 'udp' && sessionIsVideo && cfg.videoOutput === 'rtsp_listen'
        ? {
            mode: 'rtsp_listen',
            listen_ip: '0.0.0.0',
            listen_port: cfg.rtspListenPort || 8554,
            media: 'rtsp_tcp',
          }
        : mapped.config;
    await engine.startReplay({
      type: replayType,
      config,
      topicPrefix: mapped.topicPrefix,
      speed,
      fromUs: startUs,
      loop,
      fault,
    });
    showToast(
      `已开始引擎重放：${replayType}${speedHint}${faultHint}${loopHint}`
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

  const handleEngineSeekReplay = async (relativeUs: number) => {
    const engine = engineRef.current;
    if (!engine?.connected) {
      throw new Error('引擎未连接');
    }
    return engine.seekReplay(Math.max(0, Math.floor(relativeUs)));
  };

  const handleListSerialPorts = async () => {
    const engine = engineRef.current;
    if (!engine?.connected) {
      throw new Error('引擎未连接');
    }
    const result = await engine.listSerialPorts();
    return Array.isArray(result.ports) ? result.ports : [];
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

  const handleExportSession = async (
    metadata: CaptureSessionMetadata,
    options?: { alsoPcap?: boolean }
  ) => {
    setSessionMetadata(metadata);
    const fileName =
      metadata.sessionTitle || `现场数据捕获包_${Date.now()}.fcap`;
    const engine = engineRef.current;
    const alsoPcap = Boolean(options?.alsoPcap);

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

        if (alsoPcap) {
          try {
            const pcap = await engine.exportSessionPcapBytes();
            if (
              pcap.bytes.length < 4
              || pcap.bytes[0] !== 0xd4
              || pcap.bytes[1] !== 0xc3
              || pcap.bytes[2] !== 0xb2
              || pcap.bytes[3] !== 0xa1
            ) {
              throw new Error('引擎返回内容不是经典 .pcap');
            }
            const pcapName = fileName.replace(/\.fcap$/i, '') + '.pcap';
            const pab = new ArrayBuffer(pcap.bytes.byteLength);
            new Uint8Array(pab).set(pcap.bytes);
            downloadBlob(new Blob([pab], { type: 'application/vnd.tcpdump.pcap' }), pcapName);
            showToast(
              `已导出 .fcap 与 .pcap（写入 ${pcap.written} 帧，跳过 ${pcap.skipped}）`
            );
          } catch (pe) {
            const pmsg = pe instanceof Error ? pe.message : String(pe);
            showToast(`已导出 .fcap；pcap 失败：${pmsg}`);
          }
          return;
        }

        showToast('已导出真实二进制 .fcap，可直接发给研发重放');
        return;
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        showToast(`导出失败：${msg}`);
        return;
      }
    }

    if (alsoPcap) {
      showToast('离线模式无法导出 pcap，请先连接引擎');
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

  const refreshSessionFromEngine = async () => {
    const engine = engineRef.current;
    if (!engine?.connected) {
      throw new Error('引擎未连接');
    }
    const bytes = await engine.exportSessionBytes();
    const dto = await engine.loadSessionBytes(bytes);
    const mapped = mapEngineSession(dto);
    setSessionMetadata(mapped.metadata);
    setRecordedPackets(mapped.packets);
    return mapped;
  };

  const handleAddMarkerAt = async (text: string, relativeUs: number) => {
    const engine = engineRef.current;
    if (!engine?.connected) {
      showToast('未连引擎：不会本地假写入正式包');
      return;
    }
    try {
      const result = await engine.addMarker({ text, relativeUs });
      if (!result.live) {
        await refreshSessionFromEngine();
      }
      showToast(
        result.live
          ? `已热插标记 @ ${result.relativeUs} µs`
          : `已插入标记 @ ${result.relativeUs} µs`
      );
    } catch (e) {
      showToast(`插入标记失败：${e instanceof Error ? e.message : String(e)}`);
    }
  };

  const handleAddMarkerLive = async (text: string) => {
    const engine = engineRef.current;
    if (!engine?.connected) {
      showToast('未连引擎：不会本地假写入正式包');
      return;
    }
    try {
      const result = await engine.addMarker({ text });
      showToast(`已热插标记 @ ${result.relativeUs} µs`);
    } catch (e) {
      showToast(`插入标记失败：${e instanceof Error ? e.message : String(e)}`);
    }
  };

  const handleTrimDownload = async (fromUs: number, toUs: number) => {
    const engine = engineRef.current;
    if (!engine?.connected) {
      showToast('未连引擎：无法裁剪正式包');
      return;
    }
    try {
      const { bytes, packetCount } = await engine.trimSessionBytes({
        fromUs,
        toUs,
        replaceSession: false,
      });
      if (
        bytes.length < 4
        || bytes[0] !== 0x46
        || bytes[1] !== 0x43
        || bytes[2] !== 0x41
        || bytes[3] !== 0x50
      ) {
        throw new Error('引擎返回内容不是二进制 .fcap');
      }
      const base =
        sessionMetadata.sessionTitle?.replace(/\.fcap$/i, '')
        || `现场裁剪_${Date.now()}`;
      const fileName = `${base}_trim_${fromUs}-${toUs}.fcap`;
      const ab = new ArrayBuffer(bytes.byteLength);
      new Uint8Array(ab).set(bytes);
      downloadBlob(new Blob([ab], { type: 'application/octet-stream' }), fileName);
      showToast(`已另存裁剪包（${packetCount} 帧，再基 [ ${fromUs}, ${toUs} ]）`);
    } catch (e) {
      showToast(`裁剪失败：${e instanceof Error ? e.message : String(e)}`);
    }
  };

  const handleLoadCaptureFile = async (file: File) => {
    const engine = engineRef.current;
    const lower = file.name.toLowerCase();
    const looksPcapName = lower.endsWith('.pcap');

    // E14d：经典 pcap 必须经引擎；未连接不假装本地假导入
    if (looksPcapName || (engine?.connected && !lower.endsWith('.json') && !lower.endsWith('.fcap'))) {
      const buf = new Uint8Array(await file.arrayBuffer());
      if (isClassicPcapMagic(buf) || looksPcapName) {
        if (!engine?.connected) {
          showToast('未连引擎：不会本地假导入 .pcap，请先连接引擎');
          return;
        }
        try {
          const result = await engine.importSessionPcap(buf, file.name);
          const mapped = mapEngineSession(result.session);
          setSessionMetadata(mapped.metadata);
          setRecordedPackets(mapped.packets);
          showToast(
            `已导入 pcap：写入 ${result.written} 帧，跳过 ${result.skipped}，` +
              `${result.channelCount} 路通道（列表 ${mapped.packets.length} 帧）`
          );
          return;
        } catch (e) {
          showToast(
            `导入 pcap 失败：${e instanceof Error ? e.message : String(e)}`
          );
          return;
        }
      }
    }

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
      showToast('无法识别为二进制 .fcap、经典 .pcap 或 JSON 演示包');
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
            onListSerialPorts={handleListSerialPorts}
            onAddMarkerLive={handleAddMarkerLive}
          />
        )}

        {currentTab === 'replay' && (
          <DevReplayerView
            sessionMetadata={sessionMetadata}
            packets={recordedPackets}
            onUploadSession={handleUploadSession}
            onLoadCaptureFile={handleLoadCaptureFile}
            onPcapImportNeedEngine={() =>
              showToast('未连引擎：不会本地假导入 .pcap，请先连接引擎')
            }
            onSelectPacketForInspection={() => setCurrentTab('packets')}
            engineConnected={engineState === 'connected'}
            engineReplayTick={engineReplayTick}
            engineReplayDoneSeq={engineReplayDoneSeq}
            onEngineStartReplay={handleEngineStartReplay}
            onEngineStopReplay={handleEngineStopReplay}
            onEnginePauseReplay={handleEnginePauseReplay}
            onEngineResumeReplay={handleEngineResumeReplay}
            onEngineSeekReplay={handleEngineSeekReplay}
            onListSerialPorts={handleListSerialPorts}
            onAddMarkerAt={handleAddMarkerAt}
            onTrimDownload={handleTrimDownload}
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
