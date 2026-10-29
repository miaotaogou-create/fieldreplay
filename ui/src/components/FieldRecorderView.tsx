/**
 * @license
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useState, useEffect, useRef } from 'react';
import {
  ChannelConfig,
  CapturedPacket,
  CaptureSessionMetadata,
} from '../types/recorder';
import {
  createModbusPacket,
  createUdpLidarPacket,
  createTcpVisionPacket,
  createMqttPacket,
} from '../utils/mockFieldData';
import {
  Play,
  Square,
  Pause,
  Download,
  Plus,
  Search,
  Activity,
  FileCheck,
} from 'lucide-react';

interface FieldRecorderViewProps {
  channels: ChannelConfig[];
  onToggleChannel: (id: string) => void;
  onAddChannel: (channel: ChannelConfig) => void;
  recordedPackets: CapturedPacket[];
  onAppendPacket: (packet: CapturedPacket) => void;
  onClearSession: () => void;
  onExportSession: (metadata: CaptureSessionMetadata) => void | Promise<void>;
  onGoToReplay: () => void;
  engineConnected: boolean;
  onEngineStartRecord: (channel: ChannelConfig) => Promise<void>;
  onEngineStopRecord: () => Promise<unknown>;
  pickRecordChannel: (channels: ChannelConfig[]) => ChannelConfig | null;
}

export const FieldRecorderView: React.FC<FieldRecorderViewProps> = ({
  channels,
  onToggleChannel,
  onAddChannel,
  recordedPackets,
  onAppendPacket,
  onClearSession,
  onExportSession,
  onGoToReplay,
  engineConnected,
  onEngineStartRecord,
  onEngineStopRecord,
  pickRecordChannel,
}) => {
  const [recordState, setRecordState] = useState<'idle' | 'recording' | 'paused'>('idle');
  const [elapsedMs, setElapsedMs] = useState(0);
  // 引擎已连接时默认关模拟；未连接时保留离线演示
  const [simulateHardwareStream, setSimulateHardwareStream] = useState(!engineConnected);
  const [busy, setBusy] = useState(false);
  const [localHint, setLocalHint] = useState<string | null>(null);

  useEffect(() => {
    if (engineConnected) setSimulateHardwareStream(false);
  }, [engineConnected]);
  const [siteNotes, setSiteNotes] = useState('现场3号工位伺服偶现卡顿，录制10分钟正常及突发异常报文。');
  const [recordedBy, setRecordedBy] = useState('现场调试员-张工');
  const [siteLocation, setSiteLocation] = useState('苏州二厂自动化冲压车间');
  const [filterChannel, setFilterChannel] = useState<string>('all');
  const [searchQuery, setSearchQuery] = useState('');
  const [isExportModalOpen, setIsExportModalOpen] = useState(false);
  const [showAddChannelModal, setShowAddChannelModal] = useState(false);

  // New channel state
  const [newChanName, setNewChanName] = useState('COM4 (力矩传感器)');
  const [newChanType, setNewChanType] = useState<'serial' | 'udp' | 'tcp' | 'mqtt'>('serial');
  const [newChanParam, setNewChanParam] = useState('COM4, 921600, 8-N-1');

  // Auto-scroll log
  const logContainerRef = useRef<HTMLDivElement>(null);
  const [autoScroll, setAutoScroll] = useState(true);

  // Timer loop
  useEffect(() => {
    let timer: any;
    if (recordState === 'recording') {
      const startTime = Date.now() - elapsedMs;
      timer = setInterval(() => {
        setElapsedMs(Date.now() - startTime);
      }, 50);
    }
    return () => clearInterval(timer);
  }, [recordState]);

  // 仅离线演示：引擎未连接时才跑 mock 发包
  useEffect(() => {
    let packetTimer: any;
    if (
      recordState === 'recording'
      && simulateHardwareStream
      && !engineConnected
    ) {
      let counter = recordedPackets.length + 1;
      packetTimer = setInterval(() => {
        const nowUs = elapsedMs * 1000 + Math.floor(Math.random() * 500);

        // Send Modbus packet if serial channel is active
        const serialCh = channels.find((c) => c.type === 'serial' && c.enabled);
        if (serialCh && Math.random() > 0.3) {
          const rpm = 1450 + Math.floor(Math.sin(counter / 8) * 80);
          const p = createModbusPacket(counter++, nowUs, 1, 3, [rpm, 480, 210, 420]);
          p.channelId = serialCh.id;
          p.channelName = serialCh.name;
          onAppendPacket(p);
        }

        // Send UDP Lidar packet if active
        const udpCh = channels.find((c) => c.type === 'udp' && c.enabled);
        if (udpCh && Math.random() > 0.4) {
          const dist = 780 + Math.floor(Math.random() * 15);
          const p = createUdpLidarPacket(counter++, nowUs + 12000, dist);
          p.channelId = udpCh.id;
          p.channelName = udpCh.name;
          onAppendPacket(p);
        }

        // Send TCP packet occasionally
        const tcpCh = channels.find((c) => c.type === 'tcp' && c.enabled);
        if (tcpCh && Math.random() > 0.75) {
          const isOk = Math.random() > 0.15;
          const p = createTcpVisionPacket(counter++, nowUs + 25000, isOk, `SN-${2000 + counter}`);
          p.channelId = tcpCh.id;
          p.channelName = tcpCh.name;
          onAppendPacket(p);
        }

        // Send MQTT telemetry if active
        const mqttCh = channels.find((c) => c.type === 'mqtt' && c.enabled);
        if (mqttCh && Math.random() > 0.55) {
          const topic = mqttCh.params.mqttTopic ?? 'factory/line3/+/telemetry';
          const p = createMqttPacket(counter++, nowUs + 35000, topic.replace('+/', 'servo/'), {
            axis: 'Axis_Z',
            load_pct: 40 + Math.floor(Math.random() * 10),
            state: 'NOMINAL',
          });
          p.channelId = mqttCh.id;
          p.channelName = mqttCh.name;
          onAppendPacket(p);
        }
      }, 200);
    }
    return () => clearInterval(packetTimer);
  }, [
    recordState,
    simulateHardwareStream,
    engineConnected,
    elapsedMs,
    channels,
    recordedPackets.length,
    onAppendPacket,
  ]);

  // 有新报文且开启「锁定最新」时滚到底
  useEffect(() => {
    if (autoScroll && logContainerRef.current) {
      logContainerRef.current.scrollTop = logContainerRef.current.scrollHeight;
    }
  }, [recordedPackets, autoScroll]);

  // 用户上翻离开底部 → 切到自由浏览；回到底部可再锁
  const handleLogScroll = () => {
    const el = logContainerRef.current;
    if (!el) return;
    const nearBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
    if (!nearBottom && autoScroll) setAutoScroll(false);
  };

  const handleStartRecord = async () => {
    if (engineConnected) {
      const ch = pickRecordChannel(channels);
      if (!ch) {
        setLocalHint('请至少启用一个 UDP / 串口 / TCP / MQTT 通道');
        return;
      }
      setBusy(true);
      setLocalHint(null);
      try {
        await onEngineStartRecord(ch);
        setElapsedMs(0);
        setRecordState('recording');
      } catch (e) {
        setLocalHint(e instanceof Error ? e.message : String(e));
      } finally {
        setBusy(false);
      }
      return;
    }
    setRecordState('recording');
  };

  const handlePauseRecord = () => {
    if (engineConnected) {
      setLocalHint('真实引擎录制暂不支持暂停，请直接停止');
      return;
    }
    setRecordState('paused');
  };

  const handleStopAndPackage = async () => {
    if (engineConnected && recordState !== 'idle') {
      setBusy(true);
      try {
        await onEngineStopRecord();
        setRecordState('idle');
        setIsExportModalOpen(true);
      } catch (e) {
        setLocalHint(e instanceof Error ? e.message : String(e));
      } finally {
        setBusy(false);
      }
      return;
    }
    setRecordState('idle');
    setIsExportModalOpen(true);
  };

  const handleCreateChannel = (e: React.FormEvent) => {
    e.preventDefault();
    const isMqtt = newChanType === 'mqtt';
    const isSerial = newChanType === 'serial';
    const newChan: ChannelConfig = {
      id: `ch_custom_${Date.now()}`,
      name: newChanName,
      type: newChanType,
      enabled: true,
      color: isMqtt ? '#0d9488' : '#0891b2',
      params: {
        portName: isSerial ? newChanParam.split(',')[0]?.trim() || 'COM4' : undefined,
        baudRate: isSerial ? 921600 : undefined,
        ip: !isSerial && !isMqtt ? '127.0.0.1' : undefined,
        port: !isSerial && !isMqtt ? 8888 : undefined,
        mqttBroker: isMqtt ? (newChanParam.split(',')[0]?.trim() || '127.0.0.1:1883') : undefined,
        mqttTopic: isMqtt ? (newChanParam.split(',')[1]?.trim() || 'factory/+/telemetry') : undefined,
        mqttQoS: isMqtt ? 1 : undefined,
      },
      connected: true,
      rxBytes: 0,
      rxFrames: 0,
      currentRateBps: 0,
    };
    onAddChannel(newChan);
    setShowAddChannelModal(false);
  };

  const formatTimer = (ms: number) => {
    const totalSec = Math.floor(ms / 1000);
    const m = Math.floor(totalSec / 60)
      .toString()
      .padStart(2, '0');
    const s = (totalSec % 60).toString().padStart(2, '0');
    const mmm = (ms % 1000).toString().padStart(3, '0');
    return `${m}:${s}.${mmm}`;
  };

  const totalBytes = recordedPackets.reduce((acc, p) => acc + p.length, 0);
  const totalMb = (totalBytes / (1024 * 1024)).toFixed(2);
  const approxRateKb = elapsedMs > 0 ? ((totalBytes / (elapsedMs / 1000)) / 1024).toFixed(1) : '0.0';

  const filteredPackets = recordedPackets.filter((p) => {
    if (filterChannel !== 'all' && p.channelId !== filterChannel) return false;
    if (searchQuery) {
      const q = searchQuery.toLowerCase();
      return (
        p.hexPreview.toLowerCase().includes(q) ||
        p.asciiPreview.toLowerCase().includes(q) ||
        p.channelName.toLowerCase().includes(q) ||
        (p.protocolHint && p.protocolHint.toLowerCase().includes(q)) ||
        (p.mqttTopic && p.mqttTopic.toLowerCase().includes(q))
      );
    }
    return true;
  });
  const isFilterActive = filterChannel !== 'all' || searchQuery.trim().length > 0;

  const handleExportConfirm = () => {
    if (recordedPackets.length === 0) return;
    const meta: CaptureSessionMetadata = {
      sessionTitle: `现场捕获包_${new Date().toISOString().slice(0, 10)}_${Date.now().toString().slice(-4)}.fcap`,
      recordedBy,
      siteName: siteLocation,
      deviceModel: '工控上位机通用数据包 (跨硬件无损格式)',
      startTime: new Date().toLocaleString(),
      durationMs: elapsedMs,
      totalPackets: recordedPackets.length,
      totalBytes,
      channels: channels.map((c) => ({
        id: c.id,
        name: c.name,
        type: c.type,
        packetCount: recordedPackets.filter((p) => p.channelId === c.id).length,
      })),
      notes: siteNotes,
    };
    setIsExportModalOpen(false);
    void Promise.resolve(onExportSession(meta));
  };

  return (
    <div className="max-w-7xl mx-auto px-4 sm:px-6 py-6 space-y-6">
      {/* 3-Step Simple Guidance for Field Engineers */}
      <div className="bg-white rounded-xl border border-slate-200 p-5 shadow-xs">
        <div className="flex flex-col md:flex-row items-start md:items-center justify-between gap-4 pb-4 border-b border-slate-100">
          <div>
            <h1 className="text-lg font-bold text-slate-900 tracking-tight">
              现场数据无损录制 (Field Recorder)
            </h1>
            <p className="text-xs text-slate-500 mt-0.5">
              现场人员专用：免装专业分析软件、免改上位机代码，3步一键录制所有外部硬件流
            </p>
          </div>
          {/* Quick Step Indicators */}
          <div className="flex items-center gap-3 text-xs text-slate-500">
            <span className="flex items-center gap-1.5 font-medium text-slate-700">
              <span className="w-5 h-5 rounded-full bg-slate-900 text-white flex items-center justify-center text-[10px]">1</span>
              勾选信道
            </span>
            <span className="text-slate-300">→</span>
            <span className="flex items-center gap-1.5 font-medium text-slate-700">
              <span className="w-5 h-5 rounded-full bg-slate-900 text-white flex items-center justify-center text-[10px]">2</span>
              点击录制
            </span>
            <span className="text-slate-300">→</span>
            <span className="flex items-center gap-1.5 font-medium text-sky-600">
              <span className="w-5 h-5 rounded-full bg-sky-600 text-white flex items-center justify-center text-[10px]">3</span>
              打包发给研发
            </span>
          </div>
        </div>

        {/* Step 1: Channels Selector */}
        <div className="pt-4">
          <div className="flex items-center justify-between mb-3">
            <span className="text-xs font-semibold text-slate-700 tracking-wide uppercase">
              ① 现场输入信道 (勾选需要录制的硬件接口)
            </span>
            <button
              onClick={() => setShowAddChannelModal(true)}
              className="text-xs text-sky-600 hover:text-sky-700 font-medium flex items-center gap-1"
            >
              <Plus className="w-3.5 h-3.5" />
              添加自定义端口
            </button>
          </div>

          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-3">
            {channels.map((chan) => (
              <div
                key={chan.id}
                onClick={() => onToggleChannel(chan.id)}
                className={`cursor-pointer p-3.5 rounded-lg border transition-all text-left relative ${
                  chan.enabled
                    ? 'bg-slate-50 border-slate-300 shadow-xs ring-1 ring-sky-500/20'
                    : 'bg-white border-slate-200 opacity-60 hover:opacity-90'
                }`}
              >
                <div className="flex items-center justify-between mb-1.5">
                  <div className="flex items-center gap-2">
                    <input
                      type="checkbox"
                      checked={chan.enabled}
                      onClick={(e) => e.stopPropagation()}
                      onChange={() => onToggleChannel(chan.id)}
                      className="rounded border-slate-300 text-sky-600 focus:ring-sky-500"
                    />
                    <span className="text-xs font-semibold text-slate-900 truncate max-w-[140px]">
                      {chan.name}
                    </span>
                  </div>
                  <span
                    className="w-2 h-2 rounded-full"
                    style={{ backgroundColor: chan.color }}
                  />
                </div>

                <div className="text-[11px] text-slate-500 font-mono pl-5">
                  {chan.type === 'serial' && `${chan.params.portName ?? 'COM'} · ${chan.params.baudRate ?? 115200}bps 8-N-1`}
                  {chan.type === 'udp' && `UDP监听 :${chan.params.port ?? 8080}`}
                  {chan.type === 'tcp' && `TCP :${chan.params.port ?? 9000} (${chan.params.role ?? 'svr'})`}
                  {chan.type === 'mqtt' && `MQTT ${chan.params.mqttBroker ?? '127.0.0.1:1883'} · ${chan.params.mqttTopic ?? '#'}`}
                  {chan.type === 'video_stream' && `RTSP相机 1080P`}
                </div>
              </div>
            ))}
          </div>
        </div>
      </div>

      {/* Step 2: Main Master Recording Control Bar */}
      <div className="bg-slate-900 text-white rounded-xl p-6 shadow-md border border-slate-800">
        <div className="flex flex-col lg:flex-row items-center justify-between gap-6">
          {/* Big Recording Timer */}
          <div className="flex items-center gap-5">
            <div className="relative">
              <div
                className={`w-14 h-14 rounded-full flex items-center justify-center transition-all ${
                  recordState === 'recording'
                    ? 'bg-rose-600 text-white shadow-lg shadow-rose-600/40 ring-4 ring-rose-500/30'
                    : recordState === 'paused'
                    ? 'bg-amber-500 text-white'
                    : 'bg-slate-800 text-slate-400'
                }`}
              >
                {recordState === 'recording' && <span className="w-5 h-5 rounded-sm bg-white" />}
                {recordState === 'paused' && <Pause className="w-6 h-6" />}
                {recordState === 'idle' && <Square className="w-5 h-5 text-slate-500" />}
              </div>
            </div>

            <div>
              <div className="text-[11px] font-mono tracking-wider text-slate-400 uppercase">
                {recordState === 'recording'
                  ? '● 正在微秒级无损录制中...'
                  : recordState === 'paused'
                  ? '❚❚ 录制已暂停'
                  : '待命就绪 (点击开始录制)'}
              </div>
              <div className="text-3xl font-mono font-bold tracking-tight text-white tabular-nums">
                {formatTimer(elapsedMs)}
              </div>
            </div>
          </div>

          {/* Core Action Buttons */}
          <div className="flex items-center gap-3">
            {recordState === 'idle' ? (
              <button
                onClick={() => void handleStartRecord()}
                disabled={busy}
                className="flex items-center gap-2.5 px-6 py-3 bg-rose-600 hover:bg-rose-500 disabled:opacity-50 text-white rounded-lg font-semibold text-sm shadow-md transition-all active:scale-98"
              >
                <span className="w-2.5 h-2.5 rounded-full bg-white" />
                <span>
                  {engineConnected ? '开始引擎录制' : '开始现场录制'}
                </span>
              </button>
            ) : (
              <>
                {recordState === 'recording' ? (
                  <button
                    onClick={handlePauseRecord}
                    disabled={busy || engineConnected}
                    title={
                      engineConnected
                        ? '真实录制暂不支持暂停'
                        : undefined
                    }
                    className="flex items-center gap-2 px-4 py-2.5 bg-slate-800 hover:bg-slate-700 disabled:opacity-40 text-slate-200 rounded-lg text-xs font-medium border border-slate-700 transition-colors"
                  >
                    <Pause className="w-4 h-4 text-amber-400" />
                    <span>暂停</span>
                  </button>
                ) : (
                  <button
                    onClick={() => void handleStartRecord()}
                    disabled={busy}
                    className="flex items-center gap-2 px-4 py-2.5 bg-rose-600 hover:bg-rose-500 disabled:opacity-50 text-white rounded-lg text-xs font-medium transition-colors"
                  >
                    <Play className="w-4 h-4" />
                    <span>继续</span>
                  </button>
                )}

                <button
                  onClick={() => void handleStopAndPackage()}
                  disabled={busy}
                  className="flex items-center gap-2 px-5 py-2.5 bg-sky-600 hover:bg-sky-500 disabled:opacity-50 text-white rounded-lg text-xs font-semibold shadow-md transition-all"
                >
                  <Square className="w-3.5 h-3.5 fill-white" />
                  <span>停止并打包导出</span>
                </button>
              </>
            )}

            {recordedPackets.length > 0 && recordState === 'idle' && (
              <button
                onClick={() => setIsExportModalOpen(true)}
                className="flex items-center gap-2 px-4 py-2.5 bg-emerald-600 hover:bg-emerald-500 text-white rounded-lg text-xs font-medium transition-colors"
              >
                <Download className="w-4 h-4" />
                <span>导出现场包 (.fcap)</span>
              </button>
            )}
          </div>

          {/* Live Telemetry KPI counters */}
          <div className="grid grid-cols-3 gap-6 text-center border-t lg:border-t-0 lg:border-l border-slate-800 pt-4 lg:pt-0 lg:pl-6 w-full lg:w-auto">
            <div>
              <div className="text-[10px] text-slate-400 uppercase tracking-wider">已录总帧数</div>
              <div className="text-lg font-bold font-mono text-slate-100 tabular-nums">
                {recordedPackets.length.toLocaleString()}
              </div>
            </div>
            <div>
              <div className="text-[10px] text-slate-400 uppercase tracking-wider">数据量</div>
              <div className="text-lg font-bold font-mono text-slate-100 tabular-nums">
                {totalMb} MB
              </div>
            </div>
            <div>
              <div className="text-[10px] text-slate-400 uppercase tracking-wider">瞬时吞吐</div>
              <div className="text-lg font-bold font-mono text-slate-100 tabular-nums">
                {approxRateKb} KB/s
              </div>
            </div>
          </div>
        </div>

        {/* 引擎状态 / 离线模拟条 */}
        <div className="mt-4 pt-3 border-t border-slate-800/80 flex flex-wrap items-center justify-between text-xs text-slate-400 gap-2">
          <div className="flex items-center gap-2 flex-wrap">
            {engineConnected ? (
              <span className="px-2.5 py-1 rounded text-[11px] font-medium bg-emerald-950 text-emerald-400 border border-emerald-800">
                ● 引擎已连接 · 启停走真实通道（取首个已启用通道）
              </span>
            ) : (
              <>
                <span className="text-slate-400">硬件测试信号发生器：</span>
                <button
                  onClick={() => setSimulateHardwareStream(!simulateHardwareStream)}
                  className={`px-2.5 py-1 rounded text-[11px] font-medium transition-colors ${
                    simulateHardwareStream
                      ? 'bg-sky-950 text-sky-400 border border-sky-800'
                      : 'bg-slate-800 text-slate-400'
                  }`}
                >
                  {simulateHardwareStream
                    ? '● 模拟PLC/雷达设备流运行中'
                    : '○ 已停止硬件模拟'}
                </button>
                <span className="text-slate-500 text-[11px]">
                  （引擎未连接时的离线演示）
                </span>
              </>
            )}
            {localHint && (
              <span className="text-amber-400 text-[11px]">{localHint}</span>
            )}
          </div>

          <div className="flex items-center gap-3">
            {recordedPackets.length > 0 && (
              <button
                onClick={onClearSession}
                className="text-slate-400 hover:text-slate-200 text-xs underline underline-offset-2"
              >
                清空当前缓存
              </button>
            )}
            <button
              onClick={onGoToReplay}
              className="text-sky-400 hover:text-sky-300 font-medium flex items-center gap-1"
            >
              <span>前往研发重演端 →</span>
            </button>
          </div>
        </div>
      </div>

      {/* Step 3: Realtime Packet Inspection Stream */}
      <div className="bg-white rounded-xl border border-slate-200 overflow-hidden shadow-xs">
        <div className="px-5 py-3.5 border-b border-slate-100 flex flex-wrap items-center justify-between gap-3 bg-slate-50/50">
          <div className="flex items-center gap-3">
            <h2 className="text-xs font-bold text-slate-800 uppercase tracking-wide">
              实时数据流监视 (Raw Hex / ASCII Stream)
            </h2>
            <span className="text-xs text-slate-500 font-mono">
              {isFilterActive
                ? `显示 ${filteredPackets.length} / 共 ${recordedPackets.length} 帧`
                : `共 ${recordedPackets.length} 帧`}
            </span>
          </div>

          <div className="flex items-center gap-2">
            {/* Filter by channel */}
            <select
              value={filterChannel}
              onChange={(e) => setFilterChannel(e.target.value)}
              className="text-xs bg-white border border-slate-200 rounded-md px-2 py-1 text-slate-700 focus:outline-none focus:ring-1 focus:ring-sky-500"
            >
              <option value="all">全部信道</option>
              {channels.map((c) => (
                <option key={c.id} value={c.id}>
                  {c.name}
                </option>
              ))}
            </select>

            {/* Search */}
            <div className="relative">
              <Search className="w-3.5 h-3.5 text-slate-400 absolute left-2.5 top-1/2 -translate-y-1/2" />
              <input
                type="text"
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                placeholder="搜索 Hex / ASCII / Topic..."
                className="text-xs bg-white border border-slate-200 rounded-md pl-7 pr-3 py-1 text-slate-700 w-40 focus:w-56 transition-all focus:outline-none focus:ring-1 focus:ring-sky-500"
              />
            </div>

            {/* Auto scroll toggle */}
            <button
              onClick={() => {
                const next = !autoScroll;
                setAutoScroll(next);
                if (next && logContainerRef.current) {
                  logContainerRef.current.scrollTop = logContainerRef.current.scrollHeight;
                }
              }}
              className={`px-2 py-1 text-xs rounded border transition-colors ${
                autoScroll
                  ? 'bg-slate-900 text-white border-slate-900'
                  : 'bg-white text-slate-600 border-slate-200'
              }`}
            >
              {autoScroll ? '锁定最新' : '自由浏览'}
            </button>
          </div>
        </div>

        {/* Packet Stream List */}
        <div
          ref={logContainerRef}
          onScroll={handleLogScroll}
          className="h-80 overflow-y-auto divide-y divide-slate-100 font-mono text-xs"
        >
          {filteredPackets.length === 0 ? (
            <div className="flex flex-col items-center justify-center h-full text-slate-400 space-y-2 py-12">
              <Activity className="w-8 h-8 text-slate-300 stroke-[1.5]" />
              {recordedPackets.length === 0 ? (
                <p className="text-xs">暂无流入数据，请点击上方“开始现场录制”</p>
              ) : (
                <p className="text-xs">当前筛选条件下无匹配报文，可改信道筛选或清空搜索</p>
              )}
            </div>
          ) : (
            filteredPackets.map((pkt) => (
              <div
                key={pkt.id}
                className="px-5 py-2 hover:bg-slate-50 flex items-center gap-4 transition-colors text-slate-700"
              >
                {/* ID & Timestamp */}
                <span className="text-slate-400 w-12 shrink-0 tabular-nums">
                  #{pkt.id.toString().padStart(4, '0')}
                </span>
                <span className="text-slate-500 w-24 shrink-0 tabular-nums">
                  +{(pkt.timestampUs / 1000).toFixed(2)}ms
                </span>

                {/* Channel Badge */}
                <div className="w-36 shrink-0 flex items-center gap-1.5">
                  <span
                    className="w-2 h-2 rounded-full shrink-0"
                    style={{
                      backgroundColor:
                        channels.find((c) => c.id === pkt.channelId)?.color || '#64748b',
                    }}
                  />
                  <span className="truncate text-slate-800 font-medium">
                    {pkt.channelName}
                  </span>
                </div>

                {/* Direction & Length */}
                <span className="w-16 shrink-0 text-[11px] text-emerald-600 font-semibold">
                  RX [{pkt.length}B]
                </span>

                {/* Protocol Hint */}
                {pkt.protocolHint && (
                  <span className="w-24 shrink-0 text-[10px] text-slate-500 truncate">
                    {pkt.protocolHint}
                  </span>
                )}

                {/* Hex dump */}
                <span className="font-mono text-slate-900 truncate flex-1">
                  {pkt.hexPreview}
                </span>

                {/* ASCII */}
                <span className="font-mono text-slate-400 w-32 shrink-0 truncate border-l border-slate-200 pl-2">
                  {pkt.asciiPreview}
                </span>
              </div>
            ))
          )}
        </div>
      </div>

      {/* Export / Packaging Modal */}
      {isExportModalOpen && (
        <div className="fixed inset-0 z-50 bg-slate-900/60 backdrop-blur-xs flex items-center justify-center p-4">
          <div className="bg-white rounded-xl max-w-lg w-full p-6 shadow-2xl border border-slate-200 space-y-4">
            <div className="flex items-center gap-3 pb-3 border-b border-slate-100">
              <div className="w-10 h-10 rounded-lg bg-sky-50 text-sky-600 flex items-center justify-center">
                <FileCheck className="w-5 h-5" />
              </div>
              <div>
                <h3 className="text-base font-bold text-slate-900">
                  导出现场捕获数据包 (.fcap)
                </h3>
                <p className="text-xs text-slate-500">
                  包含高精度微秒时间戳、所有通道二进制原始报文，直接发给研发即可还原现场
                </p>
              </div>
            </div>

            <div className="space-y-3 text-xs">
              <div>
                <label className="block font-medium text-slate-700 mb-1">现场调试人员姓名</label>
                <input
                  type="text"
                  value={recordedBy}
                  onChange={(e) => setRecordedBy(e.target.value)}
                  className="w-full bg-slate-50 border border-slate-200 rounded-lg px-3 py-2 text-slate-900 focus:bg-white focus:ring-1 focus:ring-sky-500 focus:outline-none"
                />
              </div>

              <div>
                <label className="block font-medium text-slate-700 mb-1">工位 / 现场设备描述</label>
                <input
                  type="text"
                  value={siteLocation}
                  onChange={(e) => setSiteLocation(e.target.value)}
                  className="w-full bg-slate-50 border border-slate-200 rounded-lg px-3 py-2 text-slate-900 focus:bg-white focus:ring-1 focus:ring-sky-500 focus:outline-none"
                />
              </div>

              <div>
                <label className="block font-medium text-slate-700 mb-1">
                  异常现象或操作附言 (让研发快速定位)
                </label>
                <textarea
                  rows={3}
                  value={siteNotes}
                  onChange={(e) => setSiteNotes(e.target.value)}
                  className="w-full bg-slate-50 border border-slate-200 rounded-lg px-3 py-2 text-slate-900 focus:bg-white focus:ring-1 focus:ring-sky-500 focus:outline-none"
                />
              </div>

              <div className="p-3 bg-slate-50 rounded-lg border border-slate-100 text-slate-600 space-y-1">
                <div className="flex justify-between">
                  <span>总帧数:</span>
                  <span className="font-mono font-semibold text-slate-900">{recordedPackets.length} 帧</span>
                </div>
                <div className="flex justify-between">
                  <span>录制时长:</span>
                  <span className="font-mono font-semibold text-slate-900">{formatTimer(elapsedMs)}</span>
                </div>
                <div className="flex justify-between">
                  <span>文件包体积:</span>
                  <span className="font-mono font-semibold text-slate-900">{totalMb} MB</span>
                </div>
                <div className="flex justify-between">
                  <span>已启用通道:</span>
                  <span className="font-mono font-semibold text-slate-900">
                    {channels.filter((c) => c.enabled).length} / {channels.length}
                  </span>
                </div>
              </div>

              {recordedPackets.length === 0 && (
                <p className="text-amber-700 bg-amber-50 border border-amber-100 rounded-lg px-3 py-2">
                  当前缓存为空，请先完成至少一次现场录制再导出，避免发给研发空包。
                </p>
              )}
            </div>

            <div className="flex items-center justify-end gap-3 pt-3 border-t border-slate-100">
              <button
                type="button"
                onClick={() => setIsExportModalOpen(false)}
                className="px-4 py-2 text-xs font-medium text-slate-600 hover:text-slate-800"
              >
                取消
              </button>
              <button
                type="button"
                onClick={handleExportConfirm}
                disabled={recordedPackets.length === 0}
                className="px-5 py-2 text-xs font-semibold text-white bg-sky-600 hover:bg-sky-500 rounded-lg shadow-sm flex items-center gap-1.5 disabled:opacity-40 disabled:cursor-not-allowed disabled:hover:bg-sky-600"
              >
                <Download className="w-3.5 h-3.5" />
                <span>立即下载 .fcap 数据包</span>
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Add Custom Channel Modal */}
      {showAddChannelModal && (
        <div className="fixed inset-0 z-50 bg-slate-900/60 backdrop-blur-xs flex items-center justify-center p-4">
          <form
            onSubmit={handleCreateChannel}
            className="bg-white rounded-xl max-w-md w-full p-6 shadow-2xl border border-slate-200 space-y-4 text-xs"
          >
            <h3 className="text-base font-bold text-slate-900">添加自定义输入通道</h3>
            <div>
              <label className="block font-medium text-slate-700 mb-1">通道名称</label>
              <input
                type="text"
                value={newChanName}
                onChange={(e) => setNewChanName(e.target.value)}
                className="w-full bg-slate-50 border border-slate-200 rounded-lg px-3 py-2 text-slate-900 focus:bg-white"
                required
              />
            </div>
            <div>
              <label className="block font-medium text-slate-700 mb-1">接口类型</label>
              <div className="grid grid-cols-4 gap-2">
                {([
                  { id: 'serial' as const, label: '串口 / RS485' },
                  { id: 'udp' as const, label: 'UDP 网络' },
                  { id: 'tcp' as const, label: 'TCP 套接字' },
                  { id: 'mqtt' as const, label: 'MQTT 订阅' },
                ]).map((t) => (
                  <button
                    key={t.id}
                    type="button"
                    onClick={() => {
                      setNewChanType(t.id);
                      if (t.id === 'serial') setNewChanParam('COM4, 921600, 8-N-1');
                      else if (t.id === 'mqtt') setNewChanParam('127.0.0.1:1883, factory/+/telemetry');
                      else setNewChanParam('0.0.0.0:8888');
                    }}
                    className={`py-2 px-1.5 rounded-lg font-medium border text-center leading-tight ${
                      newChanType === t.id
                        ? 'bg-sky-50 text-sky-700 border-sky-300'
                        : 'bg-white text-slate-600 border-slate-200'
                    }`}
                  >
                    {t.label}
                  </button>
                ))}
              </div>
            </div>
            <div>
              <label className="block font-medium text-slate-700 mb-1">
                {newChanType === 'serial'
                  ? '串口参数'
                  : newChanType === 'mqtt'
                  ? 'Broker 与 Topic'
                  : '监听地址'}
              </label>
              <input
                type="text"
                value={newChanParam}
                onChange={(e) => setNewChanParam(e.target.value)}
                placeholder={
                  newChanType === 'serial'
                    ? 'COM4, 921600, 8-N-1'
                    : newChanType === 'mqtt'
                    ? '127.0.0.1:1883, factory/+/telemetry'
                    : '0.0.0.0:8888'
                }
                className="w-full bg-slate-50 border border-slate-200 rounded-lg px-3 py-2 text-slate-900 focus:bg-white"
                required
              />
            </div>
            <div className="flex items-center justify-end gap-2 pt-2 border-t border-slate-100">
              <button
                type="button"
                onClick={() => setShowAddChannelModal(false)}
                className="px-4 py-2 text-slate-600 hover:text-slate-800"
              >
                取消
              </button>
              <button
                type="submit"
                className="px-4 py-2 bg-slate-900 text-white rounded-lg font-medium hover:bg-slate-800"
              >
                确认添加
              </button>
            </div>
          </form>
        </div>
      )}
    </div>
  );
};
