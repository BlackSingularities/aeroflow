import { useCallback, useEffect, useRef, useState } from "react";
import { FieldRenderer, ParticleMode, Particles, ViewMode } from "./render";
import { Frame, SimSocket, U_SCALE, P_SCALE, nacaPolygon } from "./sim";
import { Icon } from "./icons";

type Tool = "brush" | "erase" | "airfoil" | "polygon" | "circle" | "rect";
type Pt = [number, number];

const TOOLS: { id: Tool; label: string; key: string; hint: string }[] = [
  { id: "airfoil", label: "Skrzydło", key: "1", hint: "Przeciągnij od noska do krawędzi spływu — długość to cięciwa, kierunek to kąt natarcia." },
  { id: "polygon", label: "Kształt", key: "2", hint: "Narysuj obrys dowolnego kształtu — po puszczeniu zostanie wypełniony." },
  { id: "brush", label: "Pędzel", key: "3", hint: "Maluj ściany swobodnie. Prawy przycisk myszy wymazuje." },
  { id: "circle", label: "Koło", key: "4", hint: "Przeciągnij od środka, aby ustawić promień." },
  { id: "rect", label: "Prostokąt", key: "5", hint: "Przeciągnij po przekątnej." },
  { id: "erase", label: "Gumka", key: "6", hint: "Usuwaj przeszkody." },
];

const PRESETS = [
  { id: "airfoil", label: "Profil NACA" },
  { id: "cylinder", label: "Walec (Kármán)" },
  { id: "tandem", label: "Tandem" },
  { id: "plate", label: "Płyta" },
  { id: "grid", label: "Krata" },
  { id: "step", label: "Uskok" },
  { id: "empty", label: "Pusty tunel" },
];

const VIEWS: { id: ViewMode; label: string }[] = [
  { id: "speed", label: "Prędkość" },
  { id: "vorticity", label: "Wirowość" },
  { id: "pressure", label: "Ciśnienie" },
  { id: "none", label: "Brak" },
];

const nuFromSlider = (s: number) => 0.0008 * Math.pow(0.05 / 0.0008, s);
const sliderFromNu = (nu: number) => Math.log(nu / 0.0008) / Math.log(0.05 / 0.0008);

interface Stats { cd: number; cl: number; re: number; fps: number; mlups: number; step: number; length: number; nx: number; ny: number }
interface Probe { x: number; y: number; speed: number; p: number; solid: boolean }

export default function App() {
  const [tool, setTool] = useState<Tool>("airfoil");
  const [view, setView] = useState<ViewMode>("speed");
  const [pmode, setPmode] = useState<ParticleMode>("tracers");
  const [pcount, setPcount] = useState(3000);
  const [brush, setBrush] = useState(3);
  const [u, setU] = useState(0.1);
  const [nuS, setNuS] = useState(sliderFromNu(0.004));
  const [steps, setSteps] = useState(10);
  const [gain, setGain] = useState(1);
  const [paused, setPaused] = useState(false);
  const [thick, setThick] = useState(12);
  const [camber, setCamber] = useState(4);
  const [status, setStatus] = useState<"connecting" | "open" | "closed">("connecting");
  const [stats, setStats] = useState<Stats | null>(null);
  const [probe, setProbe] = useState<Probe | null>(null);
  const [ghost, setGhost] = useState<string | null>(null);

  const sock = useRef<SimSocket>();
  const frame = useRef<Frame | null>(null);
  const fresh = useRef(false);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const chartRef = useRef<HTMLCanvasElement>(null);
  const field = useRef(new FieldRenderer());
  const parts = useRef(new Particles());
  const trail = useRef(document.createElement("canvas"));
  const history = useRef<{ cd: number; cl: number }[]>([]);
  const drag = useRef<{ start: Pt; pts: Pt[]; cur: Pt; erase: boolean; pending: Pt[] } | null>(null);
  const hover = useRef<Pt | null>(null);
  const cfg = useRef({ view, pmode, pcount, gain, tool, brush, thick, camber, steps });
  cfg.current = { view, pmode, pcount, gain, tool, brush, thick, camber, steps };

  const send = useCallback((c: { type: string } & Record<string, unknown>) => sock.current?.send(c), []);

  // ---- połączenie
  useEffect(() => {
    const s = new SimSocket();
    s.onStatus = setStatus;
    s.onFrame = (f) => {
      frame.current = f;
      fresh.current = true;
    };
    s.connect();
    sock.current = s;
    return () => s.close();
  }, []);

  // ---- parametry → serwer
  useEffect(() => send({ type: "params", u, nu: nuFromSlider(nuS), steps, paused }), [u, nuS, steps, paused, send, status]);

  // ---- statystyki (5 Hz) + wykres
  useEffect(() => {
    const id = setInterval(() => {
      const f = frame.current;
      if (!f) return;
      setStats({ cd: f.cd, cl: f.cl, re: f.re, fps: f.fps, mlups: f.mlups, step: f.step, length: f.length, nx: f.nx, ny: f.ny });
      const h = history.current;
      if (f.length > 0 && !paused) {
        h.push({ cd: f.cd, cl: f.cl });
        if (h.length > 300) h.shift();
      }
      drawChart(chartRef.current, h);
      const hv = hover.current;
      if (hv) {
        const x = Math.floor(hv[0]), y = Math.floor(hv[1]);
        if (x >= 0 && y >= 0 && x < f.nx && y < f.ny) {
          const i = y * f.nx + x;
          const sp = Math.hypot(f.ux[i], f.uy[i]) * U_SCALE;
          setProbe({ x, y, speed: sp / Math.max(f.u, 1e-6), p: (f.p[i] * P_SCALE) / (0.5 * f.u * f.u), solid: !!f.solid[i] });
        }
      } else setProbe(null);
    }, 200);
    return () => clearInterval(id);
  }, [paused]);

  // ---- pętla renderująca
  useEffect(() => {
    let raf = 0;
    let lastCount = -1, lastMode: ParticleMode | "" = "";
    const loop = () => {
      raf = requestAnimationFrame(loop);
      const cv = canvasRef.current, f = frame.current;
      if (!cv || !f) return;
      const c = cfg.current;
      const rect = cv.getBoundingClientRect();
      const dpr = Math.min(window.devicePixelRatio || 1, 2);
      const W = Math.round(rect.width * dpr), H = Math.round(rect.height * dpr);
      const tr = trail.current;
      if (cv.width !== W || cv.height !== H) {
        cv.width = W; cv.height = H; tr.width = W; tr.height = H;
      }
      const ctx = cv.getContext("2d")!;
      const tctx = tr.getContext("2d")!;
      const sx = W / f.nx, sy = H / f.ny;

      if (c.pcount !== lastCount || c.pmode !== lastMode) {
        parts.current.resize(c.pmode === "off" ? 0 : c.pcount, f, c.pmode);
        tctx.clearRect(0, 0, W, H);
        lastCount = c.pcount; lastMode = c.pmode;
      }
      if (fresh.current) {
        fresh.current = false;
        field.current.draw(f, c.view, c.gain);
        if (c.pmode !== "off") {
          parts.current.step(f, c.pmode, c.steps);
          tctx.globalCompositeOperation = "destination-out";
          tctx.fillStyle = `rgba(0,0,0,${c.pmode === "smoke" ? 0.05 : 0.14})`;
          tctx.fillRect(0, 0, W, H);
          tctx.globalCompositeOperation = "source-over";
          tctx.save();
          tctx.lineWidth = dpr;
          parts.current.draw(tctx, sx, sy, c.pmode);
          tctx.restore();
        }
      }
      ctx.imageSmoothingEnabled = true;
      ctx.imageSmoothingQuality = "high";
      ctx.drawImage(field.current.canvas, 0, 0, W, H);
      if (c.pmode !== "off") ctx.drawImage(tr, 0, 0);
      drawOverlay(ctx, sx, sy, dpr, c);
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  }, []);

  function drawOverlay(ctx: CanvasRenderingContext2D, sx: number, sy: number, dpr: number, c: typeof cfg.current) {
    const d = drag.current;
    ctx.save();
    ctx.lineWidth = 1.5 * dpr;
    ctx.strokeStyle = "rgba(125,211,252,0.95)";
    ctx.fillStyle = "rgba(125,211,252,0.18)";
    ctx.setLineDash([6 * dpr, 4 * dpr]);
    if (d) {
      const [x0, y0] = d.start, [x1, y1] = d.cur;
      ctx.beginPath();
      if (c.tool === "circle") {
        ctx.ellipse(x0 * sx, y0 * sy, Math.hypot(x1 - x0, y1 - y0) * sx, Math.hypot(x1 - x0, y1 - y0) * sy, 0, 0, Math.PI * 2);
      } else if (c.tool === "rect") {
        ctx.rect(x0 * sx, y0 * sy, (x1 - x0) * sx, (y1 - y0) * sy);
      } else if (c.tool === "airfoil") {
        const poly = airfoilFromDrag(d.start, d.cur, c.thick, c.camber);
        poly.forEach(([x, y], i) => (i ? ctx.lineTo(x * sx, y * sy) : ctx.moveTo(x * sx, y * sy)));
        ctx.closePath();
      } else if (c.tool === "polygon") {
        d.pts.forEach(([x, y], i) => (i ? ctx.lineTo(x * sx, y * sy) : ctx.moveTo(x * sx, y * sy)));
        ctx.closePath();
      }
      if (c.tool !== "brush" && c.tool !== "erase") {
        ctx.fill();
        ctx.stroke();
      }
    }
    const h = hover.current;
    if (h && (c.tool === "brush" || c.tool === "erase")) {
      ctx.setLineDash([]);
      ctx.strokeStyle = c.tool === "erase" || d?.erase ? "rgba(251,113,133,0.9)" : "rgba(255,255,255,0.8)";
      ctx.beginPath();
      ctx.ellipse(h[0] * sx, h[1] * sy, c.brush * sx, c.brush * sy, 0, 0, Math.PI * 2);
      ctx.stroke();
    }
    ctx.restore();
  }

  // ---- mysz → siatka
  const toGrid = (e: React.PointerEvent): Pt => {
    const f = frame.current!;
    const r = canvasRef.current!.getBoundingClientRect();
    return [((e.clientX - r.left) / r.width) * f.nx, ((e.clientY - r.top) / r.height) * f.ny];
  };

  const flushStroke = () => {
    const d = drag.current;
    if (!d || d.pending.length === 0) return;
    const erase = tool === "erase" || d.erase;
    send({ type: "stroke", points: d.pending, radius: brush, mode: erase ? "erase" : "wall" });
    d.pending = [d.pending[d.pending.length - 1]];
  };

  const onDown = (e: React.PointerEvent) => {
    if (!frame.current) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    const p = toGrid(e);
    drag.current = { start: p, pts: [p], cur: p, erase: e.button === 2, pending: [p] };
    if (tool === "brush" || tool === "erase") flushStroke();
  };

  const onMove = (e: React.PointerEvent) => {
    if (!frame.current) return;
    const p = toGrid(e);
    hover.current = p;
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
      setGhost(`cięciwa ${Math.hypot(dx, dy).toFixed(0)} · α = ${((Math.atan2(dy, dx) * 180) / Math.PI).toFixed(1)}°`);
    }
  };

  const onUp = () => {
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

  // ---- skróty klawiszowe
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.target as HTMLElement).tagName === "INPUT") return;
      const t = TOOLS.find((t) => t.key === e.key);
      if (t) setTool(t.id);
      else if (e.code === "Space") { e.preventDefault(); setPaused((p) => !p); }
      else if (e.key === "v") setView((v) => VIEWS[(VIEWS.findIndex((x) => x.id === v) + 1) % VIEWS.length].id);
      else if (e.key === "c") send({ type: "clear" });
      else if (e.key === "r") { send({ type: "reset" }); history.current = []; }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [send]);

  const aspect = stats ? `${stats.nx} / ${stats.ny}` : "480 / 192";
  const ld = stats && Math.abs(stats.cd) > 1e-3 ? stats.cl / stats.cd : 0;
  const activeHint = TOOLS.find((t) => t.id === tool)!.hint;

  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">
          <div className="logo"><Icon name="logo" /></div>
          <div>
            <h1>AeroFlow</h1>
            <span className="sub">Lattice-Boltzmann · D2Q9 · LES</span>
          </div>
        </div>
        <div className="kpis">
          <Kpi label="Re" value={stats ? fmt(stats.re, 0) : "—"} />
          <Kpi label="Cd" value={stats?.length ? fmt(stats.cd, 3) : "—"} tone="cd" />
          <Kpi label="Cl" value={stats?.length ? fmt(stats.cl, 3) : "—"} tone="cl" />
          <Kpi label="L/D" value={stats?.length ? fmt(ld, 2) : "—"} />
        </div>
        <div className={`status ${status}`}>
          <span className="dot" />
          {status === "open" ? (paused ? "Pauza" : "Symulacja") : status === "connecting" ? "Łączenie…" : "Brak serwera"}
        </div>
      </header>

      <main className="workspace">
        <nav className="toolbar">
          {TOOLS.map((t) => (
            <button key={t.id} className={`tool ${tool === t.id ? "active" : ""}`} onClick={() => setTool(t.id)} title={`${t.label} (${t.key})`}>
              <Icon name={t.id} />
              <span>{t.label}</span>
            </button>
          ))}
          <div className="sep" />
          <button className="tool" onClick={() => send({ type: "clear" })} title="Usuń wszystkie przeszkody (C)">
            <Icon name="trash" />
            <span>Wyczyść</span>
          </button>
        </nav>

        <section className="stage">
          <div className="canvas-wrap" style={{ aspectRatio: aspect }}>
            <canvas
              ref={canvasRef}
              className={`sim tool-${tool}`}
              onPointerDown={onDown}
              onPointerMove={onMove}
              onPointerUp={onUp}
              onPointerCancel={onUp}
              onPointerLeave={() => (hover.current = null)}
              onContextMenu={(e) => e.preventDefault()}
            />
            <div className="inlet-label">wlot →</div>
            {ghost && <div className="ghost">{ghost}</div>}
            {probe && (
              <div className="probe">
                {probe.solid ? "ciało stałe" : <>|u|/U∞ <b>{probe.speed.toFixed(2)}</b> · Cp <b>{probe.p.toFixed(2)}</b></>}
                <span className="muted"> ({probe.x}, {probe.y})</span>
              </div>
            )}
            {status !== "open" && (
              <div className="offline">
                <Icon name="logo" />
                <p>{status === "connecting" ? "Łączenie z serwerem symulacji…" : "Uruchom serwer: python server/run.py"}</p>
              </div>
            )}
          </div>
          <div className="below">
            <p className="hint"><Icon name="info" /> {activeHint}</p>
            <Legend view={view} />
          </div>
          <div className="chart-card">
            <div className="chart-head">
              <span>Współczynniki aerodynamiczne</span>
              <span className="legend-dots"><i className="cl" /> Cl <i className="cd" /> Cd</span>
            </div>
            <canvas ref={chartRef} className="chart" />
          </div>
        </section>

        <aside className="panel">
          <Section title="Przepływ">
            <Slider label="Prędkość wlotu" value={u} min={0.02} max={0.18} step={0.005} onChange={setU} display={`${(u / 0.1).toFixed(2)}×`} />
            <Slider label="Lepkość" value={nuS} min={0} max={1} step={0.01} onChange={setNuS} display={nuFromSlider(nuS).toFixed(4)} />
            <Slider label="Kroki / klatkę" value={steps} min={1} max={30} step={1} onChange={setSteps} display={String(steps)} />
            <div className="row">
              <button className="btn primary" onClick={() => setPaused((p) => !p)}>
                <Icon name={paused ? "play" : "pause"} /> {paused ? "Wznów" : "Pauza"}
              </button>
              <button className="btn" disabled={!paused} onClick={() => send({ type: "step" })} title="Jeden krok"><Icon name="step" /></button>
              <button className="btn" onClick={() => { send({ type: "reset" }); history.current = []; }} title="Zresetuj przepływ (R)"><Icon name="reset" /></button>
            </div>
          </Section>

          <Section title="Wizualizacja">
            <Segmented options={VIEWS} value={view} onChange={setView} />
            <Slider label="Kontrast" value={gain} min={0.3} max={3} step={0.05} onChange={setGain} display={`${gain.toFixed(2)}×`} />
            <Segmented
              options={[{ id: "tracers", label: "Cząstki" }, { id: "smoke", label: "Smugi" }, { id: "off", label: "Wył." }]}
              value={pmode}
              onChange={(v) => setPmode(v as ParticleMode)}
            />
            {pmode !== "off" && <Slider label="Liczba cząstek" value={pcount} min={500} max={15000} step={500} onChange={setPcount} display={String(pcount)} />}
          </Section>

          <Section title="Narzędzie">
            {tool === "airfoil" && (
              <>
                <Slider label="Grubość profilu" value={thick} min={4} max={30} step={1} onChange={setThick} display={`${thick}%`} />
                <Slider label="Wygięcie (camber)" value={camber} min={0} max={9} step={1} onChange={setCamber} display={`${camber}%`} />
                <div className="naca">NACA {camber}4{String(thick).padStart(2, "0")}</div>
              </>
            )}
            {(tool === "brush" || tool === "erase") && (
              <Slider label="Rozmiar pędzla" value={brush} min={1} max={14} step={0.5} onChange={setBrush} display={`${brush}`} />
            )}
            {!["airfoil", "brush", "erase"].includes(tool) && <p className="muted small">Brak ustawień dla tego narzędzia.</p>}
          </Section>

          <Section title="Scenariusze">
            <div className="presets">
              {PRESETS.map((p) => (
                <button key={p.id} className="chip" onClick={() => { send({ type: "preset", name: p.id }); history.current = []; }}>{p.label}</button>
              ))}
            </div>
          </Section>

          <div className="perf">
            <span>{stats ? `${stats.nx}×${stats.ny}` : "—"}</span>
            <span>{stats ? `${stats.mlups.toFixed(0)} MLUPS` : ""}</span>
            <span>{stats ? `${stats.fps.toFixed(0)} fps` : ""}</span>
            <span>krok {stats?.step ?? 0}</span>
          </div>
        </aside>
      </main>
    </div>
  );
}

function airfoilFromDrag(a: Pt, b: Pt, thick: number, camber: number) {
  return nacaPolygon(a[0], a[1], Math.hypot(b[0] - a[0], b[1] - a[1]), Math.atan2(b[1] - a[1], b[0] - a[0]), thick / 100, camber / 100);
}

function fmt(v: number, d: number) {
  return Number.isFinite(v) ? v.toFixed(d) : "—";
}

function drawChart(cv: HTMLCanvasElement | null, h: { cd: number; cl: number }[]) {
  if (!cv) return;
  const dpr = Math.min(window.devicePixelRatio || 1, 2);
  const r = cv.getBoundingClientRect();
  cv.width = r.width * dpr;
  cv.height = r.height * dpr;
  const ctx = cv.getContext("2d")!;
  ctx.scale(dpr, dpr);
  const W = r.width, H = r.height;
  ctx.clearRect(0, 0, W, H);
  if (h.length < 2) {
    ctx.fillStyle = "rgba(148,163,184,0.6)";
    ctx.font = "12px 'Bricolage Grotesque'";
    ctx.fillText("Dodaj przeszkodę, aby zobaczyć siły", 12, H / 2 + 4);
    return;
  }
  let lo = Infinity, hi = -Infinity;
  for (const p of h) { lo = Math.min(lo, p.cd, p.cl); hi = Math.max(hi, p.cd, p.cl); }
  lo = Math.min(lo, 0); hi = Math.max(hi, 0.1);
  const pad = (hi - lo) * 0.1; lo -= pad; hi += pad;
  const Y = (v: number) => H - 6 - ((v - lo) / (hi - lo)) * (H - 12);
  ctx.strokeStyle = "rgba(148,163,184,0.25)";
  ctx.setLineDash([3, 3]);
  ctx.beginPath(); ctx.moveTo(0, Y(0)); ctx.lineTo(W, Y(0)); ctx.stroke();
  ctx.setLineDash([]);
  const line = (key: "cd" | "cl", color: string) => {
    ctx.strokeStyle = color;
    ctx.lineWidth = 1.75;
    ctx.beginPath();
    h.forEach((p, i) => {
      const x = (i / 299) * (W - 44);
      i ? ctx.lineTo(x, Y(p[key])) : ctx.moveTo(x, Y(p[key]));
    });
    ctx.stroke();
    const last = h[h.length - 1][key];
    ctx.fillStyle = color;
    ctx.font = "600 11px 'IBM Plex Mono'";
    ctx.fillText(last.toFixed(2), W - 40, Y(last) + 4);
  };
  line("cd", "#f59e0b");
  line("cl", "#38bdf8");
}

function Kpi({ label, value, tone }: { label: string; value: string; tone?: string }) {
  return (
    <div className={`kpi ${tone ?? ""}`}>
      <span className="kpi-label">{label}</span>
      <span className="kpi-value">{value}</span>
    </div>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="section">
      <h3>{title}</h3>
      {children}
    </section>
  );
}

function Slider(p: { label: string; value: number; min: number; max: number; step: number; display: string; onChange: (v: number) => void }) {
  const pct = ((p.value - p.min) / (p.max - p.min)) * 100;
  return (
    <label className="slider">
      <div className="slider-head"><span>{p.label}</span><b>{p.display}</b></div>
      <input type="range" min={p.min} max={p.max} step={p.step} value={p.value}
        style={{ "--pct": `${pct}%` } as React.CSSProperties}
        onChange={(e) => p.onChange(parseFloat(e.target.value))} />
    </label>
  );
}

function Segmented<T extends string>({ options, value, onChange }: { options: { id: T; label: string }[]; value: T; onChange: (v: T) => void }) {
  return (
    <div className="segmented">
      {options.map((o) => (
        <button key={o.id} className={value === o.id ? "on" : ""} onClick={() => onChange(o.id)}>{o.label}</button>
      ))}
    </div>
  );
}

function Legend({ view }: { view: ViewMode }) {
  if (view === "none") return null;
  const grad = {
    speed: "linear-gradient(90deg,#020617,#0a2a5e,#1463c7,#3fb8f5,#a8ecff,#f4fdff)",
    vorticity: "linear-gradient(90deg,#7dd3fc,#1d6fd8,#08122c,#2a0f2e,#e0487a,#ffd1a6)",
    pressure: "linear-gradient(90deg,#1e3a8a,#2f7ff0,#0b1430,#f59e0b,#fff4c2)",
  }[view];
  const labels = { speed: ["0", "|u|", "2.1 U∞"], vorticity: ["−ω", "ω", "+ω"], pressure: ["podciśnienie", "p", "nadciśnienie"] }[view];
  return (
    <div className="legend">
      <span>{labels[0]}</span>
      <div className="bar" style={{ background: grad }} />
      <span>{labels[2]}</span>
    </div>
  );
}
