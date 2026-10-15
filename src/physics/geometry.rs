//! Geometria ciał w układzie świata: metry, oś x w prawo, oś y w górę.
//!
//! Każde ciało ma kształt w układzie lokalnym, położenie osi obrotu `pos` i kąt `angle` w stopniach
//! (dodatni = nosek w górę, czyli obrót zgodny z ruchem wskazówek zegara — jak kąt natarcia).
//! Do rasteryzacji i liczenia odległości ściany (warunek Bouzidiego) ciało jest „przygotowywane”
//! do postaci [`Prepared`] z gotowym obrysem i macierzą obrotu.

use serde::{Deserialize, Serialize};

pub type V2 = [f64; 2];

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Naca {
    /// Maksymalne wygięcie (ułamek cięciwy), pierwsza cyfra / 100.
    pub m: f64,
    /// Położenie maksymalnego wygięcia (ułamek cięciwy), druga cyfra / 10.
    pub p: f64,
    /// Względna grubość, ostatnie dwie cyfry / 100.
    pub t: f64,
}

impl Naca {
    pub const fn new(m: f64, p: f64, t: f64) -> Self {
        Self { m, p, t }
    }

    /// Oznaczenie 4-cyfrowe, np. „2412”.
    pub fn designation(&self) -> String {
        let m = (self.m * 100.0).round() as i32;
        let p = if m == 0 { 0 } else { (self.p * 10.0).round() as i32 };
        format!("{}{}{:02}", m, p, (self.t * 100.0).round() as i32)
    }

    /// Parsuje oznaczenie 4-cyfrowe („0012”, „2412”, „4415”).
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim().trim_start_matches(|c: char| c.is_alphabetic() || c.is_whitespace());
        if s.len() != 4 || !s.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        let d: Vec<f64> = s.chars().map(|c| c.to_digit(10).unwrap() as f64).collect();
        let t = (d[2] * 10.0 + d[3]) / 100.0;
        if !(0.01..=0.40).contains(&t) {
            return None;
        }
        let m = d[0] / 100.0;
        let p = if m > 0.0 { (d[1] / 10.0).max(0.1) } else { 0.4 };
        Some(Self { m, p, t })
    }

    /// Linia szkieletowa y_c(x) i jej nachylenie, x ∈ [0, 1].
    pub fn camber(&self, x: f64) -> (f64, f64) {
        let (m, p) = (self.m, self.p.clamp(0.05, 0.95));
        if m <= 0.0 {
            return (0.0, 0.0);
        }
        if x < p {
            (m / (p * p) * (2.0 * p * x - x * x), 2.0 * m / (p * p) * (p - x))
        } else {
            let k = (1.0 - p).powi(2);
            (m / k * ((1.0 - 2.0 * p) + 2.0 * p * x - x * x), 2.0 * m / k * (p - x))
        }
    }

    /// Obrys profilu o cięciwie 1 (nosek w (0,0)), od krawędzi spływu po stronie górnej do
    /// krawędzi spływu po stronie dolnej. Rozkład punktów cosinusowy (gęsty przy nosku).
    pub fn outline(&self, n: usize) -> Vec<V2> {
        let mut upper = Vec::with_capacity(n);
        let mut lower = Vec::with_capacity(n);
        for k in 0..n {
            let beta = std::f64::consts::PI * k as f64 / (n - 1) as f64;
            let x = 0.5 * (1.0 - beta.cos());
            // zamknięta krawędź spływu (współczynnik −0,1036)
            let yt = 5.0 * self.t * (0.2969 * x.sqrt() - 0.1260 * x - 0.3516 * x * x + 0.2843 * x.powi(3) - 0.1036 * x.powi(4));
            let (yc, dyc) = self.camber(x);
            let th = dyc.atan();
            upper.push([x - yt * th.sin(), yc + yt * th.cos()]);
            lower.push([x + yt * th.sin(), yc - yt * th.cos()]);
        }
        let mut pts: Vec<V2> = upper.into_iter().rev().collect();
        pts.extend(lower.into_iter().skip(1).take(n - 2));
        pts
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    /// Punkty w układzie lokalnym ciała [m].
    pub pts: Vec<V2>,
    /// Promień pędzla [m].
    pub r: f64,
    /// Pociągnięcie gumką (odejmuje materiał).
    pub erase: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Shape {
    /// Profil NACA; oś obrotu w ¼ cięciwy.
    Airfoil {
        chord: f64,
        naca: Naca,
    },
    Circle {
        r: f64,
    },
    Ellipse {
        a: f64,
        b: f64,
    },
    /// Prostokąt o bokach `w × h` z zaokrągleniem narożników `radius`.
    Rect {
        w: f64,
        h: f64,
        radius: f64,
    },
    /// Wielokąt w układzie lokalnym.
    Polygon {
        pts: Vec<V2>,
    },
    /// Ściana malowana pędzlem (suma kapsuł, minus pociągnięcia gumki).
    Wall {
        strokes: Vec<Stroke>,
    },
}

impl Shape {
    pub fn kind_label(&self) -> &'static str {
        match self {
            Shape::Airfoil { .. } => "Profil",
            Shape::Circle { .. } => "Walec",
            Shape::Ellipse { .. } => "Elipsa",
            Shape::Rect { .. } => "Prostokąt",
            Shape::Polygon { .. } => "Kształt",
            Shape::Wall { .. } => "Ściana",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Body {
    pub id: u64,
    pub name: String,
    pub shape: Shape,
    /// Oś obrotu w układzie świata [m].
    pub pos: V2,
    /// Kąt [°]; dodatni podnosi nosek (kąt natarcia).
    pub angle: f64,
    /// Odbicie lustrzane względem lokalnej osi x.
    #[serde(default)]
    pub mirror: bool,
    /// Ciało zablokowane przed przypadkowym przesunięciem myszą.
    #[serde(default)]
    pub locked: bool,
}

impl Body {
    pub fn new(id: u64, shape: Shape, pos: V2, angle: f64) -> Self {
        let name = shape.kind_label().to_string();
        Self { id, name, shape, pos, angle, mirror: false, locked: false }
    }

    /// Długość odniesienia [m]: cięciwa dla profilu, średnica dla walca, wymiar czołowy dla ciał tępych.
    pub fn ref_length(&self) -> f64 {
        match &self.shape {
            Shape::Airfoil { chord, .. } => *chord,
            Shape::Circle { r } => 2.0 * r,
            Shape::Ellipse { b, a } => {
                // wymiar prostopadły do strumienia po obrocie
                let t = self.angle.to_radians();
                2.0 * ((b * t.cos()).powi(2) + (a * t.sin()).powi(2)).sqrt()
            }
            _ => {
                let p = self.prepare();
                let [x0, y0, x1, y1] = p.bbox;
                // ciało smukłe wzdłuż strumienia — długość; tępe — wysokość czołowa
                if (x1 - x0) > 3.0 * (y1 - y0) {
                    x1 - x0
                } else {
                    y1 - y0
                }
            }
        }
    }

    /// Nazwa przyjętej długości odniesienia.
    pub fn ref_length_label(&self) -> &'static str {
        match &self.shape {
            Shape::Airfoil { .. } => "cięciwa c",
            Shape::Circle { .. } => "średnica D",
            _ => "wymiar odniesienia L",
        }
    }

    pub fn prepare(&self) -> Prepared {
        let th = -self.angle.to_radians();
        let (s, c) = th.sin_cos();
        let geom = match &self.shape {
            Shape::Airfoil { chord, naca } => {
                let pts = naca.outline(90).into_iter().map(|[x, y]| [(x - 0.25) * chord, y * chord]).collect();
                Geom::Poly(pts)
            }
            Shape::Circle { r } => Geom::Circle(*r),
            Shape::Ellipse { a, b } => Geom::Ellipse(*a, *b),
            Shape::Rect { w, h, radius } => Geom::RRect(w * 0.5, h * 0.5, radius.min(0.5 * w.min(*h))),
            Shape::Polygon { pts } => Geom::Poly(pts.clone()),
            Shape::Wall { strokes } => Geom::Wall(strokes.clone()),
        };
        let mut p = Prepared { geom, pos: self.pos, cos: c, sin: s, mirror: self.mirror, bbox: [0.0; 4] };
        p.bbox = p.compute_bbox();
        p
    }

    /// Obrys w układzie świata (do rysowania). Ściany nie mają obrysu wektorowego.
    pub fn outline_world(&self) -> Vec<V2> {
        let p = self.prepare();
        p.local_outline().into_iter().map(|q| p.to_world(q)).collect()
    }

    pub fn contains(&self, w: V2) -> bool {
        self.prepare().contains(w)
    }

    /// Przesunięcie całego ciała w poziomie i pionie.
    pub fn translate(&mut self, d: V2) {
        self.pos[0] += d[0];
        self.pos[1] += d[1];
    }

    /// Skala kształtu względem osi obrotu.
    pub fn scale(&mut self, k: f64) {
        match &mut self.shape {
            Shape::Airfoil { chord, .. } => *chord *= k,
            Shape::Circle { r } => *r *= k,
            Shape::Ellipse { a, b } => {
                *a *= k;
                *b *= k;
            }
            Shape::Rect { w, h, radius } => {
                *w *= k;
                *h *= k;
                *radius *= k;
            }
            Shape::Polygon { pts } => pts.iter_mut().for_each(|p| *p = [p[0] * k, p[1] * k]),
            Shape::Wall { strokes } => strokes.iter_mut().for_each(|s| {
                s.r *= k;
                s.pts.iter_mut().for_each(|p| *p = [p[0] * k, p[1] * k]);
            }),
        }
    }

    /// Czy obrót ma sens (walec jest symetryczny).
    pub fn rotatable(&self) -> bool {
        !matches!(self.shape, Shape::Circle { .. })
    }
}

#[derive(Clone, Debug)]
pub enum Geom {
    Poly(Vec<V2>),
    Circle(f64),
    Ellipse(f64, f64),
    RRect(f64, f64, f64),
    Wall(Vec<Stroke>),
}

/// Ciało gotowe do szybkich testów przynależności punktu.
#[derive(Clone, Debug)]
pub struct Prepared {
    pub geom: Geom,
    pub pos: V2,
    cos: f64,
    sin: f64,
    mirror: bool,
    /// Prostokąt ograniczający w układzie świata [x0, y0, x1, y1].
    pub bbox: [f64; 4],
}

impl Prepared {
    pub fn to_world(&self, l: V2) -> V2 {
        let y = if self.mirror { -l[1] } else { l[1] };
        [self.pos[0] + l[0] * self.cos - y * self.sin, self.pos[1] + l[0] * self.sin + y * self.cos]
    }

    pub fn to_local(&self, w: V2) -> V2 {
        let dx = w[0] - self.pos[0];
        let dy = w[1] - self.pos[1];
        let x = dx * self.cos + dy * self.sin;
        let y = -dx * self.sin + dy * self.cos;
        [x, if self.mirror { -y } else { y }]
    }

    pub fn contains(&self, w: V2) -> bool {
        let [b0, b1, b2, b3] = self.bbox;
        if w[0] < b0 || w[0] > b2 || w[1] < b1 || w[1] > b3 {
            return false;
        }
        let l = self.to_local(w);
        match &self.geom {
            Geom::Poly(pts) => point_in_polygon(l, pts),
            Geom::Circle(r) => l[0] * l[0] + l[1] * l[1] <= r * r,
            Geom::Ellipse(a, b) => (l[0] / a).powi(2) + (l[1] / b).powi(2) <= 1.0,
            Geom::RRect(hw, hh, r) => {
                let qx = l[0].abs() - (hw - r);
                let qy = l[1].abs() - (hh - r);
                let out = qx.max(0.0).hypot(qy.max(0.0));
                out + qx.max(qy).min(0.0) - r <= 0.0
            }
            Geom::Wall(strokes) => {
                let mut inside = false;
                for s in strokes {
                    if inside == !s.erase {
                        continue; // pociągnięcie nie zmieni stanu
                    }
                    if stroke_contains(s, l) {
                        inside = !s.erase;
                    }
                }
                inside
            }
        }
    }

    /// Obrys w układzie lokalnym (pusty dla ścian).
    pub fn local_outline(&self) -> Vec<V2> {
        match &self.geom {
            Geom::Poly(pts) => pts.clone(),
            Geom::Circle(r) => ellipse_pts(*r, *r, 96),
            Geom::Ellipse(a, b) => ellipse_pts(*a, *b, 96),
            Geom::RRect(hw, hh, r) => {
                if *r <= 1e-12 {
                    vec![[-hw, -hh], [*hw, -hh], [*hw, *hh], [-hw, *hh]]
                } else {
                    let mut v = Vec::new();
                    let corners =
                        [(hw - r, hh - r, 0.0), (-(hw - r), hh - r, 90.0), (-(hw - r), -(hh - r), 180.0), (hw - r, -(hh - r), 270.0)];
                    for (cx, cy, a0) in corners {
                        for k in 0..=8 {
                            let a = (a0 + 90.0 * k as f64 / 8.0_f64).to_radians();
                            v.push([cx + r * a.cos(), cy + r * a.sin()]);
                        }
                    }
                    v
                }
            }
            Geom::Wall(_) => Vec::new(),
        }
    }

    fn compute_bbox(&self) -> [f64; 4] {
        let pts: Vec<V2> = match &self.geom {
            Geom::Wall(strokes) => {
                let mut v = Vec::new();
                for s in strokes.iter().filter(|s| !s.erase) {
                    for p in &s.pts {
                        for c in [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]] {
                            v.push(self.to_world([p[0] + c[0] * s.r, p[1] + c[1] * s.r]));
                        }
                    }
                }
                v
            }
            _ => self.local_outline().into_iter().map(|p| self.to_world(p)).collect(),
        };
        if pts.is_empty() {
            return [self.pos[0], self.pos[1], self.pos[0], self.pos[1]];
        }
        let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
        for p in pts {
            b[0] = b[0].min(p[0]);
            b[1] = b[1].min(p[1]);
            b[2] = b[2].max(p[0]);
            b[3] = b[3].max(p[1]);
        }
        // zapas na krzywizny między punktami obrysu
        let pad = 1e-3 * ((b[2] - b[0]) + (b[3] - b[1]));
        [b[0] - pad, b[1] - pad, b[2] + pad, b[3] + pad]
    }
}

fn ellipse_pts(a: f64, b: f64, n: usize) -> Vec<V2> {
    (0..n)
        .map(|k| {
            let t = std::f64::consts::TAU * k as f64 / n as f64;
            [a * t.cos(), b * t.sin()]
        })
        .collect()
}

fn stroke_contains(s: &Stroke, p: V2) -> bool {
    let r2 = s.r * s.r;
    if s.pts.len() == 1 {
        return dist2(p, s.pts[0]) <= r2;
    }
    s.pts.windows(2).any(|w| seg_dist2(p, w[0], w[1]) <= r2)
}

fn dist2(a: V2, b: V2) -> f64 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)
}

pub fn seg_dist2(p: V2, a: V2, b: V2) -> f64 {
    let d = [b[0] - a[0], b[1] - a[1]];
    let l2 = d[0] * d[0] + d[1] * d[1];
    let t = if l2 == 0.0 { 0.0 } else { (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / l2).clamp(0.0, 1.0) };
    dist2(p, [a[0] + t * d[0], a[1] + t * d[1]])
}

/// Test parzystości przecięć.
pub fn point_in_polygon(p: V2, pts: &[V2]) -> bool {
    let mut inside = false;
    let n = pts.len();
    if n < 3 {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = (pts[i][0], pts[i][1]);
        let (xj, yj) = (pts[j][0], pts[j][1]);
        if (yi > p[1]) != (yj > p[1]) && p[0] < (xj - xi) * (p[1] - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Pole wielokąta ze znakiem (dodatnie dla obiegu przeciwnego do wskazówek zegara).
pub fn signed_area(pts: &[V2]) -> f64 {
    let n = pts.len();
    (0..n)
        .map(|i| {
            let a = pts[i];
            let b = pts[(i + 1) % n];
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f64>()
        * 0.5
}

/// Triangulacja wielokąta prostego metodą obcinania uszu. Zwraca indeksy trójkątów.
pub fn triangulate(pts: &[V2]) -> Vec<[usize; 3]> {
    let n = pts.len();
    if n < 3 {
        return Vec::new();
    }
    let mut idx: Vec<usize> = (0..n).collect();
    if signed_area(pts) < 0.0 {
        idx.reverse();
    }
    let cross = |a: V2, b: V2, c: V2| (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
    let mut tris = Vec::with_capacity(n - 2);
    let mut guard = 0;
    while idx.len() > 3 && guard < 4 * n * n {
        guard += 1;
        let m = idx.len();
        let mut clipped = false;
        for k in 0..m {
            let (ia, ib, ic) = (idx[(k + m - 1) % m], idx[k], idx[(k + 1) % m]);
            let (a, b, c) = (pts[ia], pts[ib], pts[ic]);
            if cross(a, b, c) <= 1e-18 {
                continue; // wierzchołek wklęsły lub zdegenerowany
            }
            let ear = idx.iter().all(|&j| {
                if j == ia || j == ib || j == ic {
                    return true;
                }
                let p = pts[j];
                !(cross(a, b, p) >= 0.0 && cross(b, c, p) >= 0.0 && cross(c, a, p) >= 0.0)
            });
            if ear {
                tris.push([ia, ib, ic]);
                idx.remove(k);
                clipped = true;
                break;
            }
        }
        if !clipped {
            // wielokąt niepoprawny (samoprzecięcia) — usuń wierzchołek, żeby nie zapętlić
            idx.remove(0);
        }
    }
    if idx.len() == 3 {
        tris.push([idx[0], idx[1], idx[2]]);
    }
    tris
}

/// Upraszcza obrys (usuwa punkty bliższe niż `eps` od poprzedniego).
pub fn simplify(pts: &[V2], eps: f64) -> Vec<V2> {
    let mut out: Vec<V2> = Vec::with_capacity(pts.len());
    for &p in pts {
        if out.last().is_none_or(|&q| dist2(p, q) >= eps * eps) {
            out.push(p);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn naca_designation_roundtrip() {
        for s in ["0012", "2412", "4415", "6409"] {
            assert_eq!(Naca::parse(s).unwrap().designation(), s);
        }
        assert!(Naca::parse("12").is_none());
        assert_eq!(Naca::parse("NACA 2412").unwrap().designation(), "2412");
    }

    #[test]
    fn naca_thickness_matches_spec() {
        let n = Naca::new(0.0, 0.4, 0.12);
        let pts = n.outline(200);
        let max_y = pts.iter().map(|p| p[1]).fold(0.0, f64::max);
        assert!((2.0 * max_y - 0.12).abs() < 0.002, "{max_y}");
    }

    #[test]
    fn airfoil_rotation_raises_nose() {
        let b = Body::new(1, Shape::Airfoil { chord: 1.0, naca: Naca::new(0.0, 0.4, 0.12) }, [0.0, 0.0], 10.0);
        let p = b.prepare();
        let nose = p.to_world([-0.25, 0.0]);
        assert!(nose[1] > 0.0);
        let back = p.to_local(nose);
        assert!((back[0] + 0.25).abs() < 1e-12 && back[1].abs() < 1e-12);
    }

    #[test]
    fn shapes_contain_their_center() {
        let shapes = [
            Shape::Circle { r: 0.1 },
            Shape::Ellipse { a: 0.2, b: 0.05 },
            Shape::Rect { w: 0.2, h: 0.1, radius: 0.02 },
            Shape::Airfoil { chord: 1.0, naca: Naca::new(0.02, 0.4, 0.12) },
            Shape::Wall { strokes: vec![Stroke { pts: vec![[-0.1, 0.0], [0.1, 0.0]], r: 0.02, erase: false }] },
        ];
        for s in shapes {
            let b = Body::new(1, s, [1.0, 2.0], 15.0);
            assert!(b.contains([1.0, 2.0]), "{:?}", b.shape);
            assert!(!b.contains([5.0, 5.0]));
        }
    }

    #[test]
    fn eraser_removes_material() {
        let s = Shape::Wall {
            strokes: vec![
                Stroke { pts: vec![[-1.0, 0.0], [1.0, 0.0]], r: 0.1, erase: false },
                Stroke { pts: vec![[0.0, 0.0]], r: 0.2, erase: true },
            ],
        };
        let b = Body::new(1, s, [0.0, 0.0], 0.0);
        assert!(!b.contains([0.0, 0.0]));
        assert!(b.contains([0.8, 0.0]));
    }

    #[test]
    fn triangulation_covers_area() {
        let pts = Naca::new(0.04, 0.4, 0.12).outline(60);
        let tris = triangulate(&pts);
        let area: f64 = tris.iter().map(|t| signed_area(&[pts[t[0]], pts[t[1]], pts[t[2]]]).abs()).sum();
        assert!((area - signed_area(&pts).abs()).abs() < 1e-9);
    }
}
