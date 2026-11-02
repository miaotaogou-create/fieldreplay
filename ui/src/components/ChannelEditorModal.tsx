/**
 * 录制页：添加 / 编辑输入通道弹窗。
 */

import React, { useEffect, useState } from 'react';
import type { ChannelConfig, ChannelType } from '../types/recorder';
import {
  buildChannelFromForm,
  channelToFormValues,
  defaultFormValuesForType,
} from '../utils/channelForm';
import {
  formatSerialLineSummary,
  parseSerialParamLine,
} from '../utils/serialParams';

const FALLBACK_SERIAL_PORTS = ['COM1', 'COM2', 'COM3', 'COM4', 'COM10', 'COM11'];

const TYPE_BUTTONS: { id: ChannelType; label: string }[] = [
  { id: 'serial', label: '串口' },
  { id: 'udp', label: 'UDP' },
  { id: 'tcp', label: 'TCP' },
  { id: 'mqtt', label: 'MQTT' },
  { id: 'video_stream', label: '视频 RTP' },
];

interface Props {
  mode: 'add' | 'edit';
  /** edit 时必填 */
  initialChannel?: ChannelConfig;
  engineConnected: boolean;
  onListSerialPorts?: () => Promise<string[]>;
  onSubmit: (channel: ChannelConfig) => void;
  onClose: () => void;
}

export const ChannelEditorModal: React.FC<Props> = ({
  mode,
  initialChannel,
  engineConnected,
  onListSerialPorts,
  onSubmit,
  onClose,
}) => {
  const seed =
    mode === 'edit' && initialChannel
      ? channelToFormValues(initialChannel)
      : {
          name: 'COM4 (力矩传感器)',
          ...defaultFormValuesForType('serial'),
        };

  const [name, setName] = useState(seed.name);
  const [type, setType] = useState<ChannelType>(seed.type);
  const [param, setParam] = useState(seed.param);
  const [mqttTls, setMqttTls] = useState(seed.mqttTls);
  const [mqttCa, setMqttCa] = useState(seed.mqttCa);
  const [mqttSni, setMqttSni] = useState(seed.mqttSni);
  const [serialPorts, setSerialPorts] = useState<string[]>([...FALLBACK_SERIAL_PORTS]);
  const [serialListHint, setSerialListHint] = useState<string | null>(null);

  const typeLocked = mode === 'edit';

  const refreshSerialPorts = async () => {
    if (!engineConnected || !onListSerialPorts) {
      setSerialPorts([...FALLBACK_SERIAL_PORTS]);
      setSerialListHint('引擎未连接，使用本地兜底列表');
      return;
    }
    try {
      const ports = await onListSerialPorts();
      setSerialPorts(ports.length ? ports : [...FALLBACK_SERIAL_PORTS]);
      const parsed = parseSerialParamLine(param);
      setSerialListHint(
        ports.length === 0
          ? `本机未枚举到 COM，已用兜底列表 · 当前 ${parsed.portName} @ ${formatSerialLineSummary(parsed)}`
          : `本机 ${ports.length} 个串口 · 当前 ${parsed.portName} @ ${formatSerialLineSummary(parsed)}`
      );
    } catch (e) {
      setSerialListHint(e instanceof Error ? e.message : String(e));
    }
  };

  useEffect(() => {
    if (type !== 'serial') return;
    void refreshSerialPorts();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [type, engineConnected]);

  const handleTypePick = (id: ChannelType) => {
    if (typeLocked) return;
    setType(id);
    const d = defaultFormValuesForType(id);
    setParam(d.param);
    setMqttTls(d.mqttTls);
    setMqttCa(d.mqttCa);
    setMqttSni(d.mqttSni);
  };

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    const channel = buildChannelFromForm(
      { name, type, param, mqttTls, mqttCa, mqttSni },
      mode === 'edit' ? initialChannel : undefined
    );
    onSubmit(channel);
  };

  return (
    <div className="fixed inset-0 z-50 bg-slate-900/60 backdrop-blur-xs flex items-center justify-center p-4">
      <form
        onSubmit={handleSubmit}
        className="bg-white rounded-xl max-w-md w-full p-6 shadow-2xl border border-slate-200 space-y-4 text-xs"
      >
        <h3 className="text-base font-bold text-slate-900">
          {mode === 'edit' ? '编辑输入通道' : '添加输入通道'}
        </h3>
        <div>
          <label className="block font-medium text-slate-700 mb-1">通道名称</label>
          <input
            type="text"
            value={name}
            onChange={(e) => setName(e.target.value)}
            className="w-full bg-slate-50 border border-slate-200 rounded-lg px-3 py-2 text-slate-900 focus:bg-white"
            required
          />
        </div>
        <div>
          <label className="block font-medium text-slate-700 mb-1">
            接口类型
            {typeLocked && (
              <span className="ml-1 font-normal text-slate-400">（编辑不可改）</span>
            )}
          </label>
          <div className="grid grid-cols-5 gap-1.5">
            {TYPE_BUTTONS.map((t) => (
              <button
                key={t.id}
                type="button"
                disabled={typeLocked}
                onClick={() => handleTypePick(t.id)}
                className={`py-2 px-1 rounded-lg font-medium border text-center leading-tight disabled:cursor-default ${
                  type === t.id
                    ? 'bg-sky-50 text-sky-700 border-sky-300'
                    : 'bg-white text-slate-600 border-slate-200'
                } ${typeLocked && type !== t.id ? 'opacity-40' : ''}`}
              >
                {t.label}
              </button>
            ))}
          </div>
        </div>
        <div>
          <label className="block font-medium text-slate-700 mb-1">
            {type === 'serial'
              ? '串口参数'
              : type === 'mqtt'
              ? 'Broker、Topic、可选用户名/密码'
              : type === 'video_stream'
              ? 'RTP/UDP 监听或 RTSP URL（不解编码）'
              : '监听地址'}
          </label>
          {type === 'serial' && (
            <div className="flex items-center gap-2 mb-1.5">
              <select
                value={param.split(',')[0]?.trim() || serialPorts[0] || 'COM3'}
                onChange={(e) => {
                  const rest = param.includes(',')
                    ? param.slice(param.indexOf(','))
                    : ', 115200, 8-N-1';
                  setParam(`${e.target.value}${rest}`);
                }}
                className="flex-1 bg-slate-50 border border-slate-200 rounded-lg px-2 py-1.5 text-slate-900 font-mono focus:bg-white"
              >
                {Array.from(
                  new Set(
                    [param.split(',')[0]?.trim(), ...serialPorts].filter(
                      (p): p is string => !!p
                    )
                  )
                ).map((p) => (
                  <option key={p} value={p}>
                    {p}
                  </option>
                ))}
              </select>
              <button
                type="button"
                onClick={() => void refreshSerialPorts()}
                className="px-2 py-1.5 rounded-lg border border-slate-200 bg-white text-slate-600 hover:bg-slate-50 shrink-0"
              >
                刷新
              </button>
            </div>
          )}
          {type === 'serial' && serialListHint && (
            <p className="text-[10px] text-slate-500 mb-1">{serialListHint}</p>
          )}
          <input
            type="text"
            value={param}
            onChange={(e) => setParam(e.target.value)}
            placeholder={
              type === 'serial'
                ? 'COM4, 921600, 8-N-1'
                : type === 'mqtt'
                ? '127.0.0.1:1883, factory/+/telemetry[, user, pass]'
                : type === 'video_stream'
                ? '0.0.0.0:5004 或 rtsp://host:554/live'
                : '0.0.0.0:8888'
            }
            className="w-full bg-slate-50 border border-slate-200 rounded-lg px-3 py-2 text-slate-900 focus:bg-white"
            required
          />
          {type === 'mqtt' && (
            <div className="mt-2 space-y-1.5">
              <label className="flex items-center gap-2 text-xs text-slate-600">
                <input
                  type="checkbox"
                  checked={mqttTls}
                  onChange={(e) => {
                    const on = e.target.checked;
                    setMqttTls(on);
                    if (on && param.startsWith('127.0.0.1:1883')) {
                      setParam(param.replace('127.0.0.1:1883', '127.0.0.1:8883'));
                    } else if (!on && param.startsWith('127.0.0.1:8883')) {
                      setParam(param.replace('127.0.0.1:8883', '127.0.0.1:1883'));
                    }
                  }}
                />
                TLS（mqtts，常用口 8883）
              </label>
              {mqttTls && (
                <>
                  <input
                    type="text"
                    value={mqttCa}
                    onChange={(e) => setMqttCa(e.target.value)}
                    placeholder="CA PEM 路径（可选；空=系统根）"
                    className="w-full bg-slate-50 border border-slate-200 rounded-lg px-3 py-1.5 text-xs text-slate-900 font-mono focus:bg-white"
                  />
                  <input
                    type="text"
                    value={mqttSni}
                    onChange={(e) => setMqttSni(e.target.value)}
                    placeholder="TLS 服务器名 / SNI（可选）"
                    className="w-full bg-slate-50 border border-slate-200 rounded-lg px-3 py-1.5 text-xs text-slate-900 font-mono focus:bg-white"
                  />
                </>
              )}
            </div>
          )}
        </div>
        <div className="flex items-center justify-end gap-2 pt-2 border-t border-slate-100">
          <button
            type="button"
            onClick={onClose}
            className="px-4 py-2 text-slate-600 hover:text-slate-800"
          >
            取消
          </button>
          <button
            type="submit"
            className="px-4 py-2 bg-slate-900 text-white rounded-lg font-medium hover:bg-slate-800"
          >
            {mode === 'edit' ? '保存' : '确认添加'}
          </button>
        </div>
      </form>
    </div>
  );
};
