// Klient WebSocket + dekodowanie binarnych klatek z serwera Pythona.

export const U_SCALE = 0.3 / 32767;
export const P_SCALE = 0.02 / 32767;
const HEADER = 44;

export interface Frame {
  nx: number;
  ny: number;
  step: number;
  cd: number;
  cl: number;
  re: number;
  u: number;
  fps: number;
  mlups: number;
  length: number;
  nu: number;
  ux: Int16Array;
  uy: Int16Array;
  p: Int16Array;
  solid: Uint8Array;
}

export function decode(buf: ArrayBuffer): Frame | null {
  const dv = new DataView(buf);
  if (String.fromCharCode(dv.getUint8(0), dv.getUint8(1), dv.getUint8(2), dv.getUint8(3)) !== "AERO") return null;
  const nx = dv.getUint16(4, true);
  const ny = dv.getUint16(6, true);
  const n = nx * ny;
  const f = (i: number) => dv.getFloat32(12 + i * 4, true);
  return {
    nx, ny,
    step: dv.getUint32(8, true),
    cd: f(0), cl: f(1), re: f(2), u: f(3), fps: f(4), mlups: f(5), length: f(6), nu: f(7),
    ux: new Int16Array(buf, HEADER, n),
    uy: new Int16Array(buf, HEADER + 2 * n, n),
    p: new Int16Array(buf, HEADER + 4 * n, n),
    solid: unpackBits(new Uint8Array(buf, HEADER + 6 * n), n),
  };
}

function unpackBits(bits: Uint8Array, n: number) {
  const out = new Uint8Array(n);
  for (let i = 0; i < n; i++) out[i] = (bits[i >> 3] >> (i & 7)) & 1;
  return out;
}

export type Cmd = Record<string, unknown> & { type: string };

export class SimSocket {
  private ws?: WebSocket;
  private closed = false;
  onFrame: (f: Frame) => void = () => {};
  onStatus: (s: "connecting" | "open" | "closed") => void = () => {};

  connect() {
    this.onStatus("connecting");
    const proto = location.protocol === "https:" ? "wss" : "ws";
    const ws = new WebSocket(`${proto}://${location.host}/ws`);
    ws.binaryType = "arraybuffer";
    ws.onopen = () => this.onStatus("open");
    ws.onmessage = (e) => {
      const fr = decode(e.data as ArrayBuffer);
      if (fr) this.onFrame(fr);
    };
    ws.onclose = () => {
      this.onStatus("closed");
      if (!this.closed) setTimeout(() => this.connect(), 1000);
    };
    this.ws = ws;
  }

  send(cmd: Cmd) {
    if (this.ws?.readyState === WebSocket.OPEN) this.ws.send(JSON.stringify(cmd));
  }

  close() {
    this.closed = true;
    this.ws?.close();
  }
}

/** Profil NACA 4-cyfrowy — ta sama geometria co po stronie Pythona (podgląd przy rysowaniu). */
export function nacaPolygon(x: number, y: number, chord: number, angle: number, t = 0.12, m = 0.04, p = 0.4, n = 60) {
  const up: [number, number][] = [];
  const lo: [number, number][] = [];
  for (let k = 0; k < n; k++) {
    const xc = (1 - Math.cos((Math.PI * k) / (n - 1))) / 2;
    const yt = 5 * t * (0.2969 * Math.sqrt(xc) - 0.126 * xc - 0.3516 * xc ** 2 + 0.2843 * xc ** 3 - 0.1036 * xc ** 4);
    let yc = 0, dyc = 0;
    if (m > 0) {
      yc = xc < p ? (m / p ** 2) * (2 * p * xc - xc ** 2) : (m / (1 - p) ** 2) * (1 - 2 * p + 2 * p * xc - xc ** 2);
      dyc = xc < p ? ((2 * m) / p ** 2) * (p - xc) : ((2 * m) / (1 - p) ** 2) * (p - xc);
    }
    const th = Math.atan(dyc);
    up.push([xc - yt * Math.sin(th), yc + yt * Math.cos(th)]);
    lo.push([xc + yt * Math.sin(th), yc - yt * Math.cos(th)]);
  }
  const c = Math.cos(angle), s = Math.sin(angle);
  return [...up, ...lo.reverse()].map(([px, py]) => {
    px *= chord;
    py *= chord;
    return [x + px * c + py * s, y + px * s - py * c] as [number, number];
  });
}
