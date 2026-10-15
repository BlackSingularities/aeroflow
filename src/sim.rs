//! Wątek symulacji: liczy kroki solvera w tle i publikuje najświeższą klatkę dla interfejsu.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use rayon::prelude::*;

use crate::physics::lattice::{Lattice, Layout};
use crate::physics::units::Units;
use crate::scene::Scene;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Speed {
    /// Tyle kroków, ile zmieści się w budżecie czasu klatki.
    Auto,
    Fixed(usize),
}

pub enum Cmd {
    Scene(Box<Scene>),
    Pause(bool),
    StepOnce,
    Reset,
    Speed(Speed),
    Averaging(bool),
    ResetAveraging,
    Dye {
        rake: bool,
        enabled: bool,
    },
    ClearDye,
    /// Czy liczyć na karcie graficznej (gdy jest dostępna).
    Gpu(bool),
    Quit,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BodyForce {
    /// Siła i moment w jednostkach siatki: [Fx, Fy, Mz].
    pub lattice: [f64; 3],
}

#[derive(Clone, Copy, Debug)]
pub struct SurfaceSample {
    pub body: u16,
    pub local: [f32; 2],
    pub cp: f32,
}

pub struct MeanFields {
    pub ux: Vec<f32>,
    pub uy: Vec<f32>,
    pub p: Vec<f32>,
    /// Intensywność turbulencji √(½(σu² + σv²)) / U.
    pub ti: Vec<f32>,
    pub samples: u32,
    pub span_s: f64,
}

/// Migawka stanu dla interfejsu. Pola w jednostkach siatki, rząd 0 = dół obszaru.
pub struct Frame {
    pub w: usize,
    pub h: usize,
    pub units: Units,
    pub ux: Vec<f32>,
    pub uy: Vec<f32>,
    /// Ciśnienie względem strumienia swobodnego, jednostki siatki: (ρ − ρ∞)/3.
    pub p: Vec<f32>,
    /// Lepkość turbulentna LES / lepkość molekularna.
    pub nut: Vec<f32>,
    pub dye: Option<Vec<f32>>,
    pub labels: Vec<u16>,
    pub mean: Option<MeanFields>,
    pub forces: Vec<BodyForce>,
    pub surface: Vec<SurfaceSample>,
    pub steps: u64,
    /// Czas fizyczny od startu przepływu [s].
    pub time: f64,
    pub mlups: f64,
    pub steps_per_frame: usize,
    /// Ile sekund fizycznych liczy się w sekundę rzeczywistą.
    pub realtime_ratio: f64,
    pub scene_rev: u64,
    pub paused: bool,
    pub error: Option<String>,
    /// Nazwa karty graficznej, gdy obliczenia idą na GPU; `None` — CPU.
    pub gpu: Option<String>,
}

impl Frame {
    pub fn idx(&self, x: usize, y: usize) -> usize {
        y * self.w + x
    }
}

pub struct SimHandle {
    tx: Sender<Cmd>,
    latest: Arc<Mutex<Option<Arc<Frame>>>>,
    thread: Option<JoinHandle<()>>,
}

impl SimHandle {
    pub fn spawn(scene: Scene, rev: u64, use_gpu: bool, ctx: eframe::egui::Context) -> Self {
        let (tx, rx) = mpsc::channel();
        let latest = Arc::new(Mutex::new(None));
        let out = latest.clone();
        let thread = std::thread::Builder::new()
            .name("symulacja".into())
            .spawn(move || Worker::new(scene, rev, use_gpu, rx, out, ctx).run())
            .expect("nie można uruchomić wątku symulacji");
        Self { tx, latest, thread: Some(thread) }
    }

    pub fn send(&self, c: Cmd) {
        let _ = self.tx.send(c);
    }

    pub fn latest(&self) -> Option<Arc<Frame>> {
        self.latest.lock().ok().and_then(|g| g.clone())
    }
}

impl Drop for SimHandle {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Quit);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct Averager {
    su: Vec<f64>,
    sv: Vec<f64>,
    sp: Vec<f64>,
    suu: Vec<f64>,
    svv: Vec<f64>,
    n: u32,
    t0: f64,
}

impl Averager {
    fn new(n: usize, t0: f64) -> Self {
        Self { su: vec![0.0; n], sv: vec![0.0; n], sp: vec![0.0; n], suu: vec![0.0; n], svv: vec![0.0; n], n: 0, t0 }
    }
}

struct Worker {
    scene: Scene,
    rev: u64,
    lat: Lattice,
    units: Units,
    rx: Receiver<Cmd>,
    out: Arc<Mutex<Option<Arc<Frame>>>>,
    ctx: eframe::egui::Context,
    paused: bool,
    speed: Speed,
    time: f64,
    dye: Vec<f32>,
    dye_on: bool,
    rake: bool,
    avg_on: bool,
    avg: Option<Averager>,
    error: Option<String>,
    mlups: f64,
    spf: usize,
    ratio: f64,
    use_gpu: bool,
}

fn layout_of(scene: &Scene) -> Layout {
    let (nx, ny) = scene.flow.grid();
    Layout { nx, ny, top: scene.flow.top, bottom: scene.flow.bottom }
}

impl Worker {
    fn new(
        scene: Scene,
        rev: u64,
        use_gpu: bool,
        rx: Receiver<Cmd>,
        out: Arc<Mutex<Option<Arc<Frame>>>>,
        ctx: eframe::egui::Context,
    ) -> Self {
        let lat = Lattice::new(layout_of(&scene));
        let units = scene.flow.units();
        let n = lat.win.w * lat.win.h;
        let mut w = Self {
            scene: Scene::default(),
            rev,
            lat,
            units,
            rx,
            out,
            ctx,
            paused: false,
            speed: Speed::Auto,
            time: 0.0,
            dye: vec![0.0; n],
            dye_on: false,
            rake: true,
            avg_on: false,
            avg: None,
            error: None,
            mlups: 0.0,
            spf: 1,
            ratio: 0.0,
            use_gpu,
        };
        w.apply_scene(scene, true);
        w
    }

    fn apply_scene(&mut self, scene: Scene, force: bool) {
        let layout = layout_of(&scene);
        let new_grid = force || layout != self.lat.layout;
        let f = &scene.flow;
        let units = f.units();
        if new_grid {
            self.lat = Lattice::new(layout);
            if self.use_gpu {
                self.lat.enable_gpu();
            }
            self.dye = vec![0.0; self.lat.win.w * self.lat.win.h];
            self.avg = None;
        }
        self.lat.set_params(units.u_lb as f32, units.nu_lb as f32, f.smagorinsky as f32, f.turbulence as f32, f.profile);
        if new_grid || scene.bodies != self.scene.bodies || f.domain != self.scene.flow.domain {
            self.lat.set_bodies(&scene.bodies, units.dx);
            if !new_grid {
                // geometria się zmieniła — średnie przestają obowiązywać
                self.avg = None;
            }
        }
        if new_grid {
            self.lat.reset_flow();
            self.time = 0.0;
        }
        if units.dt != self.units.dt && !new_grid {
            // zmiana prędkości/płynu: czas fizyczny liczymy dalej z nowym krokiem
            self.avg = None;
        }
        self.units = units;
        self.scene = scene;
        self.error = None;
    }

    fn run(mut self) {
        let frame_budget = Duration::from_micros(15_000);
        let mut last_pub = Instant::now();
        let mut steps_hint = 4usize;
        loop {
            // polecenia (blokująco, gdy wstrzymane)
            let mut step_once = false;
            loop {
                let msg = if self.paused && !step_once {
                    match self.rx.recv_timeout(Duration::from_millis(100)) {
                        Ok(m) => Some(m),
                        Err(RecvTimeoutError::Timeout) => None,
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                } else {
                    match self.rx.try_recv() {
                        Ok(m) => Some(m),
                        Err(mpsc::TryRecvError::Empty) => None,
                        Err(mpsc::TryRecvError::Disconnected) => return,
                    }
                };
                let Some(msg) = msg else { break };
                match msg {
                    Cmd::Quit => return,
                    Cmd::Scene(s) => {
                        self.apply_scene(*s, false);
                        self.rev += 1;
                    }
                    Cmd::Pause(p) => self.paused = p,
                    Cmd::StepOnce => step_once = true,
                    Cmd::Reset => {
                        self.lat.reset_flow();
                        self.time = 0.0;
                        self.dye.iter_mut().for_each(|d| *d = 0.0);
                        self.avg = None;
                        self.error = None;
                    }
                    Cmd::Speed(s) => self.speed = s,
                    Cmd::Averaging(on) => {
                        self.avg_on = on;
                        if !on {
                            self.avg = None;
                        }
                    }
                    Cmd::ResetAveraging => self.avg = None,
                    Cmd::Dye { rake, enabled } => {
                        self.rake = rake;
                        self.dye_on = enabled;
                    }
                    Cmd::ClearDye => self.dye.iter_mut().for_each(|d| *d = 0.0),
                    Cmd::Gpu(on) => {
                        self.use_gpu = on;
                        if on {
                            // stan z CPU przechodzi na kartę bez przerywania przepływu
                            self.lat.enable_gpu();
                        } else {
                            self.lat.disable_gpu();
                            self.time = 0.0;
                            self.avg = None;
                        }
                        self.mlups = 0.0;
                    }
                }
                if self.paused && !step_once {
                    // po zmianie w pauzie od razu pokaż nowy stan
                    self.publish();
                }
            }

            let t0 = Instant::now();
            let mut done = 0usize;
            if !self.paused || step_once {
                let target = if step_once {
                    1
                } else {
                    match self.speed {
                        Speed::Fixed(n) => n.max(1),
                        Speed::Auto => usize::MAX,
                    }
                };
                // porcje kroków aż do wyczerpania budżetu klatki
                let max_chunk = if self.lat.gpu_name().is_some() { 512 } else { 64 };
                let mut failed = false;
                while done < target {
                    let n = steps_hint.min(target - done).max(1);
                    let ts = Instant::now();
                    if self.lat.advance(n).is_err() {
                        failed = true;
                        break;
                    }
                    done += n;
                    self.time += n as f64 * self.units.dt;
                    let per = ts.elapsed().as_secs_f64() / n as f64;
                    if self.speed == Speed::Auto && !step_once {
                        let left = frame_budget.saturating_sub(t0.elapsed()).as_secs_f64();
                        if left < per {
                            break;
                        }
                        steps_hint = ((left / per) as usize).clamp(1, max_chunk);
                    }
                }
                if failed || (done > 0 && self.lat.sync().is_err()) {
                    self.unstable();
                    done = 0;
                }
                if let Some(e) = self.lat.gpu_error.take() {
                    self.error = Some(format!("Karta graficzna przestała odpowiadać ({e}). Obliczenia przeniesiono na procesor."));
                    self.use_gpu = false;
                }
                if done > 0 {
                    self.advect_dye(done);
                    self.accumulate();
                    let dt = t0.elapsed().as_secs_f64().max(1e-6);
                    let cells = (self.lat.cell_count() * done) as f64;
                    self.mlups = 0.85 * self.mlups + 0.15 * cells / dt / 1e6;
                    self.spf = done;
                }
            }
            let since = last_pub.elapsed().as_secs_f64();
            if done > 0 {
                let r = done as f64 * self.units.dt / since.max(1e-3);
                self.ratio = 0.9 * self.ratio + 0.1 * r;
            }
            last_pub = Instant::now();
            self.publish();
            // niewielka przerwa, gdy liczenie zajęło mniej niż klatka (tryb stały)
            if let Some(rest) = Duration::from_micros(16_000).checked_sub(t0.elapsed()) {
                if self.speed != Speed::Auto || self.paused {
                    std::thread::sleep(rest);
                }
            }
        }
    }

    fn unstable(&mut self) {
        self.lat.reset_flow();
        self.time = 0.0;
        self.avg = None;
        self.paused = true;
        self.error = Some(
            "Obliczenia straciły stabilność i zostały zatrzymane. Zwiększ rozdzielczość, zmniejsz prędkość siatki \
             lub zwiększ stałą Smagorinsky'ego."
                .into(),
        );
    }

    fn advect_dye(&mut self, steps: usize) {
        if !self.dye_on {
            return;
        }
        let (w, h) = (self.lat.win.w, self.lat.win.h);
        let ux = self.lat.visible(&self.lat.ux);
        let uy = self.lat.visible(&self.lat.uy);
        let labels = self.lat.visible_labels();
        let src = &self.dye;
        let dt = steps as f32;
        let band = (h / 24).max(6);
        let rake = self.rake;
        let mut dst = vec![0f32; w * h];
        dst.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
            for (x, d) in row.iter_mut().enumerate() {
                let c = y * w + x;
                if labels[c] != 0 {
                    *d = 0.0;
                    continue;
                }
                if x < 2 {
                    *d = if rake && y % band < (band / 5).max(1) { 1.0 } else { 0.0 };
                    continue;
                }
                let px = (x as f32 - ux[c] * dt).clamp(0.0, w as f32 - 1.001);
                let py = (y as f32 - uy[c] * dt).clamp(0.0, h as f32 - 1.001);
                let (x0, y0) = (px as usize, py as usize);
                let (fx, fy) = (px - x0 as f32, py - y0 as f32);
                let a = src[y0 * w + x0] * (1.0 - fx) + src[y0 * w + x0 + 1] * fx;
                let b = src[(y0 + 1) * w + x0] * (1.0 - fx) + src[(y0 + 1) * w + x0 + 1] * fx;
                *d = (a * (1.0 - fy) + b * fy) * 0.9995;
            }
        });
        // źródła punktowe
        let dx = self.units.dx;
        for e in &self.scene.emitters {
            let cx = e[0] / dx;
            let cy = e[1] / dx;
            let r = 1.6f64;
            for y in (cy - r).floor().max(0.0) as usize..((cy + r).ceil() as usize).min(h) {
                for x in (cx - r).floor().max(0.0) as usize..((cx + r).ceil() as usize).min(w) {
                    let d2 = (x as f64 + 0.5 - cx).powi(2) + (y as f64 + 0.5 - cy).powi(2);
                    if d2 <= r * r && labels[y * w + x] == 0 {
                        dst[y * w + x] = 1.0;
                    }
                }
            }
        }
        self.dye = dst;
    }

    fn accumulate(&mut self) {
        if !self.avg_on {
            return;
        }
        let win = self.lat.win;
        let n = win.w * win.h;
        let avg = self.avg.get_or_insert_with(|| Averager::new(n, self.time));
        let rref = self.lat.reference_density();
        let nx = self.lat.nx;
        for y in 0..win.h {
            for x in 0..win.w {
                let c = (y + win.y0) * nx + x + win.x0;
                let i = y * win.w + x;
                let (u, v) = (self.lat.ux[c] as f64, self.lat.uy[c] as f64);
                avg.su[i] += u;
                avg.sv[i] += v;
                avg.suu[i] += u * u;
                avg.svv[i] += v * v;
                avg.sp[i] += (self.lat.rho[c] - rref) as f64 / 3.0;
            }
        }
        avg.n += 1;
    }

    fn publish(&mut self) {
        let lat = &self.lat;
        let rref = lat.reference_density();
        let nu = self.units.nu_lb.max(1e-12) as f32;
        let mut p = lat.visible(&lat.rho);
        p.iter_mut().for_each(|v| *v = (*v - rref) / 3.0);
        let mut nut = lat.visible(&lat.nut);
        nut.iter_mut().for_each(|v| *v /= nu);
        let q = 0.5 * lat.u_lb * lat.u_lb;
        let surface = lat
            .links
            .iter()
            .map(|l| SurfaceSample { body: l.body, local: l.local, cp: (lat.rho[l.cell as usize] - rref) / 3.0 / q })
            .collect();
        let mean = self.avg.as_ref().filter(|a| a.n > 0).map(|a| {
            let k = 1.0 / a.n as f64;
            let n = a.su.len();
            let mut m = MeanFields {
                ux: Vec::with_capacity(n),
                uy: Vec::with_capacity(n),
                p: Vec::with_capacity(n),
                ti: Vec::with_capacity(n),
                samples: a.n,
                span_s: self.time - a.t0,
            };
            let u0 = lat.u_lb as f64;
            for i in 0..n {
                let (mu, mv) = (a.su[i] * k, a.sv[i] * k);
                let vu = (a.suu[i] * k - mu * mu).max(0.0);
                let vv = (a.svv[i] * k - mv * mv).max(0.0);
                m.ux.push(mu as f32);
                m.uy.push(mv as f32);
                m.p.push((a.sp[i] * k) as f32);
                m.ti.push(((0.5 * (vu + vv)).sqrt() / u0) as f32);
            }
            m
        });
        let frame = Frame {
            w: lat.win.w,
            h: lat.win.h,
            units: self.units,
            ux: lat.visible(&lat.ux),
            uy: lat.visible(&lat.uy),
            p,
            nut,
            dye: self.dye_on.then(|| self.dye.clone()),
            labels: lat.visible_labels(),
            mean,
            forces: lat.forces.iter().map(|f| BodyForce { lattice: *f }).collect(),
            surface,
            steps: lat.steps,
            time: self.time,
            mlups: self.mlups,
            steps_per_frame: self.spf,
            realtime_ratio: self.ratio,
            scene_rev: self.rev,
            paused: self.paused,
            error: self.error.clone(),
            gpu: lat.gpu_name().map(str::to_string),
        };
        if let Ok(mut g) = self.out.lock() {
            *g = Some(Arc::new(frame));
        }
        self.ctx.request_repaint();
    }
}
