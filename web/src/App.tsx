import { useCallback, useEffect, useRef, useState } from "react";
import { FieldRenderer, Palette, ParticleMode, Particles, SolidLayer, ViewMode } from "./render";
import { Bodies, BodyInfo, Frame, SimSocket, U_SCALE, P_SCALE, nacaPolygon } from "./sim";
import { ContextMenu } from "./ContextMenu";
import { Icon } from "./icons";
import { DockButton, FloatingWindow, Segmented, Slider, Toggle } from "./ui";

type Tool = "select" | "brush" | "erase" | "airfoil" | "polygon" | "circle" | "rect";
type Pt = [number, number];

const TOOLS: { id: Tool; label: string; key: string; hint: string }[] = [
  { id: "select", label: "Zaznacz", key: "1", hint: "Przeciągnij obiekt, aby go przesunąć · kółko myszy obraca · prawy przycisk: opcje · Del usuwa" },
  { id: "airfoil", label: "Skrzydło", key: "2", hint: "Przeciągnij od noska do krawędzi spływu — długość to cięciwa, kierunek to kąt natarcia" },
  { id: "polygon", label: "Kształt", key: "3", hint: "Narysuj obrys — po puszczeniu zostanie wypełniony" },
  { id: "brush", label: "Pędzel", key: "4", hint: "Maluj ściany · prawy przycisk na pustym polu wymazuje" },
  { id: "circle", label: "Koło", key: "5", hint: "Przeciągnij od środka, aby ustawić promień" },
  { id: "rect", label: "Prostokąt", key: "6", hint: "Przeciągnij po przekątnej" },
  { id: "erase", label: "Gumka", key: "7", hint: "Wymazuj fragmenty przeszkód" },
];

const PRESETS: { id: string; label: string; thumb: JSX.Element }[] = [
  { id: "airfoil", label: "Profil NACA", thumb: <path d="M14 22c8-10 26-10 38-4-12 1-26 3-38 4z" /> },
  { id: "cylinder", label: "Walec Kármána", thumb: <circle cx="22" cy="20" r="7" /> },
  { id: "tandem", label: "Tandem", thumb: <><path d="M8 16c5-6 15-6 21-2-7 0-14 1-21 2z" /><path d="M32 26c5-5 15-5 21-2-7 0-14 1-21 2z" /></> },
  { id: "plate", label: "Płyta", thumb: <rect x="22" y="11" width="4" height="18" rx="1" /> },
  { id: "grid", label: "Krata", thumb: <>{[6, 13, 20, 27, 34].map((y) => <circle key={y} cx="24" cy={y} r="2.6" />)}</> },
  { id: "step", label: "Uskok", thumb: <rect x="16" y="24" width="30" height="16" /> },
  { id: "empty", label: "Pusty tunel", thumb: <path d="M8 20h48" strokeDasharray="4 4" /> },
];

const VIEWS: { id: ViewMode; label: string; icon: string }[] = [
  { id: "speed", label: "Prędkość", icon: "speed" },
  { id: "vorticity", label: "Wirowość", icon: "vorticity" },
  { id: "pressure", label: "Ciśnienie", icon: "pressure" },
  { id: "smoke", label: "Dym", icon: "smoke" },
  { id: "none", label: "Brak", icon: "none" },
];

const RESOLUTIONS = [{ id: "200", label: "Średnia" }, { id: "280", label: "Wysoka" }, { id: "360", label: "Ultra" }, { id: "440", label: "Maks." }];
const PMODES: { id: ParticleMode; label: string; icon: string }[] = [
  { id: "tracers", label: "Cząstki", icon: "particles" },
  { id: "streamlines", label: "Linie prądu", icon: "streamlines" },
  { id: "smoke", label: "Smugi", icon: "smoke" },
  { id: "off", label: "Wył.", icon: "none" },
];
const nextPmode = (m: ParticleMode) => PMODES[(PMODES.findIndex((x) => x.id === m) + 1) % PMODES.length].id;

const nuFromSlider = (s: number) => 0.001 * Math.pow(0.06 / 0.001, s);
const sliderFromNu = (nu: number) => Math.log(nu / 0.001) / Math.log(0.06 / 0.001);

interface Stats { canUndo: boolean; canRedo: boolean; cd: number; cl: number; re: number; fps: number; mlups: number; step: number; length: number; nx: number; ny: number }

function usePersist<T>(key: string, init: T) {
  const [v, setV] = useState<T>(() => {
    try { const s = localStorage.getItem(`af:${key}`); return s ? (JSON.parse(s) as T) : init; } catch { return init; }
  });
  useEffect(() => { try { localStorage.setItem(`af:${key}`, JSON.stringify(v)); } catch { /* ignore */ } }, [key, v]);
  return [v, setV] as const;
}

export default function App() {
  const [tool, setTool] = useState<Tool>("airfoil");
  const [view, setView] = usePersist<ViewMode>("view", "speed");
  const [palette, setPalette] = usePersist<Palette>("palette", "ocean");
  const [pmode, setPmode] = usePersist<ParticleMode>("pmode", "tracers");
  const [pcount, setPcount] = usePersist("pcount", 3000);
  const [trailLen, setTrailLen] = usePersist("trail", 0.5);
  const [brush, setBrush] = useState(3);
  const [u, setU] = useState(0.1);
  const [nuS, setNuS] = useState(sliderFromNu(0.008));
  const [steps, setSteps] = usePersist("steps", 10);
  const [res, setRes] = usePersist("res2", "280");
  const [arrows, setArrows] = usePersist("arrows", true);
  const [gain, setGain] = usePersist("gain", 1);
  const [paused, setPaused] = useState(false);
  const [thick, setThick] = useState(12);
  const [camber, setCamber] = useState(4);
  const [probeOn, setProbeOn] = usePersist("probe", true);
  const [status, setStatus] = useState<"connecting" | "open" | "closed">("connecting");
  const [stats, setStats] = useState<Stats | null>(null);
  const [probe, setProbe] = useState<{ sx: number; sy: number; text: string } | null>(null);
  const [ghost, setGhost] = useState<string | null>(null);
  const [win, setWin] = useState({ settings: false, chart: true, help: false });
  const [settingsTab, setSettingsTab] = useState<"flow" | "vis" | "sim">("flow");
  const [presetsOpen, setPresetsOpen] = useState(false);
  const [zen, setZen] = useState(false);
  const [toast, setToast] = useState<{ text: string; id: number } | null>(null);
  const [bodies, setBodies] = useState<Bodies | null>(null);
  const [menu, setMenu] = useState<{ id: number; x: number; y: number } | null>(null);

  const sock = useRef<SimSocket>();
  const frame = useRef<Frame | null>(null);
  const fresh = useRef(false);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const chartRef = useRef<HTMLCanvasElement>(null);
  const field = useRef(new FieldRenderer());
  const parts = useRef(new Particles());
  const solids = useRef(new SolidLayer());
  const trail = useRef(document.createElement("canvas"));
  const history = useRef<{ cd: number; cl: number }[]>([]);
  const drag = useRef<{ start: Pt; pts: Pt[]; cur: Pt; erase: boolean; pending: Pt[]; sent?: boolean } | null>(null);
  const hover = useRef<{ g: Pt; sx: number; sy: number } | null>(null);
  const bodiesRef = useRef<Bodies | null>(null);
  const hoverBody = useRef<number | null>(null);
  const moving = useRef<{ last: Pt; acc: Pt } | null>(null);
  const wheelAt = useRef(0);
  const cfg = useRef({ view, pmode, pcount, gain, tool, brush, thick, camber, steps, palette, trailLen, arrows, menuId: menu?.id ?? null });
  cfg.current = { view, pmode, pcount, gain, tool, brush, thick, camber, steps, palette, trailLen, arrows, menuId: menu?.id ?? null };

  const send = useCallback((c: { type: string } & Record<string, unknown>) => sock.current?.send(c), []);
  const notify = useCallback((text: string) => setToast({ text, id: Date.now() }), []);
  useEffect(() => {
    if (!toast) return;
    const t = setTimeout(() => setToast(null), 1800);
    return () => clearTimeout(t);
  }, [toast]);

  // ---- połączenie
  useEffect(() => {
    const s = new SimSocket();
    s.onStatus = setStatus;
    s.onFrame = (f) => { frame.current = f; fresh.current = true; };
    s.onBodies = (b) => { bodiesRef.current = b; setBodies(b); };
    s.connect();
    sock.current = s;
    return () => s.close();
  }, []);

  // ---- siatka dopasowana do proporcji okna
  useEffect(() => {
    let t = 0;
    const fit = () => {
      clearTimeout(t);
      t = window.setTimeout(() => send({ type: "resize", aspect: window.innerWidth / window.innerHeight, ny: parseInt(res) }), 250);
    };
    fit();
    window.addEventListener("resize", fit);
    return () => { clearTimeout(t); window.removeEventListener("resize", fit); };
  }, [res, send, status]);

  useEffect(() => send({ type: "params", u, nu: nuFromSlider(nuS), steps, paused }), [u, nuS, steps, paused, send, status]);
  // serwer liczy i wysyła tylko pola, które są faktycznie potrzebne
  useEffect(() => {
    const fields: string[] = [];
    if (view === "pressure" || probeOn) fields.push("p");
    if (view === "smoke") fields.push("dye");
    send({ type: "params", fields });
  }, [view, probeOn, send, status]);

  // ---- statystyki (5 Hz) + wykres + sonda
  useEffect(() => {
    const id = setInterval(() => {
      const f = frame.current;
      if (!f) return;
      setStats({ canUndo: f.canUndo, canRedo: f.canRedo, cd: f.cd, cl: f.cl, re: f.re, fps: f.fps, mlups: f.mlups, step: f.step, length: f.length, nx: f.nx, ny: f.ny });
      const h = history.current;
      if (f.length > 0 && !paused) {
        h.push({ cd: f.cd, cl: f.cl });
        if (h.length > 300) h.shift();
      }
      drawChart(chartRef.current, h);
      const hv = hover.current;
      if (hv && probeOn && !drag.current) {
        const x = Math.floor(hv.g[0]), y = Math.floor(hv.g[1]);
        if (x >= 0 && y >= 0 && x < f.nx && y < f.ny) {
          const i = y * f.nx + x;
          const sp = (Math.hypot(f.ux[i], f.uy[i]) * U_SCALE) / Math.max(f.u, 1e-6);
          const cp = f.p ? (f.p[i] * P_SCALE) / (0.5 * f.u * f.u) : NaN;
          setProbe({ sx: hv.sx, sy: hv.sy, text: f.solid[i] ? "ciało stałe" : `|u|/U∞ ${sp.toFixed(2)}` + (Number.isFinite(cp) ? `   Cp ${cp >= 0 ? "+" : ""}${cp.toFixed(2)}` : "") });
        }
      } else setProbe(null);
    }, 200);
    return () => clearInterval(id);
  }, [paused, probeOn]);

  // ---- pętla renderująca
  useEffect(() => {
    let raf = 0;
    let lastKey = "";
    const loop = () => {
      raf = requestAnimationFrame(loop);
      const cv = canvasRef.current, f = frame.current;
      if (!cv || !f) return;
      const c = cfg.current;
      const dpr = Math.min(window.devicePixelRatio || 1, 1.5);
      const W = Math.round(cv.clientWidth * dpr), H = Math.round(cv.clientHeight * dpr);
      const tr = trail.current;
      if (cv.width !== W || cv.height !== H) { cv.width = W; cv.height = H; tr.width = W; tr.height = H; }
      const ctx = cv.getContext("2d")!;
      const tctx = tr.getContext("2d")!;
      const sx = W / f.nx, sy = H / f.ny;

      const key = `${c.pcount}|${c.pmode}|${f.nx}x${f.ny}`;
      if (key !== lastKey) {
        parts.current.resize(c.pmode === "off" || c.pmode === "streamlines" ? 0 : c.pcount, f, c.pmode);
        tctx.clearRect(0, 0, W, H);
        lastKey = key;
      }
      if (fresh.current) {
        fresh.current = false;
        field.current.draw(f, c.view, c.gain, c.palette);
        if (c.pmode === "streamlines") {
          tctx.clearRect(0, 0, W, H);
          Particles.streamlines(tctx, f, sx, sy, Math.round(Math.min(140, Math.max(24, c.pcount / 80))), dpr);
        } else if (c.pmode !== "off") {
          parts.current.step(f, c.pmode, c.steps);
          tctx.globalCompositeOperation = "destination-out";
          const fade = (c.pmode === "smoke" ? 0.1 : 0.3) * (1.05 - c.trailLen);
          tctx.fillStyle = `rgba(0,0,0,${fade})`;
          tctx.fillRect(0, 0, W, H);
          tctx.globalCompositeOperation = "source-over";
          parts.current.draw(tctx, sx, sy, c.pmode);
        }
      }
      ctx.imageSmoothingEnabled = true;
      ctx.imageSmoothingQuality = "high";
      ctx.drawImage(field.current.canvas, 0, 0, W, H);
      if (c.pmode !== "off") ctx.drawImage(tr, 0, 0);
      solids.current.update(f, W, H);
      ctx.drawImage(solids.current.canvas, 0, 0);
      if (c.arrows) drawForces(ctx, f, sx, sy, dpr);
      drawOverlay(ctx, sx, sy, dpr, c);
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  }, []);

  const centroid = useRef({ key: "", x: 0, y: 0, n: 0 });
  function drawForces(ctx: CanvasRenderingContext2D, f: Frame, sx: number, sy: number, dpr: number) {
    if (!f.length) return;
    // środek ciężkości ciał liczony tylko przy zmianie geometrii
    const key = `${f.nx}x${f.ny}:${f.length}:${f.canUndo}:${f.canRedo}`;
    if (centroid.current.key !== key) {
      let X = 0, Y = 0, n = 0;
      for (let y = 0; y < f.ny; y++) for (let x = 0; x < f.nx; x++) if (f.solid[y * f.nx + x]) { X += x; Y += y; n++; }
      centroid.current = { key, x: n ? X / n : 0, y: n ? Y / n : 0, n };
    }
    const { x, y, n } = centroid.current;
    if (!n) return;
    const cx = x * sx, cy = y * sy;
    const k = 60 * dpr;
    const arrow = (dx: number, dy: number, color: string, label: string) => {
      const len = Math.hypot(dx, dy);
      if (len < 4 * dpr) return;
      const ex = cx + dx, ey = cy + dy, ux = dx / len, uy = dy / len, hs = 9 * dpr;
      ctx.strokeStyle = color; ctx.fillStyle = color; ctx.lineWidth = 2.5 * dpr;
      ctx.shadowColor = color; ctx.shadowBlur = 10 * dpr;
      ctx.beginPath(); ctx.moveTo(cx, cy); ctx.lineTo(ex - ux * hs * 0.6, ey - uy * hs * 0.6); ctx.stroke();
      ctx.beginPath(); ctx.moveTo(ex, ey);
      ctx.lineTo(ex - ux * hs - uy * hs * 0.55, ey - uy * hs + ux * hs * 0.55);
      ctx.lineTo(ex - ux * hs + uy * hs * 0.55, ey - uy * hs - ux * hs * 0.55);
      ctx.closePath(); ctx.fill();
      ctx.shadowBlur = 0;
      ctx.font = `600 ${11 * dpr}px 'IBM Plex Mono'`;
      ctx.fillText(label, ex + ux * 8 * dpr + 4 * dpr, ey + uy * 8 * dpr + 4 * dpr);
    };
    const clamp = (v: number) => Math.max(-3, Math.min(3, v));
    ctx.save();
    arrow(clamp(f.cd) * k, 0, "#f5a524", "D");
    arrow(0, -clamp(f.cl) * k, "#38bdf8", "L");
    ctx.beginPath(); ctx.arc(cx, cy, 3.5 * dpr, 0, Math.PI * 2); ctx.fillStyle = "#fff"; ctx.fill();
    ctx.restore();
  }

  function drawOverlay(ctx: CanvasRenderingContext2D, sx: number, sy: number, dpr: number, c: typeof cfg.current) {
    const d = drag.current;
    ctx.save();
    ctx.lineWidth = 1.5 * dpr;
    ctx.strokeStyle = "rgba(125,211,252,0.95)";
    ctx.fillStyle = "rgba(125,211,252,0.16)";
    ctx.setLineDash([6 * dpr, 4 * dpr]);
    if (d && c.tool !== "brush" && c.tool !== "erase") {
      const [x0, y0] = d.start, [x1, y1] = d.cur;
      ctx.beginPath();
      if (c.tool === "circle") {
        const r = Math.hypot(x1 - x0, y1 - y0);
        ctx.ellipse(x0 * sx, y0 * sy, r * sx, r * sy, 0, 0, Math.PI * 2);
      } else if (c.tool === "rect") {
        ctx.rect(x0 * sx, y0 * sy, (x1 - x0) * sx, (y1 - y0) * sy);
      } else {
        const poly = c.tool === "airfoil" ? airfoilFromDrag(d.start, d.cur, c.thick, c.camber) : d.pts;
        poly.forEach(([x, y], i) => (i ? ctx.lineTo(x * sx, y * sy) : ctx.moveTo(x * sx, y * sy)));
        ctx.closePath();
      }
      ctx.fill();
      ctx.stroke();
      if (c.tool === "airfoil") {
        ctx.setLineDash([2 * dpr, 4 * dpr]);
        ctx.strokeStyle = "rgba(255,255,255,0.5)";
        ctx.beginPath(); ctx.moveTo(x0 * sx, y0 * sy); ctx.lineTo(x1 * sx, y1 * sy); ctx.stroke();
      }
    }
    // obrys zaznaczonego / najechanego obiektu
    const bs = bodiesRef.current;
    if (bs && bs.nx === frame.current?.nx) {
      const outline = (b: BodyInfo, strong: boolean) => {
        ctx.save();
        ctx.setLineDash(strong ? [7 * dpr, 5 * dpr] : [3 * dpr, 4 * dpr]);
        ctx.lineDashOffset = strong ? -performance.now() / 40 : 0;
        ctx.lineWidth = (strong ? 1.8 : 1.2) * dpr;
        ctx.strokeStyle = strong ? "rgba(125,211,252,1)" : "rgba(255,255,255,0.55)";
        ctx.shadowColor = "rgba(56,189,248,0.8)";
        ctx.shadowBlur = strong ? 8 * dpr : 0;
        ctx.beginPath();
        const pad = 2;
        if (b.kind === "circle" && b.r) ctx.ellipse(b.x * sx, b.y * sy, (b.r + pad) * sx, (b.r + pad) * sy, 0, 0, Math.PI * 2);
        else if (b.outline) {
          b.outline.forEach(([x, y], i) => (i ? ctx.lineTo(x * sx, y * sy) : ctx.moveTo(x * sx, y * sy)));
          ctx.closePath();
        } else {
          const [x0, y0, x1, y1] = b.bbox;
          ctx.rect((x0 - pad) * sx, (y0 - pad) * sy, (x1 - x0 + 2 * pad) * sx, (y1 - y0 + 2 * pad) * sy);
        }
        ctx.stroke();
        if (strong) {
          ctx.setLineDash([]);
          ctx.fillStyle = "#e0f6ff";
          ctx.beginPath(); ctx.arc(b.x * sx, b.y * sy, 3.5 * dpr, 0, Math.PI * 2); ctx.fill();
          if (b.kind !== "circle") {
            ctx.font = `600 ${11 * dpr}px 'IBM Plex Mono'`;
            ctx.shadowBlur = 0;
            ctx.fillText(`${b.angle.toFixed(1)}°`, (b.bbox[2] + 4) * sx, (b.bbox[1]) * sy);
          }
        }
        ctx.restore();
      };
      const sel = bs.list.find((b) => b.id === (c.menuId ?? bs.selected));
      const hov = c.tool === "select" ? bs.list.find((b) => b.id === hoverBody.current) : undefined;
      if (hov && hov !== sel) outline(hov, false);
      if (sel && (c.tool === "select" || c.menuId)) outline(sel, true);
    }
    const h = hover.current;
    if (h && (c.tool === "brush" || c.tool === "erase")) {
      ctx.setLineDash([]);
      ctx.strokeStyle = c.tool === "erase" || d?.erase ? "rgba(251,113,133,0.95)" : "rgba(255,255,255,0.85)";
      ctx.beginPath();
      ctx.ellipse(h.g[0] * sx, h.g[1] * sy, c.brush * sx, c.brush * sy, 0, 0, Math.PI * 2);
      ctx.stroke();
    }
    ctx.restore();
  }

  // ---- interakcja
  const toGrid = (e: React.PointerEvent): Pt => {
    const f = frame.current!;
    const r = canvasRef.current!.getBoundingClientRect();
    return [((e.clientX - r.left) / r.width) * f.nx, ((e.clientY - r.top) / r.height) * f.ny];
  };

  const pickAt = (p: Pt): number | null => {
    const b = bodiesRef.current;
    if (!b) return null;
    const x = Math.floor(p[0]), y = Math.floor(p[1]);
    if (x < 0 || y < 0 || x >= b.nx || y >= b.ny) return null;
    const k = b.labels[y * b.nx + x];
    return k ? b.list[k - 1]?.id ?? null : null;
  };

  const flushStroke = () => {
    const d = drag.current;
    if (!d || d.pending.length === 0) return;
    // tylko pierwszy fragment pociągnięcia tworzy punkt cofania
    send({ type: "stroke", points: d.pending, radius: brush, mode: tool === "erase" || d.erase ? "erase" : "wall", continue: !!d.sent });
    d.sent = true;
    d.pending = [d.pending[d.pending.length - 1]];
  };

  const onDown = (e: React.PointerEvent) => {
    if (!frame.current) return;
    setPresetsOpen(false);
    const p = toGrid(e);
    const hit = pickAt(p);
    // prawy przycisk na obiekcie → menu opcji
    if (e.button === 2 && hit !== null) {
      send({ type: "select", id: hit });
      setMenu({ id: hit, x: e.clientX + 6, y: e.clientY + 6 });
      return;
    }
    setMenu(null);
    if (tool === "select" || (e.button === 0 && e.altKey)) {
      if (hit !== null) {
        e.currentTarget.setPointerCapture(e.pointerId);
        send({ type: "grab", x: p[0], y: p[1] });
        moving.current = { last: p, acc: [0, 0] };
        (e.currentTarget as HTMLElement).style.cursor = "grabbing";
      } else send({ type: "select", id: null });
      return;
    }
    if (e.button === 2 && tool !== "brush" && tool !== "erase") return;
    e.currentTarget.setPointerCapture(e.pointerId);
    drag.current = { start: p, pts: [p], cur: p, erase: e.button === 2, pending: [p] };
    if (tool === "brush" || tool === "erase") flushStroke();
  };

  const onMove = (e: React.PointerEvent) => {
    if (!frame.current) return;
    const p = toGrid(e);
    hover.current = { g: p, sx: e.clientX, sy: e.clientY };
    const mv = moving.current;
    if (mv) {
      mv.acc = [mv.acc[0] + p[0] - mv.last[0], mv.acc[1] + p[1] - mv.last[1]];
      mv.last = p;
      if (Math.abs(mv.acc[0]) + Math.abs(mv.acc[1]) >= 0.5) {
        send({ type: "drag", dx: mv.acc[0], dy: mv.acc[1] });
        mv.acc = [0, 0];
      }
      return;
    }
    if (tool === "select") {
      const hb = pickAt(p);
      hoverBody.current = hb;
      (e.currentTarget as HTMLElement).style.cursor = hb !== null ? "grab" : "default";
    }
    const d = drag.current;
    if (!d) return;
    d.cur = p;
    if (tool === "polygon") {
      const last = d.pts[d.pts.length - 1];
      if (Math.hypot(p[0] - last[0], p[1] - last[1]) > 0.8) d.pts.push(p);
    } else if (tool === "brush" || tool === "erase") {
      d.pending.push(p);
      if (d.pending.length >= 3) flushStroke();
    } else if (tool === "airfoil") {
      const dx = p[0] - d.start[0], dy = p[1] - d.start[1];
      setGhost(`cięciwa ${Math.hypot(dx, dy).toFixed(0)}   α ${((Math.atan2(dy, dx) * 180) / Math.PI).toFixed(1)}°`);
    } else if (tool === "circle") {
      setGhost(`r ${Math.hypot(p[0] - d.start[0], p[1] - d.start[1]).toFixed(0)}`);
    } else if (tool === "rect") {
      setGhost(`${Math.abs(p[0] - d.start[0]).toFixed(0)} × ${Math.abs(p[1] - d.start[1]).toFixed(0)}`);
    }
  };

  const onUp = (e?: React.PointerEvent) => {
    if (moving.current) {
      const mv = moving.current;
      if (mv.acc[0] || mv.acc[1]) send({ type: "drag", dx: mv.acc[0], dy: mv.acc[1] });
      send({ type: "release" });
      moving.current = null;
      if (e) (e.currentTarget as HTMLElement).style.cursor = "grab";
      return;
    }
    const d = drag.current;
    if (!d) return;
    const [x0, y0] = d.start, [x1, y1] = d.cur;
    if (tool === "brush" || tool === "erase") flushStroke();
    else if (tool === "circle") {
      const r = Math.hypot(x1 - x0, y1 - y0);
      if (r > 1) send({ type: "circle", x: x0, y: y0, r });
    } else if (tool === "rect") {
      if (Math.abs(x1 - x0) > 1 && Math.abs(y1 - y0) > 1) send({ type: "rect", x0, y0, x1, y1 });
    } else if (tool === "polygon") {
      if (d.pts.length > 3) send({ type: "polygon", points: d.pts });
    } else if (tool === "airfoil") {
      const chord = Math.hypot(x1 - x0, y1 - y0);
      if (chord > 6) send({ type: "airfoil", x: x0, y: y0, chord, angle: Math.atan2(y1 - y0, x1 - x0), thickness: thick / 100, camber: camber / 100 });
    }
    drag.current = null;
    setGhost(null);
  };

  // kółko myszy obraca obiekt pod kursorem (lub zaznaczony)
  const onWheel = (e: React.WheelEvent) => {
    if (tool !== "select" && !menu) return;
    const target = hoverBody.current ?? bodiesRef.current?.selected ?? null;
    if (target === null) return;
    const now = performance.now();
    const step = (e.shiftKey ? 0.5 : 2) * Math.sign(e.deltaY);
    send({ type: now - wheelAt.current > 600 ? "rotate_begin" : "rotate", id: target, delta: step });
    wheelAt.current = now;
  };

  const resetFlow = useCallback(() => { send({ type: "reset" }); history.current = []; notify("Przepływ zresetowany"); }, [send, notify]);
  const clearAll = useCallback(() => { send({ type: "clear" }); history.current = []; notify("Usunięto przeszkody"); }, [send, notify]);
  const loadPreset = (id: string, label: string) => {
    send({ type: "preset", name: id });
    history.current = [];
    setPresetsOpen(false);
    notify(label);
  };
  const undo = useCallback(() => { send({ type: "undo" }); notify("Cofnięto"); }, [send, notify]);
  const redo = useCallback(() => { send({ type: "redo" }); notify("Ponowiono"); }, [send, notify]);
  const screenshot = useCallback(() => {
    canvasRef.current?.toBlob((b) => {
      if (!b) return;
      const a = document.createElement("a");
      a.href = URL.createObjectURL(b);
      a.download = `aeroflow-${new Date().toISOString().slice(0, 19).replace(/[:T]/g, "-")}.png`;
      a.click();
      setTimeout(() => URL.revokeObjectURL(a.href), 1000);
      notify("Zapisano zrzut ekranu");
    });
  }, [notify]);
  const toggleFullscreen = () => {
    if (document.fullscreenElement) document.exitFullscreen();
    else document.documentElement.requestFullscreen?.();
  };

  // ---- skróty
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.target as HTMLElement).tagName === "INPUT") return;
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "z") { e.preventDefault(); (e.shiftKey ? redo : undo)(); return; }
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "y") { e.preventDefault(); redo(); return; }
      const sel = bodiesRef.current?.selected ?? null;
      if (sel !== null) {
        if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "d") { e.preventDefault(); send({ type: "duplicate", id: sel }); return; }
        if (e.key === "Delete" || e.key === "Backspace") { send({ type: "delete", id: sel }); setMenu(null); notify("Usunięto obiekt"); return; }
        const arrows: Record<string, Pt> = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] };
        if (arrows[e.key]) {
          e.preventDefault();
          const k = e.shiftKey ? 5 : 1;
          send({ type: "nudge", id: sel, dx: arrows[e.key][0] * k, dy: arrows[e.key][1] * k, continue: e.repeat });
          return;
        }
      }
      if (e.ctrlKey || e.metaKey || e.altKey) return;
      const t = TOOLS.find((t) => t.key === e.key);
      if (t) { setTool(t.id); return; }
      switch (e.key.toLowerCase()) {
        case " ": e.preventDefault(); setPaused((p) => !p); break;
        case "v": setView((v) => VIEWS[(VIEWS.findIndex((x) => x.id === v) + 1) % VIEWS.length].id); break;
        case "p": setPmode(nextPmode); break;
        case "a": setArrows((v) => !v); break;
        case "k": screenshot(); break;
        case "c": clearAll(); break;
        case "r": resetFlow(); break;
        case "s": setWin((w) => ({ ...w, settings: !w.settings })); break;
        case "g": setWin((w) => ({ ...w, chart: !w.chart })); break;
        case "h": setZen((z) => !z); break;
        case "f": toggleFullscreen(); break;
        case "?": case "/": setWin((w) => ({ ...w, help: !w.help })); break;
        case "escape": setWin({ settings: false, chart: false, help: false }); setPresetsOpen(false); setMenu(null); send({ type: "select", id: null }); break;
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [clearAll, resetFlow, setView, setPmode, setArrows, undo, redo, screenshot, send, notify]);

  const ld = stats && Math.abs(stats.cd) > 1e-3 ? stats.cl / stats.cd : 0;
  const hasBody = !!stats?.length;
  const toolInfo = TOOLS.find((t) => t.id === tool)!;
  const hasToolOpts = tool === "airfoil" || tool === "brush" || tool === "erase";

  return (
    <div className={`app ${zen ? "zen" : ""}`}>
      <canvas
        ref={canvasRef}
        className={`sim tool-${tool}`}
        onPointerDown={onDown}
        onPointerMove={onMove}
        onPointerUp={onUp}
        onPointerCancel={onUp}
        onWheel={onWheel}
        onPointerLeave={() => { hover.current = null; setProbe(null); }}
        onContextMenu={(e) => e.preventDefault()}
      />
      <div className="vignette" />

      <div className="hud brand ui">
        <div className="logo"><Icon name="logo" /></div>
        <b className="brand-name">AeroFlow</b>
        {paused && <span className="paused-tag">pauza</span>}
      </div>

      <div className="hud kpis ui" onClick={() => setWin((w) => ({ ...w, chart: !w.chart }))} title="Pokaż wykres sił (G)">
        <Kpi label="Re" value={stats ? fmtBig(stats.re) : "—"} />
        <Kpi label="Cd" value={hasBody ? fmt(stats!.cd, 3) : "—"} tone="cd" />
        <Kpi label="Cl" value={hasBody ? fmt(stats!.cl, 3) : "—"} tone="cl" />
        <Kpi label="L/D" value={hasBody ? fmt(ld, 1) : "—"} />
      </div>

      <div className="hud perf ui">
        <span><Icon name="cpu" />{stats ? `${stats.mlups.toFixed(0)} MLUPS` : "—"}</span>
        <span>{stats ? `${stats.fps.toFixed(0)} fps` : ""}</span>
        <span>{stats ? `${stats.nx}×${stats.ny}` : ""}</span>
      </div>

      <nav className="dock tools ui">
        {TOOLS.map((t) => (
          <DockButton key={t.id} icon={t.id} label={t.label} kbd={t.key} active={tool === t.id} onClick={() => setTool(t.id)} tipSide="right" />
        ))}
        <div className="dock-sep" />
        <DockButton icon="undo" label="Cofnij" kbd="Ctrl+Z" onClick={undo} disabled={!stats?.canUndo} tipSide="right" />
        <DockButton icon="redo" label="Ponów" kbd="Ctrl+Y" onClick={redo} disabled={!stats?.canRedo} tipSide="right" />
        <DockButton icon="trash" label="Wyczyść wszystko" kbd="C" onClick={clearAll} tipSide="right" />
      </nav>

      {hasToolOpts && (
        <div className="flyout ui" key={tool}>
          <div className="flyout-head"><Icon name={tool} />{toolInfo.label}</div>
          {tool === "airfoil" ? (
            <>
              <Slider label="Grubość" value={thick} min={4} max={30} step={1} onChange={setThick} display={`${thick}%`} />
              <Slider label="Wygięcie" value={camber} min={0} max={9} step={1} onChange={setCamber} display={`${camber}%`} />
              <div className="naca-row">
                <svg viewBox="-2 -12 104 24" className="naca-preview">
                  <path d={"M" + nacaPolygon(0, 0, 100, 0, thick / 100, camber / 100).map(([x, y]) => `${x.toFixed(1)},${y.toFixed(1)}`).join("L") + "Z"} />
                </svg>
                <code>NACA {camber}4{String(thick).padStart(2, "0")}</code>
              </div>
            </>
          ) : (
            <Slider label="Rozmiar" value={brush} min={1} max={14} step={0.5} onChange={setBrush} display={`${brush}`} />
          )}
        </div>
      )}

      <div className="bottom ui">
        <div className="hint"><Icon name="info" />{toolInfo.hint}</div>
        <div className="dock main">
          <DockButton icon={paused ? "play" : "pause"} label={paused ? "Wznów" : "Pauza"} kbd="Spacja" onClick={() => setPaused((p) => !p)} accent />
          <DockButton icon="step" label="Jeden krok" onClick={() => send({ type: "step" })} disabled={!paused} />
          <DockButton icon="reset" label="Reset przepływu" kbd="R" onClick={resetFlow} />
          <div className="dock-sep v" />
          <Segmented options={VIEWS.map((v) => ({ ...v, label: "", title: `${v.label} (V)` }))} value={view} onChange={setView} small />
          <div className="dock-sep v" />
          <DockButton icon={PMODES.find((m) => m.id === pmode)!.icon} label={`Znaczniki: ${PMODES.find((m) => m.id === pmode)!.label}`} kbd="P"
            active={pmode !== "off"} onClick={() => setPmode(nextPmode)} />
          <DockButton icon="arrows" label="Wektory sił" kbd="A" active={arrows} onClick={() => setArrows((v) => !v)} />
          <DockButton icon="layers" label="Scenariusze" active={presetsOpen} onClick={() => setPresetsOpen((o) => !o)} />
          <DockButton icon="chart" label="Wykres sił" kbd="G" active={win.chart} onClick={() => setWin((w) => ({ ...w, chart: !w.chart }))} />
          <DockButton icon="gear" label="Ustawienia" kbd="S" active={win.settings} onClick={() => setWin((w) => ({ ...w, settings: !w.settings }))} />
          <div className="dock-sep v" />
          <DockButton icon="help" label="Skróty" kbd="?" active={win.help} onClick={() => setWin((w) => ({ ...w, help: !w.help }))} />
          <DockButton icon="camera" label="Zrzut ekranu" kbd="K" onClick={screenshot} />
          <DockButton icon="eyeoff" label="Ukryj interfejs" kbd="H" onClick={() => setZen(true)} />
          <DockButton icon="fullscreen" label="Pełny ekran" kbd="F" onClick={toggleFullscreen} />
        </div>
        <div className="view-label">{VIEWS.find((v) => v.id === view)!.label}<Legend view={view} palette={palette} /></div>
      </div>

      {presetsOpen && (
        <div className="presets-pop ui">
          {PRESETS.map((p) => (
            <button key={p.id} className="preset" onClick={() => loadPreset(p.id, p.label)}>
              <svg viewBox="0 0 60 40">{p.thumb}</svg>
              <span>{p.label}</span>
            </button>
          ))}
        </div>
      )}

      <div className="ui">
        <FloatingWindow id="settings" title="Ustawienia" icon="gear" open={win.settings} onClose={() => setWin((w) => ({ ...w, settings: false }))}
          initial={{ x: window.innerWidth - 360, y: 80 }} width={330}>
          <Segmented options={[{ id: "flow", label: "Przepływ" }, { id: "vis", label: "Wygląd" }, { id: "sim", label: "Solver" }]} value={settingsTab} onChange={setSettingsTab} />
          <div className="tab-body" key={settingsTab}>
            {settingsTab === "flow" && (
              <>
                <Slider label="Prędkość wlotu" value={u} min={0.02} max={0.16} step={0.005} onChange={setU} display={`${(u / 0.1).toFixed(2)}×`} hint="Prędkość w jednostkach sieci; powyżej ~0,15 rośnie ściśliwość" />
                <Slider label="Lepkość" value={nuS} min={0} max={1} step={0.01} onChange={setNuS} display={nuFromSlider(nuS).toFixed(4)} hint="Mniejsza lepkość = wyższa liczba Reynoldsa i bardziej burzliwy ślad" />
                <div className="readout">
                  <div><span>Reynolds</span><b>{stats ? fmtBig(stats.re) : "—"}</b></div>
                  <div><span>L<sub>ref</sub></span><b>{stats?.length ? stats.length.toFixed(0) : "—"}</b></div>
                  <div><span>Mach</span><b>{(u * Math.sqrt(3)).toFixed(2)}</b></div>
                </div>
                <div className="quick">
                  {[{ l: "Laminarny", nu: 0.04 }, { l: "Przejściowy", nu: 0.01 }, { l: "Burzliwy", nu: 0.002 }].map((q) => (
                    <button key={q.l} className="chip" onClick={() => setNuS(sliderFromNu(q.nu))}>{q.l}</button>
                  ))}
                </div>
              </>
            )}
            {settingsTab === "vis" && (
              <>
                <div className="field-label">Pole</div>
                <Segmented options={VIEWS} value={view} onChange={setView} />
                {view === "speed" && (
                  <>
                    <div className="field-label">Paleta</div>
                    <div className="palettes">
                      {(["ocean", "arctic", "abyss"] as Palette[]).map((p) => (
                        <button key={p} className={`pal ${palette === p ? "on" : ""}`} onClick={() => setPalette(p)}>
                          <i style={{ background: GRADS[p] }} /><span>{{ ocean: "Ocean", arctic: "Arktyka", abyss: "Głębia" }[p]}</span>
                        </button>
                      ))}
                    </div>
                  </>
                )}
                <Slider label="Kontrast" value={gain} min={0.3} max={3} step={0.05} onChange={setGain} display={`${gain.toFixed(2)}×`} />
                <div className="field-label">Znaczniki</div>
                <Segmented options={PMODES.map((m) => ({ ...m, label: "", title: m.label }))} value={pmode} onChange={setPmode} />
                <div className="seg-caption">{PMODES.find((m) => m.id === pmode)!.label}</div>
                {pmode === "streamlines" && (
                  <Slider label="Gęstość linii" value={pcount} min={500} max={15000} step={500} onChange={setPcount} display={String(Math.round(Math.min(140, Math.max(24, pcount / 80))))} />
                )}
                {(pmode === "tracers" || pmode === "smoke") && (
                  <>
                    <Slider label="Liczba" value={pcount} min={500} max={15000} step={500} onChange={setPcount} display={pcount.toLocaleString("pl")} />
                    <Slider label="Długość śladu" value={trailLen} min={0} max={1} step={0.05} onChange={setTrailLen} display={`${Math.round(trailLen * 100)}%`} />
                  </>
                )}
                <Toggle label="Wektory sił na ciele" value={arrows} onChange={setArrows} />
                <Toggle label="Sonda pod kursorem" value={probeOn} onChange={setProbeOn} />
              </>
            )}
            {settingsTab === "sim" && (
              <>
                <div className="field-label">Rozdzielczość siatki</div>
                <Segmented options={RESOLUTIONS} value={res} onChange={setRes} />
                <Slider label="Kroki na klatkę" value={steps} min={1} max={30} step={1} onChange={setSteps} display={String(steps)} hint="Więcej kroków = szybsza symulacja kosztem CPU" />
                <div className="readout">
                  <div><span>Przepustowość</span><b>{stats ? `${stats.mlups.toFixed(0)} MLUPS` : "—"}</b></div>
                  <div><span>Krok</span><b>{stats ? stats.step.toLocaleString("pl") : "—"}</b></div>
                  <div><span>Siatka</span><b>{stats ? `${stats.nx}×${stats.ny}` : "—"}</b></div>
                </div>
                <p className="note">D2Q9 Lattice-Boltzmann, kolizja BGK z modelem Smagorinsky’ego (LES), bounce-back na przeszkodach, ściany poślizgowe, gąbka na wylocie. Siły metodą wymiany pędu.</p>
              </>
            )}
          </div>
        </FloatingWindow>

        <FloatingWindow id="chart" title="Współczynniki aerodynamiczne" icon="chart" open={win.chart} onClose={() => setWin((w) => ({ ...w, chart: false }))}
          initial={{ x: window.innerWidth - 400, y: window.innerHeight - 330 }} width={370}>
          <div className="chart-legend">
            <span><i className="cl" />Cl <b>{hasBody ? fmt(stats!.cl, 3) : "—"}</b></span>
            <span><i className="cd" />Cd <b>{hasBody ? fmt(stats!.cd, 3) : "—"}</b></span>
            <span>L/D <b>{hasBody ? fmt(ld, 2) : "—"}</b></span>
          </div>
          <canvas ref={chartRef} className="chart" />
        </FloatingWindow>

        <FloatingWindow id="help" title="Skróty klawiszowe" icon="help" open={win.help} onClose={() => setWin((w) => ({ ...w, help: false }))}
          initial={{ x: window.innerWidth / 2 - 170, y: 110 }} width={340}>
          <div className="keys">
            {[["1–7", "narzędzia"], ["PPM", "opcje obiektu"], ["Del", "usuń zaznaczony"], ["Strzałki", "przesuń zaznaczony"], ["Kółko", "obróć obiekt"], ["Ctrl+D", "duplikuj"], ["Spacja", "pauza / wznów"], ["V", "następne pole"], ["P", "tryb znaczników"], ["R", "reset przepływu"], ["C", "wyczyść przeszkody"],
              ["Ctrl+Z", "cofnij"], ["Ctrl+Y", "ponów"], ["A", "wektory sił"], ["K", "zrzut ekranu"],
              ["S", "ustawienia"], ["G", "wykres sił"], ["H", "ukryj interfejs"], ["F", "pełny ekran"], ["Alt+LPM", "przesuń dowolnym narzędziem"], ["Esc", "zamknij / odznacz"]].map(([k, d]) => (
              <div key={k}><kbd>{k}</kbd><span>{d}</span></div>
            ))}
          </div>
        </FloatingWindow>
      </div>

      {menu && bodies?.list.find((b) => b.id === menu.id) && (
        <ContextMenu key={menu.id} body={bodies.list.find((b) => b.id === menu.id)!} x={menu.x} y={menu.y} send={send} onClose={() => setMenu(null)} />
      )}
      {ghost && <div className="ghost">{ghost}</div>}
      {probe && <div className="probe" style={{ left: probe.sx + 16, top: probe.sy + 16 }}>{probe.text}</div>}
      {toast && <div className="toast" key={toast.id}>{toast.text}</div>}
      {zen && <button className="zen-exit" onClick={() => setZen(false)}><Icon name="eye" />Pokaż interfejs <kbd>H</kbd></button>}
      {status !== "open" && (
        <div className="offline">
          <div className="logo big"><Icon name="logo" /></div>
          <p>{status === "connecting" ? "Łączenie z solverem…" : "Brak połączenia z serwerem"}</p>
          {status === "closed" && <code>python server/run.py</code>}
        </div>
      )}
    </div>
  );
}

function airfoilFromDrag(a: Pt, b: Pt, thick: number, camber: number) {
  return nacaPolygon(a[0], a[1], Math.hypot(b[0] - a[0], b[1] - a[1]), Math.atan2(b[1] - a[1], b[0] - a[0]), thick / 100, camber / 100);
}

const fmt = (v: number, d: number) => (Number.isFinite(v) ? v.toFixed(d) : "—");
const fmtBig = (v: number) => (!Number.isFinite(v) || v === 0 ? "—" : v >= 1000 ? `${(v / 1000).toFixed(1)}k` : v.toFixed(0));

function drawChart(cv: HTMLCanvasElement | null, h: { cd: number; cl: number }[]) {
  if (!cv) return;
  const dpr = Math.min(window.devicePixelRatio || 1, 2);
  const W = cv.clientWidth, H = cv.clientHeight;
  if (!W) return;
  cv.width = W * dpr;
  cv.height = H * dpr;
  const ctx = cv.getContext("2d")!;
  ctx.scale(dpr, dpr);
  ctx.font = "11px 'IBM Plex Mono'";
  if (h.length < 2) {
    ctx.fillStyle = "rgba(148,163,184,0.7)";
    ctx.fillText("Dodaj przeszkodę, aby zmierzyć siły", 8, H / 2 + 4);
    return;
  }
  let lo = Infinity, hi = -Infinity;
  for (const p of h) { lo = Math.min(lo, p.cd, p.cl); hi = Math.max(hi, p.cd, p.cl); }
  lo = Math.min(lo, 0); hi = Math.max(hi, 0.1);
  const pad = (hi - lo) * 0.12; lo -= pad; hi += pad;
  const Y = (v: number) => H - 4 - ((v - lo) / (hi - lo)) * (H - 8);
  const X = (i: number) => 30 + (i / 299) * (W - 34);
  ctx.strokeStyle = "rgba(148,163,184,0.12)";
  ctx.fillStyle = "rgba(148,163,184,0.6)";
  for (let k = 0; k <= 3; k++) {
    const v = lo + ((hi - lo) * k) / 3;
    ctx.beginPath(); ctx.moveTo(30, Y(v)); ctx.lineTo(W, Y(v)); ctx.stroke();
    ctx.fillText(v.toFixed(1), 0, Y(v) + 4);
  }
  ctx.strokeStyle = "rgba(148,163,184,0.35)";
  ctx.setLineDash([3, 3]);
  ctx.beginPath(); ctx.moveTo(30, Y(0)); ctx.lineTo(W, Y(0)); ctx.stroke();
  ctx.setLineDash([]);
  const line = (key: "cd" | "cl", color: string) => {
    const grad = ctx.createLinearGradient(0, 0, 0, H);
    grad.addColorStop(0, color + "40");
    grad.addColorStop(1, color + "00");
    ctx.beginPath();
    h.forEach((p, i) => (i ? ctx.lineTo(X(i), Y(p[key])) : ctx.moveTo(X(i), Y(p[key]))));
    ctx.strokeStyle = color;
    ctx.lineWidth = 1.75;
    ctx.stroke();
    ctx.lineTo(X(h.length - 1), Y(0));
    ctx.lineTo(X(0), Y(0));
    ctx.fillStyle = grad;
    ctx.fill();
    ctx.beginPath();
    ctx.arc(X(h.length - 1), Y(h[h.length - 1][key]), 3, 0, Math.PI * 2);
    ctx.fillStyle = color;
    ctx.fill();
  };
  line("cd", "#f59e0b");
  line("cl", "#38bdf8");
}

function Kpi({ label, value, tone }: { label: string; value: string; tone?: string }) {
  return (
    <div className={`kpi ${tone ?? ""}`}>
      <span>{label}</span>
      <b>{value}</b>
    </div>
  );
}

const GRADS: Record<string, string> = {
  ocean: "linear-gradient(90deg,#020617,#0a2a5e,#1463c7,#3fb8f5,#a8ecff,#f4fdff)",
  arctic: "linear-gradient(90deg,#0b1220,#1b3b6f,#6aa9e9,#dbeeff,#fff)",
  abyss: "linear-gradient(90deg,#000,#00334d,#00a3cc,#7ff7ff,#fff)",
  smoke: "linear-gradient(90deg,#030a1c,#0b2553,#2a7fd4,#a5e4ff,#fff)",
  vorticity: "linear-gradient(90deg,#7dd3fc,#1d6fd8,#08122c,#2a0f2e,#e0487a,#ffd1a6)",
  pressure: "linear-gradient(90deg,#1e3a8a,#2f7ff0,#0b1430,#f59e0b,#fff4c2)",
};

function Legend({ view, palette }: { view: ViewMode; palette: Palette }) {
  if (view === "none") return null;
  const labels = { speed: ["0", "2 U∞"], vorticity: ["−ω", "+ω"], pressure: ["p−", "p+"], smoke: ["czyste", "dym"] }[view];
  return (
    <span className="legend">
      <em>{labels[0]}</em>
      <i style={{ background: view === "speed" ? GRADS[palette] : GRADS[view] }} />
      <em>{labels[1]}</em>
    </span>
  );
}
