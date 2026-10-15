//! Solver Lattice-Boltzmanna D2Q9.
//!
//! * kolizja BGK z regularyzacją (część nierównowagowa odtworzona z tensora naprężeń) i modelem
//!   podsiatkowym Smagorinsky'ego (LES) — stabilna również przy bardzo małej lepkości,
//! * strumieniowanie typu „pull” połączone z kolizją w jednym przejściu po pamięci, równolegle po
//!   wierszach (rayon),
//! * ciała: interpolowane odbicie Bouzidiego–Firdaouss–Lallemanda z dokładną odległością ściany
//!   (drugi rząd dokładności dla krzywych brzegów zamiast „schodków”),
//! * siły: metoda wymiany pędu (Galileuszowo niezmiennicza wersja Wena i in.) z momentem względem
//!   osi obrotu każdego ciała,
//! * brzegi: wlot z zadanym profilem prędkości (jednorodny, kanałowy, warstwa przyziemna) i
//!   syntetyczną turbulencją; wylot z zerowym gradientem i strefą pochłaniającą; góra/dół jako
//!   strumień swobodny (z niewidocznym marginesem), ściana poślizgowa, ściana z przyczepnością lub
//!   ruchoma podłoga.

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use super::geometry::{Body, Prepared};
use super::gpu::{self, GpuSolver, StepInput};

pub const CX: [i32; 9] = [0, 1, 0, -1, 0, 1, -1, -1, 1];
pub const CY: [i32; 9] = [0, 0, 1, 0, -1, 1, 1, -1, -1];
pub const W: [f32; 9] = [4.0 / 9.0, 1.0 / 9.0, 1.0 / 9.0, 1.0 / 9.0, 1.0 / 9.0, 1.0 / 36.0, 1.0 / 36.0, 1.0 / 36.0, 1.0 / 36.0];
pub const OPP: [usize; 9] = [0, 3, 4, 1, 2, 7, 8, 5, 6];
/// Odbicie składowej pionowej (ściana poślizgowa).
pub const MIRY: [usize; 9] = [0, 1, 4, 3, 2, 8, 7, 6, 5];

const NO_LINK: u32 = u32::MAX;
/// Czas relaksacji lepkości objętościowej τ_b = 1 (1 − 1/τ_b = 0): ślad Π_neq wraca do równowagi
/// w jednym kroku. W przepływie nieściśliwym ∇·u ≈ 0, więc nie zmienia to fizyki, a tłumi akustykę.
const KEEP_BULK: f32 = 0.0;
/// Najmniejszy czas relaksacji lepkości ścinania. Przy τ → ½ schemat traci tłumienie najkrótszych fal
/// (prążki siatkowe), więc dla bardzo dużych Re dodawana jest minimalna lepkość numeryczna — wynik
/// pozostaje jakościowy, co pokazuje ocena wiarygodności (efektywna liczba Reynoldsa siatki).
pub const TAU_MIN: f32 = 0.503;
const K_FLUID: u8 = 0;
const K_SOLID: u8 = 1;
const K_INLET: u8 = 2;
const K_SPECIAL: u8 = 3;

/// Wskaźnik do bufora wyjściowego współdzielony przez wątki — każdy wiersz pisze do rozłącznych komórek.
#[derive(Clone, Copy)]
struct SyncPtr(*mut f32);
unsafe impl Send for SyncPtr {}
unsafe impl Sync for SyncPtr {}

impl SyncPtr {
    fn get(self) -> *mut f32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Boundary {
    /// Brak ściany: niewidoczny margines i strefa pochłaniająca (opływ swobodny).
    FreeStream,
    /// Ściana bez tarcia (symetria).
    SlipWall,
    /// Ściana z przyczepnością (warstwa przyścienna jak w tunelu).
    NoSlipWall,
    /// Podłoga poruszająca się z prędkością strumienia (jak taśma w tunelu samochodowym).
    MovingGround,
}

impl Boundary {
    pub fn label(self) -> &'static str {
        match self {
            Boundary::FreeStream => "Opływ swobodny",
            Boundary::SlipWall => "Ściana bez tarcia",
            Boundary::NoSlipWall => "Ściana tunelu",
            Boundary::MovingGround => "Ruchoma podłoga",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum InletProfile {
    Uniform,
    /// Parabola Poiseuille'a między dolną a górną krawędzią (średnia = U).
    Channel,
    /// Profil potęgowy warstwy przyziemnej u = U (z/H)^α (U na górnej krawędzi obszaru).
    PowerLaw {
        alpha: f64,
    },
}

impl InletProfile {
    pub fn label(&self) -> &'static str {
        match self {
            InletProfile::Uniform => "Jednorodny",
            InletProfile::Channel => "Kanał (parabola)",
            InletProfile::PowerLaw { .. } => "Warstwa przyziemna",
        }
    }
}

/// Parametry siatki (zmiana wymaga nowej siatki).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub nx: usize,
    pub ny: usize,
    pub top: Boundary,
    pub bottom: Boundary,
}

/// Wycinek widoczny w pełnej siatce obliczeniowej.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    pub x0: usize,
    pub y0: usize,
    pub w: usize,
    pub h: usize,
}

/// Połączenie komórki płynu z komórką ciała — punkt pomiaru ciśnienia na powierzchni.
#[derive(Clone, Copy, Debug)]
pub struct Link {
    pub cell: u32,
    pub body: u16,
    /// Punkt ściany w układzie lokalnym ciała [m].
    pub local: [f32; 2],
}

pub struct Lattice {
    pub nx: usize,
    pub ny: usize,
    pub win: Window,
    pub layout: Layout,
    /// Populacje po kolizji, układ SoA: f[i·n + komórka]. Przy obliczeniach na GPU aktualne są
    /// tylko po resecie — bieżący stan żyje w pamięci karty.
    pub(crate) f: Vec<f32>,
    g: Vec<f32>,
    pub(crate) kind: Vec<u8>,
    pub rho: Vec<f32>,
    pub ux: Vec<f32>,
    pub uy: Vec<f32>,
    /// Lepkość turbulentna modelu LES (jednostki siatki).
    pub nut: Vec<f32>,
    pub labels: Vec<u16>,
    pub(crate) link_idx: Vec<u32>,
    pub(crate) link_q: Vec<[f32; 9]>,
    pub links: Vec<Link>,
    pub(crate) sponge_row: Vec<f32>,
    pub(crate) sponge_col: Vec<f32>,
    /// Bazowy profil prędkości wlotu dla każdego wiersza (jednostki siatki).
    pub profile: Vec<f32>,
    pub(crate) eq_row: Vec<[f32; 9]>,
    pub u_lb: f32,
    pub nu_lb: f32,
    pub smagorinsky: f32,
    pub turbulence: f32,
    pub tau_min: f32,
    keep_bulk: f32,
    pub profile_kind: InletProfile,
    pub(crate) pivots: Vec<[f64; 2]>,
    /// Średnia siła i moment od ostatniej synchronizacji dla każdego ciała: [Fx, Fy, Mz] (siatka).
    pub forces: Vec<[f64; 3]>,
    pub steps: u64,
    pub(crate) phases: [[f32; 2]; 8],
    gpu: Option<GpuSolver>,
    /// Suma sił z kroków policzonych na CPU od ostatniej synchronizacji.
    pending_force: Vec<[f64; 3]>,
    pending_steps: usize,
    /// Komunikat, gdy karta zawiodła w trakcie pracy i obliczenia wróciły na CPU.
    pub gpu_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unstable;

#[inline]
fn feq(i: usize, r: f32, u: f32, v: f32) -> f32 {
    let cu = 3.0 * (CX[i] as f32 * u + CY[i] as f32 * v);
    W[i] * r * (1.0 + cu + 0.5 * cu * cu - 1.5 * (u * u + v * v))
}

impl Lattice {
    pub fn new(layout: Layout) -> Self {
        let mt = if layout.top == Boundary::FreeStream { (layout.ny / 4).max(12) } else { 0 };
        let mb = if layout.bottom == Boundary::FreeStream { (layout.ny / 4).max(12) } else { 0 };
        // wlot daleko przed widocznym obszarem — zadana prędkość wlotu nie „ściska” opływu ciał
        let ml = (layout.ny / 4).max(8);
        let mr = ((layout.ny as f64 * 0.3) as usize).max(24);
        let nx = layout.nx + ml + mr;
        let ny = layout.ny + mt + mb;
        let n = nx * ny;
        let win = Window { x0: ml, y0: mb, w: layout.nx, h: layout.ny };

        // strefy pochłaniające tylko w niewidocznych marginesach
        let mut sponge_row = vec![0f32; ny];
        for (y, s) in sponge_row.iter_mut().enumerate() {
            let a = if y < mb {
                (mb - y) as f32 / mb as f32
            } else if y >= mb + layout.ny && mt > 0 {
                (y + 1 - (mb + layout.ny)) as f32 / mt as f32
            } else {
                0.0
            };
            *s = 0.12 * a * a;
        }
        let mut sponge_col = vec![0f32; nx];
        let sl = mr.min(nx);
        for (x, s) in sponge_col.iter_mut().enumerate() {
            if x + sl >= nx {
                let a = (x + sl + 1 - nx) as f32 / sl as f32;
                *s = 0.25 * a * a;
            }
        }
        let mut lat = Self {
            nx,
            ny,
            win,
            layout,
            f: vec![0.0; 9 * n],
            g: vec![0.0; 9 * n],
            kind: vec![K_FLUID; n],
            rho: vec![1.0; n],
            ux: vec![0.0; n],
            uy: vec![0.0; n],
            nut: vec![0.0; n],
            labels: vec![0; n],
            link_idx: vec![NO_LINK; n],
            link_q: Vec::new(),
            links: Vec::new(),
            sponge_row,
            sponge_col,
            profile: vec![0.0; ny],
            eq_row: vec![[0.0; 9]; ny],
            u_lb: 0.05,
            nu_lb: 0.01,
            smagorinsky: 0.15,
            turbulence: 0.0,
            tau_min: TAU_MIN,
            keep_bulk: KEEP_BULK,
            profile_kind: InletProfile::Uniform,
            pivots: Vec::new(),
            forces: Vec::new(),
            steps: 0,
            phases: [[0.0; 2]; 8],
            gpu: None,
            pending_force: Vec::new(),
            pending_steps: 0,
            gpu_error: None,
        };
        let mut seed = 0x9E37_79B9_u32;
        for p in lat.phases.iter_mut() {
            for v in p.iter_mut() {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                *v = (seed as f32 / u32::MAX as f32) * std::f32::consts::TAU;
            }
        }
        lat.rebuild_profile();
        lat.rebuild_kinds();
        lat.reset_flow();
        lat
    }

    /// Ustawia parametry fizyczne (bez zmiany siatki).
    pub fn set_params(&mut self, u_lb: f32, nu_lb: f32, smagorinsky: f32, turbulence: f32, profile: InletProfile) {
        let changed = u_lb != self.u_lb || profile != self.profile_kind;
        self.u_lb = u_lb;
        self.nu_lb = nu_lb;
        self.smagorinsky = smagorinsky;
        self.turbulence = turbulence;
        self.profile_kind = profile;
        if changed {
            self.rebuild_profile();
            if let Some(g) = &mut self.gpu {
                g.geometry_dirty = true;
            }
        }
    }

    /// Przenosi obliczenia na kartę graficzną. Zwraca nazwę karty albo `None`, gdy GPU nie jest
    /// dostępne lub siatka nie mieści się w jego pamięci (wtedy liczy dalej CPU).
    pub fn enable_gpu(&mut self) -> Option<String> {
        if let Some(g) = &self.gpu {
            return Some(g.name().to_string());
        }
        let ctx = gpu::context()?;
        let mut g = GpuSolver::new(ctx, self.nx, self.ny)?;
        g.upload_geometry(self);
        g.upload_state(self);
        let name = g.name().to_string();
        self.gpu = Some(g);
        Some(name)
    }

    pub fn gpu_name(&self) -> Option<&str> {
        self.gpu.as_ref().map(|g| g.name())
    }

    fn rebuild_profile(&mut self) {
        let Window { y0, h, .. } = self.win;
        for y in 0..self.ny {
            // wysokość nad dolną krawędzią widocznego obszaru (w ułamku wysokości)
            let s = ((y as f64 + 0.5 - y0 as f64) / h as f64).clamp(0.0, 1.0);
            let k = match self.profile_kind {
                InletProfile::Uniform => 1.0,
                InletProfile::Channel => {
                    // parabola między brzegami pełnej siatki (dla ścian = brzegi widoczne)
                    let t = (y as f64 + 0.5) / self.ny as f64;
                    6.0 * t * (1.0 - t)
                }
                InletProfile::PowerLaw { alpha } => s.max(1e-3).powf(alpha),
            };
            self.profile[y] = self.u_lb * k as f32;
            for i in 0..9 {
                self.eq_row[y][i] = feq(i, 1.0, self.profile[y], 0.0);
            }
        }
    }

    /// Stan początkowy: strumień swobodny wszędzie poza ciałami (impulsowy start).
    pub fn reset_flow(&mut self) {
        for y in 0..self.ny {
            for x in 0..self.nx {
                let c = y * self.nx + x;
                let solid = self.labels[c] != 0;
                let u = if solid { 0.0 } else { self.profile[y] };
                for i in 0..9 {
                    self.f[i * self.nx * self.ny + c] = feq(i, 1.0, u, 0.0);
                }
                self.rho[c] = 1.0;
                self.ux[c] = u;
                self.uy[c] = 0.0;
                self.nut[c] = 0.0;
            }
        }
        self.g.copy_from_slice(&self.f);
        self.steps = 0;
        self.pending_force.clear();
        self.pending_steps = 0;
        if let Some(g) = &mut self.gpu {
            g.state_dirty = true;
            g.discard_pending();
        }
    }

    /// Przekształcenie: punkt świata [m] → współrzędne siatki (środek komórki (i,j) to (i+½, j+½)).
    pub fn world_to_lattice(&self, p: [f64; 2], dx: f64) -> [f64; 2] {
        [p[0] / dx + self.win.x0 as f64, p[1] / dx + self.win.y0 as f64]
    }

    /// Rasteryzacja ciał i wyznaczenie połączeń płyn–ściana z dokładną odległością q.
    pub fn set_bodies(&mut self, bodies: &[Body], dx: f64) {
        let (nx, ny) = (self.nx, self.ny);
        let prepared: Vec<Prepared> = bodies.iter().map(|b| b.prepare()).collect();
        let mut labels = vec![0u16; nx * ny];
        let win = self.win;
        let center = |x: usize, y: usize| -> [f64; 2] { [(x as f64 + 0.5 - win.x0 as f64) * dx, (y as f64 + 0.5 - win.y0 as f64) * dx] };
        // ciała dotykające krawędzi widocznego obszaru przedłużają się w niewidoczne marginesy
        // (np. uskok lub ściana kanału ciągnie się do wlotu), zamiast kończyć się na krawędzi
        let (wv, hv) = (win.w as f64 * dx, win.h as f64 * dx);
        let e = 0.25 * dx;
        let clampw = |w: [f64; 2]| [w[0].clamp(e, wv - e), w[1].clamp(e, hv - e)];
        for (k, p) in prepared.iter().enumerate().take(u16::MAX as usize - 1) {
            let [b0, b1, b2, b3] = p.bbox;
            let touch = |v: f64, lo: f64| v <= lo + dx;
            let x0 = if touch(b0, 0.0) { 0 } else { ((b0 / dx + win.x0 as f64).floor() as isize - 1).clamp(0, nx as isize) as usize };
            let x1 = if b2 >= wv - dx { nx } else { ((b2 / dx + win.x0 as f64).ceil() as isize + 1).clamp(0, nx as isize) as usize };
            let y0 = if touch(b1, 0.0) { 0 } else { ((b1 / dx + win.y0 as f64).floor() as isize - 1).clamp(0, ny as isize) as usize };
            let y1 = if b3 >= hv - dx { ny } else { ((b3 / dx + win.y0 as f64).ceil() as isize + 1).clamp(0, ny as isize) as usize };
            if x1 <= x0 || y1 <= y0 {
                continue;
            }
            let rows: Vec<Vec<bool>> =
                (y0..y1).into_par_iter().map(|y| (x0..x1).map(|x| p.contains(clampw(center(x, y)))).collect()).collect();
            for (j, row) in rows.into_iter().enumerate() {
                for (i, inside) in row.into_iter().enumerate() {
                    if inside {
                        labels[(y0 + j) * nx + x0 + i] = k as u16 + 1;
                    }
                }
            }
        }
        // wlot musi pozostać płynem
        for y in 0..ny {
            labels[y * nx] = 0;
        }

        // połączenia z dokładną odległością ściany (bisekcja wzdłuż kierunku sieci)
        let mut link_idx = vec![NO_LINK; nx * ny];
        let mut link_q = Vec::new();
        let mut links = Vec::new();
        for y in 0..ny {
            for x in 0..nx {
                let c = y * nx + x;
                if labels[c] != 0 {
                    continue;
                }
                let mut qs = [-1f32; 9];
                let mut any = false;
                for k in 1..9 {
                    let xn = x as i32 + CX[k];
                    let yn = y as i32 + CY[k];
                    if xn < 0 || yn < 0 || xn >= nx as i32 || yn >= ny as i32 {
                        continue;
                    }
                    let lab = labels[yn as usize * nx + xn as usize];
                    if lab == 0 {
                        continue;
                    }
                    let p = &prepared[lab as usize - 1];
                    let a = center(x, y);
                    let d = [CX[k] as f64 * dx, CY[k] as f64 * dx];
                    let (mut lo, mut hi) = (0.0f64, 1.0f64);
                    for _ in 0..14 {
                        let m = 0.5 * (lo + hi);
                        if p.contains(clampw([a[0] + m * d[0], a[1] + m * d[1]])) {
                            hi = m;
                        } else {
                            lo = m;
                        }
                    }
                    let q = (0.5 * (lo + hi)).clamp(0.02, 1.0);
                    qs[k] = q as f32;
                    any = true;
                    let wpt = p.to_local([a[0] + q * d[0], a[1] + q * d[1]]);
                    links.push(Link { cell: c as u32, body: lab - 1, local: [wpt[0] as f32, wpt[1] as f32] });
                }
                if any {
                    link_idx[c] = link_q.len() as u32;
                    link_q.push(qs);
                }
            }
        }

        // komórki, które przestały być ciałem, startują ze spoczynku (już tak są zapisane);
        // komórki, które stały się ciałem, nie są dalej używane
        self.labels = labels;
        self.link_idx = link_idx;
        self.link_q = link_q;
        self.links = links;
        self.pivots = bodies.iter().map(|b| self.world_to_lattice(b.pos, dx)).collect();
        self.forces = vec![[0.0; 3]; bodies.len()];
        self.pending_force.clear();
        self.pending_steps = 0;
        self.rebuild_kinds();
        if let Some(g) = &mut self.gpu {
            g.geometry_dirty = true;
            g.discard_pending();
        }
    }

    /// Profil wlotu w chwili bieżącej: baza + syntetyczna turbulencja (suma modów o losowych fazach).
    fn inlet(&self) -> (Vec<f32>, Vec<f32>) {
        let ny = self.ny;
        let mut iu = self.profile.clone();
        let mut iv = vec![0f32; ny];
        if self.turbulence > 0.0 {
            let t = self.steps as f32 * self.u_lb / self.win.h as f32;
            // rms sumy modów z amplitudą 1/k dla k = 2..9 wynosi ≈ 0,51
            let norm = self.turbulence * self.u_lb / 0.51;
            for (y, (u, v)) in iu.iter_mut().zip(iv.iter_mut()).enumerate() {
                let s = y as f32 / ny as f32;
                let (mut du, mut dv) = (0f32, 0f32);
                for (k, ph) in self.phases.iter().enumerate() {
                    let kk = (k + 2) as f32;
                    du += (std::f32::consts::TAU * (kk * s + 1.7 * kk * t) + ph[0]).sin() / kk;
                    dv += (std::f32::consts::TAU * (kk * s - 1.3 * kk * t) + ph[1]).sin() / kk;
                }
                *u += du * norm;
                *v += dv * norm;
            }
        }
        (iu, iv)
    }

    /// Wykonuje `n` kroków i synchronizuje wyniki. Siły w `forces` to średnia z tych kroków.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn step(&mut self, n: usize) -> Result<(), Unstable> {
        self.advance(n)?;
        self.sync()
    }

    /// Wykonuje `n` kroków bez kopiowania wyników z karty — pola `rho`, `ux`, `uy`, `nut` i `forces`
    /// odświeża dopiero [`Lattice::sync`]. Na CPU niestabilność jest zgłaszana od razu.
    pub fn advance(&mut self, n: usize) -> Result<(), Unstable> {
        if let Some(mut g) = self.gpu.take() {
            if g.geometry_dirty {
                g.upload_geometry(self);
            }
            if g.state_dirty {
                g.upload_state(self);
            }
            let inp = StepInput {
                tau0: (3.0 * self.nu_lb + 0.5).max(self.tau_min),
                les_k: 18.0 * std::f32::consts::SQRT_2 * self.smagorinsky * self.smagorinsky,
                keep_bulk: self.keep_bulk,
                rho_ref: self.reference_density(),
            };
            g.run(self, self.steps, n, &inp);
            self.steps += n as u64;
            self.gpu = Some(g);
            return Ok(());
        }
        let nb = self.pivots.len();
        if self.pending_force.len() != nb {
            self.pending_force = vec![[0.0; 3]; nb];
        }
        for _ in 0..n {
            let fr = self.step_once()?;
            for (t, f) in self.pending_force.iter_mut().zip(fr) {
                for c in 0..3 {
                    t[c] += f[c];
                }
            }
            self.pending_steps += 1;
        }
        Ok(())
    }

    /// Kopiuje wyniki porcji kroków (pola i średnie siły) i sprawdza stabilność.
    pub fn sync(&mut self) -> Result<(), Unstable> {
        let (total, steps, bad) = if let Some(mut g) = self.gpu.take() {
            match g.read(self) {
                Ok(r) => {
                    self.gpu = Some(g);
                    r
                }
                Err(e) => {
                    // karta przestała odpowiadać — dalej liczy CPU, od świeżego przepływu
                    log::error!("błąd GPU: {e}");
                    self.gpu_error = Some(e);
                    self.reset_flow();
                    return Ok(());
                }
            }
        } else {
            let nb = self.pivots.len();
            let t = std::mem::replace(&mut self.pending_force, vec![[0.0; 3]; nb]);
            (t, std::mem::take(&mut self.pending_steps), false)
        };
        if steps > 0 {
            self.forces = total.into_iter().map(|t| t.map(|v| v / steps as f64)).collect();
        }
        if bad || !self.rho[self.nx * (self.ny / 2) + self.nx / 2].is_finite() {
            return Err(Unstable);
        }
        Ok(())
    }

    #[allow(clippy::needless_range_loop)]
    fn step_once(&mut self) -> Result<Vec<[f64; 3]>, Unstable> {
        let (nx, ny) = (self.nx, self.ny);
        let n = nx * ny;
        let (iu, iv) = self.inlet();
        let inlet_rho: Vec<f32> = (0..ny).map(|y| self.rho[y * nx + 1]).collect();
        let nb = self.pivots.len();
        let tau0 = (3.0 * self.nu_lb + 0.5).max(self.tau_min);
        let cs2 = self.smagorinsky * self.smagorinsky;
        let les_k = 18.0 * std::f32::consts::SQRT_2 * cs2;
        let u_wall = self.u_lb;
        let top = self.layout.top;
        let bottom = self.layout.bottom;
        // ciśnienie odniesienia: siły liczymy od nadciśnienia względem strumienia swobodnego, żeby
        // ciało stojące na podłodze (bez płynu pod spodem) nie było „dociskane” ciśnieniem bezwzględnym
        let rho_ref = self.reference_density();
        let keep_bulk = self.keep_bulk;

        let f: &[f32] = &self.f;
        let kind = &self.kind;
        let labels = &self.labels;
        let link_idx = &self.link_idx;
        let link_q = &self.link_q;
        let sponge_row = &self.sponge_row;
        let sponge_col = &self.sponge_col;
        let eq_row = &self.eq_row;
        let pivots = &self.pivots;
        let out = SyncPtr(self.g.as_mut_ptr());
        let off: [isize; 9] = std::array::from_fn(|i| CX[i] as isize + CY[i] as isize * nx as isize);

        let (forces, bad) = self
            .rho
            .par_chunks_mut(nx)
            .zip(self.ux.par_chunks_mut(nx))
            .zip(self.uy.par_chunks_mut(nx))
            .zip(self.nut.par_chunks_mut(nx))
            .enumerate()
            .fold(
                || (vec![[0f64; 3]; nb], false),
                |(mut acc, mut bad), (y, (((rr, uxr), uyr), nr))| {
                    let g = out.get();
                    // każdy wiersz zapisuje wyłącznie własne komórki (rozłączne indeksy)
                    let put = |i: usize, c: usize, v: f32| unsafe { *g.add(i * n + c) = v };
                    let mut fl = [0f32; 9];
                    for x in 0..nx {
                        let c = y * nx + x;
                        match kind[c] {
                            K_FLUID => {
                                for i in 0..9 {
                                    // komórka wewnętrzna: wszyscy sąsiedzi istnieją i są płynem
                                    fl[i] = unsafe { *f.get_unchecked(i * n + (c as isize - off[i]) as usize) };
                                }
                            }
                            K_SOLID => {
                                for i in 0..9 {
                                    put(i, c, W[i]);
                                }
                                rr[x] = 1.0;
                                uxr[x] = 0.0;
                                uyr[x] = 0.0;
                                nr[x] = 0.0;
                                continue;
                            }
                            K_INLET => {
                                let r = inlet_rho[y];
                                for i in 0..9 {
                                    put(i, c, feq(i, r, iu[y], iv[y]));
                                }
                                rr[x] = r;
                                uxr[x] = iu[y];
                                uyr[x] = iv[y];
                                nr[x] = 0.0;
                                continue;
                            }
                            _ => {
                                let li = link_idx[c];
                                for i in 0..9 {
                                    let sx = x as i32 - CX[i];
                                    let sy = y as i32 - CY[i];
                                    if sy < 0 || sy >= ny as i32 {
                                        let side = if sy < 0 { bottom } else { top };
                                        fl[i] = match side {
                                            Boundary::FreeStream => eq_row[y][i],
                                            Boundary::SlipWall => {
                                                let sc = y * nx + sx.clamp(0, nx as i32 - 1) as usize;
                                                if labels[sc] == 0 {
                                                    f[MIRY[i] * n + sc]
                                                } else {
                                                    f[MIRY[i] * n + c]
                                                }
                                            }
                                            Boundary::NoSlipWall => f[OPP[i] * n + c],
                                            Boundary::MovingGround => f[OPP[i] * n + c] + 6.0 * W[i] * CX[i] as f32 * u_wall,
                                        };
                                        continue;
                                    }
                                    if sx >= nx as i32 {
                                        // wylot: zerowy gradient
                                        fl[i] = f[i * n + c - 1];
                                        continue;
                                    }
                                    let sc = sy as usize * nx + sx as usize;
                                    let lab = labels[sc];
                                    if lab == 0 {
                                        fl[i] = f[i * n + sc];
                                        continue;
                                    }
                                    // ściana w kierunku k = −c_i od tej komórki (Bouzidi)
                                    let k = OPP[i];
                                    let q = if li != NO_LINK { link_q[li as usize][k] } else { 0.5 };
                                    let q = if q > 0.0 { q } else { 0.5 };
                                    let fk = f[k * n + c];
                                    let val = if q < 0.5 {
                                        let x2 = x as i32 + CX[i];
                                        let y2 = y as i32 + CY[i];
                                        if x2 >= 0
                                            && y2 >= 0
                                            && x2 < nx as i32
                                            && y2 < ny as i32
                                            && labels[y2 as usize * nx + x2 as usize] == 0
                                        {
                                            2.0 * q * fk + (1.0 - 2.0 * q) * f[k * n + y2 as usize * nx + x2 as usize]
                                        } else {
                                            fk
                                        }
                                    } else {
                                        fk / (2.0 * q) + (2.0 * q - 1.0) / (2.0 * q) * f[i * n + c]
                                    };
                                    fl[i] = val;
                                    // pęd przekazany ciału
                                    let b = (lab - 1) as usize;
                                    if b < nb {
                                        let s = (fk + val - 2.0 * W[k] * rho_ref) as f64;
                                        let fx = CX[k] as f64 * s;
                                        let fy = CY[k] as f64 * s;
                                        let wx = x as f64 + 0.5 + q as f64 * CX[k] as f64 - pivots[b][0];
                                        let wy = y as f64 + 0.5 + q as f64 * CY[k] as f64 - pivots[b][1];
                                        acc[b][0] += fx;
                                        acc[b][1] += fy;
                                        acc[b][2] += wx * fy - wy * fx;
                                    }
                                }
                            }
                        }

                        // momenty: gęstość, pęd i tensor naprężeń
                        let r = fl[0] + fl[1] + fl[2] + fl[3] + fl[4] + fl[5] + fl[6] + fl[7] + fl[8];
                        let jx = fl[1] - fl[3] + fl[5] - fl[6] - fl[7] + fl[8];
                        let jy = fl[2] - fl[4] + fl[5] + fl[6] - fl[7] - fl[8];
                        let diag = fl[5] + fl[6] + fl[7] + fl[8];
                        let mut pxx = fl[1] + fl[3] + diag;
                        let mut pyy = fl[2] + fl[4] + diag;
                        let mut pxy = fl[5] - fl[6] + fl[7] - fl[8];
                        let r = if r > 0.2 && r < 5.0 {
                            r
                        } else {
                            bad = true;
                            r.clamp(0.2, 5.0)
                        };
                        let inv = 1.0 / r;
                        let u = jx * inv;
                        let v = jy * inv;
                        pxx -= r * (1.0 / 3.0 + u * u);
                        pyy -= r * (1.0 / 3.0 + v * v);
                        pxy -= r * u * v;
                        // Smagorinsky: τ = ½(τ₀ + √(τ₀² + 18√2 C² |Π|/ρ))
                        let qn = (pxx * pxx + pyy * pyy + 2.0 * pxy * pxy).sqrt();
                        let tau = 0.5 * (tau0 + (tau0 * tau0 + les_k * qn * inv).sqrt());
                        let keep = 1.0 - 1.0 / tau;
                        let sg = sponge_row[y].max(sponge_col[x]);
                        let usq = 1.5 * (u * u + v * v);
                        // część nierównowagowa odtworzona z Π (regularyzacja); część dewiatorowa
                        // relaksuje z lepkością ścinania, ślad — z większą lepkością objętościową,
                        // która tłumi fale akustyczne modelu słabo ściśliwego (Dellar 2001)
                        let tr = 0.5 * (pxx + pyy);
                        let dxx = keep * (pxx - tr) + keep_bulk * tr;
                        let dyy = keep * (pyy - tr) + keep_bulk * tr;
                        let axx = 4.5 * dxx;
                        let ayy = 4.5 * dyy;
                        let axy = 9.0 * keep * pxy;
                        let t0 = -(axx + ayy) / 3.0;
                        for i in 0..9 {
                            let (cx, cy) = (CX[i] as f32, CY[i] as f32);
                            let cu = 3.0 * (cx * u + cy * v);
                            let neq = t0 + cx * cx * axx + cy * cy * ayy + cx * cy * axy;
                            let mut post = W[i] * (r * (1.0 + cu + 0.5 * cu * cu - usq) + neq);
                            if sg > 0.0 {
                                post += sg * (eq_row[y][i] - post);
                            }
                            put(i, c, post);
                        }
                        rr[x] = r;
                        uxr[x] = u;
                        uyr[x] = v;
                        nr[x] = (tau - tau0) / 3.0;
                    }
                    (acc, bad)
                },
            )
            .reduce(
                || (vec![[0f64; 3]; nb], false),
                |(mut a, ba), (b, bb)| {
                    for (x, y) in a.iter_mut().zip(b) {
                        for c in 0..3 {
                            x[c] += y[c];
                        }
                    }
                    (a, ba || bb)
                },
            );
        std::mem::swap(&mut self.f, &mut self.g);
        self.steps += 1;
        if bad || !self.rho[self.nx * (self.ny / 2) + self.nx / 2].is_finite() {
            self.pending_force.clear();
            self.pending_steps = 0;
            return Err(Unstable);
        }
        Ok(forces)
    }

    /// Klasyfikacja komórek dla jądra (szybka ścieżka dla komórek wewnętrznych).
    fn rebuild_kinds(&mut self) {
        let (nx, ny) = (self.nx, self.ny);
        self.kind = (0..nx * ny)
            .map(|c| {
                let (x, y) = (c % nx, c / nx);
                if self.labels[c] != 0 {
                    K_SOLID
                } else if x == 0 {
                    K_INLET
                } else if x == nx - 1 || y == 0 || y == ny - 1 || self.link_idx[c] != NO_LINK {
                    K_SPECIAL
                } else {
                    K_FLUID
                }
            })
            .collect();
    }

    /// Kopia pola z widocznego wycinka.
    pub fn visible(&self, src: &[f32]) -> Vec<f32> {
        let Window { x0, y0, w, h } = self.win;
        let mut out = Vec::with_capacity(w * h);
        for y in y0..y0 + h {
            out.extend_from_slice(&src[y * self.nx + x0..y * self.nx + x0 + w]);
        }
        out
    }

    pub fn visible_labels(&self) -> Vec<u16> {
        let Window { x0, y0, w, h } = self.win;
        let mut out = Vec::with_capacity(w * h);
        for y in y0..y0 + h {
            out.extend_from_slice(&self.labels[y * self.nx + x0..y * self.nx + x0 + w]);
        }
        out
    }

    /// Gęstość odniesienia (ciśnienie strumienia swobodnego): średnia na lewej krawędzi obszaru,
    /// z pominięciem komórek ciał.
    pub fn reference_density(&self) -> f32 {
        // lewa krawędź widocznego obszaru (strumień niezaburzony)
        let x = self.win.x0;
        let (mut s, mut n) = (0f64, 0usize);
        for y in self.win.y0..self.win.y0 + self.win.h {
            let c = y * self.nx + x;
            if self.labels[c] == 0 {
                s += self.rho[c] as f64;
                n += 1;
            }
        }
        if n == 0 {
            1.0
        } else {
            (s / n as f64) as f32
        }
    }

    pub fn cell_count(&self) -> usize {
        self.nx * self.ny
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::geometry::{Body, Shape};

    /// Przepływ Poiseuille'a w kanale ze ścianami: profil powinien zbiegać do paraboli.
    #[test]
    fn poiseuille_channel_profile() {
        let mut lat = Lattice::new(Layout { nx: 60, ny: 21, top: Boundary::NoSlipWall, bottom: Boundary::NoSlipWall });
        lat.set_params(0.04, 0.1, 0.0, 0.0, InletProfile::Channel);
        lat.reset_flow();
        lat.step(6000).unwrap();
        let x = lat.win.x0 + 40;
        let umax = lat.ux[(lat.ny / 2) * lat.nx + x];
        assert!((umax / 0.06 - 1.0).abs() < 0.03, "umax = {umax}");
        for y in 0..lat.ny {
            let t = (y as f32 + 0.5) / lat.ny as f32;
            let exact = 0.04 * 6.0 * t * (1.0 - t);
            let u = lat.ux[y * lat.nx + x];
            assert!((u - exact).abs() < 0.004, "y={y} u={u} exact={exact}");
        }
    }

    /// Opór walca przy Re = 20 (przepływ ustalony): literatura Cd ≈ 2,0–2,15 (zależnie od blokady).
    #[test]
    fn cylinder_drag_re20() {
        // obszar 25D × 15D (+ marginesy) — wpływ blokady i bliskiego wlotu podnosi Cd o kilka procent
        let d_cells = 16.0;
        let ny = 240;
        let nx = 400;
        let dx = 1.0;
        let mut lat = Lattice::new(Layout { nx, ny, top: Boundary::FreeStream, bottom: Boundary::FreeStream });
        let u = 0.05f32;
        let nu = u * d_cells as f32 / 20.0;
        lat.set_params(u, nu, 0.0, 0.0, InletProfile::Uniform);
        let body = Body::new(1, Shape::Circle { r: d_cells / 2.0 }, [120.0, ny as f64 / 2.0], 0.0);
        lat.set_bodies(&[body], dx);
        lat.reset_flow();
        lat.step(12000).unwrap();
        lat.step(500).unwrap();
        let cd = lat.forces[0][0] / (0.5 * (u as f64).powi(2) * d_cells);
        let cl = lat.forces[0][1] / (0.5 * (u as f64).powi(2) * d_cells);
        assert!((1.95..2.40).contains(&cd), "Cd = {cd}");
        assert!(cl.abs() < 0.05, "Cl = {cl}");
    }
}

#[cfg(test)]
mod bench {
    use super::*;

    #[test]
    #[ignore]
    fn mlups() {
        let mut lat = Lattice::new(Layout { nx: 600, ny: 300, top: Boundary::FreeStream, bottom: Boundary::FreeStream });
        lat.set_params(0.06, 0.0001, 0.15, 0.005, InletProfile::Uniform);
        let b = crate::physics::geometry::Body::new(1, crate::physics::geometry::Shape::Circle { r: 20.0 }, [150.0, 150.0], 0.0);
        lat.set_bodies(&[b], 1.0);
        lat.step(50).unwrap();
        let t = std::time::Instant::now();
        let n = 400;
        lat.step(n).unwrap();
        let dt = t.elapsed().as_secs_f64();
        println!("cells {} MLUPS {:.1}", lat.cell_count(), (lat.cell_count() * n) as f64 / dt / 1e6);
    }
}

#[cfg(test)]
mod validation {
    use super::*;
    use crate::physics::geometry::{Body, Shape};

    #[test]
    #[ignore]
    fn karman_re100() {
        for (cs, d, u) in [(0.15f32, 16.0, 0.06f32), (0.15, 12.0, 0.06)] {
            let ny = (d * 20.0) as usize;
            let nx = 2 * ny;
            let mut lat = Lattice::new(Layout { nx, ny, top: Boundary::FreeStream, bottom: Boundary::FreeStream });
            lat.set_params(u, u * d as f32 / 100.0, cs, 0.002, InletProfile::Uniform);
            lat.set_bodies(&[Body::new(1, Shape::Circle { r: d / 2.0 }, [8.0 * d, ny as f64 / 2.0], 0.0)], 1.0);
            lat.reset_flow();
            let q = 0.5 * (u as f64).powi(2) * d;
            let conv = d / u as f64;
            lat.step((120.0 * conv) as usize).unwrap();
            let mut cd = Vec::new();
            let mut cl = Vec::new();
            for _ in 0..(40.0 * conv / 10.0) as usize {
                lat.step(10).unwrap();
                cd.push(lat.forces[0][0] / q);
                cl.push(lat.forces[0][1] / q);
            }
            let m = cd.iter().sum::<f64>() / cd.len() as f64;
            let amp = 0.5 * (cl.iter().cloned().fold(f64::MIN, f64::max) - cl.iter().cloned().fold(f64::MAX, f64::min));
            let mut ups = 0;
            for k in 1..cl.len() {
                if cl[k - 1] < 0.0 && cl[k] >= 0.0 {
                    ups += 1;
                }
            }
            let st = ups as f64 / 40.0;
            println!("C={cs} D={d} u={u}: Cd={m:.3} Cl_amp={amp:.3} St~{st:.3}");
        }
    }
}

#[cfg(test)]
mod noise {
    use super::*;

    /// Szum siatkowy (prążki) przy bardzo dużym Re: średnia |Δ²u_y| w górnej połowie obszaru,
    /// względem u_lb. Bez lepkości minimalnej i rozdzielenia lepkości objętościowej wynosi ≈ 0,3.
    #[test]
    fn grid_noise_is_damped() {
        let scene = (crate::scene::presets().into_iter().find(|p| p.id == "city").unwrap().build)();
        let (nx, ny) = scene.flow.grid();
        let mut lat = Lattice::new(Layout { nx, ny, top: scene.flow.top, bottom: scene.flow.bottom });
        let u = scene.flow.units();
        lat.set_params(u.u_lb as f32, u.nu_lb as f32, scene.flow.smagorinsky as f32, scene.flow.turbulence as f32, scene.flow.profile);
        lat.set_bodies(&scene.bodies, u.dx);
        lat.reset_flow();
        lat.step(4000).unwrap();
        let w = lat.win;
        let (mut s, mut n) = (0f64, 0usize);
        for y in w.y0 + w.h / 2..w.y0 + w.h {
            for x in w.x0 + 1..w.x0 + w.w - 1 {
                let c = y * lat.nx + x;
                s += (lat.uy[c + 1] - 2.0 * lat.uy[c] + lat.uy[c - 1]).abs() as f64;
                n += 1;
            }
        }
        let noise = s / n as f64 / u.u_lb;
        assert!(noise < 0.003, "szum siatkowy {noise}");
    }
}

#[cfg(test)]
mod airfoil_validation {
    use super::*;
    use crate::physics::geometry::{Body, Naca, Shape};

    /// NACA 0012 przy Re = 2000: zwraca (Cl, Cd, średnie Cp górnej i dolnej powierzchni na 20–60 % cięciwy).
    fn naca0012(alpha: f64, res: usize, steps: usize) -> (f64, f64, f64, f64) {
        let domain = [1.2, 0.6];
        let dx = domain[1] / res as f64;
        let nx = (domain[0] / dx).round() as usize;
        let mut lat = Lattice::new(Layout { nx, ny: res, top: Boundary::FreeStream, bottom: Boundary::FreeStream });
        let chord = 0.2;
        let c_cells = chord / dx;
        let u = 0.05f32;
        lat.set_params(u, u * c_cells as f32 / 2000.0, 0.15, 0.0, InletProfile::Uniform);
        let body = Body::new(1, Shape::Airfoil { chord, naca: Naca::new(0.0, 0.4, 0.12) }, [0.36, 0.3], alpha);
        lat.set_bodies(std::slice::from_ref(&body), dx);
        lat.reset_flow();
        if lat.enable_gpu().is_none() {
            eprintln!("(liczone na CPU)");
        }
        lat.step(steps).unwrap();
        lat.step(2000).unwrap();
        let q = 0.5 * (u as f64).powi(2) * c_cells;
        let (cd, cl) = (lat.forces[0][0] / q, lat.forces[0][1] / q);
        let rref = lat.reference_density();
        let (mut up, mut nu, mut lo, mut nl) = (0f64, 0, 0f64, 0);
        for l in &lat.links {
            let xc = l.local[0] as f64 / chord + 0.25;
            if (0.2..0.6).contains(&xc) {
                let cp = ((lat.rho[l.cell as usize] - rref) / 3.0 / (0.5 * u * u)) as f64;
                if l.local[1] > 0.0 {
                    up += cp;
                    nu += 1;
                } else {
                    lo += cp;
                    nl += 1;
                }
            }
        }
        (cl, cd, up / nu.max(1) as f64, lo / nl.max(1) as f64)
    }

    /// Profil symetryczny przy α = 0: brak siły nośnej, a w środkowej części cięciwy podciśnienie
    /// z obu stron (Cp ≈ −0,35 z teorii potencjalnej dla grubości 12 %). Przy α = 4° rośnie różnica
    /// ciśnień — górna powierzchnia ssie mocniej niż dolna.
    #[test]
    #[ignore]
    fn naca0012_pressure_distribution() {
        let (cl, _, up, lo) = naca0012(0.0, 240, 16000);
        assert!(cl.abs() < 0.01, "Cl(0°) = {cl}");
        assert!((-0.42..-0.28).contains(&up) && (up - lo).abs() < 0.01, "Cp góra {up}, dół {lo}");
        let (cl, cd, up, lo) = naca0012(4.0, 240, 16000);
        println!("α = 4°: Cl = {cl:.3}, Cd = {cd:.3}, Cp góra {up:.3}, dół {lo:.3}");
        assert!(cl > 0.12 && up < lo - 0.08, "Cl = {cl}, Cp góra {up}, dół {lo}");
    }
}
