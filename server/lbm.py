"""Lattice-Boltzmann D2Q9 solver (BGK + Smagorinsky LES) w tunelu aerodynamicznym."""
from __future__ import annotations

import numpy as np

CX = np.array([0, 1, 0, -1, 0, 1, -1, -1, 1])
CY = np.array([0, 0, 1, 0, -1, 1, 1, -1, -1])
W = np.array([4 / 9] + [1 / 9] * 4 + [1 / 36] * 4, dtype=np.float32)
OPP = np.array([0, 3, 4, 1, 2, 7, 8, 5, 6])
MOM = np.stack([np.ones(9), CX, CY, CX * CX, CY * CY, CX * CY]).astype(np.float32)
CS_SMAG = 0.14
SPONGE = 24


class Simulation:
    def __init__(self, nx: int = 480, ny: int = 192):
        self.nx, self.ny = nx, ny
        self.u_in = 0.10
        self.nu = 0.004
        self.steps_per_frame = 10
        self.paused = False
        self.step_count = 0
        self.solid = np.zeros((ny, nx), dtype=bool)
        self.force = np.zeros(2)
        self.yy, self.xx = np.mgrid[0:ny, 0:nx].astype(np.float32)
        self._cx = CX[:, None, None].astype(np.float32)
        self._cy = CY[:, None, None].astype(np.float32)
        self._w = W[:, None, None]
        self.reset_flow()
        self._update_mask()

    # ------------------------------------------------------------------ core
    def equilibrium(self, rho, ux, uy):
        cu = 3.0 * (self._cx * ux + self._cy * uy)
        usq = 1.5 * (ux * ux + uy * uy)
        return (self._w * rho * (1.0 + cu + 0.5 * cu * cu - usq)).astype(np.float32)

    def reset_flow(self):
        ny, nx = self.ny, self.nx
        rho = np.ones((ny, nx), np.float32)
        ux = np.full((ny, nx), self.u_in, np.float32)
        # drobne zaburzenie, żeby szybciej złamać symetrię (ścieżka Kármána)
        uy = (0.002 * np.sin(2 * np.pi * self.xx / nx * 3)).astype(np.float32)
        ux[self.solid] = 0
        uy[self.solid] = 0
        self.f = self.equilibrium(rho, ux, uy)
        self.rho, self.ux, self.uy = rho, ux, uy
        self.step_count = 0

    def _update_mask(self):
        fluid = ~self.solid
        # link i: komórka stała, do której w kroku streamingu wpływa populacja i z płynu
        self.links = [
            self.solid & np.roll(fluid, (int(CY[i]), int(CX[i])), axis=(0, 1)) for i in range(9)
        ]
        self.link_idx = [np.flatnonzero(l) for l in self.links]
        self.solid_idx = np.flatnonzero(self.solid)
        rest = self.equilibrium(np.ones_like(self.rho), 0 * self.ux, 0 * self.uy)
        self.f[:, self.solid] = rest[:, self.solid]

    def _tau0(self):
        tau = 3.0 * self.nu + 0.5
        t = np.full(self.nx, tau, np.float32)
        ramp = np.linspace(0, 1, SPONGE, dtype=np.float32) ** 2
        t[-SPONGE:] = tau + ramp * max(0.0, 1.0 - tau)  # gąbka tłumiąca odbicia na wylocie
        return t[None, :]

    def step(self, n: int):
        if HAS_NUMBA:
            return self._step_numba(n)
        return self._step_numpy(n)

    def _step_numba(self, n: int):
        tau0 = self._tau0()[0].astype(np.float32)
        force = np.zeros(2, np.float64)
        f, g = self.f, getattr(self, "_g", None)
        if g is None or g.shape != f.shape:
            g = np.empty_like(f)
        rho = np.empty((self.ny, self.nx), np.float32)
        ux, uy = np.empty_like(rho), np.empty_like(rho)
        for _ in range(n):
            _kernel(f, g, self.solid, tau0, np.float32(self.u_in), np.float32(CS_SMAG), rho, ux, uy, force)
            f, g = g, f
        if not np.isfinite(f).all():
            self.reset_flow()
            self._update_mask()
            return
        self.f, self._g = f, g
        self.rho, self.ux, self.uy = rho, ux, uy
        self.force = force / n
        self.step_count += n

    def _step_numpy(self, n: int):
        f = self.f
        tau0 = self._tau0()
        edge = self.equilibrium(
            np.ones((1, 1), np.float32),
            np.full((1, 1), self.u_in, np.float32),
            np.zeros((1, 1), np.float32),
        )[:, 0, :]  # (9, 1)
        fx = fy = 0.0
        g = np.empty_like(f)
        flat = f.reshape(9, -1)
        sidx = self.solid_idx
        for _ in range(n):
            mom = np.tensordot(MOM, f, 1)  # rho, jx, jy, Pxx, Pyy, Pxy
            rho = np.clip(mom[0], 0.2, 5.0)
            ux = mom[1] / rho
            uy = mom[2] / rho
            speed2 = ux * ux + uy * uy
            lim = np.minimum(1.0, 0.3 / np.sqrt(np.maximum(speed2, 1e-12)))
            ux *= lim
            uy *= lim
            # tensor naprężeń nierównowagowych liczony analitycznie z momentów
            pxx = mom[3] - rho * (1 / 3 + ux * ux)
            pyy = mom[4] - rho * (1 / 3 + uy * uy)
            pxy = mom[5] - rho * ux * uy
            q = np.sqrt(pxx * pxx + pyy * pyy + 2 * pxy * pxy)
            tau = 0.5 * (tau0 + np.sqrt(tau0 * tau0 + (18.0 * CS_SMAG**2) * q / rho))
            omega = 1.0 / tau
            feq = self.equilibrium(rho, ux, uy)
            f += omega * (feq - f)
            # streaming przez przesunięcia (okresowe, brzegi i tak nadpisujemy)
            for i in range(9):
                sy, sx = int(CY[i]), int(CX[i])
                g[i] = np.roll(f[i], (sy, sx), axis=(0, 1)) if (sx or sy) else f[i]
            f, g = g, f
            flat = f.reshape(9, -1)
            # siła metodą wymiany pędu
            for i in range(1, 9):
                s = float(flat[i, self.link_idx[i]].sum())
                fx += 2.0 * CX[i] * s
                fy += 2.0 * CY[i] * s
            # bounce-back na przeszkodach
            if sidx.size:
                flat[:, sidx] = flat[OPP[:, None], sidx[None, :]]
            # brzegi: wlot, górna/dolna krawędź = swobodny strumień, wylot = ekstrapolacja
            f[:, :, 0] = edge
            f[:, 0, :] = edge
            f[:, -1, :] = edge
            f[:, :, -1] = f[:, :, -2]
        if not np.isfinite(f).all():
            self.reset_flow()
            self._update_mask()
            return
        self.f = f
        self.rho, self.ux, self.uy = rho, ux, uy
        self.ux[self.solid] = 0
        self.uy[self.solid] = 0
        self.force = np.array([fx / n, fy / n])
        self.step_count += n

    # --------------------------------------------------------------- editing
    def paint_stroke(self, pts, radius: float, value: bool):
        m = np.zeros_like(self.solid)
        for k in range(len(pts)):
            x0, y0 = pts[k]
            x1, y1 = pts[k + 1] if k + 1 < len(pts) else pts[k]
            dx, dy = x1 - x0, y1 - y0
            l2 = dx * dx + dy * dy
            if l2 == 0:
                d2 = (self.xx - x0) ** 2 + (self.yy - y0) ** 2
            else:
                t = np.clip(((self.xx - x0) * dx + (self.yy - y0) * dy) / l2, 0, 1)
                d2 = (self.xx - x0 - t * dx) ** 2 + (self.yy - y0 - t * dy) ** 2
            m |= d2 <= radius * radius
        self._apply(m, value)

    def fill_polygon(self, pts, value: bool = True):
        if len(pts) < 3:
            return
        p = np.asarray(pts, np.float32)
        x, y = self.xx + 0.5, self.yy + 0.5
        inside = np.zeros_like(self.solid)
        j = len(p) - 1
        for i in range(len(p)):
            xi, yi = p[i]
            xj, yj = p[j]
            cond = (yi > y) != (yj > y)
            xint = (xj - xi) * (y - yi) / (yj - yi + 1e-12) + xi
            inside ^= cond & (x < xint)
            j = i
        self._apply(inside, value)

    def add_circle(self, cx, cy, r):
        self._apply((self.xx - cx) ** 2 + (self.yy - cy) ** 2 <= r * r, True)

    def add_rect(self, x0, y0, x1, y1):
        xa, xb = sorted((x0, x1))
        ya, yb = sorted((y0, y1))
        self._apply((self.xx >= xa) & (self.xx <= xb) & (self.yy >= ya) & (self.yy <= yb), True)

    def add_airfoil(self, x, y, chord, angle, thickness=0.12, camber=0.04):
        self.fill_polygon(naca_polygon(x, y, chord, angle, thickness, camber))

    def clear(self):
        self.solid[:] = False
        self._update_mask()

    def _apply(self, m, value):
        m[:, :2] = False  # nie zasłaniamy wlotu
        self.solid[m] = value
        self._update_mask()

    def body_length(self):
        cols = np.where(self.solid[1:-1].any(0))[0]
        return float(cols.max() - cols.min() + 1) if len(cols) else 0.0


def naca_polygon(x, y, chord, angle, t=0.12, m=0.04, p=0.4, n=60):
    """Profil NACA 4-cyfrowy, nos w (x, y), kąt natarcia `angle` w radianach (dodatni = nos w górę)."""
    beta = np.linspace(0, np.pi, n)
    xc = (1 - np.cos(beta)) / 2
    yt = 5 * t * (0.2969 * np.sqrt(xc) - 0.1260 * xc - 0.3516 * xc**2 + 0.2843 * xc**3 - 0.1036 * xc**4)
    if m > 0:
        yc = np.where(xc < p, m / p**2 * (2 * p * xc - xc**2), m / (1 - p) ** 2 * ((1 - 2 * p) + 2 * p * xc - xc**2))
        dyc = np.where(xc < p, 2 * m / p**2 * (p - xc), 2 * m / (1 - p) ** 2 * (p - xc))
    else:
        yc = dyc = np.zeros_like(xc)
    th = np.arctan(dyc)
    xu, yu = xc - yt * np.sin(th), yc + yt * np.cos(th)
    xl, yl = xc + yt * np.sin(th), yc - yt * np.cos(th)
    px = np.concatenate([xu, xl[::-1]]) * chord
    py = np.concatenate([yu, yl[::-1]]) * chord
    # ekran: y w dół → odbijamy y; obrót tak, by spływ opadał przy dodatnim kącie
    c, s = np.cos(angle), np.sin(angle)
    rx = px * c + py * s
    ry = px * s - py * c
    return np.stack([x + rx, y + ry], 1).tolist()


# ---------------------------------------------------------------- numba kernel
try:
    from numba import njit, prange

    HAS_NUMBA = True
except ImportError:  # pragma: no cover
    HAS_NUMBA = False

if HAS_NUMBA:
    _CXi = CX.astype(np.int64)
    _CYi = CY.astype(np.int64)
    _W32 = W.astype(np.float32)
    _OPPi = OPP.astype(np.int64)

    @njit(parallel=True, fastmath=True, cache=True)
    def _kernel(fin, fout, solid, tau0, u_in, cs, rho_o, ux_o, uy_o, force):
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
                edge = x == 0 or y == 0 or y == ny - 1
                if edge:
                    r = np.float32(1.0)
                    u = u_in
                    v = np.float32(0.0)
                    usq = 1.5 * u * u
                    for i in range(9):
                        cu = 3.0 * _CXi[i] * u
                        fout[i, y, x] = _W32[i] * (1.0 + cu + 0.5 * cu * cu - usq)
                    rho_o[y, x] = r
                    ux_o[y, x] = u
                    uy_o[y, x] = v
                    continue
                for i in range(9):
                    sx = x - _CXi[i]
                    sy = y - _CYi[i]
                    if sx >= nx:
                        sx = nx - 1
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
                if sp > 0.3:
                    u *= 0.3 / sp
                    v *= 0.3 / sp
                pxx -= r * (1.0 / 3.0 + u * u)
                pyy -= r * (1.0 / 3.0 + v * v)
                pxy -= r * u * v
                q = np.sqrt(pxx * pxx + pyy * pyy + 2.0 * pxy * pxy)
                t0 = tau0[x]
                tau = 0.5 * (t0 + np.sqrt(t0 * t0 + 18.0 * cs * cs * q / r))
                om = 1.0 / tau
                usq = 1.5 * (u * u + v * v)
                for i in range(9):
                    cu = 3.0 * (_CXi[i] * u + _CYi[i] * v)
                    feq = _W32[i] * r * (1.0 + cu + 0.5 * cu * cu - usq)
                    fout[i, y, x] = fl[i] + om * (feq - fl[i])
                rho_o[y, x] = r
                ux_o[y, x] = u
                uy_o[y, x] = v
        # pęd przekazany ciału: populacja lecąca w -c_i odbija się jako c_i → 2*f*(-c_i)
        force[0] += fxs.sum()
        force[1] += fys.sum()
