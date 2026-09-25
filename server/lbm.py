"""Lattice-Boltzmann D2Q9 (BGK + Smagorinsky LES) w tunelu aerodynamicznym, kernel numba."""
from __future__ import annotations

import copy
import math

import numpy as np

from bodies import Body

CX = np.array([0, 1, 0, -1, 0, 1, -1, -1, 1])
CY = np.array([0, 0, 1, 0, -1, 1, 1, -1, -1])
W = np.array([4 / 9] + [1 / 9] * 4 + [1 / 36] * 4, dtype=np.float32)
OPP = np.array([0, 3, 4, 1, 2, 7, 8, 5, 6])
CS_SMAG = 0.14
SPONGE = 48
RAMP_STEPS = 1200  # łagodny rozruch strumienia — bez fali uderzeniowej i stojących fal akustycznych


class Simulation:
    def __init__(self, nx: int = 480, ny: int = 192):
        self.nx, self.ny = nx, ny
        self.u_in = 0.10
        self.nu = 0.008
        self.steps_per_frame = 10
        self.paused = False
        self.step_count = 0
        self.bodies: list[Body] = []
        self.bodies_version = 0
        self.selected: int | None = None
        self.grabbed: int | None = None
        self.active_paint: int | None = None
        self.solid = np.zeros((ny, nx), dtype=bool)
        self.labels = np.zeros((ny, nx), dtype=np.uint8)
        self.dye = np.zeros((ny, nx), dtype=np.float32)
        self.dye_on = False
        self.send_p = True
        self.undo_stack: list[list[Body]] = []
        self.redo_stack: list[list[Body]] = []
        self.force = np.zeros(2)
        self.reset_flow()

    # ------------------------------------------------------------------ przepływ
    def reset_flow(self):
        ny, nx = self.ny, self.nx
        rho = np.ones((ny, nx), np.float32)
        zero = np.zeros((ny, nx), np.float32)
        self.f = np.ascontiguousarray((W[:, None, None] * rho[None]).astype(np.float32))
        self._g = np.empty_like(self.f)
        self.rho, self.ux, self.uy = rho, zero, zero.copy()
        self.dye = np.zeros((ny, nx), dtype=np.float32)
        self.step_count = 0

    def inflow(self) -> float:
        a = min(1.0, self.step_count / RAMP_STEPS)
        return self.u_in * a * a * (3 - 2 * a)

    def step(self, n: int):
        tau0 = np.full(self.nx, 3.0 * self.nu + 0.5, np.float32)
        force = np.zeros(2, np.float64)
        f, g = self.f, self._g
        rho = np.empty((self.ny, self.nx), np.float32)
        ux, uy = np.empty_like(rho), np.empty_like(rho)
        for _ in range(n):
            _kernel(f, g, self.solid, tau0, np.float32(self.inflow()), np.float32(CS_SMAG), SPONGE, rho, ux, uy, force)
            f, g = g, f
            self.step_count += 1
        if not np.isfinite(f).all():
            self.reset_flow()
            return
        self.f, self._g = f, g
        self.rho, self.ux, self.uy = rho, ux, uy
        self.force = force / n
        if self.dye_on:
            out = np.empty_like(self.dye)
            _advect_dye(self.dye, out, ux, uy, self.solid, np.float32(n), max(6, self.ny // 24))
            self.dye = out

    # ------------------------------------------------------------------ ciała
    def rebuild(self):
        """Składa maskę przeszkód i mapę etykiet z rastrów wszystkich ciał."""
        solid = np.zeros((self.ny, self.nx), bool)
        labels = np.zeros((self.ny, self.nx), np.uint8)
        for k, b in enumerate(self.bodies[:255]):
            x0, y0, m = b.raster(self.nx, self.ny)
            if m.size == 0:
                continue
            h, w = m.shape
            solid[y0:y0 + h, x0:x0 + w] |= m
            labels[y0:y0 + h, x0:x0 + w][m] = k + 1
        solid[:, :2] = False
        labels[:, :2] = 0
        newly = solid & ~self.solid
        self.solid, self.labels = solid, labels
        # w komórkach, które właśnie stały się ciałem, zerujemy prędkość
        if newly.any():
            self.f[:, newly] = W[:, None]
        self.bodies_version += 1

    def body(self, bid):
        return next((b for b in self.bodies if b.id == bid), None)

    def pick(self, x, y):
        xi, yi = int(x), int(y)
        if 0 <= xi < self.nx and 0 <= yi < self.ny:
            k = int(self.labels[yi, xi])
            if k:
                return self.bodies[k - 1].id
        return None

    def add(self, b: Body | None, select=True):
        if b is None:
            return
        self.bodies.append(b)
        if select:
            self.selected = b.id
        self.rebuild()

    def remove(self, bid):
        self.bodies = [b for b in self.bodies if b.id != bid]
        if self.selected == bid:
            self.selected = None
        self.rebuild()

    def move(self, bid, dx, dy):
        b = self.body(bid)
        if b:
            b.px += dx
            b.py += dy
            self.rebuild()

    def set_angle(self, bid, angle_rad):
        b = self.body(bid)
        if b and b.kind != "circle":
            b.angle = angle_rad
            self.rebuild()

    def paint_stroke(self, pts, radius: float, erase: bool, cont: bool):
        m = np.zeros((self.ny, self.nx), bool)
        xs = [p[0] for p in pts]
        ys = [p[1] for p in pts]
        x0 = max(0, int(min(xs) - radius - 1))
        x1 = min(self.nx, int(max(xs) + radius + 2))
        y0 = max(0, int(min(ys) - radius - 1))
        y1 = min(self.ny, int(max(ys) + radius + 2))
        if x1 <= x0 or y1 <= y0:
            return
        yy, xx = np.mgrid[y0:y1, x0:x1].astype(np.float32)
        sub = np.zeros(yy.shape, bool)
        for k in range(len(pts)):
            ax, ay = pts[k]
            bx, by = pts[k + 1] if k + 1 < len(pts) else pts[k]
            dx, dy = bx - ax, by - ay
            l2 = dx * dx + dy * dy
            t = 0 if l2 == 0 else np.clip(((xx - ax) * dx + (yy - ay) * dy) / l2, 0, 1)
            sub |= (xx - ax - t * dx) ** 2 + (yy - ay - t * dy) ** 2 <= radius * radius
        m[y0:y1, x0:x1] = sub
        if erase:
            self._erase(m)
            return
        prev = self.body(self.active_paint) if cont else None
        if prev is not None and prev.kind == "paint":
            full = prev.to_paint(self.nx, self.ny) | m
            nb = Body.paint(full)
            nb.id = prev.id
            self.bodies[self.bodies.index(prev)] = nb
            self.rebuild()
        else:
            nb = Body.paint(m)
            self.add(nb, select=False)
            self.active_paint = nb.id

    def _erase(self, m):
        changed = False
        out = []
        for b in self.bodies:
            x0, y0, r = b.raster(self.nx, self.ny)
            if r.size and (m[y0:y0 + r.shape[0], x0:x0 + r.shape[1]] & r).any():
                full = b.to_paint(self.nx, self.ny) & ~m
                nb = Body.paint(full)
                if nb is not None:
                    nb.id = b.id
                    out.append(nb)
                changed = True
            else:
                out.append(b)
        if changed:
            self.bodies = out
            self.rebuild()

    def clear(self):
        self.bodies = []
        self.selected = None
        self.rebuild()

    # ------------------------------------------------------------------ historia
    def snapshot(self):
        self.undo_stack.append([b.copy() for b in self.bodies])
        del self.undo_stack[:-50]
        self.redo_stack.clear()

    def undo(self):
        if self.undo_stack:
            self.redo_stack.append([b.copy() for b in self.bodies])
            self.bodies = self.undo_stack.pop()
            self.rebuild()

    def redo(self):
        if self.redo_stack:
            self.undo_stack.append([b.copy() for b in self.bodies])
            self.bodies = self.redo_stack.pop()
            self.rebuild()

    # ------------------------------------------------------------------ inne
    def resized(self, nx: int, ny: int) -> "Simulation":
        """Nowa siatka: ciała przeskalowane względem wysokości, parametry zachowane."""
        k = ny / self.ny
        sim = Simulation(nx, ny)
        sim.u_in, sim.nu, sim.steps_per_frame, sim.paused = self.u_in, self.nu, self.steps_per_frame, self.paused
        sim.dye_on, sim.send_p, sim.selected = self.dye_on, self.send_p, self.selected
        sim.bodies = [b.scaled(k) for b in self.bodies]
        sim.undo_stack = [[b.scaled(k) for b in snap] for snap in self.undo_stack]
        sim.rebuild()
        return sim

    def ref_length(self):
        """Długość odniesienia: cięciwa dla smukłych ciał, wysokość czołowa dla tępych."""
        cols = np.where(self.solid.any(0))[0]
        rows = np.where(self.solid.any(1))[0]
        if not len(cols):
            return 0.0
        return float(max(cols.max() - cols.min(), rows.max() - rows.min()) + 1)

    def bodies_info(self) -> list[dict]:
        return [b.info() for b in self.bodies]


# ---------------------------------------------------------------- numba kernel

from numba import njit, prange


_CXi = CX.astype(np.int64)
_CYi = CY.astype(np.int64)
_W32 = W.astype(np.float32)
_OPPi = OPP.astype(np.int64)
_MIRi = np.array([0, 1, 4, 3, 2, 8, 7, 6, 5], dtype=np.int64)

@njit(parallel=True, fastmath=True, cache=True)
def _kernel(fin, fout, solid, tau0, u_in, cs, sponge, rho_o, ux_o, uy_o, force):
    """Pull-streaming + half-way bounce-back + BGK/Smagorinsky, jedna pętla po siatce."""
    _, ny, nx = fin.shape
    fxs = np.zeros(ny)
    fys = np.zeros(ny)
    for y in prange(ny):
        fl = np.empty(9, np.float32)
        for x in range(nx):
            if solid[y, x]:
                for i in range(9):
                    fout[i, y, x] = _W32[i]
                rho_o[y, x] = 1.0
                ux_o[y, x] = 0.0
                uy_o[y, x] = 0.0
                continue
            if x == 0:
                r = np.float32(0.0)
                for i in range(9):
                    r += fin[i, y, 1]
                u = u_in
                v = np.float32(0.0)
                usq = 1.5 * u * u
                for i in range(9):
                    cu = 3.0 * _CXi[i] * u
                    fout[i, y, x] = _W32[i] * r * (1.0 + cu + 0.5 * cu * cu - usq)
                rho_o[y, x] = r
                ux_o[y, x] = u
                uy_o[y, x] = v
                continue
            for i in range(9):
                sx = x - _CXi[i]
                sy = y - _CYi[i]
                if sx >= nx:
                    sx = nx - 1
                if sy < 0 or sy >= ny:
                    # ściana poślizgowa (odbicie lustrzane) na górze i dole tunelu
                    fl[i] = fin[_MIRi[i], y, sx]
                    continue
                if solid[sy, sx]:
                    # half-way bounce-back: odbita populacja z tej komórki
                    val = fin[_OPPi[i], y, x]
                    fl[i] = val
                    fxs[y] -= 2.0 * _CXi[i] * val
                    fys[y] -= 2.0 * _CYi[i] * val
                else:
                    if x == nx - 1 and _CXi[i] < 0:
                        fl[i] = fin[i, y, x - 1]  # wylot: gradient zerowy
                    else:
                        fl[i] = fin[i, sy, sx]
            r = 0.0
            jx = 0.0
            jy = 0.0
            pxx = 0.0
            pyy = 0.0
            pxy = 0.0
            for i in range(9):
                fi = fl[i]
                r += fi
                jx += fi * _CXi[i]
                jy += fi * _CYi[i]
                pxx += fi * _CXi[i] * _CXi[i]
                pyy += fi * _CYi[i] * _CYi[i]
                pxy += fi * _CXi[i] * _CYi[i]
            r = min(max(r, 0.2), 5.0)
            u = jx / r
            v = jy / r
            sp = np.sqrt(u * u + v * v)
            if sp > 0.4:
                u *= 0.4 / sp
                v *= 0.4 / sp
            pxx -= r * (1.0 / 3.0 + u * u)
            pyy -= r * (1.0 / 3.0 + v * v)
            pxy -= r * u * v
            q = np.sqrt(pxx * pxx + pyy * pyy + 2.0 * pxy * pxy)
            t0 = tau0[x]
            tau = 0.5 * (t0 + np.sqrt(t0 * t0 + 18.0 * cs * cs * q / r))
            om = 1.0 / tau
            usq = 1.5 * (u * u + v * v)
            sg = 0.0
            if x > nx - sponge:
                a = (x - (nx - sponge)) / sponge
                sg = 0.25 * a * a
            usq0 = 1.5 * u_in * u_in
            for i in range(9):
                cu = 3.0 * (_CXi[i] * u + _CYi[i] * v)
                feq = _W32[i] * r * (1.0 + cu + 0.5 * cu * cu - usq)
                post = fl[i] + om * (feq - fl[i])
                if sg > 0.0:
                    c0 = 3.0 * _CXi[i] * u_in
                    f0 = _W32[i] * (1.0 + c0 + 0.5 * c0 * c0 - usq0)
                    post += sg * (f0 - post)
                fout[i, y, x] = post
            rho_o[y, x] = r
            ux_o[y, x] = u
            uy_o[y, x] = v
    # pęd przekazany ciału: populacja lecąca w -c_i odbija się jako c_i → 2*f*(-c_i)
    force[0] += fxs.sum()
    force[1] += fys.sum()


@njit(parallel=True, fastmath=True, cache=True)
def _advect_dye(src, dst, ux, uy, solid, dt, band):
    """Semi-Lagrange'owska adwekcja dymu + wtrysk smug na wlocie."""
    ny, nx = src.shape
    for y in prange(ny):
        for x in range(nx):
            if solid[y, x]:
                dst[y, x] = 0.0
                continue
            if x < 2:
                ph = y % band
                dst[y, x] = 1.0 if ph < max(1, band // 5) else 0.0
                continue
            px = x - ux[y, x] * dt
            py = y - uy[y, x] * dt
            px = min(max(px, 0.0), nx - 1.001)
            py = min(max(py, 0.0), ny - 1.001)
            x0 = int(px)
            y0 = int(py)
            fx = px - x0
            fy = py - y0
            v = (src[y0, x0] * (1 - fx) + src[y0, x0 + 1] * fx) * (1 - fy) + (src[y0 + 1, x0] * (1 - fx) + src[y0 + 1, x0 + 1] * fx) * fy
            dst[y, x] = v * 0.9995
