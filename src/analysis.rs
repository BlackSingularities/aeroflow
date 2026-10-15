//! Analiza wyników: historia sił, współczynniki, liczba Strouhala, sondy i automatyczna biegunowa.

use std::collections::{HashMap, VecDeque};

use crate::physics::geometry::{Body, Shape};
use crate::physics::units::Units;
use crate::sim::Frame;

const MAX_SAMPLES: usize = 30_000;

/// Współczynniki aerodynamiczne i siły jednego ciała w danej chwili.
#[derive(Clone, Copy, Debug, Default)]
pub struct Coeffs {
    pub t: f64,
    pub cd: f64,
    pub cl: f64,
    pub cm: f64,
    /// Siły na metr rozpiętości [N/m].
    pub fx: f64,
    pub fy: f64,
    /// Moment pochylający względem osi obrotu [N·m/m] (dodatni = nosek w górę).
    pub m: f64,
}

pub fn coeffs(units: &Units, body: &Body, lattice: [f64; 3], t: f64) -> Coeffs {
    let l_lb = (body.ref_length() / units.dx).max(1e-9);
    let q = 0.5 * units.u_lb * units.u_lb;
    // moment: dodatni w układzie y-w-górę oznacza obrót przeciwny do wskazówek (nosek w dół)
    let mz = -lattice[2];
    Coeffs {
        t,
        cd: lattice[0] / (q * l_lb),
        cl: lattice[1] / (q * l_lb),
        cm: mz / (q * l_lb * l_lb),
        fx: units.force_per_span(lattice[0]),
        fy: units.force_per_span(lattice[1]),
        m: units.moment_per_span(mz),
    }
}

#[derive(Default)]
pub struct History {
    pub series: HashMap<u64, VecDeque<Coeffs>>,
    last_step: u64,
}

impl History {
    pub fn clear(&mut self) {
        self.series.clear();
        self.last_step = 0;
    }

    pub fn push(&mut self, frame: &Frame, bodies: &[Body]) {
        if frame.steps == self.last_step || frame.paused && frame.steps == 0 {
            return;
        }
        if frame.steps < self.last_step {
            // przepływ wystartował od nowa
            self.series.clear();
        }
        self.last_step = frame.steps;
        if frame.forces.len() != bodies.len() {
            return;
        }
        for (b, f) in bodies.iter().zip(&frame.forces) {
            let c = coeffs(&frame.units, b, f.lattice, frame.time);
            let s = self.series.entry(b.id).or_default();
            if s.back().is_some_and(|l| l.t >= c.t) {
                s.clear();
            }
            s.push_back(c);
            if s.len() > MAX_SAMPLES {
                s.pop_front();
            }
        }
        self.series.retain(|id, _| bodies.iter().any(|b| b.id == *id));
    }

    pub fn get(&self, id: u64) -> Option<&VecDeque<Coeffs>> {
        self.series.get(&id)
    }
}

/// Statystyki z końcowego okna historii.
#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub cd_mean: f64,
    pub cl_mean: f64,
    pub cm_mean: f64,
    pub cl_amp: f64,
    pub cd_amp: f64,
    /// Częstotliwość oscylacji siły nośnej [Hz] i liczba Strouhala.
    pub freq: Option<f64>,
    pub strouhal: Option<f64>,
    /// Czy przebieg jest ustalony (stały lub okresowy).
    pub settled: bool,
    pub window_s: f64,
}

/// Statystyki z ostatnich `window` sekund (lub całości, gdy krótsza).
pub fn stats(series: &VecDeque<Coeffs>, units: &Units, l_ref: f64, window: f64) -> Option<Stats> {
    let last = series.back()?;
    let t_end = last.t;
    let conv = l_ref / units.u_inf.max(1e-12);
    // pomijamy rozbieg po impulsowym starcie (pierwsze 3 c/U), gdy jest już z czego liczyć
    let t_from = if t_end > 6.0 * conv { (t_end - window).max(3.0 * conv) } else { t_end - window };
    let start = series.iter().position(|c| c.t >= t_from).unwrap_or(0);
    let s: Vec<&Coeffs> = series.iter().skip(start).collect();
    if s.len() < 8 {
        return None;
    }
    let cl: Vec<f64> = s.iter().map(|c| c.cl).collect();
    let t: Vec<f64> = s.iter().map(|c| c.t).collect();
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
    let mut st = Stats { window_s: t_end - s[0].t, ..Default::default() };
    // przejścia przez średnią w górę → okres
    let clm = mean(&cl);
    let mut ups = Vec::new();
    for k in 1..cl.len() {
        if cl[k - 1] < clm && cl[k] >= clm {
            let a = (clm - cl[k - 1]) / (cl[k] - cl[k - 1]).max(1e-30);
            ups.push(t[k - 1] + a * (t[k] - t[k - 1]));
        }
    }
    let span = |v: &[f64]| {
        let lo = v.iter().cloned().fold(f64::INFINITY, f64::min);
        let hi = v.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        0.5 * (hi - lo)
    };
    let amp = span(&cl);
    let (i0, i1) = if ups.len() >= 3 && amp > 0.01 {
        // średnie po pełnych okresach — bez obciążenia od niepełnego cyklu
        let f = (ups.len() - 1) as f64 / (ups[ups.len() - 1] - ups[0]);
        st.freq = Some(f);
        st.strouhal = Some(f * l_ref / units.u_inf);
        let a = t.iter().position(|&x| x >= ups[0]).unwrap_or(0);
        let b = t.iter().rposition(|&x| x <= ups[ups.len() - 1]).unwrap_or(t.len() - 1);
        (a, b.max(a + 1))
    } else {
        (0, s.len() - 1)
    };
    let sl = &s[i0..=i1.min(s.len() - 1)];
    st.cd_mean = sl.iter().map(|c| c.cd).sum::<f64>() / sl.len() as f64;
    st.cl_mean = sl.iter().map(|c| c.cl).sum::<f64>() / sl.len() as f64;
    st.cm_mean = sl.iter().map(|c| c.cm).sum::<f64>() / sl.len() as f64;
    st.cl_amp = amp;
    st.cd_amp = span(&sl.iter().map(|c| c.cd).collect::<Vec<_>>());
    // ustalenie: różnica średnich połówek okna mała względem wartości
    let half = s.len() / 2;
    let m1 = s[..half].iter().map(|c| c.cd).sum::<f64>() / half as f64;
    let m2 = s[half..].iter().map(|c| c.cd).sum::<f64>() / (s.len() - half) as f64;
    st.settled = t_end > 8.0 * conv && (m1 - m2).abs() < 0.02 * st.cd_mean.abs().max(0.05) + 0.5 * st.cd_amp;
    Some(st)
}

/// Sonda punktowa: przebieg prędkości i ciśnienia w czasie.
pub struct ProbeSeries {
    pub t: VecDeque<f64>,
    pub speed: VecDeque<f64>,
    pub ux: VecDeque<f64>,
    pub uy: VecDeque<f64>,
    pub p: VecDeque<f64>,
}

impl ProbeSeries {
    pub fn new() -> Self {
        Self { t: VecDeque::new(), speed: VecDeque::new(), ux: VecDeque::new(), uy: VecDeque::new(), p: VecDeque::new() }
    }

    pub fn push(&mut self, t: f64, ux: f64, uy: f64, p: f64) {
        if self.t.back().is_some_and(|&l| l >= t) {
            self.clear();
        }
        self.t.push_back(t);
        self.ux.push_back(ux);
        self.uy.push_back(uy);
        self.speed.push_back(ux.hypot(uy));
        self.p.push_back(p);
        if self.t.len() > MAX_SAMPLES {
            self.t.pop_front();
            self.ux.pop_front();
            self.uy.pop_front();
            self.speed.pop_front();
            self.p.pop_front();
        }
    }

    pub fn clear(&mut self) {
        self.t.clear();
        self.ux.clear();
        self.uy.clear();
        self.speed.clear();
        self.p.clear();
    }
}

/// Widmo amplitudowe (DFT po równomiernym przepróbkowaniu). Zwraca (częstotliwości [Hz], amplitudy).
pub fn spectrum(t: &VecDeque<f64>, v: &VecDeque<f64>, n: usize) -> Option<(Vec<f64>, Vec<f64>)> {
    if t.len() < 32 {
        return None;
    }
    let (t0, t1) = (*t.front()?, *t.back()?);
    if t1 <= t0 {
        return None;
    }
    let dt = (t1 - t0) / (n - 1) as f64;
    let mut xs = Vec::with_capacity(n);
    let mut j = 0;
    for k in 0..n {
        let tk = t0 + k as f64 * dt;
        while j + 1 < t.len() - 1 && t[j + 1] < tk {
            j += 1;
        }
        let a = ((tk - t[j]) / (t[j + 1] - t[j]).max(1e-30)).clamp(0.0, 1.0);
        xs.push(v[j] + a * (v[j + 1] - v[j]));
    }
    let m = xs.iter().sum::<f64>() / n as f64;
    // okno Hanna
    for (k, x) in xs.iter_mut().enumerate() {
        let w = 0.5 * (1.0 - (std::f64::consts::TAU * k as f64 / (n - 1) as f64).cos());
        *x = (*x - m) * w;
    }
    let half = n / 2;
    let mut freqs = Vec::with_capacity(half);
    let mut amps = Vec::with_capacity(half);
    for kf in 1..half {
        let (mut re, mut im) = (0.0, 0.0);
        let w = std::f64::consts::TAU * kf as f64 / n as f64;
        for (k, x) in xs.iter().enumerate() {
            let a = w * k as f64;
            re += x * a.cos();
            im -= x * a.sin();
        }
        freqs.push(kf as f64 / (n as f64 * dt));
        amps.push(4.0 * (re * re + im * im).sqrt() / n as f64);
    }
    Some((freqs, amps))
}

/// Punkt biegunowej profilu.
#[derive(Clone, Copy, Debug)]
pub struct PolarPoint {
    pub alpha: f64,
    pub cl: f64,
    pub cd: f64,
    pub cm: f64,
    pub cl_amp: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SweepPhase {
    /// Czekamy, aż solver przyjmie nową geometrię.
    Apply {
        rev: u64,
    },
    Settle {
        until: f64,
    },
    Measure {
        from: f64,
        until: f64,
    },
}

/// Automatyczny pomiar biegunowej: dla kolejnych kątów ustalenie przepływu, potem uśrednianie.
pub struct Sweep {
    pub body: u64,
    pub angles: Vec<f64>,
    pub idx: usize,
    pub phase: SweepPhase,
    /// Czas ustalania i pomiaru w jednostkach czasu konwekcyjnego c/U.
    pub settle_conv: f64,
    pub measure_conv: f64,
    pub results: Vec<PolarPoint>,
    acc: Vec<Coeffs>,
}

impl Sweep {
    pub fn new(body: u64, from: f64, to: f64, step: f64, settle_conv: f64, measure_conv: f64) -> Self {
        let step = step.abs().max(0.1) * if to >= from { 1.0 } else { -1.0 };
        let n = ((to - from) / step).floor() as usize + 1;
        let angles = (0..n.min(200)).map(|k| from + k as f64 * step).collect();
        Self { body, angles, idx: 0, phase: SweepPhase::Apply { rev: 0 }, settle_conv, measure_conv, results: Vec::new(), acc: Vec::new() }
    }

    pub fn current_angle(&self) -> Option<f64> {
        self.angles.get(self.idx).copied()
    }

    pub fn done(&self) -> bool {
        self.idx >= self.angles.len()
    }

    pub fn progress(&self) -> f32 {
        if self.angles.is_empty() {
            return 1.0;
        }
        self.idx as f32 / self.angles.len() as f32
    }

    /// Zwraca `Some(kąt)`, gdy trzeba ustawić nowy kąt ciała (i wysłać scenę).
    pub fn tick(&mut self, frame: &Frame, body: &Body) -> Option<f64> {
        if self.done() {
            return None;
        }
        let conv = body.ref_length() / frame.units.u_inf;
        match self.phase {
            SweepPhase::Apply { rev } => {
                if rev == 0 {
                    return self.current_angle();
                }
                if frame.scene_rev >= rev {
                    self.phase = SweepPhase::Settle { until: frame.time + self.settle_conv * conv };
                }
            }
            SweepPhase::Settle { until } => {
                if frame.time >= until {
                    self.acc.clear();
                    self.phase = SweepPhase::Measure { from: frame.time, until: frame.time + self.measure_conv * conv };
                }
            }
            SweepPhase::Measure { until, .. } => {
                if frame.time >= until && !self.acc.is_empty() {
                    let n = self.acc.len() as f64;
                    let cl: Vec<f64> = self.acc.iter().map(|c| c.cl).collect();
                    let lo = cl.iter().cloned().fold(f64::INFINITY, f64::min);
                    let hi = cl.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                    self.results.push(PolarPoint {
                        alpha: self.angles[self.idx],
                        cl: cl.iter().sum::<f64>() / n,
                        cd: self.acc.iter().map(|c| c.cd).sum::<f64>() / n,
                        cm: self.acc.iter().map(|c| c.cm).sum::<f64>() / n,
                        cl_amp: 0.5 * (hi - lo),
                    });
                    self.idx += 1;
                    self.phase = SweepPhase::Apply { rev: 0 };
                    return self.current_angle();
                }
            }
        }
        None
    }

    /// Po wysłaniu sceny z nowym kątem.
    pub fn applied(&mut self, rev: u64) {
        self.phase = SweepPhase::Apply { rev };
    }

    pub fn record(&mut self, c: Coeffs) {
        if matches!(self.phase, SweepPhase::Measure { .. }) {
            self.acc.push(c);
        }
    }
}

/// Punkt rozkładu ciśnienia na powierzchni: współrzędna (x/c, θ lub s) i Cp, z podziałem na strony.
pub struct SurfaceCurve {
    pub x_label: &'static str,
    pub upper: Vec<[f64; 2]>,
    pub lower: Vec<[f64; 2]>,
}

pub fn surface_curve(frame: &Frame, body_index: usize, body: &Body) -> Option<SurfaceCurve> {
    let pts: Vec<_> = frame.surface.iter().filter(|s| s.body as usize == body_index).collect();
    if pts.is_empty() {
        return None;
    }
    let mut upper = Vec::new();
    let mut lower = Vec::new();
    let label = match &body.shape {
        Shape::Airfoil { chord, naca } => {
            for s in pts {
                let x = (s.local[0] as f64 / chord + 0.25).clamp(0.0, 1.0);
                let (yc, _) = naca.camber(x);
                let up = s.local[1] as f64 / chord >= yc;
                if up { &mut upper } else { &mut lower }.push([x, s.cp as f64]);
            }
            "x/c"
        }
        Shape::Circle { .. } | Shape::Ellipse { .. } => {
            for s in pts {
                // kąt od przedniego punktu spiętrzenia
                let th = (s.local[1] as f64).atan2(-(s.local[0] as f64)).to_degrees();
                if th >= 0.0 { &mut upper } else { &mut lower }.push([th.abs(), s.cp as f64]);
            }
            "θ [°] od punktu spiętrzenia"
        }
        _ => {
            for s in pts {
                let th = (s.local[1] as f64).atan2(-(s.local[0] as f64)).to_degrees();
                if th >= 0.0 { &mut upper } else { &mut lower }.push([th.abs(), s.cp as f64]);
            }
            "kąt wokół osi [°]"
        }
    };
    let sort = |v: &mut Vec<[f64; 2]>| v.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap_or(std::cmp::Ordering::Equal));
    sort(&mut upper);
    sort(&mut lower);
    Some(SurfaceCurve { x_label: label, upper: smooth_bins(&upper), lower: smooth_bins(&lower) })
}

/// Uśrednia punkty w wąskich przedziałach (wiele połączeń sieci trafia w to samo miejsce).
fn smooth_bins(v: &[[f64; 2]]) -> Vec<[f64; 2]> {
    if v.len() < 4 {
        return v.to_vec();
    }
    let span = v[v.len() - 1][0] - v[0][0];
    let w = span / 120.0;
    let mut out = Vec::new();
    let mut i = 0;
    while i < v.len() {
        let x0 = v[i][0];
        let (mut sx, mut sy, mut n) = (0.0, 0.0, 0.0);
        while i < v.len() && v[i][0] <= x0 + w {
            sx += v[i][0];
            sy += v[i][1];
            n += 1.0;
            i += 1;
        }
        out.push([sx / n, sy / n]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn units() -> Units {
        Units::new(&crate::physics::units::Fluid::water(20.0), 0.01, 0.0005, 0.05)
    }

    #[test]
    fn strouhal_from_sine() {
        let u = units();
        let f0 = 0.165 * u.u_inf / 0.01;
        let mut s = VecDeque::new();
        for k in 0..4000 {
            let t = k as f64 * 0.01;
            s.push_back(Coeffs { t, cd: 1.33, cl: 0.33 * (std::f64::consts::TAU * f0 * t).sin(), ..Default::default() });
        }
        let st = stats(&s, &u, 0.01, 1e9).unwrap();
        assert!((st.strouhal.unwrap() - 0.165).abs() < 0.002, "{:?}", st.strouhal);
        assert!((st.cl_amp - 0.33).abs() < 0.01);
        assert!((st.cd_mean - 1.33).abs() < 1e-9);
    }

    #[test]
    fn spectrum_finds_peak() {
        let t: VecDeque<f64> = (0..2000).map(|k| k as f64 * 0.01).collect();
        let v: VecDeque<f64> = t.iter().map(|t| (std::f64::consts::TAU * 2.5 * t).sin()).collect();
        let (f, a) = spectrum(&t, &v, 512).unwrap();
        let k = a.iter().enumerate().max_by(|x, y| x.1.partial_cmp(y.1).unwrap()).unwrap().0;
        assert!((f[k] - 2.5).abs() < 0.1, "{}", f[k]);
    }

    #[test]
    fn sweep_angles() {
        let s = Sweep::new(1, -4.0, 12.0, 2.0, 5.0, 3.0);
        assert_eq!(s.angles, vec![-4.0, -2.0, 0.0, 2.0, 4.0, 6.0, 8.0, 10.0, 12.0]);
    }
}
