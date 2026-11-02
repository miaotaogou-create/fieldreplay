/**
 * 用法：cd ui && npx tsx src/utils/channelForm.selfcheck.ts
 */
import {
  buildChannelFromForm,
  channelToFormValues,
  defaultFormValuesForType,
} from './channelForm';
import type { ChannelConfig } from '../types/recorder';

function assert(cond: unknown, msg: string): asserts cond {
  if (!cond) throw new Error(msg);
}

const base: ChannelConfig = {
  id: 'ch_keep',
  name: 'UDP测试',
  type: 'udp',
  enabled: true,
  color: '#0891b2',
  params: { ip: '0.0.0.0', port: 56001 },
  connected: true,
  rxBytes: 0,
  rxFrames: 0,
  currentRateBps: 0,
};

const form = channelToFormValues(base);
assert(form.param === '0.0.0.0:56001', `预填参数应为 0.0.0.0:56001，得 ${form.param}`);

const edited = buildChannelFromForm(
  { ...form, name: 'UDP改名', param: '0.0.0.0:56099' },
  base
);
assert(edited.id === 'ch_keep', '编辑须保留 id');
assert(edited.name === 'UDP改名', '名称应更新');
assert(edited.params.port === 56099, '端口应更新');
assert(edited.type === 'udp', '类型不可因表单漂移');

const tcpBase: ChannelConfig = {
  ...base,
  id: 'ch_tcp',
  type: 'tcp',
  name: 'TCP',
  params: { ip: '0.0.0.0', port: 58002, role: 'client' },
};
const tcpForm = channelToFormValues(tcpBase);
const tcpSaved = buildChannelFromForm(tcpForm, tcpBase);
assert(tcpSaved.params.role === 'client', '编辑须保留 TCP role');

const created = buildChannelFromForm({
  name: '新MQTT',
  ...defaultFormValuesForType('mqtt'),
});
assert(created.type === 'mqtt', '新建 MQTT 类型');
assert(!!created.id && created.id !== 'ch_keep', '新建应有新 id');

console.log('channelForm.selfcheck: ok');
