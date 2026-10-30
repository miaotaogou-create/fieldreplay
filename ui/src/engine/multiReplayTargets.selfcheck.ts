/**
 * 最小自检：E12d 多路 targets 组装（tsx 直接跑）。
 * 用法：npx tsx src/engine/multiReplayTargets.selfcheck.ts
 */
import assert from 'node:assert/strict';
import {
  buildDefaultChannelTargets,
  isMultiReplayCapable,
  listEnabledMultiTargets,
} from './multiReplayTargets';

assert.equal(isMultiReplayCapable('udp'), true);
assert.equal(isMultiReplayCapable('tcp'), true);
assert.equal(isMultiReplayCapable('serial'), true);
assert.equal(isMultiReplayCapable('mqtt'), true);
assert.equal(isMultiReplayCapable('video_stream'), true);
assert.equal(isMultiReplayCapable('rtsp'), false);

const rows = buildDefaultChannelTargets([
  { id: 'ch_0', name: 'U', type: 'udp', packetCount: 1 },
  { id: 'ch_1', name: 'T', type: 'tcp', packetCount: 1 },
  { id: 'ch_2', name: 'S', type: 'serial', packetCount: 1 },
  { id: 'ch_3', name: 'M', type: 'mqtt', packetCount: 1 },
]);
assert.equal(rows.every((r) => r.enabled), true);

rows[2].enabled = false;
const targets = listEnabledMultiTargets(rows);
assert.equal(targets.length, 3);
assert.deepEqual(
  targets.map((t) => t.type),
  ['udp', 'tcp', 'mqtt']
);
assert.equal(targets[0].config.target_ip, '127.0.0.1');
assert.equal(targets[1].config.host, '127.0.0.1');
assert.equal(typeof targets[1].config.port, 'number');
assert.equal(targets[2].config.broker, '127.0.0.1');

const serialOnly = buildDefaultChannelTargets([
  { id: 'ch_0', name: 'S', type: 'serial', packetCount: 2 },
]);
serialOnly[0].serialPort = 'COM9';
serialOnly[0].baud = 921600;
const s = listEnabledMultiTargets(serialOnly);
assert.equal(s[0].type, 'serial');
assert.equal(s[0].config.port_name, 'COM9');
assert.equal(s[0].config.baud, 921600);

console.log('multiReplayTargets.selfcheck: ok');
