// Renderowanie pola (mapy kolorów) i cząstek-znaczników.
import { Frame, P_SCALE, U_SCALE } from "./sim";

export type ViewMode = "speed" | "vorticity" | "pressure" | "none";
export type ParticleMode = "off" | "tracers" | "smoke";

function lut(stops: [number, string][]): Uint8Array {
  const out = new Uint8Array(256 * 3);
  const rgb = stops.map(([t, h]) => [t, parseInt(h.slice(1, 3), 16), parseInt(h.slice(3, 5), 16), parseInt(h.slice(5, 7), 16)]);
  for (let i = 0; i < 256; i++) {
    const t = i / 255;
    let k = 0;
    while (k < rgb.length - 2 && t > rgb[k + 1][0]) k++;
    const [t0, r0, g0, b0] = rgb[k];
    const [t1, r1, g1, b1] = rgb[k + 1];
    const a = Math.min(1, Math.max(0, (t - t0) / (t1 - t0)));
    const e = a * a * (3 - 2 * a);
    out[i * 3] = r0 + (r1 - r0) * e;
    out[i * 3 + 1] = g0 + (g1 - g0) * e;
    out[i * 3 + 2] = b0 + (b1 - b0) * e;
  }
  return out;
}

export const LUTS = {
  ocean: lut([[0, "#020617"], [0.25, "#0a2a5e"], [0.5, "#1463c7"], [0.72, "#3fb8f5"], [0.9, "#a8ecff"], [1, "#f4fdff"]]),
  diverging: lut([[0, "#7dd3fc"], [0.2, "#1d6fd8"], [0.45, "#08122c"], [0.5, "#050b1c"], [0.55, "#2a0f2e"], [0.8, "#e0487a"], [1, "#ffd1a6"]]),
  pressure: lut([[0, "#1e3a8a"], [0.3, "#2f7ff0"], [0.5, "#0b1430"], [0.7, "#f59e0b"], [1, "#fff4c2"]]),
};

const SOLID = [226, 232, 240];

export class FieldRenderer {
  canvas = document.createElement("canvas");
  private ctx = this.canvas.getContext("2d")!;
  private img?: ImageData;

  draw(fr: Frame, mode: ViewMode, gain: number) {
    const { nx, ny, ux, uy, p, solid } = fr;
    if (!this.img || this.img.width !== nx || this.img.height !== ny) {
      this.canvas.width = nx;
      this.canvas.height = ny;
      this.img = this.ctx.createImageData(nx, ny);
    }
    const d = this.img.data;
    const L = mode === "speed" ? LUTS.ocean : mode === "vorticity" ? LUTS.diverging : LUTS.pressure;
    const ref = Math.max(fr.u, 0.02);
    for (let y = 0; y < ny; y++) {
      for (let x = 0; x < nx; x++) {
        const i = y * nx + x;
        const o = i * 4;
        if (solid[i]) {
          d[o] = SOLID[0]; d[o + 1] = SOLID[1]; d[o + 2] = SOLID[2]; d[o + 3] = 255;
          continue;
        }
        let t: number;
        if (mode === "speed") {
          const u = ux[i] * U_SCALE, v = uy[i] * U_SCALE;
          t = (Math.sqrt(u * u + v * v) / (ref * 2.1)) * gain;
        } else if (mode === "vorticity") {
          const xm = x > 0 ? i - 1 : i, xp = x < nx - 1 ? i + 1 : i;
          const ym = y > 0 ? i - nx : i, yp = y < ny - 1 ? i + nx : i;
          const w = ((uy[xp] - uy[xm]) - (ux[yp] - ux[ym])) * 0.5 * U_SCALE;
          t = 0.5 + (w / ref) * 2.2 * gain;
        } else if (mode === "pressure") {
          t = 0.5 + ((p[i] * P_SCALE) / (ref * ref)) * 0.9 * gain;
        } else {
          t = 0.04;
        }
        const k = (t <= 0 ? 0 : t >= 1 ? 255 : (t * 255) | 0) * 3;
        d[o] = L[k]; d[o + 1] = L[k + 1]; d[o + 2] = L[k + 2]; d[o + 3] = 255;
      }
    }
    this.ctx.putImageData(this.img, 0, 0);
  }
}

export class Particles {
  private xs = new Float32Array(0);
  private ys = new Float32Array(0);
  private px = new Float32Array(0);
  private py = new Float32Array(0);
  private age = new Float32Array(0);
  private seed = new Float32Array(0);
  private count = 0;

  resize(count: number, fr: Frame, mode: ParticleMode) {
    this.count = count;
    this.xs = new Float32Array(count);
    this.ys = new Float32Array(count);
    this.px = new Float32Array(count);
    this.py = new Float32Array(count);
    this.age = new Float32Array(count);
    this.seed = new Float32Array(count);
    for (let i = 0; i < count; i++) this.spawn(i, fr, mode, true);
  }

  private spawn(i: number, fr: Frame, mode: ParticleMode, anywhere: boolean) {
    if (mode === "smoke") {
      const lanes = 28;
      const lane = i % lanes;
      this.seed[i] = lane;
      this.xs[i] = anywhere ? Math.random() * fr.nx : 1 + Math.random() * 1.5;
      this.ys[i] = ((lane + 0.5) / lanes) * fr.ny;
    } else {
      this.xs[i] = anywhere ? Math.random() * fr.nx : Math.random() * 3;
      this.ys[i] = Math.random() * fr.ny;
    }
    this.px[i] = this.xs[i];
    this.py[i] = this.ys[i];
    this.age[i] = anywhere ? Math.random() * 300 : 0;
  }

  step(fr: Frame, mode: ParticleMode, dt: number) {
    const { nx, ny, ux, uy, solid } = fr;
    const sample = (a: Int16Array, x: number, y: number) => {
      const x0 = Math.min(nx - 2, Math.max(0, x | 0)), y0 = Math.min(ny - 2, Math.max(0, y | 0));
      const fx = x - x0, fy = y - y0, i = y0 * nx + x0;
      return ((a[i] * (1 - fx) + a[i + 1] * fx) * (1 - fy) + (a[i + nx] * (1 - fx) + a[i + nx + 1] * fx) * fy) * U_SCALE;
    };
    for (let i = 0; i < this.count; i++) {
      let x = this.xs[i], y = this.ys[i];
      this.px[i] = x;
      this.py[i] = y;
      // RK2 (midpoint)
      const u1 = sample(ux, x, y), v1 = sample(uy, x, y);
      const mx = x + u1 * dt * 0.5, my = y + v1 * dt * 0.5;
      x += sample(ux, mx, my) * dt;
      y += sample(uy, mx, my) * dt;
      this.age[i] += 1;
      const cell = (y | 0) * nx + (x | 0);
      const dead = x < 0 || x >= nx - 1 || y < 0 || y >= ny - 1 || solid[cell] || (mode === "tracers" && this.age[i] > 400 + (i % 97) * 4);
      if (dead) {
        this.spawn(i, fr, mode, mode === "tracers" && x < nx - 2 && Math.random() < 0.6);
        this.px[i] = this.xs[i];
        this.py[i] = this.ys[i];
      } else {
        this.xs[i] = x;
        this.ys[i] = y;
      }
    }
  }

  draw(ctx: CanvasRenderingContext2D, sx: number, sy: number, mode: ParticleMode) {
    ctx.lineCap = "round";
    ctx.lineWidth = mode === "smoke" ? 1.6 : 1.1;
    ctx.strokeStyle = mode === "smoke" ? "rgba(236,248,255,0.8)" : "rgba(214,240,255,0.55)";
    ctx.beginPath();
    for (let i = 0; i < this.count; i++) {
      const dx = this.xs[i] - this.px[i], dy = this.ys[i] - this.py[i];
      if (dx * dx + dy * dy > 25) continue; // respawn — bez przeskoków
      ctx.moveTo(this.px[i] * sx, this.py[i] * sy);
      ctx.lineTo(this.xs[i] * sx + 0.01, this.ys[i] * sy);
    }
    ctx.stroke();
  }
}
