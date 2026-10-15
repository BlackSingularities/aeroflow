//! Wizualizacja przepływu: cząstki-znaczniki ze śladami, linie prądu i wektory prędkości.
//! Wszystkie współrzędne w komórkach widocznej siatki (środek komórki (i, j) = (i+½, j+½)).

use eframe::egui::{epaint::Vertex, Color32, Mesh, Pos2, Vec2};

use crate::sim::Frame;

/// Prędkość w punkcie (interpolacja dwuliniowa); `None` w ciele lub poza obszarem.
pub fn sample(f: &Frame, x: f32, y: f32) -> Option<(f32, f32)> {
    let gx = x - 0.5;
    let gy = y - 0.5;
    if gx < 0.0 || gy < 0.0 || gx >= (f.w - 1) as f32 || gy >= (f.h - 1) as f32 {
        return None;
    }
    let (x0, y0) = (gx as usize, gy as usize);
    let (fx, fy) = (gx - x0 as f32, gy - y0 as f32);
    let c = y0 * f.w + x0;
    let idx = [c, c + 1, c + f.w, c + f.w + 1];
    if f.labels[c] != 0 && f.labels[c + 1] != 0 && f.labels[c + f.w] != 0 && f.labels[c + f.w + 1] != 0 {
        return None;
    }
    let wts = [(1.0 - fx) * (1.0 - fy), fx * (1.0 - fy), (1.0 - fx) * fy, fx * fy];
    let (mut u, mut v) = (0.0, 0.0);
    for k in 0..4 {
        u += f.ux[idx[k]] * wts[k];
        v += f.uy[idx[k]] * wts[k];
    }
    let cx = x as usize;
    let cy = y as usize;
    if cx < f.w && cy < f.h && f.labels[cy * f.w + cx] != 0 {
        return None;
    }
    Some((u, v))
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}

pub struct Particles {
    pos: Vec<[f32; 2]>,
    age: Vec<u32>,
    life: Vec<u32>,
    /// Ślady: `trail_len` ostatnich pozycji na cząstkę (bufor cykliczny).
    trail: Vec<[f32; 2]>,
    trail_len: usize,
    head: usize,
    filled: usize,
    rng: Rng,
    dims: (usize, usize),
}

impl Default for Particles {
    fn default() -> Self {
        Self {
            pos: Vec::new(),
            age: Vec::new(),
            life: Vec::new(),
            trail: Vec::new(),
            trail_len: 1,
            head: 0,
            filled: 0,
            rng: Rng(0x2545F4914F6CDD1D),
            dims: (0, 0),
        }
    }
}

impl Particles {
    pub fn clear(&mut self) {
        self.pos.clear();
    }

    fn spawn(&mut self, i: usize, f: &Frame, at_inlet: bool) {
        for _ in 0..20 {
            let x = if at_inlet { self.rng.next() * 2.0 } else { self.rng.next() * f.w as f32 };
            let y = self.rng.next() * f.h as f32;
            let c = (y as usize).min(f.h - 1) * f.w + (x as usize).min(f.w - 1);
            if f.labels[c] == 0 {
                self.pos[i] = [x, y];
                break;
            }
        }
        self.age[i] = 0;
        self.life[i] = 200 + (self.rng.next() * 400.0) as u32;
        let p = self.pos[i];
        for k in 0..self.trail_len {
            self.trail[i * self.trail_len + k] = p;
        }
    }

    /// Przesuwa cząstki o `steps` kroków solvera (RK2).
    pub fn update(&mut self, f: &Frame, count: usize, trail_len: usize, steps: f32) {
        let trail_len = trail_len.max(1);
        if self.pos.len() != count || self.dims != (f.w, f.h) || self.trail_len != trail_len {
            self.dims = (f.w, f.h);
            self.trail_len = trail_len;
            self.pos = vec![[0.0, 0.0]; count];
            self.age = vec![0; count];
            self.life = vec![0; count];
            self.trail = vec![[0.0, 0.0]; count * trail_len];
            self.head = 0;
            self.filled = 0;
            for i in 0..count {
                self.spawn(i, f, false);
                self.age[i] = (self.rng.next() * self.life[i] as f32) as u32;
            }
        }
        self.head = (self.head + 1) % self.trail_len;
        self.filled = (self.filled + 1).min(self.trail_len);
        for i in 0..self.pos.len() {
            let [x, y] = self.pos[i];
            let mut dead = self.age[i] > self.life[i];
            let mut np = [x, y];
            // podkroki punktu środkowego po najwyżej 16 krokach solvera — cząstki nadążają za
            // przepływem także wtedy, gdy na klatkę przypada wiele kroków (GPU)
            let subs = (steps / 16.0).ceil().max(1.0);
            let h = steps / subs;
            for _ in 0..subs as usize {
                if dead {
                    break;
                }
                let [x, y] = np;
                match sample(f, x, y) {
                    Some((u, v)) => match sample(f, x + 0.5 * u * h, y + 0.5 * v * h) {
                        Some((u2, v2)) => np = [x + u2 * h, y + v2 * h],
                        None => dead = true,
                    },
                    None => dead = true,
                }
            }
            self.age[i] += 1;
            if np[0] >= f.w as f32 - 0.6 || np[0] < 0.0 || np[1] < 0.0 || np[1] >= f.h as f32 {
                dead = true;
            }
            if dead {
                let inlet = np[0] >= f.w as f32 - 0.6 && self.rng.next() < 0.7;
                self.spawn(i, f, inlet);
            } else {
                self.pos[i] = np;
                self.trail[i * self.trail_len + self.head] = np;
            }
        }
    }

    /// Siatka trójkątów ze śladami. `to_screen` zamienia współrzędne siatki na ekranowe.
    pub fn mesh(&self, to_screen: impl Fn([f32; 2]) -> Pos2, width: f32, color: impl Fn([f32; 2]) -> Color32) -> Mesh {
        let mut mesh = Mesh::default();
        let n = self.trail_len;
        let segs = self.filled.saturating_sub(1).min(n - 1);
        mesh.reserve_triangles(self.pos.len() * segs.max(1) * 2);
        mesh.reserve_vertices(self.pos.len() * segs.max(1) * 4);
        for i in 0..self.pos.len() {
            let base = color(self.pos[i]);
            if segs == 0 {
                let p = to_screen(self.pos[i]);
                quad(&mut mesh, p - Vec2::new(0.8, 0.0), p + Vec2::new(0.8, 0.0), width, base, base);
                continue;
            }
            for s in 0..segs {
                // od najnowszego do najstarszego
                let ia = (self.head + n - s) % n;
                let ib = (self.head + n - s - 1) % n;
                let a = self.trail[i * n + ia];
                let b = self.trail[i * n + ib];
                if (a[0] - b[0]).abs() > 8.0 || (a[1] - b[1]).abs() > 8.0 {
                    break; // przeskok po odrodzeniu
                }
                let fa = 1.0 - s as f32 / segs as f32;
                let fb = 1.0 - (s + 1) as f32 / segs as f32;
                quad(&mut mesh, to_screen(a), to_screen(b), width, base.gamma_multiply(fa), base.gamma_multiply(fb));
            }
        }
        mesh
    }
}

fn quad(mesh: &mut Mesh, a: Pos2, b: Pos2, width: f32, ca: Color32, cb: Color32) {
    let d = b - a;
    let len = d.length();
    let n = if len < 1e-4 { Vec2::new(0.0, 0.5 * width) } else { Vec2::new(-d.y, d.x) / len * 0.5 * width };
    let i = mesh.vertices.len() as u32;
    let uv = Pos2::ZERO;
    mesh.vertices.push(Vertex { pos: a + n, uv, color: ca });
    mesh.vertices.push(Vertex { pos: a - n, uv, color: ca });
    mesh.vertices.push(Vertex { pos: b + n, uv, color: cb });
    mesh.vertices.push(Vertex { pos: b - n, uv, color: cb });
    mesh.indices.extend_from_slice(&[i, i + 1, i + 2, i + 1, i + 3, i + 2]);
}

/// Linie prądu o w przybliżeniu równym odstępie (siatka zajętości, uproszczony algorytm Jobard–Lefer).
pub fn streamlines(f: &Frame, spacing: f32) -> Vec<Vec<[f32; 2]>> {
    let spacing = spacing.max(2.0);
    let cell = spacing * 0.5;
    let gw = (f.w as f32 / cell).ceil() as usize + 1;
    let gh = (f.h as f32 / cell).ceil() as usize + 1;
    let mut occ = vec![0u32; gw * gh];
    let mut lines = Vec::new();
    let max_steps = ((f.w + f.h) * 3).min(4000);
    let speed_ref = {
        let mut m = 0f32;
        for i in (0..f.ux.len()).step_by(7) {
            m = m.max(f.ux[i].hypot(f.uy[i]));
        }
        m.max(1e-9)
    };
    let trace = |sx: f32, sy: f32, dir: f32, id: u32, occ: &mut Vec<u32>, own: &mut Vec<usize>| -> Vec<[f32; 2]> {
        let mut pts = Vec::new();
        let (mut x, mut y) = (sx, sy);
        for _ in 0..max_steps {
            let Some((u, v)) = sample(f, x, y) else { break };
            let sp = u.hypot(v);
            if sp < 1e-3 * speed_ref {
                break;
            }
            let h = 0.6 * dir / sp;
            let Some((u2, v2)) = sample(f, x + 0.5 * h * u, y + 0.5 * h * v) else { break };
            let sp2 = u2.hypot(v2).max(1e-12);
            let h2 = 0.6 * dir / sp2;
            x += h2 * u2;
            y += h2 * v2;
            let gx = (x / cell) as usize;
            let gy = (y / cell) as usize;
            if gx >= gw || gy >= gh {
                break;
            }
            let g = gy * gw + gx;
            if occ[g] != 0 && occ[g] != id {
                break;
            }
            if occ[g] == 0 {
                occ[g] = id;
                own.push(g);
            }
            pts.push([x, y]);
        }
        pts
    };
    let step = spacing.max(3.0);
    let mut sy = step * 0.5;
    let mut seeds = Vec::new();
    while sy < f.h as f32 {
        let mut sx = step * 0.5;
        while sx < f.w as f32 {
            seeds.push([sx, sy]);
            sx += step;
        }
        sy += step;
    }
    // najpierw ziarna z lewej krawędzi — linie wychodzące z wlotu wyglądają najczytelniej
    seeds.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap_or(std::cmp::Ordering::Equal));
    for [sx, sy] in seeds {
        let gx = (sx / cell) as usize;
        let gy = (sy / cell) as usize;
        if gx >= gw || gy >= gh || occ[gy * gw + gx] != 0 || sample(f, sx, sy).is_none() {
            continue;
        }
        let id = lines.len() as u32 + 1;
        occ[gy * gw + gx] = id;
        let mut own = vec![gy * gw + gx];
        let mut back = trace(sx, sy, -1.0, id, &mut occ, &mut own);
        let fwd = trace(sx, sy, 1.0, id, &mut occ, &mut own);
        if back.len() + fwd.len() < 8 {
            for g in own {
                occ[g] = 0;
            }
            continue;
        }
        back.reverse();
        back.push([sx, sy]);
        back.extend(fwd);
        lines.push(back);
    }
    lines
}
