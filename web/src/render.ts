// Renderowanie pola (mapy kolorów) i cząstek-znaczników.
import { Frame, P_SCALE, U_SCALE } from "./sim";

export type Palette = "ocean" | "arctic" | "abyss";
export type ViewMode = "speed" | "vorticity" | "pressure" | "smoke" | "none";
export type ParticleMode = "off" | "tracers" | "smoke" | "streamlines";

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
  arctic: lut([[0, "#0b1220"], [0.35, "#1b3b6f"], [0.6, "#6aa9e9"], [0.85, "#dbeeff"], [1, "#ffffff"]]),
  abyss: lut([[0, "#000000"], [0.4, "#00334d"], [0.7, "#00a3cc"], [0.9, "#7ff7ff"], [1, "#ffffff"]]),
  dye: lut([[0, "#030a1c"], [0.25, "#0b2553"], [0.55, "#2a7fd4"], [0.8, "#a5e4ff"], [1, "#ffffff"]]),
  ocean: lut([[0, "#020617"], [0.25, "#0a2a5e"], [0.5, "#1463c7"], [0.72, "#3fb8f5"], [0.9, "#a8ecff"], [1, "#f4fdff"]]),
  diverging: lut([[0, "#7dd3fc"], [0.2, "#1d6fd8"], [0.45, "#08122c"], [0.5, "#050b1c"], [0.55, "#2a0f2e"], [0.8, "#e0487a"], [1, "#ffd1a6"]]),
  pressure: lut([[0, "#1e3a8a"], [0.3, "#2f7ff0"], [0.5, "#0b1430"], [0.7, "#f59e0b"], [1, "#fff4c2"]]),
};

const SOLID = [226, 232, 240];

export class FieldRenderer {
  canvas = document.createElement("canvas");
  private ctx = this.canvas.getContext("2d")!;
  private img?: ImageData;

  draw(fr: Frame, mode: ViewMode, gain: number, palette: Palette = "ocean") {
    const { nx, ny, ux, uy, p, solid } = fr;
    if (!this.img || this.img.width !== nx || this.img.height !== ny) {
      this.canvas.width = nx;
      this.canvas.height = ny;
      this.img = this.ctx.createImageData(nx, ny);
    }
    const d = this.img.data;
    const L = mode === "speed" ? LUTS[palette] : mode === "vorticity" ? LUTS.diverging : mode === "smoke" ? LUTS.dye : LUTS.pressure;
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
          t = 0.5 + 0.5 * Math.tanh((w / ref) * 1.4 * gain);
        } else if (mode === "pressure" && p) {
          t = 0.5 + 0.5 * Math.tanh(((p[i] * P_SCALE) / (ref * ref)) * 1.2 * gain);
        } else if (mode === "smoke" && fr.dye) {
          // dym + delikatne tło prędkości, żeby było widać, gdzie płynie powietrze
          const u = ux[i] * U_SCALE, v = uy[i] * U_SCALE;
          t = Math.min(1, (fr.dye[i] / 255) * gain * 0.95 + Math.sqrt(u * u + v * v) / ref * 0.12);
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

  /** Linie prądu: całkowanie RK2 z punktów startowych na wlocie (chwilowe pole prędkości). */
  static streamlines(ctx: CanvasRenderingContext2D, fr: Frame, sx: number, sy: number, count: number, dpr: number) {
    const { nx, ny, ux, uy, solid } = fr;
    const sample = (a: Int16Array, x: number, y: number) => {
      const x0 = Math.min(nx - 2, Math.max(0, x | 0)), y0 = Math.min(ny - 2, Math.max(0, y | 0));
      const fx = x - x0, fy = y - y0, i = y0 * nx + x0;
      return (a[i] * (1 - fx) + a[i + 1] * fx) * (1 - fy) + (a[i + nx] * (1 - fx) + a[i + nx + 1] * fx) * fy;
    };
    ctx.lineWidth = 1.1 * dpr;
    ctx.lineJoin = "round";
    ctx.strokeStyle = "rgba(225,245,255,0.7)";
    ctx.beginPath();
    const h = 0.9;
    const maxSteps = nx * 3;
    for (let k = 0; k < count; k++) {
      let x = 1.5, y = ((k + 0.5) / count) * ny;
      ctx.moveTo(x * sx, y * sy);
      for (let s = 0; s < maxSteps; s++) {
        let u = sample(ux, x, y), v = sample(uy, x, y);
        let m = Math.hypot(u, v);
        if (m < 1) break;
        const mx = x + (u / m) * h * 0.5, my = y + (v / m) * h * 0.5;
        u = sample(ux, mx, my); v = sample(uy, mx, my); m = Math.hypot(u, v);
        if (m < 1) break;
        x += (u / m) * h; y += (v / m) * h;
        if (x < 0 || x >= nx - 1 || y < 0 || y >= ny - 1 || solid[(y | 0) * nx + (x | 0)]) break;
        if (s % 3 === 0) ctx.lineTo(x * sx, y * sy);
      }
      ctx.lineTo(x * sx, y * sy);
    }
    ctx.stroke();
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

/** Gładka warstwa ciał stałych: maska próbkowana bilinearnie w rozdzielczości ekranu, liczona tylko przy zmianie kształtu. */
export class SolidLayer {
  canvas = document.createElement("canvas");
  private sig = "";

  update(fr: Frame, W: number, H: number) {
    const { nx, ny, solid } = fr;
    let h = 0, cnt = 0;
    for (let i = 0; i < solid.length; i++) if (solid[i]) { h = (h * 31 + i) | 0; cnt++; }
    const sig = `${nx}x${ny}:${W}x${H}:${cnt}:${h}`;
    if (sig === this.sig) return;
    this.sig = sig;
    const cv = this.canvas;
    cv.width = W;
    cv.height = H;
    const ctx = cv.getContext("2d")!;
    const img = ctx.createImageData(W, H);
    const d = img.data;
    // maska rozmyta 2× filtrem 3×3 → kontur 0.5 staje się gładki zamiast schodkowego
    let m = Float32Array.from(solid);
    for (let pass = 0; pass < 2; pass++) {
      const o = new Float32Array(m.length);
      for (let y = 0; y < ny; y++) {
        for (let x = 0; x < nx; x++) {
          let acc = 0, n = 0;
          for (let dy = -1; dy <= 1; dy++) {
            const yy = y + dy;
            if (yy < 0 || yy >= ny) continue;
            for (let dx = -1; dx <= 1; dx++) {
              const xx = x + dx;
              if (xx < 0 || xx >= nx) continue;
              acc += m[yy * nx + xx]; n++;
            }
          }
          o[y * nx + x] = acc / n;
        }
      }
      m = o;
    }
    const kx = nx / W, ky = ny / H;
    const aa = 0.35 * 1.6 / Math.min(W / nx, H / ny); // szerokość wygładzenia ~1.6 px ekranu
    for (let py = 0; py < H; py++) {
      const gy = (py + 0.5) * ky - 0.5;
      const y0 = Math.max(0, Math.min(ny - 2, Math.floor(gy)));
      const fy = Math.min(1, Math.max(0, gy - y0));
      for (let px = 0; px < W; px++) {
        const gx = (px + 0.5) * kx - 0.5;
        const x0 = Math.max(0, Math.min(nx - 2, Math.floor(gx)));
        const i = y0 * nx + x0;
        const a = m[i], b = m[i + 1], c = m[i + nx], e = m[i + nx + 1];
        if (a + b + c + e < 0.4) continue;
        const fx = Math.min(1, Math.max(0, gx - x0));
        const v = (a * (1 - fx) + b * fx) * (1 - fy) + (c * (1 - fx) + e * fx) * fy;
        const t = Math.min(1, Math.max(0, (v - 0.5) / aa + 0.5));
        if (t <= 0) continue;
        const o = (py * W + px) * 4;
        // lekki gradient pionowy daje wrażenie bryły
        const shade = 236 - (py / H) * 26;
        d[o] = shade - 8; d[o + 1] = shade - 2; d[o + 2] = shade + 10; d[o + 3] = t * 255;
      }
    }
    // poświata wypiekana raz do bufora, nie w każdej klatce
    const tmp = document.createElement("canvas");
    tmp.width = W;
    tmp.height = H;
    tmp.getContext("2d")!.putImageData(img, 0, 0);
    ctx.clearRect(0, 0, W, H);
    ctx.shadowColor = "rgba(56,189,248,0.55)";
    ctx.shadowBlur = 16;
    ctx.drawImage(tmp, 0, 0);
    ctx.shadowBlur = 0;
    ctx.drawImage(tmp, 0, 0);
  }
}
