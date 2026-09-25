import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Icon } from "./icons";
import { BodyInfo } from "./sim";
import { Slider } from "./ui";

type Send = (c: { type: string } & Record<string, unknown>) => void;

const KIND_ICON: Record<BodyInfo["kind"], string> = {
  airfoil: "airfoil", polygon: "polygon", circle: "circle", rect: "rect", paint: "brush",
};

/** Menu kontekstowe obiektu (prawy przycisk myszy): obrót, parametry profilu, akcje. */
export function ContextMenu({ body, x, y, send, onClose }: { body: BodyInfo; x: number; y: number; send: Send; onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ x, y });
  const [angle, setAngle] = useState(body.angle);
  const [t, setT] = useState(Math.round((body.t ?? 0.12) * 100));
  const [m, setM] = useState(Math.round((body.m ?? 0.04) * 100));
  const began = useRef(false);

  // wartości z serwera (np. po obrocie kółkiem) — gdy użytkownik nie przeciąga suwaka
  useEffect(() => { if (!began.current) setAngle(body.angle); }, [body.angle]);

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    setPos({
      x: Math.min(x, window.innerWidth - r.width - 12),
      y: Math.min(y, window.innerHeight - r.height - 12),
    });
  }, [x, y]);

  useEffect(() => {
    const onDown = (e: PointerEvent) => { if (!ref.current?.contains(e.target as Node)) onClose(); };
    const t = setTimeout(() => window.addEventListener("pointerdown", onDown), 0);
    return () => { clearTimeout(t); window.removeEventListener("pointerdown", onDown); };
  }, [onClose]);

  const rotate = (deg: number, fresh = false) => {
    const a = Math.max(-180, Math.min(180, deg));
    setAngle(a);
    send({ type: fresh || !began.current ? "rotate_begin" : "rotate", id: body.id, angle: a });
    began.current = true;
  };
  const endDrag = () => { began.current = false; };
  const act = (type: string) => { send({ type, id: body.id }); if (type === "delete") onClose(); };
  const foil = (nt: number, nm: number, cont: boolean) => {
    setT(nt); setM(nm);
    send({ type: "airfoil_params", id: body.id, t: nt / 100, m: nm / 100, continue: cont });
  };
  const canRotate = body.kind !== "circle";
  const title = body.kind === "airfoil" ? `NACA ${body.naca}` : body.label;
  const aoa = body.kind === "airfoil";

  return (
    <div ref={ref} className="ctx ui" style={{ left: pos.x, top: pos.y }} onContextMenu={(e) => e.preventDefault()}>
      <div className="ctx-head">
        <span className="ctx-icon"><Icon name={KIND_ICON[body.kind]} /></span>
        <div>
          <b>{title}</b>
          <span>{body.label} · #{body.id}{body.chord ? ` · cięciwa ${body.chord}` : ""}</span>
        </div>
      </div>

      {canRotate && (
        <div className="ctx-sec" onPointerUp={endDrag}>
          <Slider label={aoa ? "Kąt natarcia" : "Obrót"} value={angle} min={-90} max={90} step={0.5}
            display={`${angle.toFixed(1)}°`} onChange={(v) => rotate(v)} />
          <div className="ctx-row">
            {[-5, -1].map((d) => <button key={d} className="mini" onClick={() => rotate(angle + d, true)}>{d}°</button>)}
            <button className="mini" onClick={() => rotate(0, true)}>0°</button>
            {[1, 5].map((d) => <button key={d} className="mini" onClick={() => rotate(angle + d, true)}>+{d}°</button>)}
          </div>
        </div>
      )}

      {aoa && (
        <div className="ctx-sec" onPointerUp={endDrag}>
          <Slider label="Grubość" value={t} min={4} max={30} step={1} display={`${t}%`}
            onChange={(v) => { foil(v, m, began.current); began.current = true; }} />
          <Slider label="Wygięcie" value={m} min={0} max={9} step={1} display={`${m}%`}
            onChange={(v) => { foil(t, v, began.current); began.current = true; }} />
        </div>
      )}

      <div className="ctx-actions">
        <button onClick={() => act("duplicate")}><Icon name="copy" />Duplikuj<kbd>Ctrl+D</kbd></button>
        {canRotate && <button onClick={() => act("flip")}><Icon name="flip" />Odbij w pionie</button>}
        <button onClick={() => act("center")}><Icon name="center" />Wyśrodkuj w pionie</button>
        <div className="ctx-sep" />
        <button className="danger" onClick={() => act("delete")}><Icon name="trash" />Usuń<kbd>Del</kbd></button>
      </div>
      <p className="ctx-tip">Przeciągnij narzędziem <b>Zaznacz</b>, aby przesunąć · kółko myszy obraca</p>
    </div>
  );
}
