//! Własności płynów i przeliczanie między jednostkami fizycznymi (SI) a jednostkami siatki LBM.
//!
//! Solver pracuje w jednostkach siatki: krok przestrzenny Δx = 1, krok czasowy Δt = 1, gęstość
//! odniesienia ρ = 1. Wybór prędkości strumienia w jednostkach siatki `u_lb` (liczba Macha siatki)
//! ustala krok czasowy: Δt = u_lb · Δx / U. Lepkość kinematyczna płynu przechodzi wtedy w
//! ν_lb = ν · Δt / Δx², a czas relaksacji τ = 3 ν_lb + ½.

use serde::{Deserialize, Serialize};

/// Uniwersalna stała gazowa powietrza suchego [J/(kg·K)].
pub const R_AIR: f64 = 287.05;
/// Prędkość dźwięku w jednostkach siatki (c_s = 1/√3).
pub const CS_LB: f64 = 0.577_350_269_189_625_8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FluidKind {
    Air,
    Water,
    Custom,
}

impl FluidKind {
    pub fn label(self) -> &'static str {
        match self {
            FluidKind::Air => "Powietrze",
            FluidKind::Water => "Woda",
            FluidKind::Custom => "Własny płyn",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fluid {
    pub kind: FluidKind,
    /// Temperatura [°C].
    pub temperature_c: f64,
    /// Ciśnienie statyczne strumienia swobodnego [kPa] (powietrze: wpływa na gęstość).
    pub pressure_kpa: f64,
    /// Płyn własny: gęstość [kg/m³].
    pub custom_density: f64,
    /// Płyn własny: lepkość dynamiczna [Pa·s].
    pub custom_viscosity: f64,
}

impl Default for Fluid {
    fn default() -> Self {
        Self::air(15.0)
    }
}

impl Fluid {
    pub fn air(t: f64) -> Self {
        Self { kind: FluidKind::Air, temperature_c: t, pressure_kpa: 101.325, custom_density: 1.0, custom_viscosity: 1e-3 }
    }

    pub fn water(t: f64) -> Self {
        Self { kind: FluidKind::Water, temperature_c: t, pressure_kpa: 101.325, custom_density: 1.0, custom_viscosity: 1e-3 }
    }

    fn t_kelvin(&self) -> f64 {
        self.temperature_c + 273.15
    }

    /// Gęstość [kg/m³].
    pub fn density(&self) -> f64 {
        match self.kind {
            // gaz doskonały
            FluidKind::Air => self.pressure_kpa * 1000.0 / (R_AIR * self.t_kelvin()),
            // równanie Thiesena (0–100 °C, błąd < 0,01%)
            FluidKind::Water => {
                let t = self.temperature_c.clamp(0.0, 100.0);
                1000.0 * (1.0 - (t + 288.9414) / (508_929.2 * (t + 68.129_63)) * (t - 3.9863).powi(2))
            }
            FluidKind::Custom => self.custom_density.max(1e-6),
        }
    }

    /// Lepkość dynamiczna μ [Pa·s].
    pub fn dynamic_viscosity(&self) -> f64 {
        match self.kind {
            // wzór Sutherlanda
            FluidKind::Air => {
                let t = self.t_kelvin();
                1.716e-5 * (t / 273.15).powf(1.5) * (273.15 + 110.4) / (t + 110.4)
            }
            // równanie Vogela–Fulchera–Tammanna
            FluidKind::Water => {
                let t = self.temperature_c.clamp(0.0, 100.0) + 273.15;
                2.414e-5 * 10f64.powf(247.8 / (t - 140.0))
            }
            FluidKind::Custom => self.custom_viscosity.max(1e-9),
        }
    }

    /// Lepkość kinematyczna ν [m²/s].
    pub fn kinematic_viscosity(&self) -> f64 {
        self.dynamic_viscosity() / self.density()
    }

    /// Prędkość dźwięku [m/s] (do oceny ściśliwości).
    pub fn speed_of_sound(&self) -> f64 {
        match self.kind {
            FluidKind::Air => (1.4 * R_AIR * self.t_kelvin()).sqrt(),
            FluidKind::Water => {
                // przybliżenie Marczaka (0–95 °C)
                let t = self.temperature_c.clamp(0.0, 95.0);
                1402.385 + 5.038_813 * t - 5.799_136e-2 * t * t + 3.287_156e-4 * t.powi(3) - 1.398_845e-6 * t.powi(4)
                    + 2.787_860e-9 * t.powi(5)
            }
            FluidKind::Custom => 1500.0,
        }
    }

    /// Ciśnienie pary nasyconej [Pa] — tylko woda (wzór Bucka), do oceny ryzyka kawitacji.
    pub fn vapor_pressure(&self) -> Option<f64> {
        match self.kind {
            FluidKind::Water => {
                let t = self.temperature_c;
                Some(611.21 * ((18.678 - t / 234.5) * (t / (257.14 + t))).exp())
            }
            _ => None,
        }
    }
}

/// Przeliczanie jednostek: wszystko, czego UI potrzebuje, by pokazać wyniki w SI.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Units {
    /// Rozmiar komórki [m].
    pub dx: f64,
    /// Krok czasowy [s].
    pub dt: f64,
    /// Gęstość płynu [kg/m³].
    pub rho: f64,
    /// Lepkość kinematyczna [m²/s].
    pub nu: f64,
    /// Prędkość strumienia swobodnego [m/s].
    pub u_inf: f64,
    /// Prędkość strumienia w jednostkach siatki.
    pub u_lb: f64,
    /// Lepkość w jednostkach siatki.
    pub nu_lb: f64,
    /// Prędkość dźwięku w płynie [m/s].
    pub sound: f64,
}

impl Units {
    pub fn new(fluid: &Fluid, u_inf: f64, dx: f64, u_lb: f64) -> Self {
        let u_inf = u_inf.max(1e-6);
        let nu = fluid.kinematic_viscosity();
        let dt = u_lb * dx / u_inf;
        Self { dx, dt, rho: fluid.density(), nu, u_inf, u_lb, nu_lb: nu * dt / (dx * dx), sound: fluid.speed_of_sound() }
    }

    /// Czas relaksacji molekularnej lepkości.
    pub fn tau(&self) -> f64 {
        3.0 * self.nu_lb + 0.5
    }

    /// Lepkość siatki faktycznie użyta przez solver (z minimalną lepkością numeryczną).
    pub fn nu_lb_effective(&self) -> f64 {
        self.nu_lb.max((crate::physics::lattice::TAU_MIN as f64 - 0.5) / 3.0)
    }

    /// Efektywna liczba Reynoldsa siatki dla długości `l` [m] — mniejsza od fizycznej, gdy
    /// obliczenia ogranicza lepkość numeryczna.
    pub fn reynolds_effective(&self, l: f64) -> f64 {
        self.u_lb * (l / self.dx) / self.nu_lb_effective()
    }

    /// Prędkość siatki → m/s.
    #[cfg(test)]
    pub fn vel(&self, u: f64) -> f64 {
        u * self.dx / self.dt
    }

    /// Odchyłka gęstości siatki (ρ − ρ_ref) → nadciśnienie [Pa].
    pub fn pressure(&self, drho: f64) -> f64 {
        drho / 3.0 * self.rho * (self.dx / self.dt).powi(2)
    }

    /// Siła siatki (2D) → N na metr rozpiętości.
    pub fn force_per_span(&self, f: f64) -> f64 {
        f * self.rho * (self.dx / self.dt).powi(2) * self.dx
    }

    /// Moment siatki → N·m na metr rozpiętości.
    pub fn moment_per_span(&self, m: f64) -> f64 {
        self.force_per_span(m) * self.dx
    }

    /// Ciśnienie dynamiczne strumienia [Pa].
    pub fn q_inf(&self) -> f64 {
        0.5 * self.rho * self.u_inf * self.u_inf
    }

    /// Liczba Reynoldsa dla długości `l` [m].
    pub fn reynolds(&self, l: f64) -> f64 {
        self.u_inf * l / self.nu
    }

    /// Fizyczna liczba Macha strumienia.
    pub fn mach(&self) -> f64 {
        self.u_inf / self.sound
    }

    /// Liczba Macha siatki (decyduje o błędzie ściśliwości modelu).
    pub fn mach_lb(&self) -> f64 {
        self.u_lb / CS_LB
    }
}

/// Ocena wiarygodności obliczeń przy danej rozdzielczości.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fidelity {
    /// Warstwa przyścienna rozdzielona — wyniki ilościowe (DNS).
    Resolved,
    /// Warstwa przyścienna ledwo rozdzielona — błędy rzędu kilku–kilkunastu procent.
    Marginal,
    /// Model LES bez rozdzielonej warstwy przyściennej — obraz jakościowy.
    Qualitative,
}

impl Fidelity {
    pub fn label(self) -> &'static str {
        match self {
            Fidelity::Resolved => "DNS — rozdzielczy",
            Fidelity::Marginal => "częściowo rozdzielczy",
            Fidelity::Qualitative => "LES — jakościowy",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Assessment {
    pub fidelity: Fidelity,
    /// Szacowana liczba komórek w grubości laminarnej warstwy przyściennej.
    pub bl_cells: f64,
    pub notes: Vec<String>,
}

/// Ocena wiarygodności dla ciała o długości odniesienia `l_cells` (w komórkach) przy liczbie Re.
pub fn assess(units: &Units, l_cells: f64, re: f64) -> Assessment {
    let mut notes = Vec::new();
    // grubość laminarnej warstwy przyściennej Blasiusa: δ ≈ 5 L / √Re
    let bl_cells = if re > 0.0 { 5.0 * l_cells / re.sqrt() } else { f64::INFINITY };
    let mut fidelity = if bl_cells >= 4.0 {
        Fidelity::Resolved
    } else if bl_cells >= 1.5 {
        Fidelity::Marginal
    } else {
        Fidelity::Qualitative
    };
    if re > 2.0e5 {
        fidelity = Fidelity::Qualitative;
        notes.push(
            "Re > 2·10⁵: w rzeczywistości przepływ jest turbulentny i trójwymiarowy; model 2D pokazuje tylko jego obraz jakościowy.".into(),
        );
    }
    if units.nu_lb < units.nu_lb_effective() {
        let l = l_cells * units.dx;
        notes.push(format!(
            "Lepkość płynu jest mniejsza niż najmniejsza lepkość stabilna na tej siatce — liczona jest efektywna Re ≈ {} \
             zamiast {}. Większa rozdzielczość lub prędkość siatki u_lb podnoszą tę granicę.",
            crate::render::field::fmt_num(units.reynolds_effective(l), 2),
            crate::render::field::fmt_num(re, 2)
        ));
    }
    if bl_cells < 1.5 && re <= 2.0e5 {
        notes.push(format!(
            "Warstwa przyścienna ma ok. {bl_cells:.1} komórki — zwiększ rozdzielczość lub powiększ ciało, by liczyć ilościowo."
        ));
    }
    let ma = units.mach();
    if ma > 0.3 {
        notes.push(format!("Ma = {ma:.2} > 0,3 — w rzeczywistości ściśliwość jest istotna, a model zakłada przepływ nieściśliwy."));
    }
    if l_cells < 12.0 && l_cells > 0.0 {
        notes.push(format!("Ciało ma tylko {l_cells:.0} komórek — kształt jest odwzorowany zgrubnie."));
    }
    Assessment { fidelity, bl_cells, notes }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn air_properties_at_15c() {
        let a = Fluid::air(15.0);
        assert!((a.density() - 1.225).abs() < 0.002, "{}", a.density());
        assert!((a.dynamic_viscosity() - 1.789e-5).abs() < 0.005e-5);
        assert!((a.speed_of_sound() - 340.3).abs() < 0.5);
    }

    #[test]
    fn water_properties_at_20c() {
        let w = Fluid::water(20.0);
        assert!((w.density() - 998.2).abs() < 0.1, "{}", w.density());
        assert!((w.dynamic_viscosity() - 1.002e-3).abs() < 0.02e-3, "{}", w.dynamic_viscosity());
        assert!((w.vapor_pressure().unwrap() - 2339.0).abs() < 15.0);
    }

    #[test]
    fn unit_roundtrip_is_consistent() {
        let u = Units::new(&Fluid::air(15.0), 10.0, 0.001, 0.05);
        assert!((u.vel(u.u_lb) - 10.0).abs() < 1e-12);
        // współczynnik siły nie zależy od układu jednostek
        let f_lb = 0.37;
        let l_lb = 80.0;
        let cd_lb = f_lb / (0.5 * u.u_lb * u.u_lb * l_lb);
        let cd_si = u.force_per_span(f_lb) / (u.q_inf() * l_lb * u.dx);
        assert!((cd_lb - cd_si).abs() < 1e-9);
    }
}
