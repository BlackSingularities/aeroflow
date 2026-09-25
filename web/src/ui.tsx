import { ReactNode, useEffect, useRef, useState } from "react";
import { Icon } from "./icons";

// ---------------------------------------------------------------- pływające okno

type WinPos = { x: number; y: number };

function loadPos(id: string, fallback: WinPos): WinPos {
  try {
    const v = JSON.parse(localStorage.getItem(`win:${id}`) || "null");
    if (v && typeof v.x === "number") return v;
  } catch { /* brak dostępu do storage */ }
  return fallback;
}

let zTop = 20;

export function FloatingWindow(p: {
  id: string;
  title: string;
  icon: string;
  open: boolean;
  onClose: () => void;
  initial: WinPos;
  width?: number;
  children: ReactNode;
}) {
  const [pos, setPos] = useState(() => loadPos(p.id, p.initial));
  const [z, setZ] = useState(() => ++zTop);
  const [min, setMin] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const drag = useRef<{ dx: number; dy: number } | null>(null);

  useEffect(() => { if (p.open) setZ(++zTop); }, [p.open]);

  // pilnuj, żeby okno nie wyjechało poza ekran po zmianie rozmiaru
  useEffect(() => {
    const clamp = () => setPos((q) => ({
      x: Math.min(Math.max(8, q.x), window.innerWidth - 120),
      y: Math.min(Math.max(8, q.y), window.innerHeight - 48),
    }));
    clamp();
    window.addEventListener("resize", clamp);
    return () => window.removeEventListener("resize", clamp);
  }, []);

  const onDown = (e: React.PointerEvent) => {
    if ((e.target as HTMLElement).closest("button")) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    drag.current = { dx: e.clientX - pos.x, dy: e.clientY - pos.y };
    setZ(++zTop);
  };
  const onMove = (e: React.PointerEvent) => {
    if (!drag.current) return;
    setPos({
      x: Math.min(Math.max(8, e.clientX - drag.current.dx), window.innerWidth - 120),
      y: Math.min(Math.max(8, e.clientY - drag.current.dy), window.innerHeight - 48),
    });
  };
  const onUp = () => {
    if (!drag.current) return;
    drag.current = null;
    try { localStorage.setItem(`win:${p.id}`, JSON.stringify(pos)); } catch { /* ignore */ }
  };

  if (!p.open) return null;
  return (
    <div ref={ref} className={`window ${min ? "min" : ""}`} style={{ left: pos.x, top: pos.y, zIndex: z, width: p.width ?? 320 }}
      onPointerDown={() => setZ(++zTop)}>
      <div className="window-head" onPointerDown={onDown} onPointerMove={onMove} onPointerUp={onUp} onDoubleClick={() => setMin((m) => !m)}>
        <Icon name={p.icon} />
        <span className="window-title">{p.title}</span>
        <button className="wbtn" onClick={() => setMin((m) => !m)} title="Zwiń"><Icon name={min ? "expand" : "minus"} /></button>
        <button className="wbtn" onClick={p.onClose} title="Zamknij (Esc)"><Icon name="close" /></button>
      </div>
      {!min && <div className="window-body">{p.children}</div>}
    </div>
  );
}

// ---------------------------------------------------------------- kontrolki

export function Slider(p: { label: string; value: number; min: number; max: number; step: number; display: string; onChange: (v: number) => void; hint?: string }) {
  const pct = ((p.value - p.min) / (p.max - p.min)) * 100;
  return (
    <label className="slider" title={p.hint}>
      <div className="slider-head"><span>{p.label}</span><b>{p.display}</b></div>
      <input type="range" min={p.min} max={p.max} step={p.step} value={p.value}
        style={{ "--pct": `${pct}%` } as React.CSSProperties}
        onChange={(e) => p.onChange(parseFloat(e.target.value))} />
    </label>
  );
}

export function Segmented<T extends string>({ options, value, onChange, small }: { options: { id: T; label: string; icon?: string; title?: string }[]; value: T; onChange: (v: T) => void; small?: boolean }) {
  const idx = Math.max(0, options.findIndex((o) => o.id === value));
  return (
    <div className={`segmented ${small ? "small" : ""}`} style={{ "--n": options.length, "--i": idx } as React.CSSProperties}>
      <div className="seg-thumb" />
      {options.map((o) => (
        <button key={o.id} className={value === o.id ? "on" : ""} onClick={() => onChange(o.id)} title={o.title} aria-label={o.title ?? o.label}>
          {o.icon && <Icon name={o.icon} />}{o.label}
        </button>
      ))}
    </div>
  );
}

export function Toggle({ label, value, onChange }: { label: string; value: boolean; onChange: (v: boolean) => void }) {
  return (
    <button className={`toggle ${value ? "on" : ""}`} onClick={() => onChange(!value)}>
      <span>{label}</span>
      <i />
    </button>
  );
}

export function DockButton(p: { icon: string; label: string; kbd?: string; active?: boolean; onClick: () => void; tipSide?: "top" | "right"; accent?: boolean; disabled?: boolean }) {
  return (
    <button className={`dock-btn ${p.active ? "active" : ""} ${p.accent ? "accent" : ""}`} onClick={p.onClick} disabled={p.disabled} aria-label={p.label}>
      <Icon name={p.icon} />
      <span className={`tip ${p.tipSide ?? "top"}`}>{p.label}{p.kbd && <kbd>{p.kbd}</kbd>}</span>
    </button>
  );
}
