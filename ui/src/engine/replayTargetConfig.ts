/**
 * @license
 * SPDX-License-Identifier: Apache-2.0
 */

import { ReplayConfig } from '../types/recorder';

export type ReplayTargetType = 'udp' | 'serial' | 'tcp' | 'mqtt';

/** 将重演页目标映射转为引擎 `replay.start` 的 type + config。 */
export function replayConfigToEngine(
  tab: ReplayTargetType,
  cfg: ReplayConfig
): { type: ReplayTargetType; config: Record<string, unknown>; topicPrefix?: string } {
  switch (tab) {
    case 'udp':
      return {
        type: 'udp',
        config: {
          target_ip: cfg.targetIp || '127.0.0.1',
          target_port: cfg.targetUdpPort || 8080,
        },
      };
    case 'serial':
      return {
        type: 'serial',
        config: {
          port_name: cfg.serial.selectedPort || cfg.virtualComPort || 'COM2',
          baud: cfg.serial.baudRate || 115200,
          data_bits: 8,
          stop_bits: 1,
          parity: 'none',
        },
      };
    case 'tcp':
      // 角色由会话内录制配置取反；此处只传重放侧地址
      if (cfg.tcp.mode === 'tool_is_server') {
        return {
          type: 'tcp',
          config: {
            host: '0.0.0.0',
            port: cfg.tcp.serverPort || 9000,
          },
        };
      }
      return {
        type: 'tcp',
        config: {
          host: cfg.tcp.clientTargetHost || '127.0.0.1',
          port: cfg.tcp.clientTargetPort || 9000,
        },
      };
    case 'mqtt':
      return {
        type: 'mqtt',
        config: {
          broker: cfg.mqtt.brokerHost || '127.0.0.1',
          port: cfg.mqtt.brokerPort || 1883,
          topic_prefix: cfg.mqtt.topicPrefix || '',
        },
        topicPrefix: cfg.mqtt.topicPrefix || '',
      };
    default:
      throw new Error(`不支持的重放目标: ${tab}`);
  }
}
