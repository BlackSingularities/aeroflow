//! Okna dialogowe, skróty klawiszowe i powiadomienia.

use eframe::egui::{self, Align2, Color32, Key, RichText};

use crate::app::{App, Tool};
use crate::render::field::FieldKind;
use crate::sim::Cmd;
use crate::ui::theme;
use crate::update::{self, State};

const SHORTCUTS: &[(&str, &str)] = &[
    ("Spacja", "wstrzymaj / wznów"),
    (".", "jeden krok"),
    ("R", "zresetuj przepływ"),
    ("1 … 0", "narzędzia (zaznacz, profil, walec, prostokąt, elipsa, kształt, pędzel, gumka, dym, sonda)"),
    ("V / Shift+V", "następne / poprzednie pole"),
    ("P / L", "cząstki / linie prądu"),
    ("Del", "usuń zaznaczony obiekt"),
    ("Ctrl+D", "duplikuj"),
    ("Q / E", "obróć o −1° / +1° (z Shift: 5°)"),
    ("Alt + kółko", "obrót zaznaczonego obiektu (z Shift: 0,1°)"),
    ("Strzałki", "przesuń zaznaczony o komórkę (z Shift: 10)"),
    ("[ / ]", "rozmiar pędzla"),
    ("Kółko myszy", "powiększenie"),
    ("Środkowy przycisk", "przesuwanie widoku (dwuklik: dopasuj)"),
    ("Home", "dopasuj widok"),
    ("Ctrl+Z / Ctrl+Y", "cofnij / ponów"),
    ("Ctrl+O / Ctrl+S", "otwórz / zapisz scenę"),
    ("K", "zrzut widoku do PNG"),
    ("S / G / H", "panel ustawień / wykresy / ukryj interfejs"),
    ("F11", "pełny ekran"),
    ("F1", "ta lista"),
    ("Esc", "odznacz, zamknij okna, pokaż interfejs"),
];

impl App {
    pub fn windows(&mut self, ctx: &egui::Context) {
        let mut open = self.win.shortcuts;
        egui::Window::new("Skróty klawiszowe")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                egui::Grid::new("keys").num_columns(2).spacing([18.0, 5.0]).striped(true).show(ui, |ui| {
                    for (k, v) in SHORTCUTS {
                        ui.label(RichText::new(*k).monospace().color(theme::ACCENT));
                        ui.label(*v);
                        ui.end_row();
                    }
                });
            });
        self.win.shortcuts = open;

        let mut open = self.win.about;
        egui::Window::new("O programie")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("◆ AeroFlow").size(26.0).strong().color(theme::ACCENT));
                    ui.label(format!("wersja {}", update::VERSION));
                    ui.add_space(6.0);
                    ui.label("Interaktywny tunel aerodynamiczny 2D.");
                    ui.label("Lattice-Boltzmann D2Q9 · LES Smagorinsky · odbicie Bouzidiego");
                    ui.label(RichText::new(format!("{} rdzeni procesora", rayon::current_num_threads())).color(theme::TEXT_DIM));
                    ui.add_space(6.0);
                    if ui.link("Strona projektu i wydania").clicked() {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(update::releases_url()));
                    }
                    ui.label(RichText::new("Licencja MIT").small().color(theme::TEXT_DIM));
                });
            });
        self.win.about = open;

        let mut open = self.win.physics;
        egui::Window::new("Model fizyczny i ograniczenia").open(&mut open).default_width(560.0).default_height(520.0).vscroll(true).show(
            ctx,
            |ui| {
                physics_text(ui);
            },
        );
        self.win.physics = open;

        let mut open = self.win.changelog;
        egui::Window::new("Historia zmian").open(&mut open).default_width(560.0).default_height(520.0).vscroll(true).show(ctx, |ui| {
            markdown(ui, update::changelog());
        });
        self.win.changelog = open;

        if let Some(text) = self.win.whats_new.clone() {
            let mut open = true;
            egui::Window::new(format!("Co nowego w wersji {}", update::VERSION))
                .open(&mut open)
                .collapsible(false)
                .default_width(520.0)
                .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| markdown(ui, &text));
                    ui.separator();
                    if ui.button("Świetnie!").clicked() {
                        self.win.whats_new = None;
                    }
                });
            if !open {
                self.win.whats_new = None;
            }
        }

        self.update_window(ctx);
    }

    fn update_window(&mut self, ctx: &egui::Context) {
        if !self.win.update {
            return;
        }
        let mut open = true;
        let state = self.updater.state();
        egui::Window::new("Aktualizacje")
            .open(&mut open)
            .collapsible(false)
            .default_width(540.0)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(RichText::new(format!("Zainstalowana wersja: {}", update::VERSION)).color(theme::TEXT_DIM));
                ui.add_space(4.0);
                match state {
                    State::Idle => {
                        if ui.button("Sprawdź teraz").clicked() {
                            self.updater.check(true, ui.ctx());
                        }
                    }
                    State::Checking => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Sprawdzanie dostępności nowej wersji…");
                        });
                    }
                    State::UpToDate { .. } => {
                        ui.label(RichText::new("✔ Masz najnowszą wersję.").color(theme::GOOD));
                        if ui.button("Sprawdź ponownie").clicked() {
                            self.updater.check(true, ui.ctx());
                        }
                    }
                    State::Failed { message, .. } => {
                        ui.label(RichText::new(format!("Nie udało się: {message}")).color(theme::WARN));
                        ui.horizontal(|ui| {
                            if ui.button("Spróbuj ponownie").clicked() {
                                self.updater.check(true, ui.ctx());
                            }
                            if ui.button("Otwórz stronę wydań").clicked() {
                                ui.ctx().open_url(egui::OpenUrl::new_tab(update::releases_url()));
                            }
                        });
                    }
                    State::Available(r) => {
                        ui.label(RichText::new(format!("{} — {}", r.title, r.date)).strong().size(15.0));
                        ui.add_space(4.0);
                        egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                            if r.notes.trim().is_empty() {
                                ui.label(RichText::new("Brak opisu zmian.").color(theme::TEXT_DIM));
                            } else {
                                markdown(ui, &r.notes);
                            }
                        });
                        ui.separator();
                        ui.horizontal(|ui| {
                            let can_install = cfg!(windows) && r.installer.is_some();
                            if can_install {
                                let size = r.installer.as_ref().map(|a| a.size as f64 / 1e6).unwrap_or(0.0);
                                if ui.button(RichText::new(format!("⬇ Pobierz i zainstaluj ({size:.1} MB)")).strong()).clicked() {
                                    self.updater.download(r.clone(), ui.ctx());
                                }
                            } else if ui.button("Otwórz stronę wydania").clicked() {
                                ui.ctx().open_url(egui::OpenUrl::new_tab(&r.url));
                            }
                            if ui.button("Pomiń tę wersję").clicked() {
                                self.prefs.skipped_version = r.version.to_string();
                                self.win.update = false;
                            }
                            if ui.button("Później").clicked() {
                                self.win.update = false;
                            }
                        });
                    }
                    State::Downloading { release, done, total } => {
                        ui.label(format!("Pobieranie {}…", release.title));
                        let f = if total > 0 { done as f32 / total as f32 } else { 0.0 };
                        ui.add(egui::ProgressBar::new(f).show_percentage().text(format!(
                            "{:.1} / {:.1} MB",
                            done as f64 / 1e6,
                            total as f64 / 1e6
                        )));
                    }
                    State::Ready { release, path } => {
                        ui.label(RichText::new(format!("✔ Pobrano i zweryfikowano {}.", release.title)).color(theme::GOOD));
                        ui.label(
                            "Program zamknie się, zainstaluje aktualizację i uruchomi ponownie. Niezapisana scena zostanie przywrócona.",
                        );
                        if ui.button(RichText::new("Zainstaluj teraz").strong()).clicked() {
                            self.prefs.update_notes = Some((release.version.to_string(), release.notes.clone()));
                            match update::launch_installer(&path) {
                                Ok(()) => ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close),
                                Err(e) => self.notify(e),
                            }
                        }
                    }
                }
            });
        if !open {
            self.win.update = false;
        }
        // wynik ręcznego sprawdzania pokazujemy też jako powiadomienie
        if let State::Failed { manual: false, .. } = self.updater.state() {
            self.updater.dismiss();
        }
    }

    pub fn shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }
        let ev = ctx.input(|i| {
            let k = |key| i.key_pressed(key);
            (
                i.modifiers,
                [
                    k(Key::Space),
                    k(Key::Period),
                    k(Key::R),
                    k(Key::V),
                    k(Key::P),
                    k(Key::L),
                    k(Key::Delete) || k(Key::Backspace),
                    k(Key::D),
                    k(Key::Q),
                    k(Key::E),
                    k(Key::Home),
                    k(Key::Z),
                    k(Key::Y),
                    k(Key::O),
                    k(Key::S),
                    k(Key::K),
                    k(Key::G),
                    k(Key::H),
                    k(Key::F11),
                    k(Key::F1),
                    k(Key::Escape),
                    k(Key::OpenBracket),
                    k(Key::CloseBracket),
                    k(Key::N),
                ],
                [k(Key::ArrowLeft), k(Key::ArrowRight), k(Key::ArrowUp), k(Key::ArrowDown)],
                [
                    k(Key::Num1),
                    k(Key::Num2),
                    k(Key::Num3),
                    k(Key::Num4),
                    k(Key::Num5),
                    k(Key::Num6),
                    k(Key::Num7),
                    k(Key::Num8),
                    k(Key::Num9),
                    k(Key::Num0),
                ],
            )
        });
        let (m, k, arrows, nums) = ev;
        let [space, period, r, v, p, l, del, d, q, e, home, z, y, o, s, kk, g, h, f11, f1, esc, lb, rb, n] = k;
        let cmd = m.command;
        if cmd {
            if z && !m.shift {
                self.undo();
            }
            if y || (z && m.shift) {
                self.redo();
            }
            if d {
                self.duplicate_selected();
            }
            if o {
                self.open_dialog();
            }
            if s {
                self.save(m.shift);
            }
            if n {
                self.load_preset("empty");
            }
            return;
        }
        if space {
            self.set_paused(!self.paused);
        }
        if period {
            self.sim.send(Cmd::StepOnce);
        }
        if r {
            self.reset_flow();
        }
        if v {
            let all = FieldKind::ALL;
            let i = all.iter().position(|f| *f == self.prefs.field).unwrap_or(0);
            let j = if m.shift { (i + all.len() - 1) % all.len() } else { (i + 1) % all.len() };
            self.set_field(all[j]);
        }
        if p {
            self.prefs.particles = !self.prefs.particles;
        }
        if l {
            self.prefs.streamlines = !self.prefs.streamlines;
        }
        if del {
            self.delete_selected();
        }
        let step = if m.shift { 5.0 } else { 1.0 };
        if q {
            self.rotate_selected(-step);
        }
        if e {
            self.rotate_selected(step);
        }
        if home {
            self.view.fit = true;
        }
        if kk {
            self.request_screenshot(ctx);
        }
        if g {
            self.prefs.show_bottom = !self.prefs.show_bottom;
        }
        if s {
            self.prefs.show_side = !self.prefs.show_side;
        }
        if h {
            self.hide_ui = !self.hide_ui;
        }
        if f11 {
            let fs = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fs));
        }
        if f1 {
            self.win.shortcuts = !self.win.shortcuts;
        }
        if esc {
            if self.hide_ui {
                self.hide_ui = false;
            } else if self.win.shortcuts || self.win.about || self.win.physics || self.win.update || self.win.changelog {
                self.win.shortcuts = false;
                self.win.about = false;
                self.win.physics = false;
                self.win.update = false;
                self.win.changelog = false;
            } else {
                self.selected = None;
                self.drag = None;
            }
        }
        if lb {
            self.prefs.brush_cells = (self.prefs.brush_cells - 1.0).max(1.0);
        }
        if rb {
            self.prefs.brush_cells = (self.prefs.brush_cells + 1.0).min(20.0);
        }
        for (i, pressed) in nums.iter().enumerate() {
            if *pressed {
                self.tool = Tool::ALL[i];
            }
        }
        if arrows.iter().any(|a| *a) {
            let dx = self.scene.flow.dx() * if m.shift { 10.0 } else { 1.0 };
            let d = [
                if arrows[1] { dx } else { 0.0 } - if arrows[0] { dx } else { 0.0 },
                if arrows[2] { dx } else { 0.0 } - if arrows[3] { dx } else { 0.0 },
            ];
            if let Some(b) = self.selected.and_then(|id| self.scene.body_mut(id)) {
                if !b.locked {
                    b.translate(d);
                }
            }
        }
    }

    pub fn toast_ui(&mut self, ctx: &egui::Context) {
        // komunikaty z ręcznego sprawdzania aktualizacji
        match self.updater.state() {
            State::UpToDate { manual: true } if !self.win.update => {
                self.notify("Masz najnowszą wersję AeroFlow.");
                self.updater.dismiss();
            }
            State::Failed { message, manual: true } if !self.win.update => {
                self.notify(format!("Sprawdzanie aktualizacji: {message}"));
                self.updater.dismiss();
            }
            _ => {}
        }
        let Some((text, at)) = &self.toast else { return };
        let age = at.elapsed().as_secs_f32();
        let life = 2.5 + text.len() as f32 * 0.03;
        if age > life {
            self.toast = None;
            return;
        }
        let alpha = ((life - age) / 0.4).clamp(0.0, 1.0);
        egui::Area::new(egui::Id::new("toast"))
            .anchor(Align2::CENTER_BOTTOM, [0.0, -54.0])
            .interactable(false)
            .order(egui::Order::Tooltip)
            .show(ctx, |ui| {
                egui::Frame::NONE
                    .fill(Color32::from_rgba_unmultiplied(24, 30, 42, (235.0 * alpha) as u8))
                    .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(56, 189, 248, (120.0 * alpha) as u8)))
                    .corner_radius(8)
                    .inner_margin(egui::Margin::symmetric(14, 8))
                    .show(ui, |ui| {
                        ui.set_max_width(520.0);
                        ui.label(RichText::new(text.as_str()).color(theme::TEXT.gamma_multiply(alpha)));
                    });
            });
        ctx.request_repaint();
    }
}

/// Bardzo prosty renderer Markdown (nagłówki, wypunktowania, pogrubienia).
pub fn markdown(ui: &mut egui::Ui, text: &str) {
    for line in text.lines() {
        let t = line.trim_end();
        if t.is_empty() {
            ui.add_space(4.0);
        } else if let Some(h) = t.strip_prefix("### ") {
            ui.add_space(4.0);
            ui.label(RichText::new(strip_md(h)).strong().color(theme::ACCENT));
        } else if let Some(h) = t.strip_prefix("## ") {
            ui.add_space(6.0);
            ui.label(RichText::new(strip_md(h)).strong().size(16.0));
        } else if let Some(h) = t.strip_prefix("# ") {
            ui.label(RichText::new(strip_md(h)).strong().size(18.0));
        } else if let Some(b) = t.trim_start().strip_prefix("- ").or_else(|| t.trim_start().strip_prefix("* ")) {
            ui.horizontal_wrapped(|ui| {
                ui.label("  •");
                ui.label(strip_md(b));
            });
        } else {
            ui.label(strip_md(t));
        }
    }
}

fn strip_md(s: &str) -> String {
    s.replace("**", "").replace('`', "")
}

fn physics_text(ui: &mut egui::Ui) {
    markdown(
        ui,
        "## Co jest liczone
AeroFlow rozwiązuje nieściśliwe równania Naviera–Stokesa w 2D metodą siatkową Boltzmanna (D2Q9). \
Wszystkie wielkości wejściowe są fizyczne: płyn (gęstość i lepkość z temperatury), prędkość [m/s] i rozmiar obszaru [m]. \
Program przelicza je na jednostki siatki tak, by liczba Reynoldsa była dokładnie taka jak w rzeczywistości.

### Jednostki
- Δx = wysokość obszaru / liczba komórek.
- Prędkość strumienia w siatce u_lb (domyślnie 0,06) ustala krok czasu Δt = u_lb·Δx/U.
- Lepkość siatki ν_lb = ν·Δt/Δx², czas relaksacji τ = 3ν_lb + ½.
- Siły z metody wymiany pędu są przeliczane na N na metr rozpiętości; ciśnienie na Pa.

### Model
- Kolizja BGK z regularyzacją części nierównowagowej.
- Model podsiatkowy Smagorinsky'ego (LES): gdy siatka nie rozdziela najmniejszych wirów, lepkość turbulentna ν_t zastępuje ich działanie. Pole „Lepkość turbulentna” pokazuje, gdzie model pracuje.
- Brzegi ciał: interpolowane odbicie Bouzidiego z dokładną odległością ściany (drugi rząd dokładności).
- Brzegi obszaru: wlot z profilem prędkości i turbulencją, wylot z zerowym gradientem i niewidoczną strefą pochłaniającą; góra i dół jako opływ swobodny lub ściana.

### Ograniczenia — czytaj wyniki świadomie
- Przepływ jest dwuwymiarowy (nieskończenie długie ciało). Prawdziwy ślad za walcem staje się trójwymiarowy powyżej Re ≈ 190, a skrzydła mają opór indukowany — model 2D ich nie odtwarza.
- Model jest nieściśliwy: przy Ma > 0,3 rzeczywistość odbiega od wyniku.
- Przy dużych Re (np. skrzydło samolotu, Re ~ 10⁶) warstwa przyścienna jest cieńsza niż komórka siatki; brak modelu przyściennego i przejścia laminarno-turbulentnego oznacza, że opór i maksymalna siła nośna są tylko jakościowe. Wskaźnik „Wiarygodność” informuje, jak traktować wynik.
- Blokada: ciało zajmujące ponad ~10% wysokości obszaru ma zawyżony opór (jak w ciasnym tunelu).

### Walidacja
Scenariusze z grupy „Walidacja” mają wartości odniesienia z literatury; po ustaleniu przepływu panel „Siły” porównuje Cd, amplitudę Cl i liczbę Strouhala.",
    );
}
