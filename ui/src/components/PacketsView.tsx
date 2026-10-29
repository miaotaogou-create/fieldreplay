/**
 * @license
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useEffect, useMemo, useState } from 'react';
import { CapturedPacket, ChannelType } from '../types/recorder';
import {
  Search,
  Download,
  AlertTriangle,
  Copy,
  Check,
  ChevronRight,
  Inbox,
} from 'lucide-react';

interface PacketsViewProps {
  packets: CapturedPacket[];
  onSelectPacket: (packet: CapturedPacket) => void;
}

const TYPE_LABEL: Record<ChannelType, string> = {
  serial: '串口',
  udp: 'UDP',
  tcp: 'TCP',
  mqtt: 'MQTT',
  video_stream: '视频',
};

const TYPE_BADGE: Record<ChannelType, string> = {
  serial: 'bg-amber-50 text-amber-700 border-amber-100',
  udp: 'bg-sky-50 text-sky-700 border-sky-100',
  tcp: 'bg-indigo-50 text-indigo-700 border-indigo-100',
  mqtt: 'bg-teal-50 text-teal-700 border-teal-100',
  video_stream: 'bg-violet-50 text-violet-700 border-violet-100',
};

export const PacketsView: React.FC<PacketsViewProps> = ({ packets, onSelectPacket }) => {
  const [selectedId, setSelectedId] = useState<number | null>(packets[0]?.id ?? null);
  const [filterType, setFilterType] = useState<string>('all');
  const [onlyErrors, setOnlyErrors] = useState<boolean>(false);
  const [search, setSearch] = useState<string>('');
  const [copied, setCopied] = useState<boolean>(false);

  const isFilterActive =
    filterType !== 'all' || onlyErrors || search.trim().length > 0;

  const filtered = useMemo(() => {
    return packets.filter((p) => {
      if (onlyErrors && !p.hasError) return false;
      if (filterType !== 'all' && p.channelType !== filterType) return false;
      if (search.trim()) {
        const q = search.toLowerCase();
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
  }, [packets, onlyErrors, filterType, search]);

  // 筛选变化后，若当前选中不在结果内，落到首条或清空
  useEffect(() => {
    if (filtered.length === 0) {
      setSelectedId(null);
      return;
    }
    if (selectedId === null || !filtered.some((p) => p.id === selectedId)) {
      setSelectedId(filtered[0].id);
    }
  }, [filtered, selectedId]);

  const selectedPacket =
    filtered.find((p) => p.id === selectedId) ??
    (selectedId !== null ? packets.find((p) => p.id === selectedId) : undefined);

  const selectPacket = (pkt: CapturedPacket) => {
    setSelectedId(pkt.id);
    onSelectPacket(pkt);
  };

  const clearFilters = () => {
    setFilterType('all');
    setOnlyErrors(false);
    setSearch('');
  };

  const handleCopyHex = (hex: string) => {
    if (!hex) return;
    navigator.clipboard.writeText(hex);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const exportCsv = () => {
    if (filtered.length === 0) return;
    const headers = [
      'ID',
      'Timestamp_Us',
      'AbsoluteTime',
      'Channel',
      'Type',
      'Bytes',
      'HexData',
      'Protocol',
      'MqttTopic',
      'MqttQoS',
      'HasError',
    ];
    const rows = filtered.map((p) => [
      p.id,
      p.timestampUs,
      `"${p.absoluteTime}"`,
      `"${p.channelName}"`,
      p.channelType,
      p.length,
      `"${p.hexPreview}"`,
      p.protocolHint || 'RAW',
      `"${p.mqttTopic || ''}"`,
      typeof p.mqttQoS === 'number' ? p.mqttQoS : '',
      p.hasError ? 'YES' : 'NO',
    ]);
    const csvContent =
      'data:text/csv;charset=utf-8,\uFEFF' +
      [headers.join(','), ...rows.map((e) => e.join(','))].join('\n');
    const encodedUri = encodeURI(csvContent);
    const link = document.createElement('a');
    link.setAttribute('href', encodedUri);
    link.setAttribute('download', `FieldPackets_Export_${Date.now()}.csv`);
    document.body.appendChild(link);
    link.click();
    document.body.removeChild(link);
  };

  const filterTabs: { id: string; label: string; activeClass: string }[] = [
    { id: 'all', label: '全部通道', activeClass: 'bg-white text-slate-900 shadow-xs' },
    { id: 'serial', label: '串口 / RS485', activeClass: 'bg-white text-slate-900 shadow-xs' },
    { id: 'udp', label: 'UDP 网络', activeClass: 'bg-white text-slate-900 shadow-xs' },
    { id: 'tcp', label: 'TCP 套接字', activeClass: 'bg-white text-slate-900 shadow-xs' },
    { id: 'mqtt', label: 'MQTT 消息', activeClass: 'bg-white text-teal-700 shadow-xs' },
  ];

  return (
    <div className="max-w-7xl mx-auto px-4 sm:px-6 py-6 space-y-5">
      {/* Filters & Actions Bar */}
      <div className="bg-white rounded-xl border border-slate-200 p-4 shadow-xs flex flex-wrap items-center justify-between gap-4">
        <div className="flex flex-wrap items-center gap-3">
          <div className="flex items-center gap-1 p-1 bg-slate-100 rounded-lg">
            {filterTabs.map((tab) => (
              <button
                key={tab.id}
                type="button"
                onClick={() => setFilterType(tab.id)}
                className={`px-3 py-1.5 text-xs font-medium rounded-md transition-colors ${
                  filterType === tab.id
                    ? tab.activeClass
                    : 'text-slate-600 hover:text-slate-900'
                }`}
              >
                {tab.label}
              </button>
            ))}
          </div>

          <label className="flex items-center gap-1.5 cursor-pointer text-xs font-medium text-slate-700 bg-slate-50 px-3 py-1.5 rounded-lg border border-slate-200">
            <input
              type="checkbox"
              checked={onlyErrors}
              onChange={(e) => setOnlyErrors(e.target.checked)}
              className="rounded text-rose-600 focus:ring-rose-500"
            />
            <span>仅显示故障/异常帧</span>
          </label>

          {isFilterActive && (
            <button
              type="button"
              onClick={clearFilters}
              className="text-[11px] text-slate-500 hover:text-slate-800 underline underline-offset-2"
            >
              清除筛选
            </button>
          )}
        </div>

        <div className="flex items-center gap-3">
          <div className="relative">
            <Search className="w-3.5 h-3.5 text-slate-400 absolute left-2.5 top-1/2 -translate-y-1/2" />
            <input
              type="text"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="搜索 HEX / ASCII / Topic..."
              className="text-xs bg-slate-50 border border-slate-200 rounded-lg pl-8 pr-3 py-1.5 text-slate-800 focus:bg-white focus:outline-none focus:ring-1 focus:ring-sky-500 w-52"
            />
          </div>

          <button
            type="button"
            onClick={exportCsv}
            disabled={filtered.length === 0}
            className={`flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium rounded-lg transition-colors border ${
              filtered.length === 0
                ? 'text-slate-400 bg-slate-50 border-slate-100 cursor-not-allowed'
                : 'text-slate-700 bg-white border-slate-200 hover:bg-slate-50'
            }`}
          >
            <Download className="w-3.5 h-3.5 text-slate-500" />
            <span>导出 CSV</span>
          </button>
        </div>
      </div>

      {/* Main Two-Pane: Left list + Right detail drawer */}
      <div className="grid grid-cols-1 lg:grid-cols-12 gap-6">
        <div className="lg:col-span-7 bg-white rounded-xl border border-slate-200 overflow-hidden shadow-xs">
          <div className="px-4 py-3 bg-slate-50 border-b border-slate-200 flex items-center justify-between text-xs font-semibold text-slate-700">
            <span>
              {isFilterActive
                ? `数据帧列表（显示 ${filtered.length} / 共 ${packets.length} 帧）`
                : `数据帧列表（${filtered.length} 帧）`}
            </span>
            <span className="text-[11px] text-slate-400 font-normal">
              点击任意行打开右侧详情
            </span>
          </div>

          <div className="max-h-[580px] overflow-y-auto divide-y divide-slate-100 font-mono text-xs">
            {filtered.length === 0 ? (
              <div className="flex flex-col items-center justify-center py-16 text-slate-400 space-y-2 font-sans">
                <Inbox className="w-8 h-8 text-slate-300 stroke-[1.5]" />
                {packets.length === 0 ? (
                  <p className="text-xs">暂无报文，请先在录制页采集或从重演页载入 `.fcap`</p>
                ) : (
                  <>
                    <p className="text-xs">当前筛选条件下无匹配报文</p>
                    <button
                      type="button"
                      onClick={clearFilters}
                      className="text-[11px] text-sky-600 hover:text-sky-800 underline underline-offset-2"
                    >
                      清除筛选条件
                    </button>
                  </>
                )}
              </div>
            ) : (
              filtered.map((pkt) => {
                const isSelected = pkt.id === selectedPacket?.id;
                const preview =
                  pkt.channelType === 'mqtt' && pkt.mqttTopic
                    ? pkt.mqttTopic
                    : pkt.hexPreview;
                return (
                  <div
                    key={pkt.id}
                    onClick={() => selectPacket(pkt)}
                    className={`px-4 py-2.5 cursor-pointer flex items-center gap-3 transition-colors ${
                      isSelected
                        ? 'bg-sky-50/80 border-l-2 border-sky-600'
                        : 'hover:bg-slate-50 border-l-2 border-transparent'
                    }`}
                  >
                    <span className="text-slate-400 w-10 shrink-0 tabular-nums">
                      #{pkt.id}
                    </span>
                    <span className="text-slate-500 w-20 shrink-0 tabular-nums text-[11px]">
                      +{(pkt.timestampUs / 1000).toFixed(1)}ms
                    </span>
                    <span
                      className={`text-[10px] font-sans font-semibold px-1.5 py-0.5 rounded border shrink-0 ${TYPE_BADGE[pkt.channelType]}`}
                    >
                      {TYPE_LABEL[pkt.channelType]}
                    </span>
                    <span className="text-slate-800 font-medium truncate w-28 shrink-0">
                      {pkt.channelName.split(' ')[0]}
                    </span>
                    <span
                      className={`truncate flex-1 text-[11px] ${
                        pkt.channelType === 'mqtt' && pkt.mqttTopic
                          ? 'font-sans text-teal-800'
                          : 'font-mono text-slate-700'
                      }`}
                      title={preview}
                    >
                      {preview}
                    </span>

                    {pkt.hasError ? (
                      <span className="w-4 h-4 rounded-full bg-rose-100 text-rose-600 flex items-center justify-center shrink-0">
                        !
                      </span>
                    ) : (
                      <ChevronRight className="w-3.5 h-3.5 text-slate-300 shrink-0" />
                    )}
                  </div>
                );
              })
            )}
          </div>
        </div>

        {/* 右侧详情抽屉 */}
        <div className="lg:col-span-5 bg-white rounded-xl border border-slate-200 p-5 shadow-xs space-y-4 lg:sticky lg:top-4 lg:self-start">
          <div className="flex items-center justify-between pb-3 border-b border-slate-100">
            <h3 className="text-xs font-bold text-slate-900 uppercase tracking-wide">
              {selectedPacket
                ? `选定帧详细剖析 (Frame #${selectedPacket.id})`
                : '选定帧详细剖析'}
            </h3>
            <button
              type="button"
              onClick={() => handleCopyHex(selectedPacket?.hexPreview || '')}
              disabled={!selectedPacket}
              className={`flex items-center gap-1 text-[11px] transition-colors ${
                selectedPacket
                  ? 'text-slate-500 hover:text-slate-800'
                  : 'text-slate-300 cursor-not-allowed'
              }`}
            >
              {copied ? (
                <Check className="w-3.5 h-3.5 text-emerald-600" />
              ) : (
                <Copy className="w-3.5 h-3.5" />
              )}
              <span>{copied ? '已复制' : '复制 HEX'}</span>
            </button>
          </div>

          {selectedPacket ? (
            <div className="space-y-4 text-xs font-mono">
              <div className="grid grid-cols-2 gap-2 text-slate-600 font-sans">
                <div className="p-2.5 bg-slate-50 rounded-lg border border-slate-100">
                  <div className="text-[10px] text-slate-400">微秒时间戳 (Δt)</div>
                  <div className="font-mono font-bold text-slate-900">
                    +{selectedPacket.timestampUs} µs
                  </div>
                </div>
                <div className="p-2.5 bg-slate-50 rounded-lg border border-slate-100">
                  <div className="text-[10px] text-slate-400">墙钟时间</div>
                  <div className="font-semibold text-slate-900 truncate">
                    {selectedPacket.absoluteTime}
                  </div>
                </div>
                <div className="p-2.5 bg-slate-50 rounded-lg border border-slate-100">
                  <div className="text-[10px] text-slate-400">物理信道</div>
                  <div className="font-semibold text-slate-900 truncate">
                    {selectedPacket.channelName}
                  </div>
                </div>
                <div className="p-2.5 bg-slate-50 rounded-lg border border-slate-100">
                  <div className="text-[10px] text-slate-400">报文大小 / 方向</div>
                  <div className="font-mono font-bold text-slate-900">
                    {selectedPacket.length} B · {selectedPacket.direction}
                  </div>
                </div>
                <div className="p-2.5 bg-slate-50 rounded-lg border border-slate-100 col-span-2">
                  <div className="text-[10px] text-slate-400">通道类型</div>
                  <div className="font-semibold text-sky-700 flex items-center gap-2">
                    <span
                      className={`text-[10px] px-1.5 py-0.5 rounded border ${TYPE_BADGE[selectedPacket.channelType]}`}
                    >
                      {TYPE_LABEL[selectedPacket.channelType]}
                    </span>
                    <span className="text-slate-500 font-normal text-xs">
                      {selectedPacket.protocolHint
                        ? `样本提示：${selectedPacket.protocolHint}`
                        : '原始二进制（不解析业务协议）'}
                    </span>
                  </div>
                </div>
              </div>

              {selectedPacket.mqttTopic && (
                <div className="p-2.5 bg-teal-50/70 border border-teal-100 rounded-lg font-sans text-[11px] text-teal-900">
                  <div className="flex flex-wrap items-baseline gap-x-2 gap-y-1">
                    <span className="font-semibold">MQTT Topic:</span>
                    <code className="font-mono break-all">{selectedPacket.mqttTopic}</code>
                    {typeof selectedPacket.mqttQoS === 'number' && (
                      <span className="text-teal-700">QoS {selectedPacket.mqttQoS}</span>
                    )}
                  </div>
                </div>
              )}

              {selectedPacket.hasError && (
                <div className="p-3 bg-rose-50 border border-rose-200 rounded-lg text-rose-800 font-sans text-xs">
                  <div className="font-bold flex items-center gap-1.5 mb-0.5">
                    <AlertTriangle className="w-4 h-4 text-rose-600" />
                    <span>现场异常捕获：</span>
                  </div>
                  <div>{selectedPacket.errorReason}</div>
                </div>
              )}

              <div>
                <div className="text-[11px] font-sans font-semibold text-slate-700 mb-1">
                  十六进制原始数据 (HEX):
                </div>
                <div className="p-3 bg-slate-900 text-emerald-400 rounded-lg font-mono text-xs overflow-x-auto leading-relaxed border border-slate-800 select-all">
                  {selectedPacket.hexPreview}
                </div>
              </div>

              <div>
                <div className="text-[11px] font-sans font-semibold text-slate-700 mb-1">
                  ASCII 可见字符映射:
                </div>
                <div className="p-2.5 bg-slate-50 border border-slate-200 rounded-lg text-slate-700 select-all break-all">
                  {selectedPacket.asciiPreview}
                </div>
              </div>
            </div>
          ) : (
            <div className="text-center py-10 text-slate-400 text-xs font-sans space-y-1">
              {packets.length === 0 ? (
                <p>暂无数据帧可剖析</p>
              ) : (
                <p>请在左侧选择数据帧，或调整筛选条件</p>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
};
