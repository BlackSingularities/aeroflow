//! Stan aplikacji, cykl klatki, edycja sceny, historia zmian i obsługa plików.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use eframe::egui::{self, Color32, TextureHandle, TextureOptions};

use crate::analysis::{History, ProbeSeries, Sweep};
use crate::physics::geometry::{Body, Shape, V2};
use crate::prefs::{Prefs, SpeedPref};
use crate::render::field::{build_image, AutoRange, FieldKind};
use crate::render::flowviz::{streamlines, Particles};
use crate::scene::{presets, Scene, FILE_EXT};
use crate::sim::{Cmd, Frame, SimHandle, Speed};
use crate::ui::theme;
use crate::update::{self, Updater};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Select,
    Airfoil,
    Circle,
    Rect,
    Ellipse,
    Polygon,
    Brush,
    Eraser,
    Emitter,
    Probe,
}

impl Tool {
    pub const ALL: [Tool; 10] = [
        Tool::Select,
        Tool::Airfoil,
        Tool::Circle,
        Tool::Rect,
        Tool::Ellipse,
        Tool::Polygon,
        Tool::Brush,
        Tool::Eraser,
        Tool::Emitter,
        Tool::Probe,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Tool::Select => "Zaznacz i przesuwaj",
            Tool::Airfoil => "Profil NACA",
            Tool::Circle => "Walec",
            Tool::Rect => "Prostokąt",
            Tool::Ellipse => "Elipsa",
            Tool::Polygon => "Dowolny kształt",
            Tool::Brush => "Pędzel ściany",
            Tool::Eraser => "Gumka",
            Tool::Emitter => "Źródło dymu",
            Tool::Probe => "Sonda pomiarowa",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Tool::Select => "1",
            Tool::Airfoil => "2",
            Tool::Circle => "3",
            Tool::Rect => "4",
            Tool::Ellipse => "5",
            Tool::Polygon => "6",
            Tool::Brush => "7",
            Tool::Eraser => "8",
            Tool::Emitter => "9",
            Tool::Probe => "0",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Tool::Select => "Kliknij obiekt, by go zaznaczyć; przeciągnij, by przesunąć. Uchwyt obraca, Q/E obraca o 1° (Shift: 5°), strzałki przesuwają. Przeciąganie tła przesuwa widok.",
            Tool::Airfoil => "Przeciągnij od noska do krawędzi spływu: długość = cięciwa, kierunek = kąt natarcia.",
            Tool::Circle => "Przeciągnij od środka, by ustawić promień.",
            Tool::Rect => "Przeciągnij po przekątnej.",
            Tool::Ellipse => "Przeciągnij prostokąt opisany na elipsie.",
            Tool::Polygon => "Narysuj obrys — po puszczeniu zostanie wypełniony.",
            Tool::Brush => "Maluj ściany. Rozmiar pędzla: [ i ].",
            Tool::Eraser => "Wymazuj fragmenty ścian malowanych pędzlem.",
            Tool::Emitter => "Kliknij, by dodać źródło dymu (pole „Dym”). Kliknięcie istniejącego źródła je usuwa.",
            Tool::Probe => "Kliknij, by postawić sondę mierzącą prędkość i ciśnienie w czasie. Kliknięcie sondy ją usuwa.",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SideTab {
    Flow,
    Objects,
    View,
    Solver,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChartTab {
    Forces,
    Surface,
    Polar,
    Probes,
}

/// Przekształcenie widoku: środek [m] i skala [px/m].
#[derive(Clone, Copy, Debug)]
pub struct ViewXf {
    pub center: V2,
    pub ppm: f64,
    pub fit: bool,
}

pub enum Drag {
    Move { id: u64, last: V2 },
    Rotate { id: u64 },
    Pan { last: egui::Pos2 },
    Create { start: V2, cur: V2, pts: Vec<V2> },
    Paint { id: u64 },
    Erase,
}

#[derive(Default)]
pub struct Windows {
    pub about: bool,
    pub shortcuts: bool,
    pub physics: bool,
    pub update: bool,
    pub whats_new: Option<String>,
    pub changelog: bool,
}

/// Parametry uruchomienia z wiersza poleceń.
#[derive(Clone, Default)]
pub struct Launch {
    pub open: Option<PathBuf>,
    pub preset: Option<String>,
    pub field: Option<String>,
}

pub struct SweepCfg {
    pub from: f64,
    pub to: f64,
    pub step: f64,
    pub settle: f64,
    pub measure: f64,
}

pub struct App {
    pub scene: Scene,
    pub last_sent: Scene,
    pub rev: u64,
    pub sim: SimHandle,
    pub frame: Option<Arc<Frame>>,
    pub prefs: Prefs,
    pub undo: Vec<Scene>,
    pub redo: Vec<Scene>,
    pending_undo: Option<Scene>,
    pub suppress_undo: bool,
    pub selected: Option<u64>,
    pub tool: Tool,
    pub paused: bool,
    pub averaging: bool,
    pub view: ViewXf,
    pub drag: Option<Drag>,
    pub history: History,
    pub probe_series: Vec<ProbeSeries>,
    pub sweep: Option<Sweep>,
    sweep_apply: bool,
    pub sweep_cfg: SweepCfg,
    pub field_tex: Option<TextureHandle>,
    pub field_values: Option<Vec<f32>>,
    pub field_range: (f32, f32),
    pub ranges: HashMap<FieldKind, AutoRange>,
    view_dirty: bool,
    pub particles: Particles,
    last_steps: u64,
    pub stream_cache: Vec<Vec<[f32; 2]>>,
    stream_at: Option<Instant>,
    pub updater: Updater,
    pub win: Windows,
    pub toast: Option<(String, Instant)>,
    pub file_path: Option<PathBuf>,
    pub saved_scene: Scene,
    pub side_tab: SideTab,
    pub chart_tab: ChartTab,
    pub chart_body: Option<u64>,
    pub selected_probe: usize,
    pub screenshot: Option<(PathBuf, egui::Rect)>,
    pub viewport_rect: egui::Rect,
    pub fps: f32,
    last_paint: Instant,
    pub hover: Option<V2>,
    pub re_target: f64,
    pub hide_ui: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, launch: Launch) -> Self {
        let open = launch.open.clone();
        theme::apply(&cc.egui_ctx);
        let mut prefs: Prefs = cc.storage.and_then(|s| eframe::get_value(s, "prefs")).unwrap_or_default();
        cc.egui_ctx.set_zoom_factor(prefs.ui_scale.clamp(0.6, 2.5));

        let mut toast = None;
        let mut file_path = None;
        let scene = match open.as_ref().map(|p| (p, std::fs::read_to_string(p))) {
            Some((p, Ok(text))) => match Scene::from_json(&text) {
                Ok(s) => {
                    file_path = Some(p.clone());
                    s
                }
                Err(e) => {
                    toast = Some((e, Instant::now()));
                    (presets()[0].build)()
                }
            },
            Some((p, Err(e))) => {
                toast = Some((format!("Nie można otworzyć {}: {e}", p.display()), Instant::now()));
                (presets()[0].build)()
            }
            None => match launch.preset.as_deref().and_then(|id| presets().into_iter().find(|p| p.id == id)) {
                Some(p) => (p.build)(),
                None => prefs.last_scene.as_deref().and_then(|s| Scene::from_json(s).ok()).unwrap_or_else(|| (presets()[0].build)()),
            },
        };
        if let Some(f) = launch.field.as_deref().and_then(|n| FieldKind::ALL.into_iter().find(|k| format!("{k:?}").eq_ignore_ascii_case(n)))
        {
            prefs.field = f;
        }

        let sim = SimHandle::spawn(scene.clone(), 0, cc.egui_ctx.clone());
        sim.send(Cmd::Speed(match prefs.speed {
            SpeedPref::Auto => Speed::Auto,
            SpeedPref::Fixed(n) => Speed::Fixed(n),
        }));
        sim.send(Cmd::Dye { rake: prefs.smoke_rake, enabled: prefs.field == FieldKind::Smoke });

        let updater = Updater::default();
        if prefs.check_updates {
            updater.check(false, &cc.egui_ctx);
        }
        let mut win = Windows::default();
        let current = update::VERSION.to_string();
        if !prefs.last_version.is_empty() && prefs.last_version != current {
            let notes = prefs.update_notes.take().filter(|(v, n)| *v == current && !n.trim().is_empty()).map(|(_, n)| n);
            win.whats_new = Some(
                notes
                    .or_else(|| update::changelog_section(&current))
                    .unwrap_or_else(|| "Program został zaktualizowany do najnowszej wersji.".into()),
            );
        }
        prefs.update_notes = None;
        prefs.last_version = current;
        let n_probes = scene.probes.len();
        let saved_scene = if file_path.is_some() { scene.clone() } else { Scene::default() };

        Self {
            last_sent: scene.clone(),
            saved_scene,
            scene,
            rev: 0,
            sim,
            frame: None,
            prefs,
            undo: Vec::new(),
            redo: Vec::new(),
            pending_undo: None,
            suppress_undo: false,
            selected: None,
            tool: Tool::Select,
            paused: false,
            averaging: false,
            view: ViewXf { center: [0.0, 0.0], ppm: 1.0, fit: true },
            drag: None,
            history: History::default(),
            probe_series: (0..n_probes).map(|_| ProbeSeries::new()).collect(),
            sweep: None,
            sweep_apply: false,
            sweep_cfg: SweepCfg { from: -4.0, to: 14.0, step: 2.0, settle: 6.0, measure: 4.0 },
            field_tex: None,
            field_values: None,
            field_range: (0.0, 1.0),
            ranges: HashMap::new(),
            view_dirty: true,
            particles: Particles::default(),
            last_steps: 0,
            stream_cache: Vec::new(),
            stream_at: None,
            updater,
            win,
            toast,
            file_path,
            side_tab: SideTab::Flow,
            chart_tab: ChartTab::Forces,
            chart_body: None,
            selected_probe: 0,
            screenshot: None,
            viewport_rect: egui::Rect::NOTHING,
            fps: 60.0,
            last_paint: Instant::now(),
            hover: None,
            re_target: 1000.0,
            hide_ui: false,
        }
    }

    // ------------------------------------------------------------------ pomocnicze

    pub fn notify(&mut self, text: impl Into<String>) {
        self.toast = Some((text.into(), Instant::now()));
    }

    pub fn mark_view_dirty(&mut self) {
        self.view_dirty = true;
    }

    pub fn body_index(&self, id: u64) -> Option<usize> {
        self.scene.bodies.iter().position(|b| b.id == id)
    }

    /// Ciało, którego dotyczą wykresy i HUD: zaznaczone, wybrane na wykresie lub pierwsze nie-ściana.
    pub fn main_body(&self) -> Option<&Body> {
        let pick = |id: Option<u64>| id.and_then(|id| self.scene.body(id));
        pick(self.selected)
            .or_else(|| pick(self.chart_body))
            .or_else(|| self.scene.bodies.iter().find(|b| !b.locked && !matches!(b.shape, Shape::Wall { .. })))
    }

    pub fn modified(&self) -> bool {
        self.scene != self.saved_scene
    }

    pub fn set_paused(&mut self, p: bool) {
        self.paused = p;
        self.sim.send(Cmd::Pause(p));
    }

    pub fn reset_flow(&mut self) {
        self.sim.send(Cmd::Reset);
        self.history.clear();
        self.probe_series.iter_mut().for_each(|p| p.clear());
        self.particles.clear();
    }

    pub fn set_speed(&mut self, s: SpeedPref) {
        self.prefs.speed = s;
        self.sim.send(Cmd::Speed(match s {
            SpeedPref::Auto => Speed::Auto,
            SpeedPref::Fixed(n) => Speed::Fixed(n),
        }));
    }

    pub fn set_field(&mut self, f: FieldKind) {
        if self.prefs.field != f {
            self.prefs.field = f;
            self.sim.send(Cmd::Dye { rake: self.prefs.smoke_rake, enabled: f == FieldKind::Smoke });
            if f.needs_mean() && !self.averaging {
                self.set_averaging(true);
            }
            self.view_dirty = true;
        }
    }

    pub fn set_averaging(&mut self, on: bool) {
        self.averaging = on;
        self.sim.send(Cmd::Averaging(on));
    }

    // ------------------------------------------------------------------ historia zmian

    pub fn undo(&mut self) {
        if let Some(s) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.scene, s));
            self.after_history_jump();
        }
    }

    pub fn redo(&mut self) {
        if let Some(s) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.scene, s));
            self.after_history_jump();
        }
    }

    fn after_history_jump(&mut self) {
        self.suppress_undo = true;
        if self.selected.is_some_and(|id| self.scene.body(id).is_none()) {
            self.selected = None;
        }
        self.probe_series.resize_with(self.scene.probes.len(), ProbeSeries::new);
    }

    /// Wywoływane na końcu klatki: zapisuje punkt cofania i wysyła scenę do solvera.
    fn finish_edits(&mut self, ctx: &egui::Context, before: Scene) {
        let changed = self.scene != before;
        let pointer_down = ctx.input(|i| i.pointer.any_down());
        if changed && !self.suppress_undo {
            if pointer_down {
                if self.pending_undo.is_none() {
                    self.pending_undo = Some(before);
                }
            } else {
                let snap = self.pending_undo.take().unwrap_or(before);
                self.push_undo(snap);
            }
        } else if !pointer_down {
            if let Some(s) = self.pending_undo.take() {
                if s != self.scene {
                    self.push_undo(s);
                }
            }
        }
        self.suppress_undo = false;
        if self.probe_series.len() != self.scene.probes.len() {
            self.probe_series.resize_with(self.scene.probes.len(), ProbeSeries::new);
        }
        if self.scene != self.last_sent {
            let geometry = self.scene.bodies != self.last_sent.bodies;
            let emitters = self.scene.emitters != self.last_sent.emitters;
            if self.scene.flow != self.last_sent.flow {
                self.view_dirty = true;
                for r in self.ranges.values_mut() {
                    r.reset();
                }
            }
            self.last_sent = self.scene.clone();
            self.sim.send(Cmd::Scene(Box::new(self.scene.clone())));
            self.rev += 1;
            if emitters && !self.scene.emitters.is_empty() && self.prefs.field != FieldKind::Smoke {
                self.notify("Źródła dymu są widoczne w polu „Dym” (klawisz V lub lista pól).");
            }
            if geometry {
                self.stream_at = None;
            }
            if self.sweep_apply {
                self.sweep_apply = false;
                if let Some(s) = self.sweep.as_mut() {
                    s.applied(self.rev);
                }
            }
        }
    }

    fn push_undo(&mut self, s: Scene) {
        self.undo.push(s);
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    // ------------------------------------------------------------------ edycja

    pub fn delete_selected(&mut self) {
        if let Some(id) = self.selected.take() {
            self.scene.bodies.retain(|b| b.id != id);
        }
    }

    pub fn duplicate_selected(&mut self) {
        let Some(b) = self.selected.and_then(|id| self.scene.body(id)).cloned() else { return };
        let id = self.scene.alloc_id();
        let mut nb = b;
        nb.id = id;
        nb.locked = false;
        let off = self.scene.flow.dx() * 12.0;
        nb.translate([off, -off]);
        self.scene.bodies.push(nb);
        self.selected = Some(id);
    }

    pub fn rotate_selected(&mut self, delta: f64) {
        if let Some(b) = self.selected.and_then(|id| self.scene.body_mut(id)) {
            if b.rotatable() {
                b.angle = wrap_angle(b.angle + delta);
            }
        }
    }

    pub fn load_scene(&mut self, scene: Scene, path: Option<PathBuf>) {
        let before = std::mem::replace(&mut self.scene, scene);
        self.push_undo(before);
        self.suppress_undo = true;
        self.selected = None;
        self.chart_body = None;
        self.sweep = None;
        self.view.fit = true;
        self.history.clear();
        self.probe_series = (0..self.scene.probes.len()).map(|_| ProbeSeries::new()).collect();
        self.saved_scene = if path.is_some() { self.scene.clone() } else { Scene::default() };
        self.file_path = path;
        for r in self.ranges.values_mut() {
            r.reset();
        }
        // nowa scena zawsze startuje od strumienia swobodnego
        self.sim.send(Cmd::Scene(Box::new(self.scene.clone())));
        self.rev += 1;
        self.last_sent = self.scene.clone();
        self.sim.send(Cmd::Reset);
        self.particles.clear();
        if self.paused {
            self.set_paused(false);
        }
    }

    pub fn load_preset(&mut self, id: &str) {
        if let Some(p) = presets().into_iter().find(|p| p.id == id) {
            self.load_scene((p.build)(), None);
            self.notify(format!("Scenariusz: {}", p.name));
        }
    }

    // ------------------------------------------------------------------ pliki

    pub fn open_dialog(&mut self) {
        let dlg = rfd::FileDialog::new().add_filter("Scena AeroFlow", &[FILE_EXT]).set_title("Otwórz scenę");
        if let Some(p) = dlg.pick_file() {
            self.open_path(p);
        }
    }

    pub fn open_path(&mut self, p: PathBuf) {
        match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|t| Scene::from_json(&t)) {
            Ok(s) => {
                self.prefs.add_recent(&p.display().to_string());
                self.load_scene(s, Some(p));
                self.notify("Wczytano scenę");
            }
            Err(e) => self.notify(format!("Błąd odczytu: {e}")),
        }
    }

    pub fn save(&mut self, save_as: bool) {
        let path = match (&self.file_path, save_as) {
            (Some(p), false) => Some(p.clone()),
            _ => rfd::FileDialog::new()
                .add_filter("Scena AeroFlow", &[FILE_EXT])
                .set_file_name(format!("{}.{FILE_EXT}", sanitize(&self.scene.name)))
                .set_title("Zapisz scenę")
                .save_file(),
        };
        let Some(mut p) = path else { return };
        if p.extension().is_none() {
            p.set_extension(FILE_EXT);
        }
        match std::fs::write(&p, self.scene.to_json()) {
            Ok(()) => {
                self.prefs.add_recent(&p.display().to_string());
                self.file_path = Some(p);
                self.saved_scene = self.scene.clone();
                self.notify("Zapisano scenę");
            }
            Err(e) => self.notify(format!("Nie można zapisać: {e}")),
        }
    }

    pub fn export_csv(&mut self, what: &str) {
        let (name, content) = match what {
            "forces" => {
                let Some(b) = self.main_body() else {
                    self.notify("Brak ciała do eksportu");
                    return;
                };
                let Some(s) = self.history.get(b.id) else {
                    self.notify("Brak danych — uruchom symulację");
                    return;
                };
                let mut c = String::from("t_s;Cd;Cl;Cm;Fx_N_per_m;Fy_N_per_m;M_Nm_per_m\n");
                for v in s {
                    c += &format!("{:.6};{:.6};{:.6};{:.6};{:.6};{:.6};{:.6}\n", v.t, v.cd, v.cl, v.cm, v.fx, v.fy, v.m);
                }
                (format!("{}-sily.csv", sanitize(&b.name)), c)
            }
            "polar" => {
                let Some(s) = &self.sweep else {
                    self.notify("Brak biegunowej");
                    return;
                };
                let mut c = String::from("alpha_deg;Cl;Cd;Cm;Cl_amplituda;L_D\n");
                for p in &s.results {
                    c += &format!("{:.3};{:.5};{:.5};{:.5};{:.5};{:.3}\n", p.alpha, p.cl, p.cd, p.cm, p.cl_amp, p.cl / p.cd);
                }
                ("biegunowa.csv".into(), c)
            }
            "probes" => {
                if self.probe_series.is_empty() {
                    self.notify("Brak sond");
                    return;
                }
                let mut c = String::from("sonda;x_m;y_m;t_s;ux_m_s;uy_m_s;u_m_s;p_Pa\n");
                for (k, (s, pos)) in self.probe_series.iter().zip(&self.scene.probes).enumerate() {
                    for i in 0..s.t.len() {
                        c += &format!(
                            "{};{:.6};{:.6};{:.6};{:.6};{:.6};{:.6};{:.4}\n",
                            k + 1,
                            pos[0],
                            pos[1],
                            s.t[i],
                            s.ux[i],
                            s.uy[i],
                            s.speed[i],
                            s.p[i]
                        );
                    }
                }
                ("sondy.csv".into(), c)
            }
            "field" => {
                let Some(f) = self.frame.clone() else { return };
                let u = f.units;
                let vel = u.dx / u.dt;
                let mut c = String::from("x_m;y_m;ux_m_s;uy_m_s;p_Pa;cialo\n");
                for y in 0..f.h {
                    for x in 0..f.w {
                        let i = f.idx(x, y);
                        c += &format!(
                            "{:.6};{:.6};{:.6};{:.6};{:.4};{}\n",
                            (x as f64 + 0.5) * u.dx,
                            (y as f64 + 0.5) * u.dx,
                            f.ux[i] as f64 * vel,
                            f.uy[i] as f64 * vel,
                            u.pressure(3.0 * f.p[i] as f64),
                            (f.labels[i] != 0) as u8
                        );
                    }
                }
                ("pole.csv".into(), c)
            }
            _ => return,
        };
        if let Some(p) = rfd::FileDialog::new().add_filter("CSV", &["csv"]).set_file_name(name).save_file() {
            match std::fs::write(&p, content) {
                Ok(()) => self.notify(format!("Zapisano {}", p.display())),
                Err(e) => self.notify(format!("Nie można zapisać: {e}")),
            }
        }
    }

    pub fn request_screenshot(&mut self, ctx: &egui::Context) {
        if let Some(p) =
            rfd::FileDialog::new().add_filter("PNG", &["png"]).set_file_name(format!("{}.png", sanitize(&self.scene.name))).save_file()
        {
            self.screenshot = Some((p, self.viewport_rect));
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
    }

    fn handle_screenshot(&mut self, ctx: &egui::Context) {
        let img = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        let (Some(img), Some((path, rect))) = (img, self.screenshot.take()) else { return };
        let ppp = ctx.pixels_per_point();
        let crop = img.region(&rect, Some(ppp));
        let [w, h] = crop.size;
        let mut buf = Vec::with_capacity(w * h * 4);
        for p in &crop.pixels {
            buf.extend_from_slice(&[p.r(), p.g(), p.b(), 255]);
        }
        let res = image::save_buffer(&path, &buf, w as u32, h as u32, image::ExtendedColorType::Rgba8);
        match res {
            Ok(()) => self.notify(format!("Zapisano zrzut {}", path.display())),
            Err(e) => self.notify(format!("Błąd zapisu PNG: {e}")),
        }
    }

    // ------------------------------------------------------------------ klatki z solvera

    fn ingest_frame(&mut self, ctx: &egui::Context) {
        let Some(f) = self.sim.latest() else { return };
        let is_new = self.frame.as_ref().is_none_or(|old| !Arc::ptr_eq(old, &f));
        if !is_new {
            return;
        }
        self.frame = Some(f.clone());
        self.view_dirty = true;
        if f.scene_rev == self.rev {
            self.history.push(&f, &self.scene.bodies);
            // sondy
            let u = f.units;
            let vel = u.dx / u.dt;
            if f.steps != self.last_steps {
                for (k, p) in self.scene.probes.iter().enumerate() {
                    let x = (p[0] / u.dx) as isize;
                    let y = (p[1] / u.dx) as isize;
                    if x >= 0 && y >= 0 && (x as usize) < f.w && (y as usize) < f.h {
                        let i = f.idx(x as usize, y as usize);
                        if let Some(s) = self.probe_series.get_mut(k) {
                            s.push(f.time, f.ux[i] as f64 * vel, f.uy[i] as f64 * vel, u.pressure(3.0 * f.p[i] as f64));
                        }
                    }
                }
            }
            // biegunowa
            if let Some(sw) = self.sweep.as_mut() {
                if let Some(c) = self.history.get(sw.body).and_then(|s| s.back()).copied() {
                    sw.record(c);
                }
                if let Some(body) = self.scene.body(sw.body).cloned() {
                    if let Some(angle) = sw.tick(&f, &body) {
                        if let Some(b) = self.scene.body_mut(sw.body) {
                            b.angle = angle;
                        }
                        self.suppress_undo = true;
                        self.sweep_apply = true;
                    }
                    if sw.done() {
                        self.notify("Biegunowa gotowa");
                    }
                } else {
                    self.sweep = None;
                }
            }
        }
        if let Some(e) = &f.error {
            if !self.paused {
                self.paused = true;
                let e = e.clone();
                self.notify(e);
            }
        }
        // cząstki
        let steps = f.steps.saturating_sub(self.last_steps).min(64) as f32;
        if f.steps < self.last_steps {
            self.particles.clear();
        }
        self.last_steps = f.steps;
        if self.prefs.particles && steps > 0.0 {
            self.particles.update(&f, self.prefs.particle_count, self.prefs.trail_len, steps);
        }
        if self.prefs.streamlines && self.stream_at.is_none_or(|t| t.elapsed().as_millis() > 120) {
            let spacing = self.prefs.streamline_spacing * (f.h as f32 / 280.0).max(0.5);
            self.stream_cache = streamlines(&f, spacing);
            self.stream_at = Some(Instant::now());
        }
        let _ = ctx;
    }

    fn update_field_texture(&mut self, ctx: &egui::Context) {
        if !self.view_dirty {
            return;
        }
        let Some(f) = self.frame.clone() else { return };
        self.view_dirty = false;
        let kind = self.prefs.field;
        let solid = theme::BODY_FILL;
        let values = kind.compute(&f);
        let (lo, hi) = match &values {
            Some(v) => {
                if let Some([lo, hi]) = self.prefs.manual_range(kind) {
                    (lo, hi)
                } else {
                    let r = self.ranges.entry(kind).or_default();
                    let nonneg = matches!(
                        kind,
                        FieldKind::Speed | FieldKind::MeanSpeed | FieldKind::Smoke | FieldKind::Turbulence | FieldKind::EddyViscosity
                    );
                    if kind == FieldKind::Smoke {
                        r.lo = 0.0;
                        r.hi = 1.0;
                    } else if kind == FieldKind::QCriterion {
                        // tylko dodatnie Q (rdzenie wirów)
                        let pos: Vec<f32> = v.iter().map(|q| q.max(0.0)).collect();
                        r.update(&pos, &f.labels, false, true);
                    } else {
                        r.update(v, &f.labels, kind.symmetric(), nonneg);
                        if matches!(kind, FieldKind::Speed | FieldKind::MeanSpeed) {
                            // strumień swobodny ≈ ¾ skali — ślady i przyspieszenia pozostają czytelne
                            r.hi = r.hi.max(1.35 * f.units.u_inf as f32);
                        }
                    }
                    (r.lo, r.hi)
                }
            }
            None => (0.0, 1.0),
        };
        let img = match &values {
            Some(v) => build_image(v, f.w, f.h, &f.labels, lo, hi, &self.prefs.colormap(kind).lut(), solid),
            None => {
                let bg = theme::FIELD_OFF;
                let mut img = egui::ColorImage::new([f.w, f.h], vec![bg; f.w * f.h]);
                for y in 0..f.h {
                    for x in 0..f.w {
                        if f.labels[(f.h - 1 - y) * f.w + x] != 0 {
                            img.pixels[y * f.w + x] = solid;
                        }
                    }
                }
                img
            }
        };
        self.field_range = (lo, hi);
        self.field_values = values;
        match &mut self.field_tex {
            Some(t) if t.size() == [f.w, f.h] => t.set(img, TextureOptions::LINEAR),
            _ => self.field_tex = Some(ctx.load_texture("pole", img, TextureOptions::LINEAR)),
        }
    }
}

pub fn wrap_angle(a: f64) -> f64 {
    let mut a = a % 360.0;
    if a > 180.0 {
        a -= 360.0;
    }
    if a <= -180.0 {
        a += 360.0;
    }
    a
}

fn sanitize(s: &str) -> String {
    let t: String = s.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    if t.is_empty() {
        "scena".into()
    } else {
        t
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_paint).as_secs_f32().max(1e-4);
        self.last_paint = now;
        self.fps = 0.95 * self.fps + 0.05 / dt;

        self.ingest_frame(ctx);
        self.update_field_texture(ctx);
        let before = self.scene.clone();

        if !self.hide_ui {
            self.menu_bar(ctx);
            self.update_banner(ctx);
            self.status_bar(ctx);
            if self.prefs.show_bottom {
                self.chart_panel(ctx);
            }
            self.tool_panel(ctx);
            if self.prefs.show_side {
                self.side_panel(ctx);
            }
        }
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(theme::BG)).show(ctx, |ui| self.viewport(ui));
        self.windows(ctx);
        self.shortcuts(ctx);
        self.toast_ui(ctx);
        self.handle_screenshot(ctx);
        self.finish_edits(ctx, before);
        if !self.paused || self.updater.busy() {
            ctx.request_repaint();
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.prefs.last_scene = Some(self.scene.to_json());
        eframe::set_value(storage, "prefs", &self.prefs);
    }

    fn auto_save_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(60)
    }
}

/// Kolor ciała na liście / wykresach (stały dla identyfikatora).
pub fn body_color(id: u64) -> Color32 {
    const C: [Color32; 6] = [
        Color32::from_rgb(56, 189, 248),
        Color32::from_rgb(251, 146, 60),
        Color32::from_rgb(163, 230, 53),
        Color32::from_rgb(232, 121, 249),
        Color32::from_rgb(250, 204, 21),
        Color32::from_rgb(45, 212, 191),
    ];
    C[(id as usize) % C.len()]
}
