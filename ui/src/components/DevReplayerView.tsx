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

interface DevReplayerViewProps {
  sessionMetadata: CaptureSessionMetadata;
  packets: CapturedPacket[];
  onUploadSession: (metadata: CaptureSessionMetadata, packets: CapturedPacket[]) => void;
  /** 优先走引擎解析真实二进制 .fcap；未连引擎时可回落 JSON 演示包 */
  onLoadCaptureFile?: (file: File) => void | Promise<void>;
  onSelectPacketForInspection: (packet: CapturedPacket) => void;
}

export const DevReplayerView: React.FC<DevReplayerViewProps> = ({
  sessionMetadata,
  packets,
  onUploadSession,
  onLoadCaptureFile,
  onSelectPacketForInspection,
}) => {
  const [isPlaying, setIsPlaying] = useState(false);
  const [currentIndex, setCurrentIndex] = useState(0);
  const [replaySpeed, setReplaySpeed] = useState<number>(1.0);
  const [isLooping, setIsLooping] = useState(true);

  // Replay Output Target Configuration
  const [replayConfig, setReplayConfig] = useState<ReplayConfig>({
    speed: 1.0,
    loop: true,
    targetIp: '127.0.0.1',
    targetUdpPort: 8080,
    virtualComPort: 'COM2',
    serial: {
      selectedPort: 'COM2',
      qtTargetPort: 'COM1',
      baudRate: 115200,
      isPortOpen: true,
      availablePorts: ['COM1', 'COM2', 'COM3', 'COM4', 'COM10', 'COM11', '/dev/ttyUSB0', '/dev/pts/2'],
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

  const [dispatchedPackets, setDispatchedPackets] = useState<
    { id: number; time: string; channel: string; status: 'ok' | 'dropped' | 'corrupted'; bytes: number }[]
  >([]);

  const fileInputRef = useRef<HTMLInputElement>(null);
  const timerRef = useRef<any>(null);
  const hasPackets = packets.length > 0;

  const totalDurationUs = hasPackets ? packets[packets.length - 1].timestampUs : 0;
  const safeIndex = hasPackets ? Math.min(currentIndex, packets.length - 1) : 0;
  const currentPacket = hasPackets ? packets[safeIndex] : undefined;
  const currentTimeUs = currentPacket?.timestampUs || 0;
  const progressPercent =
    totalDurationUs > 0 ? Math.min(100, (currentTimeUs / totalDurationUs) * 100) : 0;

  // 会话包变更时复位播放头与推送日志，避免越界
  useEffect(() => {
    setIsPlaying(false);
    setCurrentIndex(0);
    setDispatchedPackets([]);
  }, [sessionMetadata.sessionTitle, packets.length]);

  // Playback Loop Engine
  useEffect(() => {
    if (!isPlaying || packets.length === 0) {
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

        // Fault injection check
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

        // 丢包也记入日志，便于核对故障注入效果
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
  }, [isPlaying, replaySpeed, isLooping, packets, replayConfig.faultInjection]);

  const handleTogglePlay = () => {
    if (!hasPackets) return;
    if (currentIndex >= packets.length - 1) {
      setCurrentIndex(0);
    }
    setIsPlaying(!isPlaying);
  };

  const handleReset = () => {
    setIsPlaying(false);
    setCurrentIndex(0);
  };

  const handleStepForward = () => {
    if (!hasPackets) return;
    setIsPlaying(false);
    setCurrentIndex((prev) => Math.min(packets.length - 1, prev + 1));
  };

  const handleStepBack = () => {
    if (!hasPackets) return;
    setIsPlaying(false);
    setCurrentIndex((prev) => Math.max(0, prev - 1));
  };

  const handleSeek = (e: React.ChangeEvent<HTMLInputElement>) => {
    if (!hasPackets || totalDurationUs <= 0) return;
    const val = parseFloat(e.target.value);
    const targetUs = (val / 100) * totalDurationUs;
    // Find closest packet index
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
    <div className="max-w-7xl mx-auto px-4 sm:px-6 py-6 space-y-6">
      {/* Session Metadata Banner */}
      <div className="bg-white rounded-xl border border-slate-200 p-5 shadow-xs">
        <div className="flex flex-col md:flex-row items-start md:items-center justify-between gap-4 pb-4 border-b border-slate-100">
          <div>
            <div className="flex items-center gap-2">
              <span className="w-2.5 h-2.5 rounded-full bg-emerald-500" />
              <h1 className="text-base font-bold text-slate-900 tracking-tight">
                {sessionMetadata.sessionTitle}
              </h1>
            </div>
            <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-slate-500 mt-1">
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

          <div className="flex items-center gap-2">
            <input
              type="file"
              ref={fileInputRef}
              onChange={handleFileUpload}
              accept=".fcap,.json"
              className="hidden"
            />
            <button
              onClick={() => fileInputRef.current?.click()}
              className="px-3.5 py-1.5 text-xs font-medium text-slate-700 bg-slate-100 hover:bg-slate-200 rounded-lg transition-colors flex items-center gap-1.5"
            >
              <Upload className="w-3.5 h-3.5" />
              <span>载入其他 .fcap 包</span>
            </button>
          </div>
        </div>

        {sessionMetadata.notes && (
          <div className="mt-3 p-3 bg-amber-50/70 border border-amber-200/60 rounded-lg text-xs text-amber-900 flex items-start gap-2">
            <AlertTriangle className="w-4 h-4 text-amber-600 shrink-0 mt-0.5" />
            <div>
              <span className="font-semibold">现场工单留言: </span>
              {sessionMetadata.notes}
            </div>
          </div>
        )}
      </div>

      {/* Master Replayer Media Console */}
      <div className="bg-slate-900 text-white rounded-xl p-6 shadow-md border border-slate-800 space-y-5">
        {!hasPackets && (
          <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3 p-3.5 bg-amber-500/10 border border-amber-400/30 rounded-lg">
            <div className="flex items-start gap-2 text-xs text-amber-100">
              <AlertTriangle className="w-4 h-4 text-amber-400 shrink-0 mt-0.5" />
              <p>
                当前会话无数据帧，播放条已禁用。请先在「现场录制」导出包，或点击上方「载入其他 .fcap 包」。
              </p>
            </div>
            <button
              type="button"
              onClick={() => fileInputRef.current?.click()}
              className="shrink-0 px-3 py-1.5 text-xs font-medium bg-amber-500 hover:bg-amber-400 text-slate-950 rounded-lg"
            >
              载入 .fcap
            </button>
          </div>
        )}

        {/* Scrubber Timeline */}
        <div className="space-y-2">
          <div className="flex items-center justify-between text-xs font-mono">
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
              className="w-full h-2.5 bg-slate-800 rounded-lg appearance-none accent-sky-500 hover:accent-sky-400 disabled:opacity-40 disabled:cursor-not-allowed disabled:hover:accent-sky-500"
            />
            {/* 故障点：菱形标记（避免两根竖条像「暂停」） */}
            {totalDurationUs > 0 &&
              packets.map((p, idx) =>
                p.hasError ? (
                  <button
                    key={p.id}
                    type="button"
                    onClick={() => setCurrentIndex(idx)}
                    title={`故障点：${p.errorReason || '异常帧'}（点击跳转）`}
                    aria-label={`故障点：${p.errorReason || '异常帧'}`}
                    className="absolute top-1/2 -translate-y-1/2 -translate-x-1/2 w-2.5 h-2.5 rotate-45 bg-rose-500 border border-rose-200 shadow-sm cursor-pointer hover:scale-150 transition-transform z-10"
                    style={{ left: `${(p.timestampUs / totalDurationUs) * 100}%` }}
                  />
                ) : null
              )}
          </div>
        </div>

        {/* Playback Controls & Speed Multipliers */}
        <div className="flex flex-col lg:flex-row items-center justify-between gap-4 pt-2">
          {/* Main Transport Buttons */}
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={handleReset}
              disabled={!hasPackets}
              title="重置到起点 00:00"
              className="p-2.5 bg-slate-800 hover:bg-slate-700 text-slate-300 rounded-lg transition-colors disabled:opacity-40 disabled:cursor-not-allowed disabled:hover:bg-slate-800"
            >
              <RotateCcw className="w-4 h-4" />
            </button>

            <button
              type="button"
              onClick={handleStepBack}
              disabled={!hasPackets}
              title="逐帧单步后退"
              className="p-2.5 bg-slate-800 hover:bg-slate-700 text-slate-300 rounded-lg transition-colors disabled:opacity-40 disabled:cursor-not-allowed disabled:hover:bg-slate-800"
            >
              <StepBack className="w-4 h-4" />
            </button>

            <button
              type="button"
              onClick={handleTogglePlay}
              disabled={!hasPackets}
              className={`flex items-center gap-2 px-6 py-2.5 rounded-lg font-semibold text-xs transition-all shadow-md active:scale-98 disabled:opacity-40 disabled:cursor-not-allowed disabled:active:scale-100 ${
                isPlaying
                  ? 'bg-amber-500 hover:bg-amber-400 text-slate-950'
                  : 'bg-sky-600 hover:bg-sky-500 text-white disabled:hover:bg-sky-600'
              }`}
            >
              {isPlaying ? (
                <>
                  <Pause className="w-4 h-4" />
                  <span>暂停重演</span>
                </>
              ) : (
                <>
                  <Play className="w-4 h-4 fill-white" />
                  <span>高保真重演</span>
                </>
              )}
            </button>

            <button
              type="button"
              onClick={handleStepForward}
              disabled={!hasPackets}
              title="逐帧单步前进（微秒级步进）"
              className="p-2.5 bg-slate-800 hover:bg-slate-700 text-slate-300 rounded-lg transition-colors disabled:opacity-40 disabled:cursor-not-allowed disabled:hover:bg-slate-800"
            >
              <StepForward className="w-4 h-4" />
            </button>

            <button
              type="button"
              onClick={() => {
                const next = !isLooping;
                setIsLooping(next);
                setReplayConfig((cfg) => ({ ...cfg, loop: next }));
              }}
              className={`px-3 py-2 text-xs rounded-lg border transition-colors ${
                isLooping
                  ? 'bg-slate-800 text-sky-400 border-sky-500/50'
                  : 'bg-slate-900 text-slate-500 border-slate-800'
              }`}
            >
              循环回放: {isLooping ? '开' : '关'}
            </button>
          </div>

          {/* Speed Presets */}
          <div className="flex items-center gap-1.5 p-1 bg-slate-800/80 rounded-lg">
            <span className="text-[11px] text-slate-400 px-2">倍速:</span>
            {[0.1, 0.5, 1.0, 2.0, 5.0, 10.0].map((s) => (
              <button
                key={s}
                type="button"
                onClick={() => handleSpeedChange(s)}
                className={`px-2.5 py-1 text-xs font-mono font-medium rounded-md transition-colors ${
                  replaySpeed === s
                    ? 'bg-sky-600 text-white shadow-xs'
                    : 'text-slate-400 hover:text-slate-200'
                }`}
              >
                {s === 1.0 ? '1x 原速' : `${s}x`}
              </button>
            ))}
          </div>
        </div>
      </div>

      {/* Target Output Mapping & Fault Injection Panel */}
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        {/* Card 1: Replay Output Target Configuration (Where to send data to Qt) */}
        <div className="bg-white rounded-xl border border-slate-200 p-5 space-y-4 shadow-xs">
          <div className="flex items-center justify-between pb-3 border-b border-slate-100">
            <div className="flex items-center gap-2">
              <Cpu className="w-4 h-4 text-sky-600" />
              <h2 className="text-xs font-bold text-slate-900 uppercase tracking-wide">
                重放输出目标映射 (发给本机 Qt 上位机)
              </h2>
            </div>
            <span className="text-[11px] text-emerald-600 font-medium flex items-center gap-1">
              <CheckCircle2 className="w-3.5 h-3.5" />
              就绪
            </span>
          </div>

          <p className="text-xs text-slate-500">
            针对工控常用的 4 种通信协议，重演工具提供零侵入的本机端口注入：
          </p>

          {/* 4-Protocol Tab Switcher */}
          <div className="grid grid-cols-4 gap-1 p-1 bg-slate-100 rounded-lg text-xs">
            <button
              onClick={() => setActiveTargetTab('tcp')}
              className={`py-1.5 px-2 rounded-md font-medium transition-colors text-center truncate ${
                activeTargetTab === 'tcp'
                  ? 'bg-white text-sky-700 shadow-xs'
                  : 'text-slate-600 hover:text-slate-900'
              }`}
            >
              TCP 套接字
            </button>
            <button
              onClick={() => setActiveTargetTab('mqtt')}
              className={`py-1.5 px-2 rounded-md font-medium transition-colors text-center truncate ${
                activeTargetTab === 'mqtt'
                  ? 'bg-white text-teal-700 shadow-xs'
                  : 'text-slate-600 hover:text-slate-900'
              }`}
            >
              MQTT 消息
            </button>
            <button
              onClick={() => setActiveTargetTab('udp')}
              className={`py-1.5 px-2 rounded-md font-medium transition-colors text-center truncate ${
                activeTargetTab === 'udp'
                  ? 'bg-white text-slate-900 shadow-xs'
                  : 'text-slate-600 hover:text-slate-900'
              }`}
            >
              UDP 网络
            </button>
            <button
              onClick={() => setActiveTargetTab('serial')}
              className={`py-1.5 px-2 rounded-md font-medium transition-colors text-center truncate ${
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
            <div className="p-3.5 bg-sky-50/60 rounded-lg border border-sky-200/80 space-y-3 text-xs">
              <div className="flex items-center justify-between gap-2">
                <span className="font-semibold text-slate-900">TCP 重演连接策略</span>
                <div className="flex items-center gap-2">
                  {replayConfig.tcp.mode === 'tool_is_server' && (
                    <span className="text-[10px] px-1.5 py-0.5 rounded font-medium bg-sky-100 text-sky-800 border border-sky-200">
                      已连接客户端 {replayConfig.tcp.connectedClientsCount}
                    </span>
                  )}
                  <span className="text-[11px] text-sky-700 font-mono">上位机套接字对接</span>
                </div>
              </div>

              {/* Mode switcher */}
              <div className="grid grid-cols-2 gap-2">
                <button
                  type="button"
                  onClick={() =>
                    setReplayConfig({
                      ...replayConfig,
                      tcp: { ...replayConfig.tcp, mode: 'tool_is_server' },
                    })
                  }
                  className={`p-2 rounded-md text-left border transition-all ${
                    replayConfig.tcp.mode === 'tool_is_server'
                      ? 'bg-white border-sky-400 text-sky-950 shadow-xs ring-1 ring-sky-400'
                      : 'bg-white/60 border-slate-200 text-slate-600'
                  }`}
                >
                  <div className="font-semibold text-[11px]">方案 A：重演工具充当 Server (推荐)</div>
                  <div className="text-[10px] text-slate-500 mt-0.5">
                    现场设备是服务端 (如PLC/工控机)，Qt上位机作为 Client 连过来
                  </div>
                </button>

                <button
                  type="button"
                  onClick={() =>
                    setReplayConfig({
                      ...replayConfig,
                      tcp: { ...replayConfig.tcp, mode: 'tool_is_client' },
                    })
                  }
                  className={`p-2 rounded-md text-left border transition-all ${
                    replayConfig.tcp.mode === 'tool_is_client'
                      ? 'bg-white border-sky-400 text-sky-950 shadow-xs ring-1 ring-sky-400'
                      : 'bg-white/60 border-slate-200 text-slate-600'
                  }`}
                >
                  <div className="font-semibold text-[11px]">方案 B：重演工具充当 Client</div>
                  <div className="text-[10px] text-slate-500 mt-0.5">
                    Qt上位机内部在监听端口 (QTcpServer)，重演工具主动连上去推流
                  </div>
                </button>
              </div>

              {/* Parameters according to mode */}
              {replayConfig.tcp.mode === 'tool_is_server' ? (
                <div className="p-2.5 bg-white rounded border border-sky-100 space-y-2">
                  <div className="flex items-center justify-between">
                    <span className="text-slate-600 text-[11px]">重演服务监听端口:</span>
                    <div className="flex items-center gap-1 font-mono">
                      <span className="text-slate-500">127.0.0.1 :</span>
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
                  <p className="text-[11px] text-slate-500 leading-relaxed">
                    💡 <strong>Qt 对接方式：</strong>Qt 上位机将连接目标从现场 IP 改为{' '}
                    <code className="text-sky-700 bg-sky-50 px-1 py-0.5 rounded font-mono font-bold">
                      127.0.0.1:{replayConfig.tcp.serverPort}
                    </code>
                    。握手成功后，重演工具会按录制的微秒间隔向该 TCP 连接 <code>write()</code> 原始数据！
                  </p>
                </div>
              ) : (
                <div className="p-2.5 bg-white rounded border border-sky-100 space-y-2">
                  <div className="flex items-center justify-between">
                    <span className="text-slate-600 text-[11px]">连接目标 Qt 上位机:</span>
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
                  <p className="text-[11px] text-slate-500 leading-relaxed">
                    💡 <strong>Qt 对接方式：</strong>Qt 运行 <code>QTcpServer::listen()</code>，重演工具会自动作为客户端发起连接并发送现场数据流。
                  </p>
                </div>
              )}
            </div>
          )}

          {/* Tab 2: MQTT Detailed Handler */}
          {activeTargetTab === 'mqtt' && (
            <div className="p-3.5 bg-teal-50/60 rounded-lg border border-teal-200/80 space-y-3 text-xs">
              <div className="flex items-center justify-between gap-2">
                <span className="font-semibold text-slate-900">MQTT 物联网重演策略</span>
                <div className="flex items-center gap-2">
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
                  <span className="text-[11px] text-teal-700 font-mono">MQTT 客户端对接</span>
                </div>
              </div>

              {/* Mode switcher */}
              <div className="grid grid-cols-2 gap-2">
                <button
                  type="button"
                  onClick={() =>
                    setReplayConfig({
                      ...replayConfig,
                      mqtt: { ...replayConfig.mqtt, mode: 'virtual_publisher' },
                    })
                  }
                  className={`p-2 rounded-md text-left border transition-all ${
                    replayConfig.mqtt.mode === 'virtual_publisher'
                      ? 'bg-white border-teal-400 text-teal-950 shadow-xs ring-1 ring-teal-400'
                      : 'bg-white/60 border-slate-200 text-slate-600'
                  }`}
                >
                  <div className="font-semibold text-[11px]">方案 A：虚拟现场发布者 (最推荐)</div>
                  <div className="text-[10px] text-slate-500 mt-0.5">
                    重演工具作为 Client 连上本地 Broker，准时 Publish 原始 Topic 与载荷
                  </div>
                </button>

                <button
                  type="button"
                  onClick={() =>
                    setReplayConfig({
                      ...replayConfig,
                      mqtt: { ...replayConfig.mqtt, mode: 'embedded_broker' },
                    })
                  }
                  className={`p-2 rounded-md text-left border transition-all ${
                    replayConfig.mqtt.mode === 'embedded_broker'
                      ? 'bg-white border-teal-400 text-teal-950 shadow-xs ring-1 ring-teal-400'
                      : 'bg-white/60 border-slate-200 text-slate-600'
                  }`}
                >
                  <div className="font-semibold text-[11px]">方案 B：内置轻量 Broker 模式</div>
                  <div className="text-[10px] text-slate-500 mt-0.5">
                    重演工具在本地模拟简易 Broker 服务，免装 Mosquitto/EMQX
                  </div>
                </button>
              </div>

              <div className="p-2.5 bg-white rounded border border-teal-100 space-y-2">
                <div className="flex items-center justify-between">
                  <span className="text-slate-600 text-[11px]">开发机本地 MQTT Broker:</span>
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
                  <span className="text-slate-600 text-[11px] shrink-0">主题前缀改写 (可选):</span>
                  <input
                    type="text"
                    value={replayConfig.mqtt.topicPrefix}
                    onChange={(e) =>
                      setReplayConfig({
                        ...replayConfig,
                        mqtt: { ...replayConfig.mqtt, topicPrefix: e.target.value },
                      })
                    }
                    placeholder="留空=原 Topic；如 lab/"
                    className="flex-1 min-w-0 bg-slate-50 border border-slate-300 rounded px-1.5 py-0.5 text-xs text-slate-800 font-mono"
                  />
                </div>

                <div className="text-[11px] text-slate-600 space-y-1">
                  <div>
                    · <strong>主题重放 (Topic Replay):</strong> 严格按录制时序，向原始主题（如{' '}
                    <code className="text-teal-700 font-mono">factory/line3/servo/telemetry</code>
                    ）发布
                    {replayConfig.mqtt.topicPrefix
                      ? `；当前前缀「${replayConfig.mqtt.topicPrefix}」会拼到 Topic 前`
                      : ''}
                    ；
                  </div>
                  <div>
                    · <strong>QoS 完整保真:</strong> 原包的 QoS 0 / 1 / 2 及 Retain 标志完全保留；
                  </div>
                  <div>
                    · <strong>上位机零改动:</strong> 订阅原有通配符（如{' '}
                    <code className="text-teal-700 font-mono">factory/+/telemetry</code>
                    ），即可准时收到消息触发。
                  </div>
                </div>
              </div>
            </div>
          )}

          {/* Tab 3: UDP Handler */}
          {activeTargetTab === 'udp' && (
            <div className="p-3.5 bg-slate-50 rounded-lg border border-slate-200/80 space-y-2 text-xs">
              <div className="flex items-center justify-between">
                <span className="font-semibold text-slate-800">UDP 网络无连接目标</span>
                <span className="text-[11px] text-slate-500 font-mono">Qt QUdpSocket 对接</span>
              </div>
              <div className="flex items-center gap-2">
                <span className="text-slate-500">推送到:</span>
                <input
                  type="text"
                  value={replayConfig.targetIp}
                  onChange={(e) =>
                    setReplayConfig({ ...replayConfig, targetIp: e.target.value })
                  }
                  className="bg-white border border-slate-300 rounded px-2 py-1 text-slate-800 w-28 font-mono text-xs"
                />
                <span className="text-slate-400">:</span>
                <input
                  type="number"
                  value={replayConfig.targetUdpPort}
                  onChange={(e) =>
                    setReplayConfig({
                      ...replayConfig,
                      targetUdpPort: parseInt(e.target.value) || 8080,
                    })
                  }
                  className="bg-white border border-slate-300 rounded px-2 py-1 text-slate-800 w-20 font-mono text-xs"
                />
              </div>
              <p className="text-[11px] text-slate-500">
                UDP 属于无连接协议，重演工具直接将录制的 Datagram 数据包使用 <code>QUdpSocket::writeDatagram</code> 发送到本地回环端口即可。
              </p>
            </div>
          )}

          {/* Tab 4: Serial Handler */}
          {activeTargetTab === 'serial' && (
            <div className="p-3.5 bg-slate-50 rounded-lg border border-slate-200/80 space-y-3 text-xs">
              <div className="flex items-center justify-between">
                <span className="font-semibold text-slate-800">串口 / RS485 虚拟端口对重放</span>
                <span className="text-[11px] text-slate-500 font-mono">Qt QSerialPort 对接</span>
              </div>

              {/* Port & Baudrate Selector Bar */}
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 p-3 bg-white rounded-lg border border-slate-200/90">
                {/* Replayer output port */}
                <div>
                  <div className="flex items-center justify-between mb-1">
                    <label className="text-[11px] font-semibold text-slate-700">
                      ① 重演工具发送串口:
                    </label>
                    <span className={`text-[10px] px-1.5 py-0.2 rounded font-medium ${
                      replayConfig.serial.isPortOpen
                        ? 'bg-emerald-50 text-emerald-700 border border-emerald-200'
                        : 'bg-slate-100 text-slate-500'
                    }`}>
                      {replayConfig.serial.isPortOpen ? '● 端口已就绪' : '○ 端口关闭'}
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
                      className="bg-slate-50 border border-slate-300 rounded px-2 py-1 text-xs text-slate-900 font-mono flex-1 focus:bg-white focus:outline-none focus:ring-1 focus:ring-sky-500"
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
                      className={`px-2.5 py-1 rounded text-[11px] font-medium border transition-colors ${
                        replayConfig.serial.isPortOpen
                          ? 'bg-rose-50 text-rose-700 border-rose-200 hover:bg-rose-100'
                          : 'bg-emerald-50 text-emerald-700 border-emerald-200 hover:bg-emerald-100'
                      }`}
                    >
                      {replayConfig.serial.isPortOpen ? '关闭' : '打开'}
                    </button>
                  </div>
                </div>

                {/* Baudrate selector */}
                <div>
                  <label className="block text-[11px] font-semibold text-slate-700 mb-1">
                    波特率 (Baud Rate):
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
                    className="w-full bg-slate-50 border border-slate-300 rounded px-2 py-1 text-xs text-slate-900 font-mono focus:bg-white focus:outline-none focus:ring-1 focus:ring-sky-500"
                  >
                    {[9600, 19200, 38400, 57600, 115200, 460800, 921600].map((b) => (
                      <option key={b} value={b}>
                        {b} bps (8-N-1)
                      </option>
                    ))}
                  </select>
                </div>
              </div>

              {/* Topology / Pairing Preview Box */}
              <div className="p-3 bg-white rounded-lg border border-slate-200/90 space-y-2">
                <div className="flex items-center justify-between">
                  <span className="text-[11px] font-semibold text-slate-700">
                    ② 虚拟串口对拓扑链路 (开发者视角):
                  </span>
                  <div className="flex items-center gap-1.5 text-[11px]">
                    <span className="text-slate-500">Qt上位机打开:</span>
                    <input
                      type="text"
                      value={replayConfig.serial.qtTargetPort}
                      onChange={(e) =>
                        setReplayConfig({
                          ...replayConfig,
                          serial: { ...replayConfig.serial, qtTargetPort: e.target.value },
                        })
                      }
                      className="w-16 bg-slate-50 border border-slate-300 rounded px-1.5 py-0.5 text-xs text-slate-900 font-mono font-bold text-center"
                    />
                  </div>
                </div>

                {/* Visual Topology Diagram */}
                <div className="p-2.5 bg-slate-900 text-slate-100 rounded-lg flex items-center justify-between text-xs font-mono">
                  <div className="text-center">
                    <div className="text-[10px] text-slate-400">重演工具写入端</div>
                    <div className="text-emerald-400 font-bold mt-0.5">
                      {replayConfig.serial.selectedPort}
                    </div>
                  </div>

                  <div className="flex-1 mx-3 text-center">
                    <div className="text-[10px] text-sky-300">══ 虚拟串口互通链路 (com0com) ══▶</div>
                    <div className="text-[9px] text-slate-500 mt-0.5">零延迟内核级数据管道</div>
                  </div>

                  <div className="text-center">
                    <div className="text-[10px] text-slate-400">Qt 上位机读取端</div>
                    <div className="text-sky-400 font-bold mt-0.5">
                      {replayConfig.serial.qtTargetPort}
                    </div>
                  </div>
                </div>

                {/* Fast Presets Switcher */}
                <div className="flex items-center gap-2 pt-1">
                  <span className="text-[11px] text-slate-500">常用配对预设:</span>
                  <div className="flex flex-wrap gap-1.5">
                    {[
                      { send: 'COM2', recv: 'COM1', label: 'COM1 ↔ COM2' },
                      { send: 'COM4', recv: 'COM3', label: 'COM3 ↔ COM4' },
                      { send: 'COM11', recv: 'COM10', label: 'COM10 ↔ COM11' },
                      { send: '/dev/pts/2', recv: '/dev/pts/1', label: 'Linux pts' },
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
                              qtTargetPort: preset.recv,
                            },
                          })
                        }
                        className={`px-2 py-0.5 rounded text-[10px] font-mono border transition-colors ${
                          replayConfig.serial.selectedPort === preset.send &&
                          replayConfig.serial.qtTargetPort === preset.recv
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

              <div className="text-[11px] text-slate-500 leading-relaxed px-1">
                💡 <strong>开发者说明：</strong>开发机安装虚拟串口驱动工具（Windows 下常用免费开源的{' '}
                <code className="text-slate-800 font-mono font-bold">com0com</code> 或{' '}
                <code className="text-slate-800 font-mono font-bold">VSPD</code>；Linux 下用{' '}
                <code className="text-slate-800 font-mono font-bold">socat</code>）后，重演工具只需下拉选择写入端（如 COM2），Qt 上位机连接对应的配对端（如 COM1），上位机无需硬件即可 100% 接收现场报文！
              </div>
            </div>
          )}
        </div>

        {/* Card 2: Fault Injection Stress Testing */}
        <div className="bg-white rounded-xl border border-slate-200 p-5 space-y-4 shadow-xs">
          <div className="flex items-center justify-between pb-3 border-b border-slate-100">
            <div className="flex items-center gap-2">
              <Zap className="w-4 h-4 text-amber-500" />
              <h2 className="text-xs font-bold text-slate-900 uppercase tracking-wide">
                故障注入与健壮性测试 (Fault Injection)
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
              <span className="text-xs font-medium text-slate-700">启用故障注入</span>
            </label>
          </div>

          <p className="text-xs text-slate-500">
            在重播过程中主动引入干扰，验证Qt上位机在恶劣工业工况下的容错与重连能力：
          </p>

          <div
            className={`space-y-3 text-xs transition-opacity ${
              replayConfig.faultInjection.enabled ? 'opacity-100' : 'opacity-40 pointer-events-none'
            }`}
          >
            {/* Packet Loss */}
            <div>
              <div className="flex justify-between mb-1">
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
                className="w-full accent-amber-500"
              />
            </div>

            {/* Checksum corruption */}
            <div className="flex items-center justify-between pt-1">
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
      <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
        {/* Left 2 Cols: Detailed Inspector of Current Replay Frame */}
        <div className="lg:col-span-2 bg-white rounded-xl border border-slate-200 p-5 space-y-4 shadow-xs">
          <div className="flex items-center justify-between pb-3 border-b border-slate-100 gap-2">
            <h3 className="text-xs font-bold text-slate-900 uppercase tracking-wide">
              当前重播数据帧深度检查
              {currentPacket ? ` (Frame #${currentPacket.id})` : ''}
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
                  className="text-[11px] text-sky-700 hover:text-sky-900 font-medium flex items-center gap-1 px-2 py-1 rounded border border-sky-200 bg-sky-50 hover:bg-sky-100"
                >
                  <ExternalLink className="w-3 h-3" />
                  报文页查看
                </button>
              )}
            </div>
          </div>

          {currentPacket ? (
            <div className="space-y-4 text-xs font-mono">
              <div className="grid grid-cols-2 sm:grid-cols-4 gap-3 p-3 bg-slate-50 rounded-lg border border-slate-100 text-slate-600">
                <div>
                  <span className="text-[10px] text-slate-400 block uppercase">相对时间戳</span>
                  <strong className="text-slate-900 text-xs">
                    +{(currentPacket.timestampUs / 1000).toFixed(2)} ms
                  </strong>
                </div>
                <div>
                  <span className="text-[10px] text-slate-400 block uppercase">来源通道</span>
                  <strong className="text-slate-900 text-xs truncate block">
                    {currentPacket.channelName}
                  </strong>
                </div>
                <div>
                  <span className="text-[10px] text-slate-400 block uppercase">报文长度</span>
                  <strong className="text-slate-900 text-xs">{currentPacket.length} Bytes</strong>
                </div>
                <div>
                  <span className="text-[10px] text-slate-400 block uppercase">通道类型</span>
                  <strong className="text-sky-700 text-xs">
                    {currentPacket.channelType.toUpperCase()}
                    {currentPacket.protocolHint ? ` · ${currentPacket.protocolHint}` : ''}
                  </strong>
                </div>
              </div>

              {currentPacket.mqttTopic && (
                <div className="p-2.5 bg-teal-50/70 border border-teal-100 rounded-lg font-sans text-[11px] text-teal-900">
                  <span className="font-semibold">MQTT Topic: </span>
                  <code className="font-mono">{currentPacket.mqttTopic}</code>
                  {typeof currentPacket.mqttQoS === 'number' && (
                    <span className="ml-2 text-teal-700">QoS {currentPacket.mqttQoS}</span>
                  )}
                </div>
              )}

              {/* Hex：只展示原始字节，不做业务协议解包 */}
              <div>
                <span className="text-[11px] font-sans font-semibold text-slate-700 block mb-1">
                  原始字节数据 (HEX Dump):
                </span>
                <div className="p-3 bg-slate-900 text-slate-100 rounded-lg font-mono text-xs overflow-x-auto leading-relaxed border border-slate-800">
                  <div className="text-emerald-400 select-all">{currentPacket.hexPreview}</div>
                </div>
              </div>
            </div>
          ) : (
            <div className="flex flex-col items-center justify-center py-12 text-slate-400 space-y-2">
              <Activity className="w-8 h-8 text-slate-300 stroke-[1.5]" />
              <p className="text-xs">暂无数据帧可检查，请先载入 .fcap 或从录制页导出会话</p>
            </div>
          )}
        </div>

        {/* Right 1 Col: Live Dispatched Packets to host */}
        <div className="bg-white rounded-xl border border-slate-200 p-5 space-y-3 shadow-xs">
          <div className="flex items-center justify-between pb-3 border-b border-slate-100">
            <h3 className="text-xs font-bold text-slate-900 uppercase tracking-wide">
              实时发往上位机日志
            </h3>
            <span className="text-[11px] text-slate-400 font-mono">
              已记 {dispatchedPackets.length} 条
              {dispatchedPackets.filter((d) => d.status === 'dropped').length > 0 &&
                ` · 丢 ${dispatchedPackets.filter((d) => d.status === 'dropped').length}`}
            </span>
          </div>

          <div className="h-72 overflow-y-auto divide-y divide-slate-100 font-mono text-[11px]">
            {dispatchedPackets.length === 0 ? (
              <div className="flex flex-col items-center justify-center h-full text-slate-400 font-sans text-xs space-y-2 py-12">
                <Activity className="w-7 h-7 text-slate-300 stroke-[1.5]" />
                <p>
                  {hasPackets
                    ? '点击「高保真重演」开始按时间轴推送'
                    : '无数据可推送，请先载入会话包'}
                </p>
              </div>
            ) : (
              dispatchedPackets.map((dp, i) => (
                <div key={`${dp.id}-${i}`} className="py-2 flex items-center justify-between text-slate-700">
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
  );
};
