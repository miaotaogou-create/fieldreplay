/**
 * @license
 * SPDX-License-Identifier: Apache-2.0
 */

const B64 =
  'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';

export function bytesToBase64(bytes: Uint8Array): string {
  let out = '';
  for (let i = 0; i < bytes.length; i += 3) {
    const a = bytes[i];
    const b = i + 1 < bytes.length ? bytes[i + 1] : 0;
    const c = i + 2 < bytes.length ? bytes[i + 2] : 0;
    const n = (a << 16) | (b << 8) | c;
    out += B64[(n >> 18) & 63];
    out += B64[(n >> 12) & 63];
    out += i + 1 < bytes.length ? B64[(n >> 6) & 63] : '=';
    out += i + 2 < bytes.length ? B64[n & 63] : '=';
  }
  return out;
}

export function base64ToBytes(s: string): Uint8Array {
  const clean = s.replace(/\s+/g, '');
  const len = clean.length;
  if (len % 4 !== 0) throw new Error('base64 长度非法');
  const pad = clean.endsWith('==') ? 2 : clean.endsWith('=') ? 1 : 0;
  const out = new Uint8Array((len / 4) * 3 - pad);
  let o = 0;
  for (let i = 0; i < len; i += 4) {
    const v = (dec(clean.charCodeAt(i)) << 18)
      | (dec(clean.charCodeAt(i + 1)) << 12)
      | (dec(clean.charCodeAt(i + 2)) << 6)
      | dec(clean.charCodeAt(i + 3));
    out[o++] = (v >> 16) & 0xff;
    if (clean[i + 2] !== '=') out[o++] = (v >> 8) & 0xff;
    if (clean[i + 3] !== '=') out[o++] = v & 0xff;
  }
  return out;
}

function dec(c: number): number {
  if (c >= 65 && c <= 90) return c - 65;
  if (c >= 97 && c <= 122) return c - 71;
  if (c >= 48 && c <= 57) return c + 4;
  if (c === 43) return 62;
  if (c === 47) return 63;
  if (c === 61) return 0;
  throw new Error('base64 字符非法');
}
