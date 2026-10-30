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
          data_bits: cfg.serial.dataBits ?? 8,
          stop_bits: cfg.serial.stopBits ?? 1,
          parity: cfg.serial.parity || 'none',
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
    case 'mqtt': {
      const config: Record<string, unknown> = {
        broker: cfg.mqtt.brokerHost || '127.0.0.1',
        port: cfg.mqtt.brokerPort || 1883,
        topic_prefix: cfg.mqtt.topicPrefix || '',
      };
      if (cfg.mqtt.username) config.username = cfg.mqtt.username;
      if (cfg.mqtt.password) config.password = cfg.mqtt.password;
      return {
        type: 'mqtt',
        config,
        topicPrefix: cfg.mqtt.topicPrefix || '',
      };
    }
    default:
      throw new Error(`不支持的重放目标: ${tab}`);
  }
}
