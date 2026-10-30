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
      return {
        broker: host || '127.0.0.1',
        port: Number(portStr) || 1883,
        topic: ch.params.mqttTopic || '#',
        qos: ch.params.mqttQoS ?? 0,
      };
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

export function pickRecordChannel(channels: ChannelConfig[]): ChannelConfig | null {
  return (
    channels.find(
      (c) =>
        c.enabled
        && (c.type === 'udp'
          || c.type === 'serial'
          || c.type === 'tcp'
          || c.type === 'mqtt'
          || c.type === 'video_stream')
    ) ?? null
  );
}
