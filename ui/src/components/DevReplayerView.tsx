/**
 * @license
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useState, useEffect, useRef } from 'react';
import {
  CapturedPacket,
  CaptureSessionMetadata,
  ReplayConfig,
} from '../types/recorder';
import { ReplayTargetType } from '../engine/replayTargetConfig';
import {
  buildDefaultChannelTargets,
  ChannelReplayTargetRow,
  listEnabledMultiTargets,
} from '../engine/multiReplayTargets';
import { formatSerialLineSummary } from '../utils/serialParams';
import { ReplayChannelTargets } from './ReplayChannelTargets';
import { SessionMarkerTrimPanel } from './SessionMarkerTrimPanel';
import {
  Play,
  Pause,
  RotateCcw,
  StepForward,
  StepBack,
  Upload,
  AlertTriangle,
  Zap,
  CheckCircle2,
  Cpu,
  Activity,
  ExternalLink,
} from 'lucide-react';

export type EngineStartReplayRequest =
  | {
      mode: 'single';
      tab: ReplayTargetType;
      cfg: ReplayConfig;
      fromUs: number;
    }
  | {
      mode: 'multi';
      targets: Array<{
        ch: number;
        type: string;
        config: Record<string, unknown>;
      }>;
      cfg: ReplayConfig;
      fromUs: number;
    };

interface DevReplayerViewProps {
  sessionMetadata: CaptureSessionMetadata;
  packets: CapturedPacket[];
  onUploadSession: (metadata: CaptureSessionMetadata, packets: CapturedPacket[]) => void;
  /** 优先走引擎解析真实二进制 .fcap；未连引擎时可回落 JSON 演示包 */
  onLoadCaptureFile?: (file: File) => void | Promise<void>;
  onSelectPacketForInspection: (packet: CapturedPacket) => void;
  engineConnected?: boolean;
  engineReplayTick?: {
    sent: number;
    relativeUs: number;
    durationUs: number;
  } | null;
  engineReplayDoneSeq?: number;
  onEngineStartReplay?: (req: EngineStartReplayRequest) => Promise<void>;
  onEngineStopReplay?: () => Promise<unknown>;
  onEnginePauseReplay?: () => Promise<unknown>;
  onEngineResumeReplay?: () => Promise<unknown>;
  onEngineSeekReplay?: (relativeUs: number) => Promise<unknown>;
  /** E8a：向引擎枚举本机 COM；未连引擎时不调用 */
  onListSerialPorts?: () => Promise<string[]>;
  /** E13d：播放头插 marker（须连引擎；重放中由引擎拒绝） */
  onAddMarkerAt?: (text: string, relativeUs: number) => void | Promise<void>;
  /** E13d：时间窗 trim 另存下载 */
  onTrimDownload?: (fromUs: number, toUs: number) => void | Promise<void>;
}

const FALLBACK_SERIAL_PORTS = [
  'COM1',
  'COM2',
  'COM3',
  'COM4',
  'COM10',
  'COM11',
  '/dev/ttyUSB0',
  '/dev/pts/2',
];

export const DevReplayerView: React.FC<DevReplayerViewProps> = ({
  sessionMetadata,
  packets,
  onUploadSession,
  onLoadCaptureFile,
  onSelectPacketForInspection,
  engineConnected = false,
  engineReplayTick = null,
  engineReplayDoneSeq = 0,
  onEngineStartReplay,
  onEngineStopReplay,
  onEnginePauseReplay,
  onEngineResumeReplay,
  onEngineSeekReplay,
  onListSerialPorts,
  onAddMarkerAt,
  onTrimDownload,
}) => {
  const [isPlaying, setIsPlaying] = useState(false);
  /** 引擎重放已启动且处于暂停（时间轴冻结）；离线 mock 不用此态 */
  const [isPaused, setIsPaused] = useState(false);
  const [currentIndex, setCurrentIndex] = useState(0);
  const [replaySpeed, setReplaySpeed] = useState<number>(1.0);
  const [isLooping, setIsLooping] = useState(true);

  // Replay Output Target Configuration
  const [replayConfig, setReplayConfig] = useState<ReplayConfig>({
    speed: 1.0,
    loop: true,
    targetIp: '127.0.0.1',
    targetUdpPort: 8080,
    videoOutput: 'udp',
    rtspListenPort: 8554,
    virtualComPort: 'COM2',
    serial: {
      selectedPort: 'COM2',
      hostComPort: 'COM1',
      baudRate: 115200,
      dataBits: 8,
      stopBits: 1,
      parity: 'none',
      isPortOpen: true,
      availablePorts: [...FALLBACK_SERIAL_PORTS],
    },
    tcp: {
      mode: 'tool_is_server',
      serverPort: 9000,
      clientTargetHost: '127.0.0.1',
      clientTargetPort: 9000,
      connectedClientsCount: 1,
    },
    mqtt: {
      enabled: true,
      mode: 'virtual_publisher',
      brokerHost: '127.0.0.1',
      brokerPort: 1883,
      topicPrefix: '',
      username: '',
      password: '',
      tls: false,
      caCertPath: '',
      tlsServerName: '',
      connectedToBroker: true,
    },
    simulateOutput: true,
    faultInjection: {
      enabled: false,
      lossRatePercent: 0,
      jitterMs: 0,
      corruptCrc: false,
      latencyMultiplier: 1,
    },
  });

  const [activeTargetTab, setActiveTargetTab] = useState<'udp' | 'serial' | 'tcp' | 'mqtt'>('tcp');
  const [channelTargets, setChannelTargets] = useState<ChannelReplayTargetRow[]>(
    () => buildDefaultChannelTargets(sessionMetadata.channels)
  );
  const [replayHint, setReplayHint] = useState<string | null>(null);

  const sessionIsVideo =
    packets.some((p) => p.channelType === 'video_stream')
    || sessionMetadata.channels.some((c) => c.type === 'video_stream');
  /** ≥2 通道走 E11d 多目标；单通道保留完整协议页 */
  const useMultiChannelTargets = sessionMetadata.channels.length >= 2;

  const [dispatchedPackets, setDispatchedPackets] = useState<
    { id: number; time: string; channel: string; status: 'ok' | 'dropped' | 'corrupted'; bytes: number }[]
  >([]);

  const fileInputRef = useRef<HTMLInputElement>(null);

  const sessionChannelKey = sessionMetadata.channels
    .map((c) => `${c.id}:${c.type}:${c.packetCount}`)
    .join('|');

  // 会话通道表变化时重建每路目标默认值（避免父组件重渲染冲掉用户改口）
  useEffect(() => {
    setChannelTargets(buildDefaultChannelTargets(sessionMetadata.channels));
    setReplayHint(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionMetadata.sessionTitle, sessionChannelKey]);

  // 载入视频会话时切到 UDP/RTP 目标区，默认推送口 5004（单通道页）
  useEffect(() => {
    if (!sessionIsVideo || useMultiChannelTargets) return;
    setActiveTargetTab('udp');
    setReplayConfig((prev) =>
      prev.targetUdpPort === 8080
        ? { ...prev, targetUdpPort: 5004 }
        : prev
    );
  }, [sessionIsVideo, useMultiChannelTargets, sessionMetadata.sessionTitle, sessionMetadata.totalPackets]);

  const [serialListBusy, setSerialListBusy] = useState(false);
  const [serialListHint, setSerialListHint] = useState<string | null>(null);

  const refreshSerialPorts = async () => {
    if (!engineConnected || !onListSerialPorts) {
      setSerialListHint('引擎未连接，使用本地兜底列表');
      setReplayConfig((prev) => ({
        ...prev,
        serial: { ...prev.serial, availablePorts: [...FALLBACK_SERIAL_PORTS] },
      }));
      return;
    }
    setSerialListBusy(true);
    setSerialListHint(null);
    try {
      const ports = await onListSerialPorts();
      setReplayConfig((prev) => {
        const keep = [prev.serial.selectedPort, prev.serial.hostComPort].filter(Boolean);
        const merged = Array.from(new Set([...(ports.length ? ports : FALLBACK_SERIAL_PORTS), ...keep]));
        return {
          ...prev,
          serial: {
            ...prev.serial,
            availablePorts: merged,
          },
        };
      });
      const summary = formatSerialLineSummary(replayConfig.serial);
      const port = replayConfig.serial.selectedPort;
      setSerialListHint(
        ports.length === 0
          ? `本机未枚举到 COM，已保留兜底/当前选项 · ${port} @ ${summary}`
          : `已刷新 ${ports.length} 个串口 · ${port} @ ${summary}`
      );
    } catch (e) {
      setSerialListHint(e instanceof Error ? e.message : String(e));
    } finally {
      setSerialListBusy(false);
    }
  };

  useEffect(() => {
    if (!engineConnected) return;
    void refreshSerialPorts();
    // 仅随连接态刷新；手动点「刷新」可再取
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [engineConnected]);

  const timerRef = useRef<any>(null);
  const engineBusyRef = useRef(false);
  const hasPackets = packets.length > 0;
  const useEngineReplay =
    engineConnected
    && !!onEngineStartReplay
    && !!onEngineStopReplay
    && !!onEnginePauseReplay
    && !!onEngineResumeReplay
    && !!onEngineSeekReplay;
  const engineBusy = useEngineReplay && (isPlaying || isPaused);

  const totalDurationUs = hasPackets ? packets[packets.length - 1].timestampUs : 0;
  const safeIndex = hasPackets ? Math.min(currentIndex, packets.length - 1) : 0;
  const currentPacket = hasPackets ? packets[safeIndex] : undefined;
  const currentTimeUs = currentPacket?.timestampUs || 0;
  const progressPercent =
    totalDurationUs > 0 ? Math.min(100, (currentTimeUs / totalDurationUs) * 100) : 0;

  // 会话包变更时复位播放头与推送日志，避免越界
  useEffect(() => {
    setIsPlaying(false);
    setIsPaused(false);
    setCurrentIndex(0);
    setDispatchedPackets([]);
  }, [sessionMetadata.sessionTitle, packets.length]);

  // 引擎进度 → 播放头 + 发帧日志
  useEffect(() => {
    if (!useEngineReplay || !engineReplayTick || engineReplayTick.relativeUs <= 0) {
      return;
    }
    const targetUs = engineReplayTick.relativeUs;
    let closestIdx = 0;
    let minDiff = Infinity;
    for (let i = 0; i < packets.length; i++) {
      const diff = Math.abs(packets[i].timestampUs - targetUs);
      if (diff < minDiff) {
        minDiff = diff;
        closestIdx = i;
      }
    }
    setCurrentIndex(closestIdx);
    const pkt = packets[closestIdx];
    if (pkt) {
      setDispatchedPackets((old) => [
        {
          id: pkt.id,
          time:
            new Date().toLocaleTimeString()
            + '.'
            + (Date.now() % 1000).toString().padStart(3, '0'),
          channel: pkt.channelName,
          status: 'ok',
          bytes: pkt.length,
        },
        ...old.slice(0, 39),
      ]);
    }
  }, [engineReplayTick, packets, useEngineReplay]);

  useEffect(() => {
    if (!useEngineReplay || engineReplayDoneSeq === 0) return;
    setIsPlaying(false);
    setIsPaused(false);
    engineBusyRef.current = false;
  }, [engineReplayDoneSeq, useEngineReplay]);

  // 离线 mock 播放环；引擎已连时不走本地定时器
  useEffect(() => {
    if (useEngineReplay || !isPlaying || packets.length === 0) {
      if (timerRef.current) clearInterval(timerRef.current);
      return;
    }

    const intervalMs = Math.max(10, Math.round(50 / replaySpeed));
    timerRef.current = setInterval(() => {
      setCurrentIndex((prev) => {
        if (prev >= packets.length - 1) {
          if (isLooping) {
            return 0;
          } else {
            setIsPlaying(false);
            return prev;
          }
        }
        const nextIdx = prev + 1;
        const pkt = packets[nextIdx];

        let status: 'ok' | 'dropped' | 'corrupted' = 'ok';
        if (replayConfig.faultInjection.enabled) {
          if (
            replayConfig.faultInjection.lossRatePercent > 0 &&
            Math.random() * 100 < replayConfig.faultInjection.lossRatePercent
          ) {
            status = 'dropped';
          } else if (replayConfig.faultInjection.corruptCrc && Math.random() < 0.2) {
            status = 'corrupted';
          }
        }

        setDispatchedPackets((old) => [
          {
            id: pkt.id,
            time: new Date().toLocaleTimeString() + '.' + (Date.now() % 1000).toString().padStart(3, '0'),
            channel: pkt.channelName,
            status,
            bytes: pkt.length,
          },
          ...old.slice(0, 39),
        ]);

        return nextIdx;
      });
    }, intervalMs);

    return () => {
      if (timerRef.current) clearInterval(timerRef.current);
    };
  }, [isPlaying, replaySpeed, isLooping, packets, replayConfig.faultInjection, useEngineReplay]);

  const handleTogglePlay = () => {
    if (!hasPackets) return;
    if (useEngineReplay) {
      if (engineBusyRef.current) return;
      if (isPlaying) {
        engineBusyRef.current = true;
        void onEnginePauseReplay!()
          .then(() => {
            setIsPlaying(false);
            setIsPaused(true);
          })
          .catch((e) => {
            console.error(e);
          })
          .finally(() => {
            engineBusyRef.current = false;
          });
        return;
      }
      if (isPaused) {
        engineBusyRef.current = true;
        void onEngineResumeReplay!()
          .then(() => {
            setIsPlaying(true);
            setIsPaused(false);
          })
          .catch((e) => {
            console.error(e);
          })
          .finally(() => {
            engineBusyRef.current = false;
          });
        return;
      }
      engineBusyRef.current = true;
      const fromUs = packets[safeIndex]?.timestampUs ?? 0;
      setDispatchedPackets([]);
      setIsPaused(false);
      setReplayHint(null);

      let startReq: EngineStartReplayRequest;
      if (useMultiChannelTargets) {
        const targets = listEnabledMultiTargets(channelTargets);
        if (targets.length === 0) {
          engineBusyRef.current = false;
          setReplayHint(
            '请至少启用一路可多路重放的通道目标（UDP / 视频 UDP / TCP / 串口 / MQTT）'
          );
          return;
        }
        startReq = {
          mode: 'multi',
          targets,
          cfg: replayConfig,
          fromUs,
        };
      } else {
        startReq = {
          mode: 'single',
          tab: activeTargetTab,
          cfg: replayConfig,
          fromUs,
        };
      }

      void onEngineStartReplay!(startReq)
        .then(() => {
          setIsPlaying(true);
        })
        .catch((e) => {
          console.error(e);
          setReplayHint(e instanceof Error ? e.message : String(e));
          setIsPlaying(false);
          setIsPaused(false);
        })
        .finally(() => {
          engineBusyRef.current = false;
        });
      return;
    }
    if (currentIndex >= packets.length - 1) {
      setCurrentIndex(0);
    }
    setIsPlaying(!isPlaying);
  };

  const handleReset = () => {
    if (useEngineReplay && (isPlaying || isPaused)) {
      void onEngineStopReplay?.().catch(() => undefined);
    }
    setIsPlaying(false);
    setIsPaused(false);
    setCurrentIndex(0);
  };

  const handleStepForward = () => {
    if (!hasPackets || engineBusy) return;
    setIsPlaying(false);
    setIsPaused(false);
    setCurrentIndex((prev) => Math.min(packets.length - 1, prev + 1));
  };

  const handleStepBack = () => {
    if (!hasPackets || engineBusy) return;
    setIsPlaying(false);
    setIsPaused(false);
    setCurrentIndex((prev) => Math.max(0, prev - 1));
  };

  const seekToUs = (targetUs: number) => {
    if (!hasPackets) return;
    let closestIdx = 0;
    let minDiff = Infinity;
    for (let i = 0; i < packets.length; i++) {
      const diff = Math.abs(packets[i].timestampUs - targetUs);
      if (diff < minDiff) {
        minDiff = diff;
        closestIdx = i;
      }
    }
    setCurrentIndex(closestIdx);
    if (useEngineReplay && (isPlaying || isPaused)) {
      void onEngineSeekReplay!(packets[closestIdx].timestampUs).catch((e) => {
        console.error(e);
      });
    }
  };

  const handleSeek = (e: React.ChangeEvent<HTMLInputElement>) => {
    if (!hasPackets || totalDurationUs <= 0) return;
    const val = parseFloat(e.target.value);
    const targetUs = (val / 100) * totalDurationUs;
    seekToUs(targetUs);
  };

  const handleSpeedChange = (s: number) => {
    setReplaySpeed(s);
    setReplayConfig((cfg) => ({ ...cfg, speed: s }));
  };

  const handleFileUpload = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    e.target.value = '';

    if (onLoadCaptureFile) {
      void Promise.resolve(onLoadCaptureFile(file)).then(() => {
        setCurrentIndex(0);
        setIsPlaying(false);
        setIsPaused(false);
      });
      return;
    }

    const reader = new FileReader();
    reader.onload = (event) => {
      try {
        const text = event.target?.result as string;
        const parsed = JSON.parse(text);
        if (parsed.metadata && parsed.packets) {
          const rehydrated = parsed.packets.map((p: any) => ({
            ...p,
            data: new Uint8Array(p.data ? Object.values(p.data) : []),
          }));
          onUploadSession(parsed.metadata, rehydrated);
          setCurrentIndex(0);
          setIsPlaying(false);
        }
      } catch (err) {
        console.error('Failed to parse fcap bundle', err);
      }
    };
    reader.readAsText(file);
  };

  const formatMicroseconds = (us: number) => {
    const ms = Math.floor(us / 1000);
    const totalSec = Math.floor(ms / 1000);
    const m = Math.floor(totalSec / 60)
      .toString()
      .padStart(2, '0');
    const s = (totalSec % 60).toString().padStart(2, '0');
    const mmm = (ms % 1000).toString().padStart(3, '0');
    return `${m}:${s}.${mmm}`;
  };

  return (
    <div className="fr-page">
      {/* Session Metadata Banner */}
      <div className="bg-white rounded-lg border border-slate-200 px-2.5 py-1.5 shadow-xs shrink-0">
        <div className="flex flex-col md:flex-row items-start md:items-center justify-between gap-1.5">
          <div className="min-w-0">
            <div className="flex items-center gap-2">
              <span className="w-2 h-2 rounded-full bg-emerald-500 shrink-0" />
              <h1 className="text-sm font-bold text-slate-900 tracking-tight truncate">
                {sessionMetadata.sessionTitle}
              </h1>
            </div>
            <div className="flex flex-wrap items-center gap-x-2 gap-y-0.5 text-[11px] text-slate-500 mt-0.5">
              <span>调试员: {sessionMetadata.recordedBy}</span>
              <span>·</span>
              <span>地点: {sessionMetadata.siteName}</span>
              <span>·</span>
              <span>录制时间: {sessionMetadata.startTime}</span>
              <span>·</span>
              <span className="font-mono">
                总帧数: {packets.length} 帧 ({(sessionMetadata.totalBytes / 1024).toFixed(1)} KB)
              </span>
            </div>
          </div>

          <div className="flex items-center gap-2 shrink-0">
            <input
              type="file"
              ref={fileInputRef}
              onChange={handleFileUpload}
              accept=".fcap,.json"
              className="hidden"
            />
            <button
              onClick={() => fileInputRef.current?.click()}
              className="px-2.5 py-1 text-xs font-medium text-slate-700 bg-slate-100 hover:bg-slate-200 rounded-md transition-colors flex items-center gap-1.5"
            >
              <Upload className="w-3.5 h-3.5" />
              <span>载入其他 .fcap 包</span>
            </button>
          </div>
        </div>

        {sessionMetadata.notes && (
          <div className="mt-1.5 px-2 py-1 bg-amber-50/70 border border-amber-200/60 rounded-md text-[11px] text-amber-900 flex items-start gap-1.5">
            <AlertTriangle className="w-3.5 h-3.5 text-amber-600 shrink-0 mt-0.5" />
            <div>
              <span className="font-semibold">现场工单留言: </span>
              {sessionMetadata.notes}
            </div>
          </div>
        )}
      </div>

      {/* Master Replayer Media Console */}
      <div className="bg-slate-900 text-white rounded-lg px-3 py-2 shadow-md border border-slate-800 space-y-2 shrink-0">
        {!hasPackets && (
          <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-2 px-2.5 py-2 bg-amber-500/10 border border-amber-400/30 rounded-md">
            <div className="flex items-start gap-2 text-[11px] text-amber-100">
              <AlertTriangle className="w-3.5 h-3.5 text-amber-400 shrink-0 mt-0.5" />
              <p>
                当前会话无数据帧，播放条已禁用。请先在「现场录制」导出包，或点击上方「载入其他 .fcap 包」。
              </p>
            </div>
            <button
              type="button"
              onClick={() => fileInputRef.current?.click()}
              className="shrink-0 px-2.5 py-1 text-xs font-medium bg-amber-500 hover:bg-amber-400 text-slate-950 rounded-md"
            >
              载入 .fcap
            </button>
          </div>
        )}

        {/* Scrubber Timeline */}
        <div className="space-y-1">
          <div className="flex items-center justify-between text-[11px] font-mono">
            <span className="text-slate-400">
              当前回放时间:{' '}
              <strong className="text-sky-400">{formatMicroseconds(currentTimeUs)}</strong>
            </span>
            <span className="text-slate-500">
              总时长: {formatMicroseconds(totalDurationUs)}{' '}
              {hasPackets
                ? `(第 ${safeIndex + 1} / ${packets.length} 帧)`
                : '(暂无帧)'}
            </span>
          </div>

          {/* Interactive Scrub Bar */}
          <div className="relative group">
            <input
              type="range"
              min="0"
              max="100"
              step="0.05"
              value={progressPercent}
              onChange={handleSeek}
              disabled={!hasPackets}
              className="w-full h-2 bg-slate-800 rounded-md appearance-none accent-sky-500 hover:accent-sky-400 disabled:opacity-40 disabled:cursor-not-allowed disabled:hover:accent-sky-500"
            />
            {/* 故障点：菱形；标记点：圆点 */}
            {totalDurationUs > 0 &&
              packets.map((p) => {
                if (p.isMarker) {
                  return (
                    <button
                      key={`m-${p.id}`}
                      type="button"
                      onClick={() => seekToUs(p.timestampUs)}
                      title={`标记：${p.markerText || '(空)'}（点击跳转）`}
                      aria-label={`标记：${p.markerText || '(空)'}`}
                      className="absolute top-1/2 -translate-y-1/2 -translate-x-1/2 w-2 h-2 rounded-full bg-violet-400 border border-violet-100 shadow-sm cursor-pointer hover:scale-150 transition-transform z-10"
                      style={{
                        left: `${(p.timestampUs / totalDurationUs) * 100}%`,
                      }}
                    />
                  );
                }
                if (p.hasError) {
                  return (
                    <button
                      key={`e-${p.id}`}
                      type="button"
                      onClick={() => seekToUs(p.timestampUs)}
                      title={`故障点：${p.errorReason || '异常帧'}（点击跳转）`}
                      aria-label={`故障点：${p.errorReason || '异常帧'}`}
                      className="absolute top-1/2 -translate-y-1/2 -translate-x-1/2 w-2 h-2 rotate-45 bg-rose-500 border border-rose-200 shadow-sm cursor-pointer hover:scale-150 transition-transform z-10"
                      style={{
                        left: `${(p.timestampUs / totalDurationUs) * 100}%`,
                      }}
                    />
                  );
                }
                return null;
              })}
          </div>
        </div>

        {/* Playback Controls & Speed Multipliers */}
        <div className="flex flex-col lg:flex-row items-center justify-between gap-2">
          {/* Main Transport Buttons */}
          <div className="flex items-center gap-1.5">
            <button
              type="button"
              onClick={handleReset}
              disabled={!hasPackets}
              title="重置到起点 00:00"
              className="p-1.5 bg-slate-800 hover:bg-slate-700 text-slate-300 rounded-md transition-colors disabled:opacity-40 disabled:cursor-not-allowed disabled:hover:bg-slate-800"
            >
              <RotateCcw className="w-3.5 h-3.5" />
            </button>

            <button
              type="button"
              onClick={handleStepBack}
              disabled={!hasPackets || engineBusy}
              title={
                engineBusy
                  ? '引擎重放中不可单步（可先暂停后复位）'
                  : '逐帧单步后退'
              }
              className="p-1.5 bg-slate-800 hover:bg-slate-700 text-slate-300 rounded-md transition-colors disabled:opacity-40 disabled:cursor-not-allowed disabled:hover:bg-slate-800"
            >
              <StepBack className="w-3.5 h-3.5" />
            </button>

            <button
              type="button"
              onClick={handleTogglePlay}
              disabled={!hasPackets}
              className={`flex items-center gap-1.5 px-4 py-1.5 rounded-md font-semibold text-xs transition-all shadow-md active:scale-98 disabled:opacity-40 disabled:cursor-not-allowed disabled:active:scale-100 ${
                isPlaying
                  ? 'bg-amber-500 hover:bg-amber-400 text-slate-950'
                  : 'bg-sky-600 hover:bg-sky-500 text-white disabled:hover:bg-sky-600'
              }`}
            >
              {isPlaying ? (
                <>
                  <Pause className="w-3.5 h-3.5" />
                  <span>暂停重演</span>
                </>
              ) : (
                <>
                  <Play className="w-3.5 h-3.5 fill-white" />
                  <span>
                    {useEngineReplay
                      ? isPaused
                        ? '继续重演'
                        : useMultiChannelTargets
                          ? '多路引擎重演'
                          : '引擎重演'
                      : '高保真重演'}
                  </span>
                </>
              )}
            </button>

            <button
              type="button"
              onClick={handleStepForward}
              disabled={!hasPackets || engineBusy}
              title={
                engineBusy
                  ? '引擎重放中不可单步（可先暂停后复位）'
                  : '逐帧单步前进（微秒级步进）'
              }
              className="p-1.5 bg-slate-800 hover:bg-slate-700 text-slate-300 rounded-md transition-colors disabled:opacity-40 disabled:cursor-not-allowed disabled:hover:bg-slate-800"
            >
              <StepForward className="w-3.5 h-3.5" />
            </button>

            <button
              type="button"
              title={
                useEngineReplay
                  ? engineBusy
                    ? '循环在起播时生效；播放中切换将用于下次起播'
                    : '引擎重放：播完回卷，直到停止'
                  : '离线演示循环'
              }
              onClick={() => {
                const next = !isLooping;
                setIsLooping(next);
                setReplayConfig((cfg) => ({ ...cfg, loop: next }));
              }}
              className={`px-2.5 py-1 text-xs rounded-md border transition-colors ${
                isLooping
                  ? 'bg-slate-800 text-sky-400 border-sky-500/50'
                  : 'bg-slate-900 text-slate-500 border-slate-800'
              }`}
            >
              循环: {isLooping ? '开' : '关'}
            </button>
          </div>

          {/* Speed Presets */}
          <div className="flex items-center gap-1 p-0.5 bg-slate-800/80 rounded-md">
            <span className="text-[10px] text-slate-400 px-1.5">
              倍速:
            </span>
            {[0.1, 0.5, 1.0, 2.0, 5.0, 10.0].map((s) => (
              <button
                key={s}
                type="button"
                title={
                  useEngineReplay
                    ? '引擎重放按所选倍速调度墙钟间隔'
                    : '离线演示按所选倍速推进'
                }
                onClick={() => handleSpeedChange(s)}
                className={`px-2 py-0.5 text-[11px] font-mono font-medium rounded transition-colors ${
                  replaySpeed === s
                    ? 'bg-sky-600 text-white shadow-xs'
                    : 'text-slate-400 hover:text-slate-200'
                }`}
              >
                {s === 1.0 ? '1x' : `${s}x`}
              </button>
            ))}
          </div>
        </div>
      </div>

      {onAddMarkerAt && onTrimDownload && (
        <SessionMarkerTrimPanel
          engineConnected={engineConnected}
          hasPackets={hasPackets}
          currentTimeUs={currentTimeUs}
          durationUs={totalDurationUs}
          engineReplayBusy={engineBusy}
          onAddMarker={onAddMarkerAt}
          onTrimDownload={onTrimDownload}
        />
      )}

      {/* 下方面板随窗口伸缩；内容过多时仅此区域滚动 */}
      <div className="fr-page-scroll flex flex-col gap-3">
      {/* Target Output Mapping & Fault Injection Panel */}
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-3 shrink-0">
        {/* Card 1: Replay Output Target Configuration */}
        <div className="bg-white rounded-lg border border-slate-200 p-2.5 space-y-2 shadow-xs">
          <div className="flex items-center justify-between pb-2 border-b border-slate-100">
            <div className="flex items-center gap-1.5">
              <Cpu className="w-3.5 h-3.5 text-sky-600" />
              <h2 className="text-xs font-bold text-slate-900 tracking-wide">
                {useMultiChannelTargets
                  ? '多通道重放目标'
                  : '重放输出目标映射'}
              </h2>
            </div>
            <span className="text-[11px] text-emerald-600 font-medium flex items-center gap-1">
              <CheckCircle2 className="w-3.5 h-3.5" />
              {useMultiChannelTargets
                ? `${sessionMetadata.channels.length} 通道`
                : '就绪'}
            </span>
          </div>

          {replayHint && (
            <div className="px-2 py-1.5 bg-rose-50 border border-rose-200 rounded-md text-[11px] text-rose-800 flex items-start gap-1.5">
              <AlertTriangle className="w-3.5 h-3.5 shrink-0 mt-0.5" />
              <span>{replayHint}</span>
            </div>
          )}

          {useMultiChannelTargets ? (
            <ReplayChannelTargets
              rows={channelTargets}
              disabled={engineBusy}
              onChange={setChannelTargets}
            />
          ) : (
            <>
          {/* 4-Protocol Tab Switcher */}
          <div className="grid grid-cols-4 gap-0.5 p-0.5 bg-slate-100 rounded-md text-xs">
            <button
              onClick={() => setActiveTargetTab('tcp')}
              className={`py-1 px-1.5 rounded font-medium transition-colors text-center truncate ${
                activeTargetTab === 'tcp'
                  ? 'bg-white text-sky-700 shadow-xs'
                  : 'text-slate-600 hover:text-slate-900'
              }`}
            >
              TCP 套接字
            </button>
            <button
              onClick={() => setActiveTargetTab('mqtt')}
              className={`py-1 px-1.5 rounded font-medium transition-colors text-center truncate ${
                activeTargetTab === 'mqtt'
                  ? 'bg-white text-teal-700 shadow-xs'
                  : 'text-slate-600 hover:text-slate-900'
              }`}
            >
              MQTT 消息
            </button>
            <button
              onClick={() => setActiveTargetTab('udp')}
              className={`py-1 px-1.5 rounded font-medium transition-colors text-center truncate ${
                activeTargetTab === 'udp'
                  ? sessionIsVideo
                    ? 'bg-white text-violet-700 shadow-xs'
                    : 'bg-white text-slate-900 shadow-xs'
                  : 'text-slate-600 hover:text-slate-900'
              }`}
            >
              {sessionIsVideo ? '视频 RTP' : 'UDP 网络'}
            </button>
            <button
              onClick={() => setActiveTargetTab('serial')}
              className={`py-1 px-1.5 rounded font-medium transition-colors text-center truncate ${
                activeTargetTab === 'serial'
                  ? 'bg-white text-slate-900 shadow-xs'
                  : 'text-slate-600 hover:text-slate-900'
              }`}
            >
              串口 / RS485
            </button>
          </div>

          {/* Tab 1: TCP Detailed Handler */}
          {activeTargetTab === 'tcp' && (
            <div className="p-2.5 bg-sky-50/60 rounded-md border border-sky-200/80 space-y-2 text-xs">
              <div className="flex items-center justify-between gap-2">
                <span className="font-semibold text-slate-900">TCP 重演连接策略</span>
                <div className="flex items-center gap-2">
                  {replayConfig.tcp.mode === 'tool_is_server' && (
                    <span className="text-[10px] px-1.5 py-0.5 rounded font-medium bg-sky-100 text-sky-800 border border-sky-200">
                      已连接客户端 {replayConfig.tcp.connectedClientsCount}
                    </span>
                  )}
                </div>
              </div>

              {/* Mode switcher */}
              <div className="grid grid-cols-2 gap-1.5">
                <button
                  type="button"
                  onClick={() =>
                    setReplayConfig({
                      ...replayConfig,
                      tcp: { ...replayConfig.tcp, mode: 'tool_is_server' },
                    })
                  }
                  title="工具作 Server：上位机作为 Client 连入"
                  className={`px-2 py-1.5 rounded-md text-left border transition-all ${
                    replayConfig.tcp.mode === 'tool_is_server'
                      ? 'bg-white border-sky-400 text-sky-950 shadow-xs ring-1 ring-sky-400'
                      : 'bg-white/60 border-slate-200 text-slate-600'
                  }`}
                >
                  <div className="font-semibold text-[11px]">A：工具作 Server（推荐）</div>
                </button>

                <button
                  type="button"
                  onClick={() =>
                    setReplayConfig({
                      ...replayConfig,
                      tcp: { ...replayConfig.tcp, mode: 'tool_is_client' },
                    })
                  }
                  title="工具作 Client：主动连接上位机监听端口"
                  className={`px-2 py-1.5 rounded-md text-left border transition-all ${
                    replayConfig.tcp.mode === 'tool_is_client'
                      ? 'bg-white border-sky-400 text-sky-950 shadow-xs ring-1 ring-sky-400'
                      : 'bg-white/60 border-slate-200 text-slate-600'
                  }`}
                >
                  <div className="font-semibold text-[11px]">B：工具作 Client</div>
                </button>
              </div>

              {/* Parameters according to mode */}
              {replayConfig.tcp.mode === 'tool_is_server' ? (
                <div className="px-2 py-1.5 bg-white rounded border border-sky-100">
                  <div className="flex items-center justify-between gap-2">
                    <span className="text-slate-600 text-[11px]">本地监听:</span>
                    <div className="flex items-center gap-1 font-mono">
                      <span className="text-slate-500 text-[11px]">127.0.0.1 :</span>
                      <input
                        type="number"
                        value={replayConfig.tcp.serverPort}
                        onChange={(e) =>
                          setReplayConfig({
                            ...replayConfig,
                            tcp: {
                              ...replayConfig.tcp,
                              serverPort: parseInt(e.target.value) || 9000,
                            },
                          })
                        }
                        className="w-16 bg-slate-50 border border-slate-300 rounded px-1.5 py-0.5 text-xs text-slate-800"
                      />
                    </div>
                  </div>
                </div>
              ) : (
                <div className="px-2 py-1.5 bg-white rounded border border-sky-100">
                  <div className="flex items-center justify-between gap-2">
                    <span className="text-slate-600 text-[11px]">连接目标:</span>
                    <div className="flex items-center gap-1 font-mono">
                      <input
                        type="text"
                        value={replayConfig.tcp.clientTargetHost}
                        onChange={(e) =>
                          setReplayConfig({
                            ...replayConfig,
                            tcp: { ...replayConfig.tcp, clientTargetHost: e.target.value },
                          })
                        }
                        className="w-24 bg-slate-50 border border-slate-300 rounded px-1.5 py-0.5 text-xs text-slate-800"
                      />
                      <span className="text-slate-400">:</span>
                      <input
                        type="number"
                        value={replayConfig.tcp.clientTargetPort}
                        onChange={(e) =>
                          setReplayConfig({
                            ...replayConfig,
                            tcp: {
                              ...replayConfig.tcp,
                              clientTargetPort: parseInt(e.target.value) || 9000,
                            },
                          })
                        }
                        className="w-16 bg-slate-50 border border-slate-300 rounded px-1.5 py-0.5 text-xs text-slate-800"
                      />
                    </div>
                  </div>
                </div>
              )}
            </div>
          )}

          {/* Tab 2: MQTT Detailed Handler */}
          {activeTargetTab === 'mqtt' && (
            <div className="p-2.5 bg-teal-50/60 rounded-md border border-teal-200/80 space-y-2 text-xs">
              <div className="flex items-center justify-between gap-2">
                <span className="font-semibold text-slate-900">MQTT 重演策略</span>
                <button
                  type="button"
                  onClick={() =>
                    setReplayConfig({
                      ...replayConfig,
                      mqtt: {
                        ...replayConfig.mqtt,
                        connectedToBroker: !replayConfig.mqtt.connectedToBroker,
                      },
                    })
                  }
                  className={`text-[10px] px-1.5 py-0.5 rounded font-medium border transition-colors ${
                    replayConfig.mqtt.connectedToBroker
                      ? 'bg-emerald-50 text-emerald-700 border-emerald-200'
                      : 'bg-slate-100 text-slate-500 border-slate-200'
                  }`}
                >
                  {replayConfig.mqtt.connectedToBroker ? '● Broker 已连接' : '○ Broker 未连接'}
                </button>
              </div>

              <div className="grid grid-cols-2 gap-1.5">
                <button
                  type="button"
                  onClick={() =>
                    setReplayConfig({
                      ...replayConfig,
                      mqtt: { ...replayConfig.mqtt, mode: 'virtual_publisher' },
                    })
                  }
                  title="作为 Client 连本地 Broker 并按原 Topic 发布"
                  className={`px-2 py-1.5 rounded-md text-left border transition-all ${
                    replayConfig.mqtt.mode === 'virtual_publisher'
                      ? 'bg-white border-teal-400 text-teal-950 shadow-xs ring-1 ring-teal-400'
                      : 'bg-white/60 border-slate-200 text-slate-600'
                  }`}
                >
                  <div className="font-semibold text-[11px]">A：虚拟发布者（推荐）</div>
                </button>

                <button
                  type="button"
                  onClick={() =>
                    setReplayConfig({
                      ...replayConfig,
                      mqtt: { ...replayConfig.mqtt, mode: 'embedded_broker' },
                    })
                  }
                  title="内置轻量 Broker，免装 Mosquitto/EMQX"
                  className={`px-2 py-1.5 rounded-md text-left border transition-all ${
                    replayConfig.mqtt.mode === 'embedded_broker'
                      ? 'bg-white border-teal-400 text-teal-950 shadow-xs ring-1 ring-teal-400'
                      : 'bg-white/60 border-slate-200 text-slate-600'
                  }`}
                >
                  <div className="font-semibold text-[11px]">B：内置 Broker</div>
                </button>
              </div>

              <div className="px-2 py-1.5 bg-white rounded border border-teal-100 space-y-1.5">
                <div className="flex items-center justify-between gap-2">
                  <span className="text-slate-600 text-[11px] shrink-0">Broker:</span>
                  <div className="flex items-center gap-1 font-mono">
                    <input
                      type="text"
                      value={replayConfig.mqtt.brokerHost}
                      onChange={(e) =>
                        setReplayConfig({
                          ...replayConfig,
                          mqtt: { ...replayConfig.mqtt, brokerHost: e.target.value },
                        })
                      }
                      className="w-24 bg-slate-50 border border-slate-300 rounded px-1.5 py-0.5 text-xs text-slate-800"
                    />
                    <span className="text-slate-400">:</span>
                    <input
                      type="number"
                      value={replayConfig.mqtt.brokerPort}
                      onChange={(e) =>
                        setReplayConfig({
                          ...replayConfig,
                          mqtt: {
                            ...replayConfig.mqtt,
                            brokerPort: parseInt(e.target.value) || 1883,
                          },
                        })
                      }
                      className="w-16 bg-slate-50 border border-slate-300 rounded px-1.5 py-0.5 text-xs text-slate-800"
                    />
                  </div>
                </div>

                <div className="flex items-center justify-between gap-2">
                  <span className="text-slate-600 text-[11px] shrink-0">Topic 前缀:</span>
                  <input
                    type="text"
                    value={replayConfig.mqtt.topicPrefix}
                    onChange={(e) =>
                      setReplayConfig({
                        ...replayConfig,
                        mqtt: { ...replayConfig.mqtt, topicPrefix: e.target.value },
                      })
                    }
                    placeholder="留空=原 Topic"
                    className="flex-1 min-w-0 bg-slate-50 border border-slate-300 rounded px-1.5 py-0.5 text-xs text-slate-800 font-mono"
                  />
                </div>

                <div className="flex items-center justify-between gap-2">
                  <span className="text-slate-600 text-[11px] shrink-0">用户名:</span>
                  <input
                    type="text"
                    value={replayConfig.mqtt.username}
                    onChange={(e) =>
                      setReplayConfig({
                        ...replayConfig,
                        mqtt: { ...replayConfig.mqtt, username: e.target.value },
                      })
                    }
                    placeholder="可选"
                    autoComplete="off"
                    className="flex-1 min-w-0 bg-slate-50 border border-slate-300 rounded px-1.5 py-0.5 text-xs text-slate-800 font-mono"
                  />
                </div>

                <div className="flex items-center justify-between gap-2">
                  <span className="text-slate-600 text-[11px] shrink-0">密码:</span>
                  <input
                    type="password"
                    value={replayConfig.mqtt.password}
                    onChange={(e) =>
                      setReplayConfig({
                        ...replayConfig,
                        mqtt: { ...replayConfig.mqtt, password: e.target.value },
                      })
                    }
                    placeholder="可选"
                    autoComplete="off"
                    className="flex-1 min-w-0 bg-slate-50 border border-slate-300 rounded px-1.5 py-0.5 text-xs text-slate-800 font-mono"
                  />
                </div>

                <label className="flex items-center gap-2 text-[11px] text-slate-600">
                  <input
                    type="checkbox"
                    checked={replayConfig.mqtt.tls}
                    onChange={(e) =>
                      setReplayConfig({
                        ...replayConfig,
                        mqtt: {
                          ...replayConfig.mqtt,
                          tls: e.target.checked,
                          brokerPort: e.target.checked
                            ? replayConfig.mqtt.brokerPort === 1883
                              ? 8883
                              : replayConfig.mqtt.brokerPort
                            : replayConfig.mqtt.brokerPort === 8883
                              ? 1883
                              : replayConfig.mqtt.brokerPort,
                        },
                      })
                    }
                  />
                  TLS（mqtts）
                </label>

                {replayConfig.mqtt.tls && (
                  <>
                    <div className="flex items-center justify-between gap-2">
                      <span className="text-slate-600 text-[11px] shrink-0">CA PEM:</span>
                      <input
                        type="text"
                        value={replayConfig.mqtt.caCertPath}
                        onChange={(e) =>
                          setReplayConfig({
                            ...replayConfig,
                            mqtt: { ...replayConfig.mqtt, caCertPath: e.target.value },
                          })
                        }
                        placeholder="可选；空=系统根"
                        className="flex-1 min-w-0 bg-slate-50 border border-slate-300 rounded px-1.5 py-0.5 text-xs text-slate-800 font-mono"
                      />
                    </div>
                    <div className="flex items-center justify-between gap-2">
                      <span className="text-slate-600 text-[11px] shrink-0">SNI:</span>
                      <input
                        type="text"
                        value={replayConfig.mqtt.tlsServerName}
                        onChange={(e) =>
                          setReplayConfig({
                            ...replayConfig,
                            mqtt: { ...replayConfig.mqtt, tlsServerName: e.target.value },
                          })
                        }
                        placeholder="可选；默认用 Broker 主机"
                        className="flex-1 min-w-0 bg-slate-50 border border-slate-300 rounded px-1.5 py-0.5 text-xs text-slate-800 font-mono"
                      />
                    </div>
                  </>
                )}
              </div>
            </div>
          )}

          {/* Tab 3: UDP / 视频 RTP Handler */}
          {activeTargetTab === 'udp' && (
            <div
              className={`p-2.5 rounded-md border space-y-1.5 text-xs ${
                sessionIsVideo
                  ? 'bg-violet-50/70 border-violet-200/80'
                  : 'bg-slate-50 border-slate-200/80'
              }`}
            >
              <div className="flex items-center justify-between gap-2">
                <span className="font-semibold text-slate-800">
                  {sessionIsVideo ? '视频 RTP 推送目标' : 'UDP 推送目标'}
                </span>
                {sessionIsVideo && (
                  <span className="text-[10px] px-1.5 py-0.5 rounded font-medium bg-violet-100 text-violet-800 border border-violet-200 shrink-0">
                    会话含视频通道
                  </span>
                )}
              </div>
              {sessionIsVideo && (
                <>
                  <p className="text-[11px] text-violet-900/80 leading-snug">
                    不解 H.264。可选 UDP 推送已录数据报，或本机 RTSP 服务端（交织 TCP）供上位机拉流。
                  </p>
                  <div className="flex items-center gap-3 text-[11px]">
                    <label className="flex items-center gap-1.5 cursor-pointer">
                      <input
                        type="radio"
                        name="videoOutput"
                        checked={(replayConfig.videoOutput || 'udp') === 'udp'}
                        onChange={() =>
                          setReplayConfig({ ...replayConfig, videoOutput: 'udp' })
                        }
                      />
                      UDP 推送
                    </label>
                    <label className="flex items-center gap-1.5 cursor-pointer">
                      <input
                        type="radio"
                        name="videoOutput"
                        checked={replayConfig.videoOutput === 'rtsp_listen'}
                        onChange={() =>
                          setReplayConfig({
                            ...replayConfig,
                            videoOutput: 'rtsp_listen',
                          })
                        }
                      />
                      RTSP 服务端
                    </label>
                  </div>
                </>
              )}
              {sessionIsVideo && replayConfig.videoOutput === 'rtsp_listen' ? (
                <div className="flex items-center gap-2">
                  <span className="text-slate-500">监听口:</span>
                  <span className="font-mono text-slate-700">0.0.0.0:</span>
                  <input
                    type="number"
                    value={replayConfig.rtspListenPort ?? 8554}
                    onChange={(e) =>
                      setReplayConfig({
                        ...replayConfig,
                        rtspListenPort: parseInt(e.target.value) || 8554,
                      })
                    }
                    className="bg-white border border-slate-300 rounded px-2 py-0.5 text-slate-800 w-20 font-mono text-xs"
                  />
                  <span className="text-[10px] text-slate-500">
                    上位机连 rtsp://本机:端口/…
                  </span>
                </div>
              ) : (
                <div className="flex items-center gap-2">
                  <span className="text-slate-500">推送到:</span>
                  <input
                    type="text"
                    value={replayConfig.targetIp}
                    onChange={(e) =>
                      setReplayConfig({ ...replayConfig, targetIp: e.target.value })
                    }
                    className="bg-white border border-slate-300 rounded px-2 py-0.5 text-slate-800 w-28 font-mono text-xs"
                  />
                  <span className="text-slate-400">:</span>
                  <input
                    type="number"
                    value={replayConfig.targetUdpPort}
                    onChange={(e) =>
                      setReplayConfig({
                        ...replayConfig,
                        targetUdpPort:
                          parseInt(e.target.value) || (sessionIsVideo ? 5004 : 8080),
                      })
                    }
                    className="bg-white border border-slate-300 rounded px-2 py-0.5 text-slate-800 w-20 font-mono text-xs"
                  />
                </div>
              )}
            </div>
          )}

          {/* Tab 4: Serial Handler */}
          {activeTargetTab === 'serial' && (
            <div className="p-2.5 bg-slate-50 rounded-md border border-slate-200/80 space-y-2 text-xs">
              <div className="flex items-center justify-between gap-2">
                <span className="font-semibold text-slate-800">串口 / RS485 虚拟对</span>
                <button
                  type="button"
                  onClick={() => void refreshSerialPorts()}
                  disabled={serialListBusy}
                  className="px-2 py-0.5 rounded text-[11px] font-medium border border-slate-200 bg-white text-slate-600 hover:bg-slate-50 disabled:opacity-50"
                >
                  {serialListBusy ? '刷新中…' : '刷新本机口'}
                </button>
              </div>
              {serialListHint && (
                <p className="text-[10px] text-slate-500">{serialListHint}</p>
              )}

              <div className="grid grid-cols-1 sm:grid-cols-2 gap-2 p-2 bg-white rounded-md border border-slate-200/90">
                <div>
                  <div className="flex items-center justify-between mb-1">
                    <label className="text-[11px] font-semibold text-slate-700">
                      发送串口:
                    </label>
                    <span className={`text-[10px] px-1.5 py-0.2 rounded font-medium ${
                      replayConfig.serial.isPortOpen
                        ? 'bg-emerald-50 text-emerald-700 border border-emerald-200'
                        : 'bg-slate-100 text-slate-500'
                    }`}>
                      {replayConfig.serial.isPortOpen ? '● 就绪' : '○ 关闭'}
                    </span>
                  </div>
                  <div className="flex items-center gap-1.5">
                    <select
                      value={replayConfig.serial.selectedPort}
                      onChange={(e) =>
                        setReplayConfig({
                          ...replayConfig,
                          virtualComPort: e.target.value,
                          serial: { ...replayConfig.serial, selectedPort: e.target.value },
                        })
                      }
                      className="bg-slate-50 border border-slate-300 rounded px-2 py-0.5 text-xs text-slate-900 font-mono flex-1 focus:bg-white focus:outline-none focus:ring-1 focus:ring-sky-500"
                    >
                      {replayConfig.serial.availablePorts.map((p) => (
                        <option key={p} value={p}>
                          {p}
                        </option>
                      ))}
                    </select>

                    <button
                      type="button"
                      onClick={() =>
                        setReplayConfig({
                          ...replayConfig,
                          serial: {
                            ...replayConfig.serial,
                            isPortOpen: !replayConfig.serial.isPortOpen,
                          },
                        })
                      }
                      className={`px-2 py-0.5 rounded text-[11px] font-medium border transition-colors ${
                        replayConfig.serial.isPortOpen
                          ? 'bg-rose-50 text-rose-700 border-rose-200 hover:bg-rose-100'
                          : 'bg-emerald-50 text-emerald-700 border-emerald-200 hover:bg-emerald-100'
                      }`}
                    >
                      {replayConfig.serial.isPortOpen ? '关闭' : '打开'}
                    </button>
                  </div>
                </div>

                <div className="space-y-1.5">
                  <div>
                    <label className="block text-[11px] font-semibold text-slate-700 mb-1">
                      波特率:
                    </label>
                    <select
                      value={replayConfig.serial.baudRate}
                      onChange={(e) =>
                        setReplayConfig({
                          ...replayConfig,
                          serial: {
                            ...replayConfig.serial,
                            baudRate: parseInt(e.target.value) || 115200,
                          },
                        })
                      }
                      className="w-full bg-slate-50 border border-slate-300 rounded px-2 py-0.5 text-xs text-slate-900 font-mono focus:bg-white focus:outline-none focus:ring-1 focus:ring-sky-500"
                    >
                      {[9600, 19200, 38400, 57600, 115200, 460800, 921600].map((b) => (
                        <option key={b} value={b}>
                          {b} bps
                        </option>
                      ))}
                    </select>
                  </div>
                  <div className="flex items-center gap-1.5">
                    <label className="text-[11px] font-semibold text-slate-700 shrink-0">
                      校验:
                    </label>
                    <select
                      value={replayConfig.serial.parity || 'none'}
                      onChange={(e) =>
                        setReplayConfig({
                          ...replayConfig,
                          serial: {
                            ...replayConfig.serial,
                            parity: e.target.value as 'none' | 'even' | 'odd',
                          },
                        })
                      }
                      className="flex-1 bg-slate-50 border border-slate-300 rounded px-2 py-0.5 text-xs text-slate-900 font-mono focus:bg-white focus:outline-none focus:ring-1 focus:ring-sky-500"
                    >
                      <option value="none">None</option>
                      <option value="even">Even</option>
                      <option value="odd">Odd</option>
                    </select>
                    <span className="text-[10px] text-slate-500 font-mono shrink-0">
                      {formatSerialLineSummary(replayConfig.serial)}
                    </span>
                  </div>
                </div>
              </div>

              <div className="p-2 bg-white rounded-md border border-slate-200/90 space-y-1.5">
                <div className="flex items-center justify-between gap-2">
                  <span className="text-[11px] font-semibold text-slate-700">
                    上位机串口:
                  </span>
                  <input
                    type="text"
                    value={replayConfig.serial.hostComPort}
                    onChange={(e) =>
                      setReplayConfig({
                        ...replayConfig,
                        serial: { ...replayConfig.serial, hostComPort: e.target.value },
                      })
                    }
                    className="w-16 bg-slate-50 border border-slate-300 rounded px-1.5 py-0.5 text-xs text-slate-900 font-mono font-bold text-center"
                  />
                </div>

                <div className="px-2 py-1.5 bg-slate-900 text-slate-100 rounded-md flex items-center justify-between text-[11px] font-mono">
                  <span className="text-emerald-400 font-bold">{replayConfig.serial.selectedPort}</span>
                  <span className="text-sky-300 text-[10px]">══ com0com ══▶</span>
                  <span className="text-sky-400 font-bold">{replayConfig.serial.hostComPort}</span>
                </div>

                <div className="flex items-center gap-1.5 flex-wrap">
                  <span className="text-[11px] text-slate-500">预设:</span>
                  {[
                    { send: 'COM2', recv: 'COM1', label: 'COM1↔2' },
                    { send: 'COM4', recv: 'COM3', label: 'COM3↔4' },
                    { send: 'COM11', recv: 'COM10', label: 'COM10↔11' },
                  ].map((preset) => (
                    <button
                      key={preset.label}
                      type="button"
                      onClick={() =>
                        setReplayConfig({
                          ...replayConfig,
                          virtualComPort: preset.send,
                          serial: {
                            ...replayConfig.serial,
                            selectedPort: preset.send,
                            hostComPort: preset.recv,
                          },
                        })
                      }
                      className={`px-1.5 py-0.5 rounded text-[10px] font-mono border transition-colors ${
                        replayConfig.serial.selectedPort === preset.send &&
                        replayConfig.serial.hostComPort === preset.recv
                          ? 'bg-sky-50 text-sky-800 border-sky-300 font-bold'
                          : 'bg-slate-50 text-slate-600 border-slate-200 hover:bg-slate-100'
                      }`}
                    >
                      {preset.label}
                    </button>
                  ))}
                </div>
              </div>
            </div>
          )}
            </>
          )}
        </div>

        {/* Card 2: Fault Injection Stress Testing */}
        <div className="bg-white rounded-lg border border-slate-200 p-2.5 space-y-2 shadow-xs">
          <div className="flex items-center justify-between pb-1.5 border-b border-slate-100">
            <div className="flex items-center gap-1.5">
              <Zap className="w-3.5 h-3.5 text-amber-500" />
              <h2 className="text-xs font-bold text-slate-900 tracking-wide">
                故障注入与健壮性测试
              </h2>
            </div>
            <label className="flex items-center gap-1.5 cursor-pointer">
              <input
                type="checkbox"
                checked={replayConfig.faultInjection.enabled}
                onChange={(e) =>
                  setReplayConfig({
                    ...replayConfig,
                    faultInjection: {
                      ...replayConfig.faultInjection,
                      enabled: e.target.checked,
                    },
                  })
                }
                className="rounded border-slate-300 text-sky-600 focus:ring-sky-500"
              />
              <span className="text-xs font-medium text-slate-700">启用</span>
            </label>
          </div>
          {useEngineReplay && (
            <p className="text-[10px] text-slate-500 leading-snug">
              引擎已连：丢包/篡改在发送热路径生效（不解析业务 CRC，只扰动原始载荷）。
            </p>
          )}

          <div
            className={`space-y-2 text-xs transition-opacity ${
              replayConfig.faultInjection.enabled ? 'opacity-100' : 'opacity-40 pointer-events-none'
            }`}
          >
            <div>
              <div className="flex justify-between mb-0.5">
                <span className="font-medium text-slate-700">模拟随机丢包率:</span>
                <span className="font-mono text-slate-900">
                  {replayConfig.faultInjection.lossRatePercent}%
                </span>
              </div>
              <input
                type="range"
                min="0"
                max="40"
                step="5"
                value={replayConfig.faultInjection.lossRatePercent}
                onChange={(e) =>
                  setReplayConfig({
                    ...replayConfig,
                    faultInjection: {
                      ...replayConfig.faultInjection,
                      lossRatePercent: parseInt(e.target.value),
                    },
                  })
                }
                className="w-full accent-amber-500 h-1.5"
              />
            </div>

            <div className="flex items-center justify-between">
              <span className="font-medium text-slate-700">随机篡改 CRC16 校验码:</span>
              <input
                type="checkbox"
                checked={replayConfig.faultInjection.corruptCrc}
                onChange={(e) =>
                  setReplayConfig({
                    ...replayConfig,
                    faultInjection: {
                      ...replayConfig.faultInjection,
                      corruptCrc: e.target.checked,
                    },
                  })
                }
                className="rounded border-slate-300 text-amber-600"
              />
            </div>
          </div>
        </div>
      </div>

      {/* Current Replaying Packet Inspector & Dispatched Stream */}
      <div className="grid grid-cols-1 lg:grid-cols-3 gap-3">
        <div className="lg:col-span-2 bg-white rounded-lg border border-slate-200 p-2.5 space-y-2 shadow-xs">
          <div className="flex items-center justify-between pb-1.5 border-b border-slate-100 gap-2">
            <h3 className="text-xs font-bold text-slate-900 tracking-wide">
              当前重播帧
              {currentPacket ? ` #${currentPacket.id}` : ''}
            </h3>
            <div className="flex items-center gap-2 shrink-0">
              {currentPacket?.hasError && (
                <span className="text-[11px] text-rose-600 font-semibold flex items-center gap-1">
                  <AlertTriangle className="w-3.5 h-3.5" />
                  {currentPacket.errorReason}
                </span>
              )}
              {currentPacket && (
                <button
                  type="button"
                  onClick={() => onSelectPacketForInspection(currentPacket)}
                  className="text-[11px] text-sky-700 hover:text-sky-900 font-medium flex items-center gap-1 px-2 py-0.5 rounded border border-sky-200 bg-sky-50 hover:bg-sky-100"
                >
                  <ExternalLink className="w-3 h-3" />
                  报文页查看
                </button>
              )}
            </div>
          </div>

          {currentPacket ? (
            <div className="space-y-2 text-xs font-mono">
              <div className="grid grid-cols-2 sm:grid-cols-4 gap-2 px-2 py-1.5 bg-slate-50 rounded-md border border-slate-100 text-slate-600">
                <div>
                  <span className="text-[10px] text-slate-400 block">相对时间</span>
                  <strong className="text-slate-900 text-xs">
                    +{(currentPacket.timestampUs / 1000).toFixed(2)} ms
                  </strong>
                </div>
                <div>
                  <span className="text-[10px] text-slate-400 block">来源通道</span>
                  <strong className="text-slate-900 text-xs truncate block">
                    {currentPacket.channelName}
                  </strong>
                </div>
                <div>
                  <span className="text-[10px] text-slate-400 block">长度</span>
                  <strong className="text-slate-900 text-xs">{currentPacket.length} B</strong>
                </div>
                <div>
                  <span className="text-[10px] text-slate-400 block">类型</span>
                  <strong className="text-sky-700 text-xs">
                    {currentPacket.channelType.toUpperCase()}
                    {currentPacket.protocolHint ? ` · ${currentPacket.protocolHint}` : ''}
                  </strong>
                </div>
              </div>

              {currentPacket.mqttTopic && (
                <div className="px-2 py-1 bg-teal-50/70 border border-teal-100 rounded-md font-sans text-[11px] text-teal-900">
                  <span className="font-semibold">MQTT Topic: </span>
                  <code className="font-mono">{currentPacket.mqttTopic}</code>
                  {typeof currentPacket.mqttQoS === 'number' && (
                    <span className="ml-2 text-teal-700">QoS {currentPacket.mqttQoS}</span>
                  )}
                </div>
              )}

              <div>
                <span className="text-[11px] font-sans font-semibold text-slate-700 block mb-0.5">
                  HEX:
                </span>
                <div className="px-2 py-1.5 bg-slate-900 text-slate-100 rounded-md font-mono text-[11px] overflow-x-auto leading-snug border border-slate-800 max-h-24 overflow-y-auto">
                  <div className="text-emerald-400 select-all">{currentPacket.hexPreview}</div>
                </div>
              </div>
            </div>
          ) : (
            <div className="flex flex-col items-center justify-center py-6 text-slate-400 space-y-1.5">
              <Activity className="w-6 h-6 text-slate-300 stroke-[1.5]" />
              <p className="text-xs">暂无数据帧，请先载入 .fcap</p>
            </div>
          )}
        </div>

        <div className="bg-white rounded-lg border border-slate-200 p-2.5 space-y-2 shadow-xs flex flex-col min-h-[10rem]">
          <div className="flex items-center justify-between pb-1.5 border-b border-slate-100 shrink-0">
            <h3 className="text-xs font-bold text-slate-900 tracking-wide">
              发往上位机日志
            </h3>
            <span className="text-[11px] text-slate-400 font-mono">
              {dispatchedPackets.length} 条
              {dispatchedPackets.filter((d) => d.status === 'dropped').length > 0 &&
                ` · 丢 ${dispatchedPackets.filter((d) => d.status === 'dropped').length}`}
            </span>
          </div>

          <div className="flex-1 min-h-[6rem] max-h-48 overflow-y-auto divide-y divide-slate-100 font-mono text-[11px]">
            {dispatchedPackets.length === 0 ? (
              <div className="flex flex-col items-center justify-center h-full text-slate-400 font-sans text-xs space-y-1.5 py-4">
                <Activity className="w-5 h-5 text-slate-300 stroke-[1.5]" />
                <p>
                  {hasPackets
                    ? '点击「高保真重演」开始推送'
                    : '无数据可推送，请先载入会话'}
                </p>
              </div>
            ) : (
              dispatchedPackets.map((dp, i) => (
                <div key={`${dp.id}-${i}`} className="py-1 flex items-center justify-between text-slate-700">
                  <div className="truncate mr-2">
                    <span className="text-slate-400">[{dp.time.slice(3)}]</span>{' '}
                    <span className="font-semibold text-slate-900">#{dp.id}</span>{' '}
                    <span className="text-slate-500 truncate">{dp.channel.split(' ')[0]}</span>
                  </div>
                  <div>
                    {dp.status === 'ok' && (
                      <span className="text-emerald-600 font-semibold">{dp.bytes}B TX</span>
                    )}
                    {dp.status === 'dropped' && (
                      <span className="text-rose-600 font-semibold">DROPPED</span>
                    )}
                    {dp.status === 'corrupted' && (
                      <span className="text-amber-600 font-semibold">CRC_ERR</span>
                    )}
                  </div>
                </div>
              ))
            )}
          </div>
        </div>
      </div>
      </div>
    </div>
  );
};
