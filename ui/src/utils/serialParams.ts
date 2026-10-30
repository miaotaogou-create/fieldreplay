/**
 * 解析录制页串口参数行，如 `COM4, 921600, 8-N-1` / `COM3,115200,8E1`。
 */

export type SerialParity = 'none' | 'even' | 'odd';

export interface ParsedSerialParams {
  portName: string;
  baudRate: number;
  dataBits: 7 | 8;
  parity: SerialParity;
  stopBits: 1 | 2;
}

const DEFAULTS: ParsedSerialParams = {
  portName: 'COM3',
  baudRate: 115200,
  dataBits: 8,
  parity: 'none',
  stopBits: 1,
};

/** 将校验位压成简写，如 `8N1`。 */
export function formatSerialLineSummary(p: {
  baudRate?: number;
  dataBits?: number;
  parity?: SerialParity | string;
  stopBits?: number;
}): string {
  const baud = p.baudRate ?? 115200;
  const db = p.dataBits ?? 8;
  const parityChar =
    p.parity === 'even' || p.parity === 'e'
      ? 'E'
      : p.parity === 'odd' || p.parity === 'o'
        ? 'O'
        : 'N';
  const sb = p.stopBits ?? 1;
  return `${baud} ${db}${parityChar}${sb}`;
}

export function parseSerialParamLine(line: string): ParsedSerialParams {
  const parts = line.split(',').map((s) => s.trim()).filter(Boolean);
  const portName = parts[0] || DEFAULTS.portName;
  let baudRate = DEFAULTS.baudRate;
  let dataBits: 7 | 8 = DEFAULTS.dataBits;
  let parity: SerialParity = DEFAULTS.parity;
  let stopBits: 1 | 2 = DEFAULTS.stopBits;

  if (parts[1]) {
    const n = parseInt(parts[1], 10);
    if (Number.isFinite(n) && n > 0) baudRate = n;
  }

  const mode = (parts[2] || '').replace(/[\s\-_/]/g, '').toUpperCase();
  if (mode.length >= 3) {
    const db = mode.charAt(0);
    if (db === '7') dataBits = 7;
    else if (db === '8') dataBits = 8;

    const p = mode.charAt(1);
    if (p === 'E') parity = 'even';
    else if (p === 'O') parity = 'odd';
    else parity = 'none';

    const sb = mode.charAt(2);
    if (sb === '2') stopBits = 2;
    else stopBits = 1;
  }

  return { portName, baudRate, dataBits, parity, stopBits };
}
