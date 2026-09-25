"""Serwer FastAPI: symulacja LBM strumieniowana do przeglądarki przez WebSocket."""
from __future__ import annotations

import asyncio
import json
import math
import queue
import struct
import threading
import time
from pathlib import Path

import numpy as np
from fastapi import FastAPI, WebSocket, WebSocketDisconnect
from fastapi.staticfiles import StaticFiles

from bodies import Body
from lbm import Simulation

app = FastAPI(title="AeroFlow")
DIST = Path(__file__).resolve().parent.parent / "web" / "dist"
U_SCALE = 32767 / 0.3
P_SCALE = 32767 / 0.02


def load_preset(sim: Simulation, name: str):
    """Scenariusze skalowane względem krótszego wymiaru, żeby blokada tunelu była rozsądna przy każdych proporcjach."""
    nx, ny = sim.nx, sim.ny
    s = min(ny, nx / 2.0)
    cy = ny / 2
    x0 = min(nx * 0.3, s * 1.2)
    b: list[Body] = []
    if name == "cylinder":
        b = [Body.circle(x0, cy + 0.5, s * 0.08)]
    elif name == "airfoil":
        b = [Body.airfoil(x0, cy, s * 0.55, math.radians(8))]
    elif name == "tandem":
        b = [Body.airfoil(x0 * 0.7, cy - s * 0.1, s * 0.4, math.radians(10)),
             Body.airfoil(x0 * 0.7 + s * 0.55, cy + s * 0.08, s * 0.4, math.radians(4))]
    elif name == "grid":
        b = [Body.circle(x0, cy + (i - 2) * s * 0.19, s * 0.045) for i in range(5)]
    elif name == "step":
        b = [Body.rect(x0, ny - s * 0.35, x0 + s * 0.5, ny + 2)]
    elif name == "plate":
        b = [Body.rect(x0, cy - s * 0.12, x0 + 4, cy + s * 0.12)]
    sim.bodies = b
    sim.selected = None
    sim.rebuild()
    sim.reset_flow()


def pack(sim: Simulation, fps: float, mlups: float) -> bytes:
    L = sim.ref_length()
    u = max(sim.inflow(), 1e-4)
    q = 0.5 * u**2 * max(L, 1.0)
    # podczas rozruchu strumienia współczynniki nie mają sensu
    cd, cl = (sim.force[0] / q, -sim.force[1] / q) if L and sim.inflow() > 0.3 * sim.u_in else (0.0, 0.0)
    re = sim.u_in * L / sim.nu
    flags = (1 if sim.send_p else 0) | (2 if sim.dye_on else 0) | (4 if sim.undo_stack else 0) | (8 if sim.redo_stack else 0)
    header = struct.pack(
        "<4sHHII8f", b"AERO", sim.nx, sim.ny, sim.step_count, flags,
        cd, cl, re, u, fps, mlups, L, sim.nu,
    )
    parts = [header,
             np.clip(sim.ux * U_SCALE, -32767, 32767).astype("<i2").tobytes(),
             np.clip(sim.uy * U_SCALE, -32767, 32767).astype("<i2").tobytes()]
    # pola opcjonalne wysyłane tylko wtedy, gdy klient ich potrzebuje
    if sim.send_p:
        parts.append(np.clip((sim.rho - 1.0) / 3.0 * P_SCALE, -32767, 32767).astype("<i2").tobytes())
    if sim.dye_on:
        parts.append(np.clip(sim.dye * 255, 0, 255).astype(np.uint8).tobytes())
    parts.append(np.packbits(sim.solid, bitorder="little").tobytes())
    return b"".join(parts)


def pack_bodies(sim: Simulation) -> bytes:
    """Lista ciał (JSON) + mapa etykiet do trafiania kursorem — wysyłane tylko po zmianie."""
    meta = json.dumps({"bodies": sim.bodies_info(), "selected": sim.selected}).encode()
    return b"".join([struct.pack("<4sHHI", b"BODS", sim.nx, sim.ny, len(meta)), meta, sim.labels.tobytes()])


# komendy tworzące punkt cofania (poza kolejnymi fragmentami tej samej operacji)
EDITS = {"stroke", "polygon", "circle", "rect", "airfoil", "preset", "clear", "grab", "delete",
         "duplicate", "flip", "center", "airfoil_params", "rotate_begin", "nudge"}


def handle(sim: Simulation, msg: dict):
    t = msg.get("type")
    if t in EDITS and not msg.get("continue"):
        if t != "grab" or sim.pick(msg["x"], msg["y"]) is not None:
            sim.snapshot()
    if t == "stroke":
        sim.paint_stroke(msg["points"], float(msg.get("radius", 3)), msg.get("mode") == "erase", bool(msg.get("continue")))
    elif t == "polygon":
        if len(msg["points"]) >= 3:
            sim.add(Body.polygon(msg["points"]))
    elif t == "circle":
        sim.add(Body.circle(msg["x"], msg["y"], msg["r"]))
    elif t == "rect":
        sim.add(Body.rect(msg["x0"], msg["y0"], msg["x1"], msg["y1"]))
    elif t == "airfoil":
        sim.add(Body.airfoil(msg["x"], msg["y"], msg["chord"], msg["angle"], msg.get("thickness", 0.12), msg.get("camber", 0.04)))
    elif t == "clear":
        sim.clear()
    elif t == "preset":
        load_preset(sim, msg["name"])
    elif t == "reset":
        sim.reset_flow()
    # ---- edycja istniejących ciał
    elif t == "select":
        sim.selected = msg.get("id")
        sim.bodies_version += 1
    elif t == "grab":
        sim.grabbed = sim.pick(msg["x"], msg["y"])
        sim.selected = sim.grabbed
        sim.bodies_version += 1
    elif t == "drag":
        if sim.grabbed is not None:
            sim.move(sim.grabbed, float(msg["dx"]), float(msg["dy"]))
    elif t == "release":
        sim.grabbed = None
    elif t == "nudge":
        sim.move(msg["id"], float(msg["dx"]), float(msg["dy"]))
    elif t in ("rotate", "rotate_begin"):
        b = sim.body(msg["id"])
        if b is not None:
            ang = math.radians(float(msg["angle"])) if "angle" in msg else b.angle + math.radians(float(msg.get("delta", 0)))
            sim.set_angle(b.id, max(-math.pi, min(math.pi, ang)))
    elif t == "delete":
        sim.remove(msg["id"])
    elif t == "duplicate":
        b = sim.body(msg["id"])
        if b is not None:
            nb = b.copy()
            nb.id = Body.new_id()
            nb.px += 12
            nb.py += 12
            sim.add(nb)
    elif t == "flip":
        b = sim.body(msg["id"])
        if b is not None:
            b.flip()
            sim.rebuild()
    elif t == "center":
        b = sim.body(msg["id"])
        if b is not None:
            x0, y0, x1, y1 = b.bbox()
            sim.move(b.id, 0, sim.ny / 2 - (y0 + y1) / 2)
    elif t == "airfoil_params":
        b = sim.body(msg["id"])
        if b is not None and b.kind == "airfoil":
            b.params["t"] = float(np.clip(msg.get("t", b.params["t"]), 0.03, 0.35))
            b.params["m"] = float(np.clip(msg.get("m", b.params["m"]), 0.0, 0.1))
            b._rebuild_airfoil()
            sim.rebuild()
    elif t == "undo":
        sim.undo()
    elif t == "redo":
        sim.redo()
    elif t == "params":
        if "u" in msg:
            sim.u_in = float(np.clip(msg["u"], 0.01, 0.2))
        if "nu" in msg:
            sim.nu = float(np.clip(msg["nu"], 0.0005, 0.2))
        if "turb" in msg:
            sim.turbulence = float(np.clip(msg["turb"], 0.0, 0.15))
        if "steps" in msg:
            sim.steps_per_frame = int(np.clip(msg["steps"], 1, 30))
        if "paused" in msg:
            sim.paused = bool(msg["paused"])
        if "fields" in msg:
            f = set(msg["fields"])
            sim.send_p = "p" in f
            sim.dye_on = "dye" in f
    elif t == "step":
        sim.step(1)


class Runner(threading.Thread):
    """Wątek symulacji: liczy kroki w stałym tempie i publikuje najświeższą klatkę."""

    TARGET_FPS = 60

    def __init__(self, loop: asyncio.AbstractEventLoop, ready: asyncio.Event):
        super().__init__(daemon=True)
        self.sim = Simulation()
        load_preset(self.sim, "airfoil")
        self.cmds: queue.SimpleQueue = queue.SimpleQueue()
        self.loop, self.ready = loop, ready
        self.latest: bytes | None = None
        self.bodies_msg: tuple[int, bytes] | None = None
        self.running = True

    def run(self):
        sim = self.sim
        fps = mlups = 0.0
        last = time.perf_counter()
        period = 1.0 / self.TARGET_FPS
        sent_version = -1
        seq = 0
        while self.running:
            t0 = time.perf_counter()
            while True:
                try:
                    msg = self.cmds.get_nowait()
                except queue.Empty:
                    break
                try:
                    if msg.get("type") == "resize":
                        ny = int(np.clip(msg.get("ny", 280), 100, 480))
                        nx = int(np.clip(round(ny * float(msg["aspect"])), 160, 1600))
                        if nx != sim.nx or ny != sim.ny:
                            sim = self.sim = sim.resized(nx, ny)
                            sent_version = -1
                        continue
                    handle(sim, msg)
                except (KeyError, TypeError, ValueError, IndexError) as e:
                    print("bad message:", msg.get("type"), e)
            if not sim.paused:
                n = sim.steps_per_frame
                sim.step(n)
                dt = time.perf_counter() - t0
                mlups = 0.9 * mlups + 0.1 * (sim.nx * sim.ny * n / max(dt, 1e-6) / 1e6)
            if sim.bodies_version != sent_version:
                sent_version = sim.bodies_version
                seq += 1  # licznik rosnący także po podmianie symulacji (zmiana siatki)
                self.bodies_msg = (seq, pack_bodies(sim))
            self.latest = pack(sim, fps, mlups)
            try:
                self.loop.call_soon_threadsafe(self.ready.set)
            except RuntimeError:
                return
            now = time.perf_counter()
            fps = 0.92 * fps + 0.08 / max(now - last, 1e-6)
            last = now
            # time.sleep ma wysoką rozdzielczość od Pythona 3.11 (także na Windows)
            rest = period - (time.perf_counter() - t0)
            if rest > 0:
                time.sleep(rest)


@app.websocket("/ws")
async def ws_endpoint(ws: WebSocket):
    await ws.accept()
    ready = asyncio.Event()
    runner = Runner(asyncio.get_running_loop(), ready)
    runner.start()

    async def reader():
        try:
            while True:
                runner.cmds.put(json.loads(await ws.receive_text()))
        except Exception:
            runner.running = False
            ready.set()

    rtask = asyncio.create_task(reader())
    bodies_sent = -1
    try:
        while runner.running:
            await ready.wait()
            ready.clear()
            bm = runner.bodies_msg
            if bm is not None and bm[0] != bodies_sent:
                bodies_sent = bm[0]
                await ws.send_bytes(bm[1])
            frame = runner.latest
            if frame is not None and runner.running:
                await ws.send_bytes(frame)  # wolny klient = pominięte klatki, nie kolejka
    except (WebSocketDisconnect, RuntimeError):
        pass
    finally:
        runner.running = False
        rtask.cancel()


if DIST.exists():
    app.mount("/", StaticFiles(directory=DIST, html=True), name="web")
