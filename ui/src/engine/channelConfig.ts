/**
 * @license
 * SPDX-License-Identifier: Apache-2.0
 */

import { ChannelConfig } from '../types/recorder';

/** 将 UI 通道参数转为引擎 `record.start` 的 config 对象。 */
export function channelToEngineConfig(ch: ChannelConfig): Record<string, unknown> {
  switch (ch.type) {
    case 'udp':
      return {
        bind_ip: ch.params.ip || '0.0.0.0',
        bind_port: ch.params.port ?? 8080,
        peer_ip: '',
        peer_port: 0,
      };
    case 'serial':
      return {
        port_name: ch.params.portName || 'COM3',
        baud: ch.params.baudRate ?? 115200,
        data_bits: ch.params.dataBits ?? 8,
        stop_bits: ch.params.stopBits ?? 1,
        parity: ch.params.parity || 'none',
      };
    case 'tcp':
      return {
        role: ch.params.role || 'server',
        host: ch.params.ip || '0.0.0.0',
        port: ch.params.port ?? 9000,
      };
    case 'mqtt': {
      const brokerRaw = ch.params.mqttBroker || '127.0.0.1:1883';
      const [host, portStr] = brokerRaw.includes(':')
        ? brokerRaw.split(':')
        : [brokerRaw, '1883'];
      const cfg: Record<string, unknown> = {
        broker: host || '127.0.0.1',
        port: Number(portStr) || 1883,
        topic: ch.params.mqttTopic || '#',
        qos: ch.params.mqttQoS ?? 0,
      };
      if (ch.params.mqttUsername) cfg.username = ch.params.mqttUsername;
      if (ch.params.mqttPassword) cfg.password = ch.params.mqttPassword;
      if (ch.params.mqttTls) {
        cfg.tls = true;
        if (ch.params.mqttCaCertPath) cfg.ca_cert_path = ch.params.mqttCaCertPath;
        if (ch.params.mqttTlsServerName) cfg.tls_server_name = ch.params.mqttTlsServerName;
      }
      return cfg;
    }
    case 'video_stream': {
      const rtsp = (ch.params.streamUrl || '').trim();
      return {
        bind_ip: ch.params.ip || '0.0.0.0',
        bind_port: ch.params.port ?? (rtsp ? 0 : 5004),
        peer_ip: '',
        peer_port: 0,
        media: rtsp ? 'rtsp_tcp' : 'rtp_udp',
        rtsp_url: rtsp,
      };
    }
    default:
      throw new Error(`通道类型 ${ch.type} 暂不支持引擎录制`);
  }
}

const ENGINE_RECORD_TYPES = new Set([
  'udp',
  'serial',
  'tcp',
  'mqtt',
  'video_stream',
]);

/** 是否可下发给引擎录制（类型受支持）。 */
export function isEngineRecordable(ch: ChannelConfig): boolean {
  return ENGINE_RECORD_TYPES.has(ch.type);
}

/** E11b：全部 enabled 且引擎支持的通道（顺序即 `channels[]` 下标）。 */
export function listRecordChannels(channels: ChannelConfig[]): ChannelConfig[] {
  return channels.filter((c) => c.enabled && isEngineRecordable(c));
}

/** @deprecated 请用 listRecordChannels；保留给旧调用方 */
export function pickRecordChannel(channels: ChannelConfig[]): ChannelConfig | null {
  return listRecordChannels(channels)[0] ?? null;
}
