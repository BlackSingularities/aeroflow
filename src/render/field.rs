//! Pola skalarne do wyświetlania (w jednostkach fizycznych) i ich zamiana na obraz.

use eframe::egui::{Color32, ColorImage};
use serde::{Deserialize, Serialize};

use super::colormap::Colormap;
use crate::sim::Frame;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FieldKind {
    Speed,
    Ux,
    Uy,
    Vorticity,
    Pressure,
    Cp,
    QCriterion,
    Smoke,
    MeanSpeed,
    MeanCp,
    Turbulence,
    EddyViscosity,
    Off,
}

impl FieldKind {
    pub const ALL: [FieldKind; 13] = [
        FieldKind::Speed,
        FieldKind::Ux,
        FieldKind::Uy,
        FieldKind::Vorticity,
        FieldKind::Pressure,
        FieldKind::Cp,
        FieldKind::QCriterion,
        FieldKind::Smoke,
        FieldKind::MeanSpeed,
        FieldKind::MeanCp,
        FieldKind::Turbulence,
        FieldKind::EddyViscosity,
        FieldKind::Off,
    ];

    pub fn label(self) -> &'static str {
        match self {
            FieldKind::Speed => "Prędkość |u|",
            FieldKind::Ux => "Prędkość pozioma u_x",
            FieldKind::Uy => "Prędkość pionowa u_y",
            FieldKind::Vorticity => "Wirowość ω",
            FieldKind::Pressure => "Ciśnienie p − p∞",
            FieldKind::Cp => "Współczynnik ciśnienia Cp",
            FieldKind::QCriterion => "Kryterium Q (rdzenie wirów)",
            FieldKind::Smoke => "Dym",
            FieldKind::MeanSpeed => "Średnia prędkość ⟨|u|⟩",
            FieldKind::MeanCp => "Średni Cp",
            FieldKind::Turbulence => "Intensywność turbulencji",
            FieldKind::EddyViscosity => "Lepkość turbulentna ν_t/ν",
            FieldKind::Off => "Brak",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            FieldKind::Speed => "Prędkość",
            FieldKind::Ux => "u_x",
            FieldKind::Uy => "u_y",
            FieldKind::Vorticity => "Wirowość",
            FieldKind::Pressure => "Ciśnienie",
            FieldKind::Cp => "Cp",
            FieldKind::QCriterion => "Q",
            FieldKind::Smoke => "Dym",
            FieldKind::MeanSpeed => "Średnia |u|",
            FieldKind::MeanCp => "Średni Cp",
            FieldKind::Turbulence => "Turbulencja",
            FieldKind::EddyViscosity => "ν_t/ν",
            FieldKind::Off => "Brak",
        }
    }

    pub fn unit(self) -> &'static str {
        match self {
            FieldKind::Speed | FieldKind::Ux | FieldKind::Uy | FieldKind::MeanSpeed => "m/s",
            FieldKind::Vorticity => "1/s",
            FieldKind::Pressure => "Pa",
            FieldKind::QCriterion => "1/s²",
            FieldKind::Turbulence => "%",
            _ => "",
        }
    }

    /// Pole rozbieżne (symetryczna skala wokół zera).
    pub fn symmetric(self) -> bool {
        matches!(self, FieldKind::Ux | FieldKind::Uy | FieldKind::Vorticity | FieldKind::Pressure | FieldKind::Cp | FieldKind::MeanCp)
    }

    pub fn default_colormap(self) -> Colormap {
        match self {
            FieldKind::Speed | FieldKind::MeanSpeed => Colormap::Ocean,
            FieldKind::Ux | FieldKind::Uy | FieldKind::Pressure | FieldKind::Cp | FieldKind::MeanCp => Colormap::CoolWarm,
            FieldKind::Vorticity => Colormap::IceFire,
            FieldKind::QCriterion | FieldKind::Turbulence => Colormap::Inferno,
            FieldKind::Smoke => Colormap::Gray,
            FieldKind::EddyViscosity => Colormap::Viridis,
            FieldKind::Off => Colormap::Gray,
        }
    }

    pub fn needs_mean(self) -> bool {
        matches!(self, FieldKind::MeanSpeed | FieldKind::MeanCp | FieldKind::Turbulence)
    }

    /// Wartości pola w jednostkach fizycznych (rząd 0 = dół). `None`, gdy pole niedostępne.
    pub fn compute(self, f: &Frame) -> Option<Vec<f32>> {
        let u = &f.units;
        let vel = (u.dx / u.dt) as f32;
        let q_lb = (0.5 * u.u_lb * u.u_lb) as f32;
        // (ρ − ρ∞)/3 w jednostkach siatki → Pa
        let pa = (u.rho * (u.dx / u.dt).powi(2)) as f32;
        let n = f.w * f.h;
        Some(match self {
            FieldKind::Speed => (0..n).map(|i| f.ux[i].hypot(f.uy[i]) * vel).collect(),
            FieldKind::Ux => f.ux.iter().map(|v| v * vel).collect(),
            FieldKind::Uy => f.uy.iter().map(|v| v * vel).collect(),
            FieldKind::Pressure => f.p.iter().map(|p| p * pa).collect(),
            FieldKind::Cp => f.p.iter().map(|p| p / q_lb).collect(),
            FieldKind::Vorticity => {
                let k = (1.0 / u.dt) as f32;
                gradients(f, |_, dudy, dvdx, _| (dvdx - dudy) * k)
            }
            FieldKind::QCriterion => {
                let k = (1.0 / (u.dt * u.dt)) as f32;
                gradients(f, |dudx, dudy, dvdx, dvdy| (-0.5 * (dudx * dudx + dvdy * dvdy) - dudy * dvdx) * k)
            }
            FieldKind::Smoke => f.dye.clone()?,
            FieldKind::MeanSpeed => {
                let m = f.mean.as_ref()?;
                (0..n).map(|i| m.ux[i].hypot(m.uy[i]) * vel).collect()
            }
            FieldKind::MeanCp => f.mean.as_ref()?.p.iter().map(|p| p / q_lb).collect(),
            FieldKind::Turbulence => f.mean.as_ref()?.ti.iter().map(|v| v * 100.0).collect(),
            FieldKind::EddyViscosity => f.nut.clone(),
            FieldKind::Off => return None,
        })
    }
}

/// Pochodne prędkości różnicami centralnymi (jednostki siatki); przy ścianach zero.
fn gradients(f: &Frame, op: impl Fn(f32, f32, f32, f32) -> f32) -> Vec<f32> {
    let (w, h) = (f.w, f.h);
    let mut out = vec![0f32; w * h];
    for y in 1..h.saturating_sub(1) {
        for x in 1..w - 1 {
            let c = y * w + x;
            if f.labels[c] != 0 {
                continue;
            }
            let (l, r, d, t) = (c - 1, c + 1, c - w, c + w);
            if f.labels[l] != 0 || f.labels[r] != 0 || f.labels[d] != 0 || f.labels[t] != 0 {
                continue;
            }
            let dudx = 0.5 * (f.ux[r] - f.ux[l]);
            let dudy = 0.5 * (f.ux[t] - f.ux[d]);
            let dvdx = 0.5 * (f.uy[r] - f.uy[l]);
            let dvdy = 0.5 * (f.uy[t] - f.uy[d]);
            out[c] = op(dudx, dudy, dvdx, dvdy);
        }
    }
    out
}

/// Zakres skali kolorów z wygładzaniem w czasie.
#[derive(Clone, Copy, Debug)]
pub struct AutoRange {
    pub lo: f32,
    pub hi: f32,
    init: bool,
}

impl Default for AutoRange {
    fn default() -> Self {
        Self { lo: 0.0, hi: 1.0, init: false }
    }
}

impl AutoRange {
    pub fn reset(&mut self) {
        self.init = false;
    }

    pub fn update(&mut self, values: &[f32], labels: &[u16], symmetric: bool, nonneg: bool) {
        let step = (values.len() / 20000).max(1);
        let mut s: Vec<f32> = values
            .iter()
            .zip(labels)
            .step_by(step)
            .filter(|(v, l)| **l == 0 && v.is_finite())
            .map(|(v, _)| if symmetric { v.abs() } else { *v })
            .collect();
        if s.is_empty() {
            return;
        }
        s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let pick = |q: f32| s[((s.len() - 1) as f32 * q) as usize];
        let (lo, hi) = if symmetric {
            let a = pick(0.985).max(1e-12);
            (-a, a)
        } else if nonneg {
            (0.0, pick(0.995).max(1e-12))
        } else {
            (pick(0.005), pick(0.995).max(pick(0.005) + 1e-12))
        };
        if !self.init {
            self.lo = lo;
            self.hi = hi;
            self.init = true;
        } else {
            let k = 0.08;
            self.lo += (lo - self.lo) * k;
            self.hi += (hi - self.hi) * k;
        }
    }
}

/// Buduje obraz pola (rząd 0 obrazu = góra obszaru). Komórki ciał mają kolor `solid`.
#[allow(clippy::too_many_arguments)]
pub fn build_image(
    values: &[f32],
    w: usize,
    h: usize,
    labels: &[u16],
    lo: f32,
    hi: f32,
    lut: &[Color32; 256],
    solid: Color32,
) -> ColorImage {
    let mut img = ColorImage::new([w, h], vec![Color32::BLACK; w * h]);
    let k = 255.0 / (hi - lo).max(1e-30);
    for y in 0..h {
        let src = (h - 1 - y) * w;
        let dst = y * w;
        for x in 0..w {
            img.pixels[dst + x] = if labels[src + x] != 0 {
                solid
            } else {
                let t = ((values[src + x] - lo) * k).clamp(0.0, 255.0);
                lut[t as usize]
            };
        }
    }
    img
}

/// Jednostka z przedrostkiem SI dobranym do wartości (np. 0,012 m/s → 12 mm/s).
pub fn fmt_si(v: f64, unit: &str, digits: usize) -> String {
    if unit.is_empty() || unit == "%" {
        return format!("{} {unit}", fmt_num(v, digits)).trim_end().to_string();
    }
    let a = v.abs();
    let (k, p) = if a == 0.0 || (1.0..1000.0).contains(&a) {
        (1.0, "")
    } else if a >= 1e6 {
        (1e-6, "M")
    } else if a >= 1e3 {
        (1e-3, "k")
    } else if a >= 1e-3 {
        (1e3, "m")
    } else {
        (1e6, "µ")
    };
    // nie stosujemy „mPa” ani „k1/s” — tylko dla jednostek, gdzie przedrostki są naturalne
    let natural = matches!(unit, "m/s" | "Pa" | "N" | "N/m" | "m" | "s" | "Hz" | "N·m");
    if !natural {
        return format!("{} {unit}", fmt_num(v, digits));
    }
    format!("{} {p}{unit}", fmt_num(v * k, digits))
}

/// Liczba z polskim separatorem dziesiętnym i notacją naukową dla skrajnych wartości.
pub fn fmt_num(v: f64, digits: usize) -> String {
    if !v.is_finite() {
        return "—".into();
    }
    let a = v.abs();
    let s = if a != 0.0 && !(1e-3..1e6).contains(&a) {
        let e = a.log10().floor() as i32;
        let m = v / 10f64.powi(e);
        format!("{:.*}·10{}", digits.min(3), m, superscript(e))
    } else {
        let d = if a >= 100.0 {
            digits.saturating_sub(2)
        } else if a >= 10.0 {
            digits.saturating_sub(1)
        } else {
            digits
        };
        let t = format!("{:.*}", d, v);
        if a >= 10_000.0 {
            group_thousands(&t)
        } else {
            t
        }
    };
    s.replace('.', ",")
}

/// „136927.5” → „136 927.5” (spacja niełamiąca jako separator tysięcy).
fn group_thousands(s: &str) -> String {
    let (sign, rest) = s.strip_prefix('-').map(|r| ("-", r)).unwrap_or(("", s));
    let (int, frac) = rest.split_once('.').map(|(a, b)| (a, Some(b))).unwrap_or((rest, None));
    let mut out = String::new();
    for (i, ch) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            out.push('\u{00a0}');
        }
        out.push(ch);
    }
    match frac {
        Some(f) => format!("{sign}{out}.{f}"),
        None => format!("{sign}{out}"),
    }
}

fn superscript(e: i32) -> String {
    let sup = ['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];
    let mut s = String::new();
    if e < 0 {
        s.push('⁻');
    }
    for c in e.abs().to_string().chars() {
        s.push(sup[c.to_digit(10).unwrap() as usize]);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn si_formatting() {
        assert_eq!(fmt_si(0.012, "m/s", 2), "12,0 mm/s");
        assert_eq!(fmt_si(12.5, "m/s", 2), "12,5 m/s");
        assert_eq!(fmt_si(1500.0, "Pa", 2), "1,50 kPa");
        assert_eq!(fmt_num(150000.0, 2), "150\u{a0}000");
        assert_eq!(fmt_num(1.5e7, 2), "1,50·10⁷");
    }
}
