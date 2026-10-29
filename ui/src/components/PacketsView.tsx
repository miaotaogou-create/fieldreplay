/**
 * @license
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useState } from 'react';
import { CapturedPacket } from '../types/recorder';
import {
  Search,
  Filter,
  Download,
  CheckCircle2,
  AlertTriangle,
  Copy,
  Check,
  ChevronRight,
  Layers,
} from 'lucide-react';

interface PacketsViewProps {
  packets: CapturedPacket[];
  onSelectPacket: (packet: CapturedPacket) => void;
}

export const PacketsView: React.FC<PacketsViewProps> = ({ packets, onSelectPacket }) => {
  const [selectedId, setSelectedId] = useState<number | null>(packets[0]?.id || null);
  const [filterType, setFilterType] = useState<string>('all');
  const [onlyErrors, setOnlyErrors] = useState<boolean>(false);
  const [search, setSearch] = useState<string>('');
  const [copied, setCopied] = useState<boolean>(false);

  const selectedPacket = packets.find((p) => p.id === selectedId) || packets[0];

  const filtered = packets.filter((p) => {
    if (onlyErrors && !p.hasError) return false;
    if (filterType !== 'all' && p.channelType !== filterType) return false;
    if (search) {
      const q = search.toLowerCase();
      return (
        p.hexPreview.toLowerCase().includes(q) ||
        p.asciiPreview.toLowerCase().includes(q) ||
        p.channelName.toLowerCase().includes(q) ||
        (p.protocolHint && p.protocolHint.toLowerCase().includes(q))
      );
    }
    return true;
  });

  const handleCopyHex = (hex: string) => {
    navigator.clipboard.writeText(hex);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const exportCsv = () => {
    const headers = ['ID', 'Timestamp_Us', 'AbsoluteTime', 'Channel', 'Type', 'Bytes', 'HexData', 'Protocol', 'HasError'];
    const rows = filtered.map((p) => [
      p.id,
      p.timestampUs,
      `"${p.absoluteTime}"`,
      `"${p.channelName}"`,
      p.channelType,
      p.length,
      `"${p.hexPreview}"`,
      p.protocolHint || 'RAW',
      p.hasError ? 'YES' : 'NO',
    ]);
    const csvContent = 'data:text/csv;charset=utf-8,\uFEFF' + [headers.join(','), ...rows.map((e) => e.join(','))].join('\n');
    const encodedUri = encodeURI(csvContent);
    const link = document.createElement('a');
    link.setAttribute('href', encodedUri);
    link.setAttribute('download', `FieldPackets_Export_${Date.now()}.csv`);
    document.body.appendChild(link);
    link.click();
    document.body.removeChild(link);
  };

  return (
    <div className="max-w-7xl mx-auto px-4 sm:px-6 py-6 space-y-5">
      {/* Filters & Actions Bar */}
      <div className="bg-white rounded-xl border border-slate-200 p-4 shadow-xs flex flex-wrap items-center justify-between gap-4">
        <div className="flex flex-wrap items-center gap-3">
          {/* Channel Type Filter */}
          <div className="flex items-center gap-1 p-1 bg-slate-100 rounded-lg">
            <button
              onClick={() => setFilterType('all')}
              className={`px-3 py-1.5 text-xs font-medium rounded-md transition-colors ${
                filterType === 'all'
                  ? 'bg-white text-slate-900 shadow-xs'
                  : 'text-slate-600 hover:text-slate-900'
              }`}
            >
              全部通道
            </button>
            <button
              onClick={() => setFilterType('serial')}
              className={`px-3 py-1.5 text-xs font-medium rounded-md transition-colors ${
                filterType === 'serial'
                  ? 'bg-white text-slate-900 shadow-xs'
                  : 'text-slate-600 hover:text-slate-900'
              }`}
            >
              串口 / RS485
            </button>
            <button
              onClick={() => setFilterType('udp')}
              className={`px-3 py-1.5 text-xs font-medium rounded-md transition-colors ${
                filterType === 'udp'
                  ? 'bg-white text-slate-900 shadow-xs'
                  : 'text-slate-600 hover:text-slate-900'
              }`}
            >
              UDP 网络
            </button>
            <button
              onClick={() => setFilterType('tcp')}
              className={`px-3 py-1.5 text-xs font-medium rounded-md transition-colors ${
                filterType === 'tcp'
                  ? 'bg-white text-slate-900 shadow-xs'
                  : 'text-slate-600 hover:text-slate-900'
              }`}
            >
              TCP 套接字
            </button>
            <button
              onClick={() => setFilterType('mqtt')}
              className={`px-3 py-1.5 text-xs font-medium rounded-md transition-colors ${
                filterType === 'mqtt'
                  ? 'bg-white text-teal-700 shadow-xs'
                  : 'text-slate-600 hover:text-slate-900'
              }`}
            >
              MQTT 消息
            </button>
          </div>

          {/* Only Errors Toggle */}
          <label className="flex items-center gap-1.5 cursor-pointer text-xs font-medium text-slate-700 bg-slate-50 px-3 py-1.5 rounded-lg border border-slate-200">
            <input
              type="checkbox"
              checked={onlyErrors}
              onChange={(e) => setOnlyErrors(e.target.checked)}
              className="rounded text-rose-600 focus:ring-rose-500"
            />
            <span>仅显示故障/异常帧</span>
          </label>
        </div>

        <div className="flex items-center gap-3">
          {/* Search Input */}
          <div className="relative">
            <Search className="w-3.5 h-3.5 text-slate-400 absolute left-2.5 top-1/2 -translate-y-1/2" />
            <input
              type="text"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="搜索 HEX / 字段..."
              className="text-xs bg-slate-50 border border-slate-200 rounded-lg pl-8 pr-3 py-1.5 text-slate-800 focus:bg-white focus:outline-none focus:ring-1 focus:ring-sky-500 w-44"
            />
          </div>

          {/* Export to CSV */}
          <button
            onClick={exportCsv}
            className="flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium text-slate-700 bg-white border border-slate-200 hover:bg-slate-50 rounded-lg transition-colors"
          >
            <Download className="w-3.5 h-3.5 text-slate-500" />
            <span>导出 CSV</span>
          </button>
        </div>
      </div>

      {/* Main Two-Pane Grid: Left Table, Right Frame Inspector */}
      <div className="grid grid-cols-1 lg:grid-cols-12 gap-6">
        {/* Left 7 cols: Packet List */}
        <div className="lg:col-span-7 bg-white rounded-xl border border-slate-200 overflow-hidden shadow-xs">
          <div className="px-4 py-3 bg-slate-50 border-b border-slate-200 flex items-center justify-between text-xs font-semibold text-slate-700">
            <span>数据帧列表 ({filtered.length} 帧)</span>
            <span className="text-[11px] text-slate-400">点击任意行查看详情</span>
          </div>

          <div className="max-h-[580px] overflow-y-auto divide-y divide-slate-100 font-mono text-xs">
            {filtered.length === 0 ? (
              <div className="text-center py-16 text-slate-400 text-xs font-sans">
                没有符合过滤条件的数据帧
              </div>
            ) : (
              filtered.map((pkt) => {
                const isSelected = pkt.id === selectedPacket?.id;
                return (
                  <div
                    key={pkt.id}
                    onClick={() => {
                      setSelectedId(pkt.id);
                      onSelectPacket(pkt);
                    }}
                    className={`px-4 py-2.5 cursor-pointer flex items-center gap-3 transition-colors ${
                      isSelected
                        ? 'bg-sky-50/80 border-l-2 border-sky-600'
                        : 'hover:bg-slate-50'
                    }`}
                  >
                    <span className="text-slate-400 w-10 shrink-0 tabular-nums">
                      #{pkt.id}
                    </span>
                    <span className="text-slate-500 w-20 shrink-0 tabular-nums text-[11px]">
                      +{(pkt.timestampUs / 1000).toFixed(1)}ms
                    </span>
                    <span className="text-slate-800 font-medium truncate w-32 shrink-0">
                      {pkt.channelName.split(' ')[0]}
                    </span>
                    <span className="font-mono text-slate-700 truncate flex-1 text-[11px]">
                      {pkt.hexPreview}
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

        {/* Right 5 cols: Selected Packet Detailed Card */}
        <div className="lg:col-span-5 bg-white rounded-xl border border-slate-200 p-5 shadow-xs space-y-4">
          <div className="flex items-center justify-between pb-3 border-b border-slate-100">
            <h3 className="text-xs font-bold text-slate-900 uppercase tracking-wide">
              选定帧详细剖析 (Frame #{selectedPacket?.id || 0})
            </h3>
            <button
              onClick={() => handleCopyHex(selectedPacket?.hexPreview || '')}
              className="flex items-center gap-1 text-[11px] text-slate-500 hover:text-slate-800 transition-colors"
            >
              {copied ? <Check className="w-3.5 h-3.5 text-emerald-600" /> : <Copy className="w-3.5 h-3.5" />}
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
                  <div className="text-[10px] text-slate-400">物理信道</div>
                  <div className="font-semibold text-slate-900 truncate">
                    {selectedPacket.channelName}
                  </div>
                </div>
                <div className="p-2.5 bg-slate-50 rounded-lg border border-slate-100">
                  <div className="text-[10px] text-slate-400">报文大小</div>
                  <div className="font-mono font-bold text-slate-900">
                    {selectedPacket.length} 字节 (Bytes)
                  </div>
                </div>
                <div className="p-2.5 bg-slate-50 rounded-lg border border-slate-100">
                  <div className="text-[10px] text-slate-400">协议类型</div>
                  <div className="font-semibold text-sky-700">
                    {selectedPacket.protocolHint || '通用二进制'}
                  </div>
                </div>
              </div>

              {selectedPacket.hasError && (
                <div className="p-3 bg-rose-50 border border-rose-200 rounded-lg text-rose-800 font-sans text-xs">
                  <div className="font-bold flex items-center gap-1.5 mb-0.5">
                    <AlertTriangle className="w-4 h-4 text-rose-600" />
                    <span>现场异常捕获：</span>
                  </div>
                  <div>{selectedPacket.errorReason}</div>
                </div>
              )}

              {/* Raw Hex */}
              <div>
                <div className="text-[11px] font-sans font-semibold text-slate-700 mb-1">
                  十六进制原始数据 (HEX):
                </div>
                <div className="p-3 bg-slate-900 text-emerald-400 rounded-lg font-mono text-xs overflow-x-auto leading-relaxed border border-slate-800 select-all">
                  {selectedPacket.hexPreview}
                </div>
              </div>

              {/* ASCII */}
              <div>
                <div className="text-[11px] font-sans font-semibold text-slate-700 mb-1">
                  ASCII 可见字符映射:
                </div>
                <div className="p-2.5 bg-slate-50 border border-slate-200 rounded-lg text-slate-700 select-all break-all">
                  {selectedPacket.asciiPreview}
                </div>
              </div>

              {/* Protocol Breakdown */}
              {selectedPacket.parsedFields && (
                <div>
                  <div className="text-[11px] font-sans font-semibold text-slate-700 mb-1.5">
                    协议字段智能解包:
                  </div>
                  <div className="space-y-1.5 font-sans">
                    {Object.entries(selectedPacket.parsedFields).map(([k, v]) => (
                      <div
                        key={k}
                        className="flex items-center justify-between p-2 bg-slate-50 rounded border border-slate-100"
                      >
                        <span className="text-slate-500 text-xs">{k}</span>
                        <span className="font-mono font-semibold text-slate-900 text-xs">
                          {String(v)}
                        </span>
                      </div>
                    ))}
                  </div>
                </div>
              )}
            </div>
          ) : (
            <div className="text-center py-10 text-slate-400 text-xs">请在左侧选择数据帧</div>
          )}
        </div>
      </div>
    </div>
  );
};
