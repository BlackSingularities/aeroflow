//! Scena: konfiguracja przepływu, ciała, źródła dymu i sondy. Zapisywana do pliku `.aeroflow` (JSON).

use serde::{Deserialize, Serialize};

use crate::physics::geometry::{Body, Naca, Shape, V2};
use crate::physics::lattice::{Boundary, InletProfile};
use crate::physics::units::{Fluid, Units};

pub const FILE_EXT: &str = "aeroflow";
/// Największa liczba widocznych komórek siatki (≈ 1,1 GB pamięci na populacje wraz z marginesami).
pub const MAX_CELLS: usize = 3_000_000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FlowConfig {
    pub fluid: Fluid,
    /// Prędkość strumienia swobodnego [m/s].
    pub velocity: f64,
    /// Rozmiar widocznego obszaru [m]: szerokość, wysokość.
    pub domain: [f64; 2],
    /// Liczba komórek na wysokość obszaru.
    pub resolution: u32,
    /// Prędkość strumienia w jednostkach siatki (liczba Macha siatki · c_s).
    pub u_lattice: f64,
    /// Stała Smagorinsky'ego modelu LES.
    pub smagorinsky: f64,
    pub top: Boundary,
    pub bottom: Boundary,
    pub profile: InletProfile,
    /// Intensywność turbulencji wlotu (ułamek U).
    pub turbulence: f64,
    /// Rozpiętość do przeliczenia sił 2D na siły całkowite [m].
    pub span: f64,
}

impl Default for FlowConfig {
    fn default() -> Self {
        Self {
            fluid: Fluid::air(15.0),
            velocity: 10.0,
            domain: [1.2, 0.6],
            resolution: 280,
            u_lattice: 0.06,
            smagorinsky: 0.15,
            top: Boundary::FreeStream,
            bottom: Boundary::FreeStream,
            profile: InletProfile::Uniform,
            turbulence: 0.005,
            span: 1.0,
        }
    }
}

impl FlowConfig {
    /// Liczba komórek na wysokość — zadana rozdzielczość, ograniczona tak, by widoczna siatka nie
    /// przekroczyła [`MAX_CELLS`] (pamięć i płynność przy bardzo szerokich obszarach).
    pub fn effective_resolution(&self) -> u32 {
        let aspect = (self.domain[0] / self.domain[1]).max(1e-3);
        let cap = (MAX_CELLS as f64 / aspect).sqrt().floor() as u32;
        self.resolution.min(cap).max(32)
    }

    pub fn dx(&self) -> f64 {
        self.domain[1] / self.effective_resolution() as f64
    }

    pub fn grid(&self) -> (usize, usize) {
        let ny = self.effective_resolution() as usize;
        let nx = ((self.domain[0] / self.dx()).round() as usize).max(16);
        (nx, ny)
    }

    pub fn units(&self) -> Units {
        Units::new(&self.fluid, self.velocity, self.dx(), self.u_lattice)
    }
}

/// Wartość odniesienia z literatury (do walidacji).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Reference {
    pub cd: Option<[f64; 2]>,
    pub cl_amp: Option<[f64; 2]>,
    pub strouhal: Option<[f64; 2]>,
    pub source: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub flow: FlowConfig,
    pub bodies: Vec<Body>,
    #[serde(default)]
    pub emitters: Vec<V2>,
    #[serde(default)]
    pub probes: Vec<V2>,
    #[serde(default)]
    pub reference: Option<Reference>,
    #[serde(default)]
    pub next_id: u64,
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            name: "Nowa scena".into(),
            description: String::new(),
            flow: FlowConfig::default(),
            bodies: Vec::new(),
            emitters: Vec::new(),
            probes: Vec::new(),
            reference: None,
            next_id: 1,
        }
    }
}

impl Scene {
    pub fn alloc_id(&mut self) -> u64 {
        let m = self.bodies.iter().map(|b| b.id).max().unwrap_or(0);
        self.next_id = self.next_id.max(m + 1);
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn add(&mut self, shape: Shape, pos: V2, angle: f64) -> u64 {
        let id = self.alloc_id();
        self.bodies.push(Body::new(id, shape, pos, angle));
        id
    }

    pub fn body(&self, id: u64) -> Option<&Body> {
        self.bodies.iter().find(|b| b.id == id)
    }

    pub fn body_mut(&mut self, id: u64) -> Option<&mut Body> {
        self.bodies.iter_mut().find(|b| b.id == id)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    pub fn from_json(s: &str) -> Result<Self, String> {
        let mut sc: Scene = serde_json::from_str(s).map_err(|e| format!("Niepoprawny plik sceny: {e}"))?;
        sc.flow.resolution = sc.flow.resolution.clamp(64, 1200);
        sc.flow.u_lattice = sc.flow.u_lattice.clamp(0.01, 0.15);
        let m = sc.bodies.iter().map(|b| b.id).max().unwrap_or(0);
        sc.next_id = sc.next_id.max(m + 1);
        Ok(sc)
    }
}

pub struct Preset {
    pub id: &'static str,
    pub name: &'static str,
    pub group: &'static str,
    pub build: fn() -> Scene,
}

pub fn presets() -> Vec<Preset> {
    vec![
        Preset { id: "airfoil", name: "Profil NACA 2412 — model samolotu", group: "Aerodynamika", build: airfoil },
        Preset { id: "stall", name: "Przeciągnięcie — NACA 0012 przy 16°", group: "Aerodynamika", build: stall },
        Preset { id: "flap", name: "Skrzydło z klapą szczelinową", group: "Aerodynamika", build: flap },
        Preset { id: "car", name: "Samochód nad ruchomą podłogą", group: "Aerodynamika", build: car },
        Preset { id: "plate", name: "Płyta prostopadła do strumienia", group: "Aerodynamika", build: plate },
        Preset { id: "karman", name: "Ścieżka Kármána — walec w wodzie, Re 100", group: "Walidacja", build: karman },
        Preset { id: "cyl20", name: "Walec, Re 20 — przepływ ustalony", group: "Walidacja", build: cyl20 },
        Preset { id: "cyl200", name: "Walec, Re 200", group: "Walidacja", build: cyl200 },
        Preset { id: "square", name: "Pręt kwadratowy, Re 100", group: "Walidacja", build: square },
        Preset { id: "city", name: "Wiatr w zabudowie miejskiej", group: "Środowisko", build: city },
        Preset { id: "venturi", name: "Zwężka Venturiego (Bernoulli)", group: "Kanały", build: venturi },
        Preset { id: "step", name: "Uskok — strefa recyrkulacji", group: "Kanały", build: step },
        Preset { id: "tandem", name: "Tandem walców — interferencja śladów", group: "Kanały", build: tandem },
        Preset { id: "empty", name: "Pusty tunel", group: "Inne", build: empty },
    ]
}

fn water_flow(velocity: f64, domain: [f64; 2], resolution: u32) -> FlowConfig {
    FlowConfig {
        fluid: Fluid::water(20.0),
        velocity,
        domain,
        resolution,
        // tunel wodny o niskiej turbulencji — zaburzenie przyspiesza pojawienie się ścieżki wirowej
        turbulence: 0.002,
        ..FlowConfig::default()
    }
}

fn airfoil() -> Scene {
    let mut s = Scene {
        name: "Profil NACA 2412".into(),
        description: "Skrzydło modelu samolotu: cięciwa 20 cm, 10 m/s w powietrzu (Re ≈ 1,4·10⁵). \
                      Kąt natarcia 6°. Przy tej liczbie Reynoldsa model 2D LES pokazuje obraz jakościowy — \
                      porównaj trendy Cl(α) przy pomocy biegunowej."
            .into(),
        flow: FlowConfig { domain: [1.2, 0.6], resolution: 300, ..FlowConfig::default() },
        ..Scene::default()
    };
    s.add(Shape::Airfoil { chord: 0.2, naca: Naca::new(0.02, 0.4, 0.12) }, [0.34, 0.3], 6.0);
    s
}

fn stall() -> Scene {
    let mut s = Scene {
        name: "Przeciągnięcie".into(),
        description: "NACA 0012 przy kącie 16°, cięciwa 10 cm, 3 m/s (Re ≈ 2·10⁴): przepływ odrywa się od \
                      strony ssącej, a ślad pulsuje. Zmniejsz kąt, by zobaczyć ponowne przylgnięcie."
            .into(),
        flow: FlowConfig { velocity: 3.0, domain: [0.6, 0.3], resolution: 300, ..FlowConfig::default() },
        ..Scene::default()
    };
    s.add(Shape::Airfoil { chord: 0.1, naca: Naca::new(0.0, 0.4, 0.12) }, [0.18, 0.15], 16.0);
    s
}

fn flap() -> Scene {
    let mut s = Scene {
        name: "Klapa szczelinowa".into(),
        description: "Profil główny z klapą wychyloną o 25°. Szczelina między elementami przyspiesza przepływ \
                      nad klapą i opóźnia oderwanie — tak działa mechanizacja skrzydła przy lądowaniu."
            .into(),
        flow: FlowConfig { velocity: 5.0, domain: [0.8, 0.4], resolution: 320, ..FlowConfig::default() },
        ..Scene::default()
    };
    s.add(Shape::Airfoil { chord: 0.15, naca: Naca::new(0.04, 0.4, 0.12) }, [0.25, 0.21], 4.0);
    let id = s.add(Shape::Airfoil { chord: 0.05, naca: Naca::new(0.02, 0.4, 0.12) }, [0.374, 0.187], 25.0);
    if let Some(b) = s.body_mut(id) {
        b.name = "Klapa".into();
    }
    s
}

fn car() -> Scene {
    let mut s = Scene {
        name: "Samochód".into(),
        description: "Model sylwetki samochodu (40 cm) przy 20 m/s nad podłogą poruszającą się z prędkością \
                      strumienia — jak w tunelach motoryzacyjnych z pasem ruchomym. Obserwuj oderwanie za tylną \
                      szybą i ślad."
            .into(),
        flow: FlowConfig { velocity: 20.0, domain: [1.6, 0.5], resolution: 260, bottom: Boundary::MovingGround, ..FlowConfig::default() },
        ..Scene::default()
    };
    let l = 0.4;
    let raw: [[f64; 2]; 14] = [
        [0.01, 0.0],
        [0.0, 0.06],
        [0.015, 0.11],
        [0.07, 0.135],
        [0.29, 0.17],
        [0.34, 0.20],
        [0.43, 0.285],
        [0.62, 0.295],
        [0.80, 0.245],
        [0.93, 0.215],
        [0.99, 0.19],
        [1.0, 0.09],
        [0.985, 0.02],
        [0.95, 0.0],
    ];
    let pts: Vec<V2> = raw.iter().map(|p| [p[0] * l, p[1] * l]).collect();
    let cx = pts.iter().map(|p| p[0]).sum::<f64>() / pts.len() as f64;
    let cy = pts.iter().map(|p| p[1]).sum::<f64>() / pts.len() as f64;
    let local = pts.iter().map(|p| [p[0] - cx, p[1] - cy]).collect();
    let id = s.add(Shape::Polygon { pts: local }, [0.4 + cx, 0.012 + cy], 0.0);
    if let Some(b) = s.body_mut(id) {
        b.name = "Samochód".into();
    }
    s
}

fn plate() -> Scene {
    let mut s = Scene {
        name: "Płyta prostopadła".into(),
        description: "Cienka płyta ustawiona w poprzek strumienia — ciało o największym oporze kształtu. \
                      Oderwanie na ostrych krawędziach nie zależy od Re, więc Cd ≈ 2 (2D) jest wiarygodne nawet w LES."
            .into(),
        flow: FlowConfig { velocity: 5.0, domain: [1.2, 0.6], resolution: 280, ..FlowConfig::default() },
        ..Scene::default()
    };
    s.add(Shape::Rect { w: 0.008, h: 0.08, radius: 0.0 }, [0.3, 0.3], 0.0);
    s
}

fn karman() -> Scene {
    let mut s = Scene {
        name: "Ścieżka Kármána, Re 100".into(),
        description: "Walec o średnicy 1 cm w wodzie o temperaturze 20 °C płynącej z prędkością 1 cm/s (Re = 100). \
                      Przepływ jest laminarny i w pełni rozdzielczy — wynik można porównać z eksperymentem."
            .into(),
        flow: water_flow(0.01, [0.4, 0.2], 320),
        reference: Some(Reference {
            cd: Some([1.36, 0.08]),
            cl_amp: Some([0.33, 0.05]),
            strouhal: Some([0.165, 0.010]),
            source: "Williamson (1996), Park i in. (1998), Braza i in. (1986)".into(),
        }),
        ..Scene::default()
    };
    s.add(Shape::Circle { r: 0.005 }, [0.08, 0.1], 0.0);
    s
}

fn cyl20() -> Scene {
    let mut s = Scene {
        name: "Walec, Re 20".into(),
        description: "Przy Re = 20 za walcem tworzą się dwa symetryczne, stacjonarne wiry. Przepływ ustala się bez \
                      oscylacji; długość strefy recyrkulacji ≈ 0,9 D."
            .into(),
        flow: water_flow(0.002, [0.5, 0.25], 320),
        reference: Some(Reference {
            cd: Some([2.05, 0.15]),
            cl_amp: None,
            strouhal: None,
            source: "Tritton (1959), Dennis i Chang (1970)".into(),
        }),
        ..Scene::default()
    };
    s.add(Shape::Circle { r: 0.005 }, [0.12, 0.125], 0.0);
    s
}

fn cyl200() -> Scene {
    let mut s = Scene {
        name: "Walec, Re 200".into(),
        description: "Re = 200: ścieżka wirowa jest silniejsza i częstsza. W rzeczywistości około Re ≈ 190 pojawia się \
                      trójwymiarowość śladu, której model 2D nie odtwarza."
            .into(),
        flow: water_flow(0.02, [0.4, 0.2], 320),
        reference: Some(Reference {
            cd: Some([1.34, 0.08]),
            cl_amp: Some([0.69, 0.08]),
            strouhal: Some([0.197, 0.012]),
            source: "Braza i in. (1986), Henderson (1995)".into(),
        }),
        ..Scene::default()
    };
    s.add(Shape::Circle { r: 0.005 }, [0.08, 0.1], 0.0);
    s
}

fn square() -> Scene {
    let mut s = Scene {
        name: "Pręt kwadratowy, Re 100".into(),
        description: "Ostre krawędzie wymuszają oderwanie w stałym miejscu. Porównaj częstotliwość odrywania wirów z walcem.".into(),
        flow: water_flow(0.01, [0.4, 0.2], 320),
        reference: Some(Reference {
            cd: Some([1.48, 0.10]),
            cl_amp: Some([0.18, 0.05]),
            strouhal: Some([0.146, 0.010]),
            source: "Sohankar i in. (1998)".into(),
        }),
        ..Scene::default()
    };
    s.add(Shape::Rect { w: 0.01, h: 0.01, radius: 0.0 }, [0.08, 0.1], 0.0);
    s
}

fn city() -> Scene {
    let mut s = Scene {
        name: "Zabudowa miejska".into(),
        description: "Wiatr 5 m/s (na wysokości 120 m) nad zabudową. Wlot ma profil potęgowy warstwy przyziemnej \
                      (α = 0,25, teren miejski), podłoże jest szorstką ścianą. Za budynkami tworzą się strefy \
                      recyrkulacji — ważne przy projektowaniu wentylacji i komfortu pieszych."
            .into(),
        flow: FlowConfig {
            velocity: 5.0,
            domain: [360.0, 120.0],
            resolution: 240,
            bottom: Boundary::NoSlipWall,
            profile: InletProfile::PowerLaw { alpha: 0.25 },
            turbulence: 0.04,
            ..FlowConfig::default()
        },
        ..Scene::default()
    };
    for (x, w, h) in [(70.0, 20.0, 18.0), (110.0, 16.0, 42.0), (145.0, 30.0, 12.0), (200.0, 18.0, 30.0), (240.0, 24.0, 22.0)] {
        let id = s.add(Shape::Rect { w, h, radius: 0.0 }, [x + w / 2.0, h / 2.0], 0.0);
        if let Some(b) = s.body_mut(id) {
            b.name = format!("Budynek {h:.0} m");
        }
    }
    s
}

fn venturi() -> Scene {
    let mut s = Scene {
        name: "Zwężka Venturiego".into(),
        description: "Kanał wodny 4 cm zwężony do 2 cm. Z równania ciągłości prędkość w gardzieli rośnie 2×, \
                      a z równania Bernoulliego ciśnienie spada o ok. 3 q∞ (Cp ≈ −3). Sprawdź sondą."
            .into(),
        flow: FlowConfig {
            velocity: 0.01,
            domain: [0.2, 0.04],
            resolution: 120,
            top: Boundary::NoSlipWall,
            bottom: Boundary::NoSlipWall,
            profile: InletProfile::Channel,
            ..water_flow(0.01, [0.2, 0.04], 120)
        },
        ..Scene::default()
    };
    let (x0, x1, h) = (0.05, 0.14, 0.01);
    let n = 40;
    for top in [false, true] {
        let mut pts: Vec<V2> = Vec::new();
        for k in 0..=n {
            let t = k as f64 / n as f64;
            let x = x0 + (x1 - x0) * t;
            let y = h * 0.5 * (1.0 - (std::f64::consts::TAU * t).cos());
            pts.push([x, if top { 0.04 - y } else { y }]);
        }
        let base = if top { 0.045 } else { -0.005 };
        pts.push([x1, base]);
        pts.push([x0, base]);
        let cx = 0.5 * (x0 + x1);
        let cy = if top { 0.04 } else { 0.0 };
        let local = pts.iter().map(|p| [p[0] - cx, p[1] - cy]).collect();
        let id = s.add(Shape::Polygon { pts: local }, [cx, cy], 0.0);
        if let Some(b) = s.body_mut(id) {
            b.name = if top { "Ścianka górna" } else { "Ścianka dolna" }.into();
            b.locked = true;
        }
    }
    s
}

fn step() -> Scene {
    let mut s = Scene {
        name: "Uskok".into(),
        description: "Przepływ za uskokiem (backward-facing step), Re_h ≈ 300. Za krawędzią powstaje wydłużona strefa \
                      recyrkulacji; punkt ponownego przylgnięcia leży kilka wysokości uskoku dalej."
            .into(),
        flow: FlowConfig { top: Boundary::NoSlipWall, bottom: Boundary::NoSlipWall, ..water_flow(0.02, [0.36, 0.045], 120) },
        ..Scene::default()
    };
    let id = s.add(Shape::Rect { w: 0.06, h: 0.015, radius: 0.0 }, [0.03, 0.0075], 0.0);
    if let Some(b) = s.body_mut(id) {
        b.name = "Uskok".into();
        b.locked = true;
    }
    s
}

fn tandem() -> Scene {
    let mut s = Scene {
        name: "Tandem walców".into(),
        description: "Dwa walce jeden za drugim (odstęp 4 D) w wodzie, Re = 150. Drugi walec znajduje się w śladzie \
                      pierwszego — ma mniejszy opór średni, ale silnie zmienną siłę nośną."
            .into(),
        flow: water_flow(0.015, [0.4, 0.2], 320),
        ..Scene::default()
    };
    s.add(Shape::Circle { r: 0.005 }, [0.07, 0.1], 0.0);
    s.add(Shape::Circle { r: 0.005 }, [0.11, 0.1], 0.0);
    s
}

fn empty() -> Scene {
    Scene { name: "Pusty tunel".into(), description: "Narysuj własne przeszkody narzędziami z lewej strony.".into(), ..Scene::default() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_valid() {
        for p in presets() {
            let s = (p.build)();
            let (nx, ny) = s.flow.grid();
            assert!(nx >= 64 && ny >= 64, "{}", p.id);
            for b in &s.bodies {
                let pr = b.prepare();
                assert!(pr.bbox[0] >= -0.1 * s.flow.domain[0] && pr.bbox[2] <= s.flow.domain[0] * 1.1, "{} {}", p.id, b.name);
            }
            let json = s.to_json();
            assert_eq!(Scene::from_json(&json).unwrap(), s);
        }
    }

    #[test]
    fn karman_reynolds_is_100() {
        let s = karman();
        let u = s.flow.units();
        let re = u.reynolds(s.bodies[0].ref_length());
        assert!((re - 100.0).abs() < 1.0, "{re}");
    }
}
