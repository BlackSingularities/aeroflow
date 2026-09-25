"""Serwer FastAPI: symulacja LBM strumieniowana do przeglądarki przez WebSocket."""
from __future__ import annotations

import asyncio
import json
import queue
import threading
import math
import struct
import time
from pathlib import Path

import numpy as np
from fastapi import FastAPI, WebSocket, WebSocketDisconnect
from fastapi.staticfiles import StaticFiles

from lbm import Simulation

app = FastAPI(title="AeroFlow")
DIST = Path(__file__).resolve().parent.parent / "web" / "dist"
U_SCALE = 32767 / 0.3
P_SCALE = 32767 / 0.02


def load_preset(sim: Simulation, name: str):
    sim.solid[:] = False
    nx, ny = sim.nx, sim.ny
    if name == "cylinder":
        sim.add_circle(nx * 0.2, ny / 2 + 0.5, ny * 0.09)
    elif name == "airfoil":
        sim.add_airfoil(nx * 0.22, ny / 2, nx * 0.3, math.radians(8))
    elif name == "tandem":
        sim.add_airfoil(nx * 0.12, ny * 0.40, nx * 0.22, math.radians(10))
        sim.add_airfoil(nx * 0.48, ny * 0.60, nx * 0.22, math.radians(4))
    elif name == "grid":
        for i in range(5):
            sim.add_circle(nx * 0.18, ny * (0.12 + 0.19 * i), ny * 0.05)
    elif name == "step":
        sim.add_rect(nx * 0.15, ny * 0.55, nx * 0.4, ny)
    elif name == "plate":
        sim.add_rect(nx * 0.2, ny * 0.3, nx * 0.2 + 4, ny * 0.7)
    else:
        sim.clear()
    sim.reset_flow()
    sim._update_mask()


def pack(sim: Simulation, fps: float, mlups: float) -> bytes:
    L = sim.body_length()
    q = 0.5 * sim.u_in**2 * max(L, 1.0)
    cd, cl = (sim.force[0] / q, -sim.force[1] / q) if L else (0.0, 0.0)
    re = sim.u_in * L / sim.nu
    header = struct.pack(
        "<4sHHI8f", b"AERO", sim.nx, sim.ny, sim.step_count,
        cd, cl, re, sim.u_in, fps, mlups, L, sim.nu,
    )
    ux = np.clip(sim.ux * U_SCALE, -32767, 32767).astype("<i2")
    uy = np.clip(sim.uy * U_SCALE, -32767, 32767).astype("<i2")
    pr = np.clip((sim.rho - 1.0) / 3.0 * P_SCALE, -32767, 32767).astype("<i2")
    return b"".join([header, ux.tobytes(), uy.tobytes(), pr.tobytes(), np.packbits(sim.solid, bitorder="little").tobytes()])


def handle(sim: Simulation, msg: dict):
    t = msg.get("type")
    if t == "stroke":
        sim.paint_stroke(msg["points"], float(msg.get("radius", 3)), msg.get("mode") != "erase")
    elif t == "polygon":
        sim.fill_polygon(msg["points"])
    elif t == "circle":
        sim.add_circle(msg["x"], msg["y"], msg["r"])
    elif t == "rect":
        sim.add_rect(msg["x0"], msg["y0"], msg["x1"], msg["y1"])
    elif t == "airfoil":
        sim.add_airfoil(msg["x"], msg["y"], msg["chord"], msg["angle"],
                        msg.get("thickness", 0.12), msg.get("camber", 0.04))
    elif t == "clear":
        sim.clear()
    elif t == "preset":
        load_preset(sim, msg["name"])
    elif t == "reset":
        sim.reset_flow()
        sim._update_mask()
    elif t == "params":
        if "u" in msg:
            sim.u_in = float(np.clip(msg["u"], 0.01, 0.2))
        if "nu" in msg:
            sim.nu = float(np.clip(msg["nu"], 0.0005, 0.2))
        if "steps" in msg:
            sim.steps_per_frame = int(np.clip(msg["steps"], 1, 30))
        if "paused" in msg:
            sim.paused = bool(msg["paused"])
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
        self.running = True

    def run(self):
        sim = self.sim
        fps = mlups = 0.0
        last = time.perf_counter()
        period = 1.0 / self.TARGET_FPS
        while self.running:
            t0 = time.perf_counter()
            while True:
                try:
                    msg = self.cmds.get_nowait()
                except queue.Empty:
                    break
                try:
                    handle(sim, msg)
                except (KeyError, TypeError, ValueError) as e:
                    print("bad message:", e)
            if not sim.paused:
                n = sim.steps_per_frame
                sim.step(n)
                dt = time.perf_counter() - t0
                mlups = 0.9 * mlups + 0.1 * (sim.nx * sim.ny * n / max(dt, 1e-6) / 1e6)
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
    try:
        while runner.running:
            await ready.wait()
            ready.clear()
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
