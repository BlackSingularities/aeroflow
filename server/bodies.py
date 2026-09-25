"""Obiekty w tunelu: każde ciało ma własną geometrię i przekształcenie (przesunięcie, obrót).

Maska przeszkód symulacji to suma rastrów wszystkich ciał. Raster każdego ciała jest liczony tylko
w jego prostokącie ograniczającym i buforowany do czasu zmiany ciała.
"""
from __future__ import annotations

import itertools
import math

import numpy as np

_ids = itertools.count(1)

KIND_LABELS = {
    "airfoil": "Skrzydło",
    "polygon": "Kształt",
    "circle": "Koło",
    "rect": "Prostokąt",
    "paint": "Ściana",
}


def naca_points(chord, t=0.12, m=0.04, p=0.4, n=60):
    """Profil NACA 4-cyfrowy w układzie ekranowym (y w dół), nosek w (0, 0), kąt natarcia 0."""
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
    py = -np.concatenate([yu, yl[::-1]]) * chord  # ekran: góra profilu = mniejsze y
    return np.stack([px, py], 1)


def rotate(pts, angle):
    """Obrót w układzie ekranowym; dodatni kąt opuszcza krawędź spływu (nos w górę)."""
    c, s = math.cos(angle), math.sin(angle)
    return np.stack([pts[:, 0] * c - pts[:, 1] * s, pts[:, 0] * s + pts[:, 1] * c], 1)


def fill_polygon_local(pts, x0, y0, w, h):
    """Wypełnienie wielokąta w podsiatce [x0, x0+w) × [y0, y0+h) — test parzystości przecięć."""
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float32)
    x = xx + x0 + 0.5
    y = yy + y0 + 0.5
    inside = np.zeros((h, w), bool)
    p = np.asarray(pts, np.float32)
    j = len(p) - 1
    for i in range(len(p)):
        xi, yi = p[i]
        xj, yj = p[j]
        if yi != yj:
            cond = (yi > y) != (yj > y)
            xint = (xj - xi) * (y - yi) / (yj - yi) + xi
            inside ^= cond & (x < xint)
        j = i
    return inside


class Body:
    def __init__(self, kind: str, pivot, angle: float = 0.0, **kw):
        self.id = next(_ids)
        self.kind = kind
        self.px, self.py = float(pivot[0]), float(pivot[1])
        self.angle = float(angle)
        self.base: np.ndarray | None = kw.get("base")  # punkty lokalne (wielokąty) względem osi obrotu
        self.r = float(kw.get("r", 0.0))  # koło
        self.mask: np.ndarray | None = kw.get("mask")  # ściana malowana: maska lokalna
        self.mask_origin = kw.get("mask_origin", (0.0, 0.0))  # położenie środka maski względem osi
        self.params = dict(kw.get("params", {}))
        self._cache = None

    # ------------------------------------------------------------ fabryki
    @staticmethod
    def new_id() -> int:
        return next(_ids)

    @staticmethod
    def airfoil(x, y, chord, angle, t=0.12, m=0.04):
        """Oś obrotu w 1/4 cięciwy — tak jak obraca się prawdziwy profil."""
        b = Body("airfoil", (0, 0), angle, params={"chord": chord, "t": t, "m": m})
        b._rebuild_airfoil()
        piv = rotate(np.array([[chord * 0.25, 0.0]]), angle)[0]
        b.px, b.py = x + piv[0], y + piv[1]
        return b

    def _rebuild_airfoil(self):
        c = self.params["chord"]
        pts = naca_points(c, self.params["t"], self.params["m"])
        pts[:, 0] -= 0.25 * c
        self.base = pts
        self._cache = None

    @staticmethod
    def polygon(pts, kind="polygon"):
        p = np.asarray(pts, np.float64)
        cx, cy = p.mean(0)
        return Body(kind, (cx, cy), 0.0, base=p - [cx, cy])

    @staticmethod
    def rect(x0, y0, x1, y1):
        xa, xb = sorted((x0, x1))
        ya, yb = sorted((y0, y1))
        return Body.polygon([(xa, ya), (xb, ya), (xb, yb), (xa, yb)], kind="rect")

    @staticmethod
    def circle(x, y, r):
        return Body("circle", (x, y), 0.0, r=r)

    @staticmethod
    def paint(mask_full: np.ndarray):
        ys, xs = np.nonzero(mask_full)
        if len(xs) == 0:
            return None
        x0, x1, y0, y1 = xs.min(), xs.max() + 1, ys.min(), ys.max() + 1
        local = mask_full[y0:y1, x0:x1].copy()
        cx, cy = xs.mean() + 0.5, ys.mean() + 0.5
        return Body("paint", (cx, cy), 0.0, mask=local, mask_origin=(x0 - cx, y0 - cy))

    # ------------------------------------------------------------ geometria
    def points(self):
        return rotate(self.base, self.angle) + [self.px, self.py]

    def bbox(self):
        if self.kind == "circle":
            return self.px - self.r, self.py - self.r, self.px + self.r, self.py + self.r
        if self.kind == "paint":
            h, w = self.mask.shape
            ox, oy = self.mask_origin
            corners = np.array([[ox, oy], [ox + w, oy], [ox, oy + h], [ox + w, oy + h]], float)
            c = rotate(corners, self.angle) + [self.px, self.py]
        else:
            c = self.points()
        return c[:, 0].min(), c[:, 1].min(), c[:, 0].max(), c[:, 1].max()

    def raster(self, nx, ny):
        """(x0, y0, maska lokalna) przycięte do siatki; buforowane."""
        key = (nx, ny, self.px, self.py, self.angle, self.r, id(self.base), id(self.mask))
        if self._cache and self._cache[0] == key:
            return self._cache[1]
        bx0, by0, bx1, by1 = self.bbox()
        x0, y0 = max(0, int(math.floor(bx0)) - 1), max(0, int(math.floor(by0)) - 1)
        x1, y1 = min(nx, int(math.ceil(bx1)) + 2), min(ny, int(math.ceil(by1)) + 2)
        if x1 <= x0 or y1 <= y0:
            out = (0, 0, np.zeros((0, 0), bool))
        else:
            w, h = x1 - x0, y1 - y0
            if self.kind == "circle":
                yy, xx = np.mgrid[y0:y1, x0:x1].astype(np.float32)
                m = (xx - self.px) ** 2 + (yy - self.py) ** 2 <= self.r * self.r
            elif self.kind == "paint":
                # próbkowanie odwrotne: piksel siatki → układ lokalny maski
                yy, xx = np.mgrid[y0:y1, x0:x1].astype(np.float32)
                lx, ly = xx + 0.5 - self.px, yy + 0.5 - self.py
                c, s = math.cos(-self.angle), math.sin(-self.angle)
                ux = lx * c - ly * s - self.mask_origin[0]
                uy = lx * s + ly * c - self.mask_origin[1]
                mh, mw = self.mask.shape
                ix, iy = np.floor(ux).astype(int), np.floor(uy).astype(int)
                ok = (ix >= 0) & (iy >= 0) & (ix < mw) & (iy < mh)
                m = np.zeros((h, w), bool)
                m[ok] = self.mask[iy[ok], ix[ok]]
            else:
                m = fill_polygon_local(self.points(), x0, y0, w, h)
            out = (x0, y0, m)
        self._cache = (key, out)
        return out

    def to_paint(self, nx, ny):
        """Zamienia ciało na ścianę malowaną (np. po częściowym wymazaniu gumką)."""
        full = np.zeros((ny, nx), bool)
        x0, y0, m = self.raster(nx, ny)
        full[y0:y0 + m.shape[0], x0:x0 + m.shape[1]] = m
        return full

    def scaled(self, k: float) -> "Body":
        """Kopia przeskalowana (zmiana rozdzielczości siatki)."""
        b = self.copy()
        b.px *= k
        b.py *= k
        b.r *= k
        if b.base is not None:
            b.base = b.base * k
        if b.kind == "airfoil":
            b.params["chord"] *= k
        if b.mask is not None:
            h, w = b.mask.shape
            nh, nw = max(1, round(h * k)), max(1, round(w * k))
            yi = np.clip((np.arange(nh) / k).astype(int), 0, h - 1)
            xi = np.clip((np.arange(nw) / k).astype(int), 0, w - 1)
            b.mask = b.mask[np.ix_(yi, xi)]
            b.mask_origin = (b.mask_origin[0] * k, b.mask_origin[1] * k)
        b._cache = None
        return b

    def copy(self) -> "Body":
        b = Body(self.kind, (self.px, self.py), self.angle, r=self.r,
                 base=None if self.base is None else self.base.copy(),
                 mask=None if self.mask is None else self.mask.copy(),
                 mask_origin=self.mask_origin, params=self.params)
        b.id = self.id
        return b

    def flip(self):
        if self.base is not None:
            self.base = self.base * [1, -1]
            self.angle = -self.angle
        elif self.mask is not None:
            self.mask = self.mask[::-1].copy()
            h = self.mask.shape[0]
            self.mask_origin = (self.mask_origin[0], -self.mask_origin[1] - h)
            self.angle = -self.angle
        self._cache = None

    def info(self) -> dict:
        d = {
            "id": self.id,
            "kind": self.kind,
            "label": KIND_LABELS[self.kind],
            "x": round(self.px, 2),
            "y": round(self.py, 2),
            "angle": round(math.degrees(self.angle), 2),
            "bbox": [round(v, 1) for v in self.bbox()],
        }
        if self.kind == "circle":
            d["r"] = round(self.r, 2)
        elif self.base is not None:
            pts = self.points()
            step = max(1, len(pts) // 80)
            d["outline"] = [[round(float(x), 1), round(float(y), 1)] for x, y in pts[::step]]
        if self.kind == "airfoil":
            p = self.params
            d["naca"] = f"{round(p['m'] * 100)}4{round(p['t'] * 100):02d}"
            d["t"], d["m"], d["chord"] = p["t"], p["m"], round(p["chord"], 1)
        return d
