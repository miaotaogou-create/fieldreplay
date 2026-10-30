/**
 * @license
 * SPDX-License-Identifier: Apache-2.0
 */

import React from 'react';
import {
  ChannelReplayTargetRow,
  isMultiReplayCapable,
} from '../engine/multiReplayTargets';

interface ReplayChannelTargetsProps {
  rows: ChannelReplayTargetRow[];
  disabled?: boolean;
  onChange: (rows: ChannelReplayTargetRow[]) => void;
}

const inputCls =
  'bg-slate-50 border border-slate-300 rounded px-1.5 py-0.5 text-xs text-slate-800 disabled:opacity-50';

/** E12d：会话内各通道重演目标（多路对齐；UDP/视频/TCP/串口/MQTT）。 */
export const ReplayChannelTargets: React.FC<ReplayChannelTargetsProps> = ({
  rows,
  disabled = false,
  onChange,
}) => {
  const patch = (channelId: string, partial: Partial<ChannelReplayTargetRow>) => {
    onChange(
      rows.map((r) => (r.channelId === channelId ? { ...r, ...partial } : r))
    );
  };

  const enabledCapable = rows.filter(
    (r) => r.enabled && isMultiReplayCapable(r.type)
  ).length;
  const willSkip = rows.length - enabledCapable;

  return (
    <div className="space-y-2 text-xs">
      <p className="text-[11px] text-slate-500 leading-snug">
        一键重演按<strong className="font-semibold text-slate-700">整会话时间轴</strong>
        对齐调度；未勾选的通道将跳过（
        {enabledCapable} 路启用
        {willSkip > 0 ? ` · ${willSkip} 路跳过` : ''}
        ）。多路 sink：UDP / 视频 UDP / TCP / 串口 / MQTT（RTSP listen 请用单通道会话）。
      </p>

      <div className="divide-y divide-slate-100 border border-slate-200 rounded-md overflow-hidden">
        {rows.map((row) => {
          const capable = isMultiReplayCapable(row.type);
          const fieldsOff = disabled || !row.enabled;
          return (
            <div
              key={row.channelId}
              className={`px-2 py-1.5 flex flex-col sm:flex-row sm:items-start gap-1.5 ${
                capable ? 'bg-white' : 'bg-slate-50/80'
              }`}
            >
              <label className="flex items-center gap-1.5 min-w-0 sm:w-44 shrink-0 sm:pt-0.5">
                <input
                  type="checkbox"
                  checked={row.enabled && capable}
                  disabled={disabled || !capable}
                  onChange={(e) =>
                    patch(row.channelId, { enabled: e.target.checked })
                  }
                  className="rounded border-slate-300 text-sky-600"
                />
                <span className="truncate font-medium text-slate-800" title={row.name}>
                  [{row.channelIndex}] {row.name}
                </span>
              </label>

              <span className="text-[10px] font-mono text-slate-500 shrink-0 w-24 sm:pt-0.5">
                {row.type}
                {row.packetCount > 0 ? ` · ${row.packetCount}帧` : ''}
              </span>

              {capable ? (
                <TargetFields
                  row={row}
                  disabled={fieldsOff}
                  onPatch={(partial) => patch(row.channelId, partial)}
                />
              ) : (
                <span className="text-[10px] text-amber-700/90 sm:pt-0.5">
                  多路对齐暂不支持，将跳过（单通道会话可用上方协议页）
                </span>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
};

const TargetFields: React.FC<{
  row: ChannelReplayTargetRow;
  disabled: boolean;
  onPatch: (partial: Partial<ChannelReplayTargetRow>) => void;
}> = ({ row, disabled, onPatch }) => {
  switch (row.type) {
    case 'serial':
      return (
        <div className="flex flex-wrap items-center gap-1 font-mono flex-1 min-w-0">
          <input
            type="text"
            value={row.serialPort}
            disabled={disabled}
            placeholder="COM2"
            onChange={(e) => onPatch({ serialPort: e.target.value })}
            className={`${inputCls} w-20`}
            title="重放写入口"
          />
          <input
            type="number"
            value={row.baud}
            disabled={disabled}
            onChange={(e) =>
              onPatch({ baud: parseInt(e.target.value, 10) || 115200 })
            }
            className={`${inputCls} w-20`}
            title="波特率"
          />
          <select
            value={row.dataBits}
            disabled={disabled}
            onChange={(e) =>
              onPatch({ dataBits: Number(e.target.value) === 7 ? 7 : 8 })
            }
            className={`${inputCls} w-12`}
            title="数据位"
          >
            <option value={8}>8</option>
            <option value={7}>7</option>
          </select>
          <select
            value={row.parity}
            disabled={disabled}
            onChange={(e) =>
              onPatch({
                parity: e.target.value as ChannelReplayTargetRow['parity'],
              })
            }
            className={`${inputCls} w-16`}
            title="校验"
          >
            <option value="none">N</option>
            <option value="even">E</option>
            <option value="odd">O</option>
          </select>
          <select
            value={row.stopBits}
            disabled={disabled}
            onChange={(e) =>
              onPatch({ stopBits: Number(e.target.value) === 2 ? 2 : 1 })
            }
            className={`${inputCls} w-12`}
            title="停止位"
          >
            <option value={1}>1</option>
            <option value={2}>2</option>
          </select>
        </div>
      );
    case 'tcp':
      return (
        <div className="flex flex-col gap-0.5 flex-1 min-w-0">
          <div className="flex items-center gap-1 font-mono">
            <input
              type="text"
              value={row.tcpHost}
              disabled={disabled}
              onChange={(e) => onPatch({ tcpHost: e.target.value })}
              className={`${inputCls} w-28`}
              title="重放侧 host"
            />
            <span className="text-slate-400">:</span>
            <input
              type="number"
              value={row.tcpPort}
              disabled={disabled}
              onChange={(e) =>
                onPatch({ tcpPort: parseInt(e.target.value, 10) || 0 })
              }
              className={`${inputCls} w-20`}
              title="重放侧 port"
            />
          </div>
          <span className="text-[10px] text-slate-400">
            角色由会话取反；此处只填重放侧地址
          </span>
        </div>
      );
    case 'mqtt':
      return (
        <div className="flex flex-col gap-1 flex-1 min-w-0">
          <div className="flex flex-wrap items-center gap-1 font-mono">
            <input
              type="text"
              value={row.mqttBroker}
              disabled={disabled}
              onChange={(e) => onPatch({ mqttBroker: e.target.value })}
              className={`${inputCls} w-28`}
              title="Broker"
            />
            <span className="text-slate-400">:</span>
            <input
              type="number"
              value={row.mqttPort}
              disabled={disabled}
              onChange={(e) =>
                onPatch({ mqttPort: parseInt(e.target.value, 10) || 1883 })
              }
              className={`${inputCls} w-16`}
              title="端口"
            />
            <input
              type="text"
              value={row.mqttTopicPrefix}
              disabled={disabled}
              placeholder="topic 前缀"
              onChange={(e) => onPatch({ mqttTopicPrefix: e.target.value })}
              className={`${inputCls} w-28`}
              title="topic_prefix（发布 topic 仍取帧 aux）"
            />
          </div>
          <div className="flex flex-wrap items-center gap-1 font-mono">
            <input
              type="text"
              value={row.mqttUsername}
              disabled={disabled}
              placeholder="用户名"
              onChange={(e) => onPatch({ mqttUsername: e.target.value })}
              className={`${inputCls} w-24`}
            />
            <input
              type="password"
              value={row.mqttPassword}
              disabled={disabled}
              placeholder="密码"
              onChange={(e) => onPatch({ mqttPassword: e.target.value })}
              className={`${inputCls} w-24`}
            />
            <label className="flex items-center gap-1 text-[10px] text-slate-600">
              <input
                type="checkbox"
                checked={row.mqttTls}
                disabled={disabled}
                onChange={(e) => onPatch({ mqttTls: e.target.checked })}
                className="rounded border-slate-300 text-sky-600"
              />
              TLS
            </label>
            {row.mqttTls && (
              <>
                <input
                  type="text"
                  value={row.mqttCaCertPath}
                  disabled={disabled}
                  placeholder="CA PEM 路径"
                  onChange={(e) => onPatch({ mqttCaCertPath: e.target.value })}
                  className={`${inputCls} w-32`}
                />
                <input
                  type="text"
                  value={row.mqttTlsServerName}
                  disabled={disabled}
                  placeholder="SNI"
                  onChange={(e) =>
                    onPatch({ mqttTlsServerName: e.target.value })
                  }
                  className={`${inputCls} w-24`}
                />
              </>
            )}
          </div>
        </div>
      );
    case 'udp':
    case 'video_stream':
    default:
      return (
        <div className="flex items-center gap-1 font-mono flex-1 min-w-0">
          <input
            type="text"
            value={row.targetIp}
            disabled={disabled}
            onChange={(e) => onPatch({ targetIp: e.target.value })}
            className={`${inputCls} w-28`}
          />
          <span className="text-slate-400">:</span>
          <input
            type="number"
            value={row.targetPort}
            disabled={disabled}
            onChange={(e) =>
              onPatch({ targetPort: parseInt(e.target.value, 10) || 0 })
            }
            className={`${inputCls} w-20`}
          />
        </div>
      );
  }
};
