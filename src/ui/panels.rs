//! Układ okna: górny pasek (scenariusz, start/pauza, pole, nakładki), pasek narzędzi z podpisami,
//! panel „Inspektor” po prawej (wyniki → obiekt → warunki → sekcje zaawansowane) i pasek stanu.
//!
//! Zasada: najczęstsze czynności są widoczne od razu i w kolejności, w jakiej się ich używa —
//! wybierz scenariusz, narysuj ciało, ustaw warunki, czytaj wyniki. Wszystko rzadsze jest zwinięte.

use eframe::egui::{self, Color32, CornerRadius, RichText, Stroke};

use crate::analysis::stats;
use crate::app::{App, ChartTab, Tool};
use crate::physics::geometry::{Naca, Shape};
use crate::physics::lattice::{Boundary, InletProfile};
use crate::physics::units::{assess, Fluid, FluidKind};
use crate::prefs::SpeedPref;
use crate::render::colormap::Colormap;
use crate::render::field::{fmt_num, fmt_si, FieldKind};
use crate::scene::presets;
use crate::sim::Cmd;
use crate::ui::theme::{self, section};
use crate::ui::viewport::fidelity_color;
use crate::update::{self, State};

/// Pola dostępne jednym kliknięciem w górnym pasku.
const QUICK_FIELDS: [(FieldKind, &str); 4] =
    [(FieldKind::Speed, "Prędkość"), (FieldKind::Cp, "Ciśnienie"), (FieldKind::Vorticity, "Wirowość"), (FieldKind::Smoke, "Dym")];

/// Karta panelu: zaokrąglone tło z delikatną obwódką.
fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::NONE
        .fill(theme::PANEL_2)
        .stroke(Stroke::new(1.0_f32, theme::CARD_EDGE))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

fn card_title(ui: &mut egui::Ui, title: &str, right: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).strong().size(14.5));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), right);
    });
    ui.add_space(4.0);
}

/// Przycisk-przełącznik w kształcie pastylki.
fn chip(ui: &mut egui::Ui, on: bool, text: &str) -> egui::Response {
    let (fill, col) = if on { (theme::ACCENT_DIM, Color32::WHITE) } else { (Color32::TRANSPARENT, theme::TEXT) };
    ui.add(
        egui::Button::new(RichText::new(text).color(col))
            .fill(fill)
            .stroke(Stroke::new(1.0_f32, if on { theme::ACCENT } else { theme::CARD_EDGE }))
            .corner_radius(CornerRadius::same(14)),
    )
}

/// Kafelek z dużą liczbą (wyniki).
fn tile(ui: &mut egui::Ui, w: f32, label: &str, value: String, color: Color32, tip: &str) {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 54.0), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 8.0, theme::BG);
    p.text(rect.left_top() + egui::vec2(9.0, 7.0), egui::Align2::LEFT_TOP, label, egui::FontId::proportional(11.5), theme::TEXT_DIM);
    p.text(rect.left_bottom() + egui::vec2(9.0, -7.0), egui::Align2::LEFT_BOTTOM, value, egui::FontId::monospace(19.0), color);
    if !tip.is_empty() {
        resp.on_hover_text(tip);
    }
}

impl App {
    // ================================================================== górny pasek

    pub fn top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top").frame(egui::Frame::NONE.fill(theme::PANEL).inner_margin(egui::Margin::symmetric(10, 7))).show(
            ctx,
            |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    ui.label(RichText::new("◆ AeroFlow").strong().size(15.0).color(theme::ACCENT));
                    ui.add_space(6.0);

                    // scenariusz / plik — od tego zaczyna się praca
                    let title = match &self.file_path {
                        Some(p) => p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
                        None => self.scene.name.clone(),
                    };
                    let star = if self.file_path.is_some() && self.modified() { " •" } else { "" };
                    let mut short: String = title.chars().take(28).collect();
                    if title.chars().count() > 28 {
                        short.push('…');
                    }
                    ui.menu_button(RichText::new(format!("📂  {short}{star}  ▾")).size(14.0), |ui| self.scene_menu(ui))
                        .response
                        .on_hover_text("Scenariusze, nowa scena, otwieranie i zapis");

                    ui.add_space(8.0);
                    // start / pauza — największy przycisk
                    let (txt, fill) = if self.paused {
                        ("▶  Start", Color32::from_rgb(22, 128, 76))
                    } else {
                        ("⏸  Pauza", Color32::from_rgb(52, 62, 80))
                    };
                    if ui
                        .add(
                            egui::Button::new(RichText::new(txt).size(14.0).strong().color(Color32::WHITE))
                                .fill(fill)
                                .corner_radius(CornerRadius::same(8))
                                .min_size(egui::vec2(96.0, 30.0)),
                        )
                        .on_hover_text("Spacja")
                        .clicked()
                    {
                        self.set_paused(!self.paused);
                    }
                    if ui
                        .add(egui::Button::new(RichText::new("⟲").size(16.0)).min_size(egui::vec2(32.0, 30.0)))
                        .on_hover_text("Przepływ od nowa (R)")
                        .clicked()
                    {
                        self.reset_flow();
                    }
                    if ui
                        .add(egui::Button::new(RichText::new("⏭").size(15.0)).min_size(egui::vec2(32.0, 30.0)))
                        .on_hover_text("Jeden krok (.)")
                        .clicked()
                    {
                        self.sim.send(Cmd::StepOnce);
                    }

                    ui.add_space(10.0);
                    ui.label(RichText::new("Pokaż").color(theme::TEXT_DIM));
                    for (f, l) in QUICK_FIELDS {
                        if chip(ui, self.prefs.field == f, l).on_hover_text(f.label()).clicked() {
                            self.set_field(f);
                        }
                    }
                    let other = !QUICK_FIELDS.iter().any(|(f, _)| *f == self.prefs.field);
                    let more = if other { format!("{} ▾", self.prefs.field.short()) } else { "Więcej ▾".into() };
                    let mut field = self.prefs.field;
                    let r = ui.menu_button(RichText::new(more).color(if other { theme::ACCENT } else { theme::TEXT }), |ui| {
                        for f in FieldKind::ALL {
                            if ui.selectable_label(field == f, f.label()).clicked() {
                                field = f;
                                ui.close();
                            }
                        }
                    });
                    r.response.on_hover_text("Wszystkie pola (klawisz V przełącza kolejne)");
                    if field != self.prefs.field {
                        self.set_field(field);
                    }

                    ui.add_space(10.0);
                    ui.label(RichText::new("Nakładki").color(theme::TEXT_DIM));
                    if chip(ui, self.prefs.particles, "Cząstki").on_hover_text("Cząstki-znaczniki (P)").clicked() {
                        self.prefs.particles = !self.prefs.particles;
                    }
                    if chip(ui, self.prefs.streamlines, "Linie prądu").on_hover_text("Linie prądu (L)").clicked() {
                        self.prefs.streamlines = !self.prefs.streamlines;
                    }
                    if chip(ui, self.prefs.vectors, "Wektory").on_hover_text("Wektory prędkości").clicked() {
                        self.prefs.vectors = !self.prefs.vectors;
                    }
                    if chip(ui, self.prefs.show_forces, "Siły").on_hover_text("Strzałki oporu D i siły nośnej L na ciałach").clicked()
                    {
                        self.prefs.show_forces = !self.prefs.show_forces;
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.menu_button(RichText::new("☰").size(16.0), |ui| self.main_menu(ui)).response.on_hover_text("Menu");
                        if chip(ui, self.prefs.show_side, "Panel").on_hover_text("Panel Inspektor (S)").clicked() {
                            self.prefs.show_side = !self.prefs.show_side;
                        }
                        if chip(ui, self.prefs.show_bottom, "Wykresy").on_hover_text("Wykresy sił, Cp, biegunowa, sondy (G)").clicked() {
                            self.prefs.show_bottom = !self.prefs.show_bottom;
                        }
                        if let State::Available(r) = self.updater.state() {
                            if ui
                                .button(RichText::new(format!("⬆ {}", r.version)).color(theme::GOOD))
                                .on_hover_text("Dostępna aktualizacja")
                                .clicked()
                            {
                                self.win.update = true;
                            }
                        }
                    });
                });
            },
        );
    }

    fn scene_menu(&mut self, ui: &mut egui::Ui) {
        ui.set_min_width(250.0);
        if ui.button("➕  Nowa pusta scena          Ctrl+N").clicked() {
            self.load_preset("empty");
            ui.close();
        }
        if ui.button("📁  Otwórz…                         Ctrl+O").clicked() {
            ui.close();
            self.open_dialog();
        }
        if ui.button("💾  Zapisz                            Ctrl+S").clicked() {
            ui.close();
            self.save(false);
        }
        if ui.button("      Zapisz jako…            Ctrl+Shift+S").clicked() {
            ui.close();
            self.save(true);
        }
        let recent = self.prefs.recent_files.clone();
        if !recent.is_empty() {
            ui.menu_button("      Ostatnie pliki", |ui| {
                for p in recent {
                    if ui.button(&p).clicked() {
                        ui.close();
                        self.open_path(p.into());
                    }
                }
            });
        }
        ui.separator();
        ui.label(RichText::new("SCENARIUSZE").size(11.0).strong().color(theme::TEXT_DIM));
        self.preset_menu(ui);
    }

    fn main_menu(&mut self, ui: &mut egui::Ui) {
        ui.set_min_width(230.0);
        ui.menu_button("Edycja", |ui| {
            if ui.add_enabled(!self.undo.is_empty(), egui::Button::new("Cofnij            Ctrl+Z")).clicked() {
                self.undo();
                ui.close();
            }
            if ui.add_enabled(!self.redo.is_empty(), egui::Button::new("Ponów             Ctrl+Y")).clicked() {
                self.redo();
                ui.close();
            }
            ui.separator();
            let has = self.selected.is_some();
            if ui.add_enabled(has, egui::Button::new("Duplikuj          Ctrl+D")).clicked() {
                self.duplicate_selected();
                ui.close();
            }
            if ui.add_enabled(has, egui::Button::new("Usuń                  Del")).clicked() {
                self.delete_selected();
                ui.close();
            }
            ui.separator();
            if ui.button("Usuń wszystkie ciała").clicked() {
                self.scene.bodies.retain(|b| b.locked);
                self.selected = None;
                ui.close();
            }
            if ui.button("Usuń sondy i źródła dymu").clicked() {
                self.scene.probes.clear();
                self.scene.emitters.clear();
                ui.close();
            }
        });
        ui.menu_button("Eksport", |ui| {
            if ui.button("Zrzut widoku (PNG)       K").clicked() {
                ui.close();
                self.request_screenshot(ui.ctx());
            }
            if ui.button("Historia sił (CSV)").clicked() {
                ui.close();
                self.export_csv("forces");
            }
            if ui.button("Biegunowa (CSV)").clicked() {
                ui.close();
                self.export_csv("polar");
            }
            if ui.button("Sondy (CSV)").clicked() {
                ui.close();
                self.export_csv("probes");
            }
            if ui.button("Pole prędkości i ciśnienia (CSV)").clicked() {
                ui.close();
                self.export_csv("field");
            }
        });
        ui.menu_button("Widok", |ui| {
            ui.checkbox(&mut self.prefs.show_side, "Panel Inspektor       S");
            ui.checkbox(&mut self.prefs.show_bottom, "Wykresy                G");
            ui.checkbox(&mut self.prefs.show_legend, "Legenda kolorów");
            ui.checkbox(&mut self.prefs.show_hud, "Wyniki na widoku (gdy panel ukryty)");
            ui.separator();
            if ui.button("Dopasuj widok          Home").clicked() {
                self.view.fit = true;
                ui.close();
            }
            if ui.button("Tylko symulacja          H").clicked() {
                self.hide_ui = true;
                ui.close();
            }
            if ui.button("Pełny ekran              F11").clicked() {
                let fs = ui.ctx().input(|i| i.viewport().fullscreen.unwrap_or(false));
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fs));
                ui.close();
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Skala interfejsu");
                if ui.add(egui::Slider::new(&mut self.prefs.ui_scale, 0.7..=2.0).step_by(0.05)).changed() {
                    ui.ctx().set_zoom_factor(self.prefs.ui_scale);
                }
            });
        });
        ui.menu_button("Analiza", |ui| {
            let mut avg = self.averaging;
            if ui.checkbox(&mut avg, "Uśrednianie w czasie").changed() {
                self.set_averaging(avg);
            }
            if ui.button("Biegunowa profilu…").clicked() {
                self.chart_tab = ChartTab::Polar;
                self.prefs.show_bottom = true;
                ui.close();
            }
        });
        ui.separator();
        if ui.button("Skróty klawiszowe       F1").clicked() {
            self.win.shortcuts = true;
            ui.close();
        }
        if ui.button("Model fizyczny i ograniczenia").clicked() {
            self.win.physics = true;
            ui.close();
        }
        if ui.button("Historia zmian").clicked() {
            self.win.changelog = true;
            ui.close();
        }
        ui.separator();
        if ui.button("Sprawdź aktualizacje").clicked() {
            self.updater.check(true, ui.ctx());
            self.win.update = true;
            ui.close();
        }
        ui.checkbox(&mut self.prefs.check_updates, "Sprawdzaj przy starcie");
        if ui.button("Strona wydań").clicked() {
            ui.ctx().open_url(egui::OpenUrl::new_tab(update::releases_url()));
            ui.close();
        }
        if ui.button("O programie").clicked() {
            self.win.about = true;
            ui.close();
        }
        ui.separator();
        if ui.button("Zakończ").clicked() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn preset_menu(&mut self, ui: &mut egui::Ui) {
        let mut group = "";
        for p in presets() {
            if p.group != group {
                group = p.group;
                ui.add_space(2.0);
                ui.label(RichText::new(group).small().color(theme::TEXT_DIM));
            }
            if ui.button(format!("   {}", p.name)).clicked() {
                ui.close();
                self.load_preset(p.id);
            }
        }
    }

    pub fn update_banner(&mut self, ctx: &egui::Context) {
        let State::Available(r) = self.updater.state() else { return };
        if self.prefs.skipped_version == r.version.to_string() {
            return;
        }
        egui::TopBottomPanel::top("update_banner")
            .frame(egui::Frame::NONE.fill(Color32::from_rgb(14, 48, 40)).inner_margin(egui::Margin::symmetric(12, 6)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("⬆").color(theme::GOOD).size(16.0));
                    ui.label(
                        RichText::new(format!("Dostępna jest nowa wersja AeroFlow {} (masz {}).", r.version, update::VERSION))
                            .color(theme::TEXT),
                    );
                    if ui.button("Co nowego i instalacja").clicked() {
                        self.win.update = true;
                    }
                    if ui.button("Pomiń tę wersję").clicked() {
                        self.prefs.skipped_version = r.version.to_string();
                    }
                    if ui.small_button("🗙").on_hover_text("Przypomnij przy następnym uruchomieniu").clicked() {
                        self.updater.dismiss();
                    }
                });
            });
    }

    // ================================================================== pasek stanu

    pub fn status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status")
            .frame(egui::Frame::NONE.fill(theme::PANEL).inner_margin(egui::Margin::symmetric(10, 4)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let small = |s: String| RichText::new(s).size(11.5).color(theme::TEXT_DIM);
                    let sep = |ui: &mut egui::Ui| {
                        ui.label(RichText::new("·").color(theme::CARD_EDGE));
                    };
                    let flow = &self.scene.flow;
                    let (nx, ny) = flow.grid();
                    let units = flow.units();
                    if let Some(f) = &self.frame {
                        let state = if self.paused { ("● wstrzymano", theme::WARN) } else { ("● liczy", theme::GOOD) };
                        ui.label(RichText::new(state.0).size(11.5).color(state.1));
                        sep(ui);
                        ui.label(small(format!("t = {}", fmt_si(f.time, "s", 3))));
                        sep(ui);
                        let r = f.realtime_ratio;
                        if !self.paused && r > 0.0 {
                            let rt = if r >= 1.0 {
                                format!("{}× szybciej niż w rzeczywistości", fmt_num(r, 2))
                            } else {
                                format!("{}× wolniej niż w rzeczywistości", fmt_num(1.0 / r.max(1e-12), 2))
                            };
                            ui.label(small(rt));
                            sep(ui);
                        }
                        let dev = f.gpu.as_deref().map(|g| format!("GPU {g}")).unwrap_or_else(|| "CPU".into());
                        ui.label(small(format!("{dev} · {} MLUPS", fmt_num(f.mlups, 0))));
                        sep(ui);
                    }
                    ui.label(small(format!("siatka {nx}×{ny}, Δx = {}, Δt = {}", fmt_si(units.dx, "m", 2), fmt_si(units.dt, "s", 2))));
                    if let Some(sw) = &self.sweep {
                        if !sw.done() {
                            ui.add(
                                egui::ProgressBar::new(sw.progress())
                                    .desired_width(140.0)
                                    .text(format!("biegunowa α = {:.1}°", sw.current_angle().unwrap_or(0.0))),
                            );
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(small(format!("v{} · {:.0} FPS", update::VERSION, self.fps)));
                        if let Some(w) = self.hover {
                            ui.label(small(format!("x {}  y {}", fmt_si(w[0], "m", 3), fmt_si(w[1], "m", 3))));
                        }
                    });
                });
            });
    }

    // ================================================================== narzędzia

    pub fn tool_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("tools")
            .exact_width(70.0)
            .resizable(false)
            .frame(egui::Frame::NONE.fill(theme::PANEL).inner_margin(egui::Margin::symmetric(6, 8)))
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    let groups: [(&str, &[Tool]); 4] = [
                        ("", &[Tool::Select]),
                        ("CIAŁA", &[Tool::Airfoil, Tool::Circle, Tool::Rect, Tool::Ellipse, Tool::Polygon]),
                        ("ŚCIANY", &[Tool::Brush, Tool::Eraser]),
                        ("POMIAR", &[Tool::Emitter, Tool::Probe]),
                    ];
                    for (title, tools) in groups {
                        if !title.is_empty() {
                            ui.add_space(8.0);
                            ui.label(RichText::new(title).size(9.5).strong().color(theme::TEXT_DIM));
                        }
                        for &t in tools {
                            self.tool_button(ui, t);
                        }
                    }
                });
            });
    }

    fn tool_button(&mut self, ui: &mut egui::Ui, t: Tool) {
        let sel = self.tool == t;
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(58.0, 48.0), egui::Sense::click());
        let bg = if sel {
            theme::ACCENT_DIM
        } else if resp.hovered() {
            Color32::from_rgb(34, 42, 56)
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, 8.0, bg);
        let col = if sel {
            Color32::WHITE
        } else if resp.hovered() {
            theme::TEXT
        } else {
            Color32::from_rgb(170, 182, 200)
        };
        let icon = egui::Rect::from_center_size(rect.center() - egui::vec2(0.0, 7.0), egui::vec2(18.0, 18.0));
        tool_icon(ui.painter(), icon, t, col);
        ui.painter().text(
            rect.center_bottom() - egui::vec2(0.0, 5.0),
            egui::Align2::CENTER_BOTTOM,
            t.short(),
            egui::FontId::proportional(10.5),
            col,
        );
        if resp.on_hover_text(format!("{}  ({})\n{}", t.label(), t.key(), t.hint())).clicked() {
            self.tool = t;
        }
    }

    // ================================================================== Inspektor

    pub fn side_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("inspector")
            .default_width(350.0)
            .min_width(300.0)
            .max_width(520.0)
            .resizable(true)
            .frame(egui::Frame::NONE.fill(theme::PANEL).inner_margin(egui::Margin::same(10)))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    self.results_card(ui);
                    ui.add_space(4.0);
                    if self.selected.is_some_and(|id| self.scene.body(id).is_some()) {
                        self.object_card(ui);
                    } else {
                        self.objects_card(ui);
                    }
                    ui.add_space(4.0);
                    self.conditions_card(ui);
                    ui.add_space(4.0);
                    self.advanced(ui);
                    ui.add_space(20.0);
                });
            });
    }

    /// Najważniejsze liczby dla głównego ciała — zawsze na górze panelu.
    fn results_card(&mut self, ui: &mut egui::Ui) {
        card(ui, |ui| {
            let units = self.scene.flow.units();
            let Some(b) = self.main_body().cloned() else {
                card_title(ui, "Wyniki", |_| {});
                ui.label(
                    RichText::new("Narysuj ciało narzędziem z lewej (np. Profil lub Walec) albo wybierz scenariusz z 📂.")
                        .color(theme::TEXT_DIM),
                );
                return;
            };
            let l = b.ref_length();
            let re = units.reynolds(l);
            let a = assess(&units, l / units.dx, re);
            card_title(ui, &format!("Wyniki · {}", b.name), |ui| {
                ui.label(RichText::new(a.fidelity.label()).size(11.0).color(fidelity_color(a.fidelity)))
                    .on_hover_text("Wiarygodność wyniku przy tej rozdzielczości i liczbie Reynoldsa. Szczegóły: sekcja Obliczenia.");
            });
            let conv = l / units.u_inf;
            let st = self.history.get(b.id).and_then(|s| stats(s, &units, l, (12.0 * conv).max(1e-9)));
            let last = self.history.get(b.id).and_then(|s| s.back().copied());
            let w = ((ui.available_width() - 2.0 * ui.spacing().item_spacing.x) / 3.0).floor();
            let dash = || "—".to_string();
            ui.horizontal(|ui| {
                tile(
                    ui,
                    w,
                    "Opór Cd",
                    st.as_ref().map(|s| fmt_num(s.cd_mean, 3)).unwrap_or_else(dash),
                    theme::DRAG,
                    "Współczynnik oporu (średnia z ostatnich 12 c/U)",
                );
                tile(
                    ui,
                    w,
                    "Nośna Cl",
                    st.as_ref().map(|s| fmt_num(s.cl_mean, 3)).unwrap_or_else(dash),
                    theme::LIFT,
                    "Współczynnik siły nośnej (średnia)",
                );
                if matches!(b.shape, Shape::Airfoil { .. }) {
                    let ld = st.as_ref().filter(|s| s.cd_mean.abs() > 1e-6).map(|s| fmt_num(s.cl_mean / s.cd_mean, 1));
                    tile(ui, w, "Doskonałość L/D", ld.unwrap_or_else(dash), theme::TEXT, "Stosunek siły nośnej do oporu");
                } else {
                    let s = st.as_ref().and_then(|s| s.strouhal).map(|v| fmt_num(v, 3));
                    tile(ui, w, "Strouhal St", s.unwrap_or_else(dash), theme::TEXT, "Bezwymiarowa częstotliwość odrywania wirów");
                }
            });
            ui.add_space(4.0);
            let span = self.scene.flow.span;
            egui::Grid::new("res_grid").num_columns(2).spacing([12.0, 3.0]).show(ui, |ui| {
                let row = |ui: &mut egui::Ui, k: &str, v: String| {
                    ui.label(RichText::new(k).color(theme::TEXT_DIM));
                    ui.label(RichText::new(v).monospace());
                    ui.end_row();
                };
                row(ui, "Liczba Reynoldsa", fmt_num(re.round(), 2));
                // siły ze średnich współczynników: F = C · q∞ · L · rozpiętość
                let k = units.q_inf() * l * span;
                if let Some(s) = &st {
                    row(ui, "Opór D", fmt_si(s.cd_mean * k, "N", 3));
                    row(ui, "Siła nośna L", fmt_si(s.cl_mean * k, "N", 3));
                } else if let Some(c) = last {
                    row(ui, "Opór D", fmt_si(c.fx * span, "N", 3));
                    row(ui, "Siła nośna L", fmt_si(c.fy * span, "N", 3));
                }
                if let Some(s) = &st {
                    if let Some(f) = s.freq {
                        row(ui, "Odrywanie wirów", fmt_si(f, "Hz", 3));
                    }
                }
            });
            ui.label(
                RichText::new(format!("Siły na {} rozpiętości (Obszar i brzegi → Rozpiętość).", fmt_si(span, "m", 2)))
                    .size(11.0)
                    .color(theme::TEXT_DIM),
            );
            if let Some(s) = &st {
                let (t, c) =
                    if s.settled { ("● przepływ ustalony", theme::GOOD) } else { ("● przepływ się ustala…", theme::WARN) };
                ui.label(RichText::new(t).size(11.5).color(c));
            } else {
                ui.label(RichText::new("Zbieranie danych…").size(11.5).color(theme::TEXT_DIM));
            }
            let re_eff = units.reynolds_effective(l);
            if re_eff < 0.95 * re {
                ui.label(
                    RichText::new(format!(
                        "Siatka rozdziela Re ≈ {} — obraz jakościowy. Większa rozdzielczość (sekcja Obliczenia) podnosi ten próg.",
                        fmt_num(re_eff, 2)
                    ))
                    .size(11.5)
                    .color(theme::TEXT_DIM),
                );
            }
        });
    }

    fn objects_card(&mut self, ui: &mut egui::Ui) {
        card(ui, |ui| {
            card_title(ui, "Obiekty", |ui| {
                ui.label(RichText::new(format!("{}", self.scene.bodies.len())).color(theme::TEXT_DIM));
            });
            if self.scene.bodies.is_empty() {
                ui.label(RichText::new("Brak obiektów. Wybierz narzędzie z lewego paska i przeciągnij na widoku.").color(theme::TEXT_DIM));
                return;
            }
            let mut action: Option<(u64, &str)> = None;
            for b in &self.scene.bodies {
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter().circle_filled(rect.center(), 4.0, crate::app::body_color(b.id));
                    if ui.selectable_label(false, format!("{}  ·  {}", b.name, b.shape.kind_label())).clicked() {
                        action = Some((b.id, "select"));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("🗑").on_hover_text("Usuń").clicked() {
                            action = Some((b.id, "delete"));
                        }
                        if ui.small_button(if b.locked { "🔒" } else { "🔓" }).on_hover_text("Zablokuj położenie").clicked() {
                            action = Some((b.id, "lock"));
                        }
                    });
                });
            }
            ui.label(
                RichText::new("Kliknij obiekt na liście lub na widoku, by zmienić jego wymiary i kąt.").size(11.5).color(theme::TEXT_DIM),
            );
            match action {
                Some((id, "select")) => {
                    self.selected = Some(id);
                    self.chart_body = Some(id);
                }
                Some((id, "delete")) => self.scene.bodies.retain(|b| b.id != id),
                Some((id, "lock")) => {
                    if let Some(b) = self.scene.body_mut(id) {
                        b.locked = !b.locked;
                    }
                }
                _ => {}
            }
        });
    }

    fn object_card(&mut self, ui: &mut egui::Ui) {
        let Some(id) = self.selected else { return };
        let units = self.scene.flow.units();
        let domain = self.scene.flow.domain;
        let mut close = false;
        let mut dup = false;
        let mut del = false;
        card(ui, |ui| {
            let Some(b) = self.scene.body_mut(id) else { return };
            let kind = b.shape.kind_label();
            card_title(ui, kind, |ui| {
                if ui.small_button("✕").on_hover_text("Odznacz (Esc)").clicked() {
                    close = true;
                }
                let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                ui.painter().circle_filled(rect.center(), 4.0, crate::app::body_color(id));
            });
            egui::Grid::new("body_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label("Nazwa");
                ui.add(egui::TextEdit::singleline(&mut b.name).desired_width(170.0));
                ui.end_row();
                if b.rotatable() {
                    ui.label(if matches!(b.shape, Shape::Airfoil { .. }) { "Kąt natarcia α" } else { "Obrót" });
                    ui.add(egui::Slider::new(&mut b.angle, -30.0..=30.0).clamping(egui::SliderClamping::Never).step_by(0.1).suffix("°"))
                        .on_hover_text("Także: uchwyt na widoku, klawisze Q/E, Alt + kółko");
                    ui.end_row();
                }
                match &mut b.shape {
                    Shape::Airfoil { chord, naca } => {
                        ui.label("Cięciwa c");
                        ui.add(
                            egui::DragValue::new(chord)
                                .speed(domain[1] * 0.002)
                                .range(domain[1] * 0.01..=domain[0])
                                .max_decimals(4)
                                .suffix(" m"),
                        );
                        ui.end_row();
                        ui.label("Profil NACA");
                        let mut s = naca.designation();
                        if ui.add(egui::TextEdit::singleline(&mut s).desired_width(60.0)).changed() {
                            if let Some(n) = Naca::parse(&s) {
                                *naca = n;
                            }
                        }
                        ui.end_row();
                        ui.label("Grubość t");
                        let mut t = naca.t * 100.0;
                        if ui.add(egui::Slider::new(&mut t, 3.0..=30.0).suffix(" %").step_by(1.0)).changed() {
                            naca.t = t / 100.0;
                        }
                        ui.end_row();
                        ui.label("Wygięcie m");
                        let mut m = naca.m * 100.0;
                        if ui.add(egui::Slider::new(&mut m, 0.0..=9.0).suffix(" %").step_by(1.0)).changed() {
                            naca.m = m / 100.0;
                        }
                        ui.end_row();
                        ui.label("Położenie wygięcia");
                        let mut p = naca.p * 10.0;
                        if ui
                            .add(egui::Slider::new(&mut p, 1.0..=9.0).step_by(1.0).custom_formatter(|v, _| format!("{:.0}0 %", v)))
                            .changed()
                        {
                            naca.p = p / 10.0;
                        }
                        ui.end_row();
                    }
                    Shape::Circle { r } => {
                        let mut d = *r * 2.0;
                        ui.label("Średnica D");
                        if ui
                            .add(
                                egui::DragValue::new(&mut d)
                                    .speed(domain[1] * 0.001)
                                    .range(domain[1] * 0.005..=domain[1])
                                    .max_decimals(5)
                                    .suffix(" m"),
                            )
                            .changed()
                        {
                            *r = d * 0.5;
                        }
                        ui.end_row();
                    }
                    Shape::Ellipse { a, b: bb } => {
                        ui.label("Półoś a");
                        ui.add(
                            egui::DragValue::new(a)
                                .speed(domain[1] * 0.001)
                                .range(domain[1] * 0.003..=domain[0])
                                .max_decimals(5)
                                .suffix(" m"),
                        );
                        ui.end_row();
                        ui.label("Półoś b");
                        ui.add(
                            egui::DragValue::new(bb)
                                .speed(domain[1] * 0.001)
                                .range(domain[1] * 0.003..=domain[0])
                                .max_decimals(5)
                                .suffix(" m"),
                        );
                        ui.end_row();
                    }
                    Shape::Rect { w, h, radius } => {
                        ui.label("Szerokość");
                        ui.add(
                            egui::DragValue::new(w)
                                .speed(domain[1] * 0.001)
                                .range(domain[1] * 0.003..=domain[0] * 2.0)
                                .max_decimals(5)
                                .suffix(" m"),
                        );
                        ui.end_row();
                        ui.label("Wysokość");
                        ui.add(
                            egui::DragValue::new(h)
                                .speed(domain[1] * 0.001)
                                .range(domain[1] * 0.003..=domain[1] * 2.0)
                                .max_decimals(5)
                                .suffix(" m"),
                        );
                        ui.end_row();
                        ui.label("Zaokrąglenie");
                        let max = 0.5 * w.min(*h);
                        ui.add(egui::DragValue::new(radius).speed(domain[1] * 0.0005).range(0.0..=max).max_decimals(5).suffix(" m"));
                        ui.end_row();
                    }
                    Shape::Polygon { pts } => {
                        ui.label("Wierzchołki");
                        ui.label(format!("{}", pts.len()));
                        ui.end_row();
                    }
                    Shape::Wall { strokes } => {
                        ui.label("Pociągnięcia");
                        ui.label(format!("{} (w tym gumka: {})", strokes.len(), strokes.iter().filter(|s| s.erase).count()));
                        ui.end_row();
                    }
                }
                let sp = domain[1] * 0.002;
                ui.label("Położenie");
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut b.pos[0]).speed(sp).max_decimals(3).prefix("x ").suffix(" m"));
                    ui.add(egui::DragValue::new(&mut b.pos[1]).speed(sp).max_decimals(3).prefix("y ").suffix(" m"));
                });
                ui.end_row();
            });
            if !matches!(b.shape, Shape::Circle { .. } | Shape::Airfoil { .. }) {
                ui.horizontal(|ui| {
                    ui.label("Skaluj");
                    for k in [0.8, 0.9, 1.1, 1.25] {
                        if ui.small_button(format!("×{k}")).clicked() {
                            b.scale(k);
                        }
                    }
                });
            }
            ui.horizontal(|ui| {
                ui.checkbox(&mut b.locked, "Zablokowany");
                if ui.checkbox(&mut b.mirror, "Odbity").changed() {
                    b.angle = -b.angle;
                }
            });
            let l = b.ref_length();
            let p = b.prepare();
            let frontal = p.bbox[3] - p.bbox[1];
            let bl = frontal / domain[1];
            ui.label(
                RichText::new(format!(
                    "{} = {} ({:.0} komórek) · blokada obszaru {:.1} %",
                    b.ref_length_label(),
                    fmt_si(l, "m", 3),
                    l / units.dx,
                    bl * 100.0
                ))
                .size(11.5)
                .color(if bl > 0.1 { theme::WARN } else { theme::TEXT_DIM }),
            )
            .on_hover_text("Blokada = wysokość czołowa / wysokość obszaru. Powyżej ~10% brzegi zawyżają opór.");
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button("🗐 Duplikuj").on_hover_text("Ctrl+D").clicked() {
                    dup = true;
                }
                if ui.button(RichText::new("🗑 Usuń").color(theme::BAD)).on_hover_text("Del").clicked() {
                    del = true;
                }
            });
        });
        if close {
            self.selected = None;
        }
        if dup {
            self.duplicate_selected();
        }
        if del {
            self.delete_selected();
        }
    }

    /// Płyn i prędkość — to, co zmienia się najczęściej.
    fn conditions_card(&mut self, ui: &mut egui::Ui) {
        let main = self.main_body().map(|b| (b.ref_length(), b.ref_length_label()));
        card(ui, |ui| {
            card_title(ui, "Warunki przepływu", |_| {});
            let flow = &mut self.scene.flow;
            ui.horizontal(|ui| {
                let mut kind = flow.fluid.kind;
                for k in [FluidKind::Air, FluidKind::Water, FluidKind::Custom] {
                    if chip(ui, kind == k, k.label()).clicked() {
                        kind = k;
                    }
                }
                if kind != flow.fluid.kind {
                    let t = flow.fluid.temperature_c.clamp(0.0, 100.0);
                    flow.fluid = match kind {
                        FluidKind::Air => Fluid::air(t),
                        FluidKind::Water => Fluid::water(t),
                        FluidKind::Custom => Fluid {
                            kind,
                            custom_density: flow.fluid.density(),
                            custom_viscosity: flow.fluid.dynamic_viscosity(),
                            ..flow.fluid.clone()
                        },
                    };
                }
            });
            ui.add_space(2.0);
            egui::Grid::new("cond_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label(RichText::new("Prędkość U∞").strong());
                let speed = (flow.velocity.abs() * 0.01).max(1e-5);
                ui.add(egui::DragValue::new(&mut flow.velocity).speed(speed).range(1e-4..=400.0).max_decimals(4).suffix(" m/s"))
                    .on_hover_text("Przeciągnij lub kliknij dwukrotnie, by wpisać");
                ui.end_row();
                match flow.fluid.kind {
                    FluidKind::Air => {
                        ui.label("Temperatura");
                        ui.add(egui::DragValue::new(&mut flow.fluid.temperature_c).speed(0.5).range(-60.0..=200.0).suffix(" °C"));
                        ui.end_row();
                        ui.label("Ciśnienie");
                        ui.add(egui::DragValue::new(&mut flow.fluid.pressure_kpa).speed(0.5).range(10.0..=500.0).suffix(" kPa"));
                        ui.end_row();
                    }
                    FluidKind::Water => {
                        ui.label("Temperatura");
                        ui.add(egui::DragValue::new(&mut flow.fluid.temperature_c).speed(0.5).range(0.0..=100.0).suffix(" °C"));
                        ui.end_row();
                    }
                    FluidKind::Custom => {
                        ui.label("Gęstość ρ");
                        ui.add(egui::DragValue::new(&mut flow.fluid.custom_density).speed(1.0).range(0.01..=20000.0).suffix(" kg/m³"));
                        ui.end_row();
                        ui.label("Lepkość μ");
                        ui.add(
                            egui::DragValue::new(&mut flow.fluid.custom_viscosity)
                                .speed(1e-5)
                                .range(1e-7..=100.0)
                                .custom_formatter(|v, _| format!("{v:.3e} Pa·s")),
                        );
                        ui.end_row();
                    }
                }
                ui.label("Turbulencja wlotu");
                let mut ti = flow.turbulence * 100.0;
                if ui.add(egui::Slider::new(&mut ti, 0.0..=10.0).suffix(" %").step_by(0.1)).changed() {
                    flow.turbulence = ti / 100.0;
                }
                ui.end_row();
            });
            let units = flow.units();
            let fl = &flow.fluid;
            ui.label(
                RichText::new(format!(
                    "ρ = {} kg/m³ · ν = {} m²/s · q∞ = {} · Ma = {}",
                    fmt_num(fl.density(), 3),
                    fmt_num(fl.kinematic_viscosity(), 3),
                    fmt_si(units.q_inf(), "Pa", 3),
                    fmt_num(units.mach(), 2)
                ))
                .size(11.5)
                .color(if units.mach() > 0.3 { theme::WARN } else { theme::TEXT_DIM }),
            );
            if let Some((l, label)) = main {
                ui.horizontal(|ui| {
                    ui.label(format!("Re ({label}) ="));
                    ui.label(RichText::new(fmt_num(units.reynolds(l).round(), 2)).monospace().strong());
                    ui.add_space(8.0);
                    let sp = self.re_target * 0.01;
                    ui.add(egui::DragValue::new(&mut self.re_target).speed(sp).range(1.0..=1e8).prefix("cel "));
                    if ui.small_button("Ustaw").on_hover_text("Dobiera prędkość strumienia dla zadanej liczby Reynoldsa").clicked() {
                        flow.velocity = self.re_target * units.nu / l.max(1e-12);
                    }
                });
            }
            if let Some(pv) = flow.fluid.vapor_pressure() {
                let sigma = (flow.fluid.pressure_kpa * 1000.0 - pv) / units.q_inf();
                ui.label(
                    RichText::new(format!("Liczba kawitacji σ = {} (kawitacja tam, gdzie Cp < −σ)", fmt_num(sigma, 3)))
                        .size(11.5)
                        .color(theme::TEXT_DIM),
                );
            }
        });
    }

    /// Rzadziej używane ustawienia w zwijanych sekcjach.
    fn advanced(&mut self, ui: &mut egui::Ui) {
        let header = |t: &str| RichText::new(t).strong().size(13.5);
        card(ui, |ui| {
            egui::CollapsingHeader::new(header("Obszar i brzegi"))
                .id_salt("adv_domain")
                .default_open(false)
                .show(ui, |ui| self.domain_section(ui));
        });
        ui.add_space(4.0);
        card(ui, |ui| {
            egui::CollapsingHeader::new(header("Wyświetlanie"))
                .id_salt("adv_view")
                .default_open(false)
                .show(ui, |ui| self.view_section(ui));
        });
        ui.add_space(4.0);
        card(ui, |ui| {
            egui::CollapsingHeader::new(header("Obliczenia"))
                .id_salt("adv_solver")
                .default_open(false)
                .show(ui, |ui| self.solver_section(ui));
        });
    }

    fn domain_section(&mut self, ui: &mut egui::Ui) {
        ui.add(egui::TextEdit::singleline(&mut self.scene.name).hint_text("nazwa sceny").desired_width(f32::INFINITY));
        if !self.scene.description.is_empty() {
            ui.label(RichText::new(&self.scene.description).size(11.5).color(theme::TEXT_DIM));
        }
        let flow = &mut self.scene.flow;
        egui::Grid::new("domain_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label("Szerokość");
            let s = flow.domain[0] * 0.005;
            ui.add(egui::DragValue::new(&mut flow.domain[0]).speed(s).range(1e-4..=1e5).max_decimals(4).suffix(" m"));
            ui.end_row();
            ui.label("Wysokość");
            let s = flow.domain[1] * 0.005;
            ui.add(egui::DragValue::new(&mut flow.domain[1]).speed(s).range(1e-4..=1e5).max_decimals(4).suffix(" m"));
            ui.end_row();
            ui.label("Rozpiętość (3D)");
            ui.add(egui::DragValue::new(&mut flow.span).speed(0.01).range(1e-4..=1e4).suffix(" m"))
                .on_hover_text("Służy tylko do przeliczenia sił 2D (na metr) na siły całkowite.");
            ui.end_row();
            ui.label("Górna krawędź");
            boundary_combo(ui, "top", &mut flow.top, false);
            ui.end_row();
            ui.label("Dolna krawędź");
            boundary_combo(ui, "bottom", &mut flow.bottom, true);
            ui.end_row();
            ui.label("Profil wlotu");
            let mut prof = flow.profile;
            egui::ComboBox::from_id_salt("profile").selected_text(prof.label()).show_ui(ui, |ui| {
                ui.selectable_value(&mut prof, InletProfile::Uniform, InletProfile::Uniform.label());
                ui.selectable_value(&mut prof, InletProfile::Channel, InletProfile::Channel.label());
                let pl = match flow.profile {
                    InletProfile::PowerLaw { alpha } => InletProfile::PowerLaw { alpha },
                    _ => InletProfile::PowerLaw { alpha: 0.2 },
                };
                ui.selectable_value(&mut prof, pl, pl.label());
            });
            flow.profile = prof;
            ui.end_row();
            if let InletProfile::PowerLaw { alpha } = &mut flow.profile {
                ui.label("Wykładnik α");
                ui.add(egui::Slider::new(alpha, 0.08..=0.4).step_by(0.01))
                    .on_hover_text("0,10 — morze, 0,16 — teren otwarty, 0,25 — przedmieścia, 0,35 — centrum miasta");
                ui.end_row();
            }
        });
        flow.domain[0] = flow.domain[0].max(flow.domain[1] * 0.5).min(flow.domain[1] * 8.0);
        if matches!(flow.profile, InletProfile::Channel) && (flow.top == Boundary::FreeStream || flow.bottom == Boundary::FreeStream) {
            ui.label(RichText::new("Profil kanałowy ma sens tylko między ścianami.").size(11.5).color(theme::WARN));
        }
        ui.horizontal(|ui| {
            ui.label("Skaluj całą scenę").on_hover_text("Obszar, ciała, sondy i źródła razem — zmienia liczbę Reynoldsa");
            for (l, k) in [("÷10", 0.1), ("÷2", 0.5), ("×2", 2.0), ("×10", 10.0)] {
                if ui.small_button(l).clicked() {
                    self.scale_scene(k);
                }
            }
        });
    }

    fn scale_scene(&mut self, k: f64) {
        self.scene.flow.domain = [self.scene.flow.domain[0] * k, self.scene.flow.domain[1] * k];
        for b in &mut self.scene.bodies {
            b.pos = [b.pos[0] * k, b.pos[1] * k];
            b.scale(k);
        }
        for p in self.scene.emitters.iter_mut().chain(self.scene.probes.iter_mut()) {
            *p = [p[0] * k, p[1] * k];
        }
        self.view.fit = true;
    }

    fn view_section(&mut self, ui: &mut egui::Ui) {
        let f = self.prefs.field;
        egui::Grid::new("view_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label("Paleta");
            let mut cm = self.prefs.colormap(f);
            egui::ComboBox::from_id_salt("cmap").width(170.0).selected_text(cm.label()).show_ui(ui, |ui| {
                for c in Colormap::ALL {
                    ui.selectable_value(&mut cm, c, c.label());
                }
            });
            if cm != self.prefs.colormap(f) {
                self.prefs.set_colormap(f, cm);
                self.mark_view_dirty();
            }
            ui.end_row();
        });
        let manual = self.prefs.manual_range(f);
        let mut is_manual = manual.is_some();
        if ui.checkbox(&mut is_manual, "Stały zakres skali").changed() {
            self.prefs.set_manual_range(f, if is_manual { Some([self.field_range.0, self.field_range.1]) } else { None });
            self.mark_view_dirty();
        }
        if let Some([mut lo, mut hi]) = manual {
            ui.horizontal(|ui| {
                let sp = ((hi - lo).abs() * 0.01).max(1e-9);
                let c1 = ui.add(egui::DragValue::new(&mut lo).speed(sp).prefix("min ")).changed();
                let c2 = ui.add(egui::DragValue::new(&mut hi).speed(sp).prefix("max ")).changed();
                ui.label(f.unit());
                if c1 || c2 {
                    self.prefs.set_manual_range(f, Some([lo, hi.max(lo + 1e-9)]));
                    self.mark_view_dirty();
                }
            });
        }
        if f.pressure_like() && manual.is_none() {
            ui.label(
                RichText::new("Skala ciśnienia: biel = ciśnienie otoczenia, czerwień = spiętrzenie (Cp = 1), błękit = ssanie.")
                    .size(11.5)
                    .color(theme::TEXT_DIM),
            );
        }
        if f == FieldKind::Smoke {
            let mut rake = self.prefs.smoke_rake;
            if ui.checkbox(&mut rake, "Grzebień dymu na wlocie").changed() {
                self.prefs.smoke_rake = rake;
                self.sim.send(Cmd::Dye { rake, enabled: true });
            }
            if ui.button("Wyczyść dym").clicked() {
                self.sim.send(Cmd::ClearDye);
            }
        }
        section(ui, "Nakładki");
        if self.prefs.particles {
            ui.add(egui::Slider::new(&mut self.prefs.particle_count, 200..=12000).logarithmic(true).text("cząstek"));
            ui.add(egui::Slider::new(&mut self.prefs.trail_len, 1..=40).text("długość śladu"));
            ui.checkbox(&mut self.prefs.particle_speed_color, "Cząstki w kolorze prędkości");
        }
        if self.prefs.streamlines {
            ui.add(egui::Slider::new(&mut self.prefs.streamline_spacing, 4.0..=30.0).text("odstęp linii prądu"));
        }
        if self.prefs.vectors {
            ui.add(egui::Slider::new(&mut self.prefs.vector_spacing, 6..=40).text("odstęp wektorów [komórki]"));
        }
        ui.checkbox(&mut self.prefs.show_probes, "Sondy i ich odczyty");
        ui.checkbox(&mut self.prefs.show_legend, "Legenda kolorów");
        section(ui, "Rysowanie");
        ui.add(egui::Slider::new(&mut self.prefs.brush_cells, 1.0..=20.0).text("pędzel [komórki]"));
        ui.horizontal(|ui| {
            ui.label("Nowe profile: NACA");
            let mut s = self.prefs.naca.clone();
            if ui.add(egui::TextEdit::singleline(&mut s).desired_width(60.0)).changed() {
                self.prefs.naca = s;
            }
            if Naca::parse(&self.prefs.naca).is_none() {
                ui.label(RichText::new("niepoprawny").color(theme::BAD).size(11.0));
            }
        });
    }

    fn solver_section(&mut self, ui: &mut egui::Ui) {
        section(ui, "Sprzęt");
        let gpu = self.frame.as_ref().and_then(|f| f.gpu.clone());
        let t = match &gpu {
            Some(g) => format!("Karta graficzna: {g}"),
            None => "Procesor (karta graficzna niedostępna)".to_string(),
        };
        ui.label(RichText::new(t).size(11.5).color(theme::TEXT_DIM));
        if let Some(f) = &self.frame {
            ui.label(
                RichText::new(format!("{} MLUPS · {} kroków na klatkę", fmt_num(f.mlups, 0), f.steps_per_frame))
                    .size(11.5)
                    .color(theme::TEXT_DIM),
            );
        }

        section(ui, "Rozdzielczość");
        let flow = &mut self.scene.flow;
        ui.horizontal_wrapped(|ui| {
            for (l, r) in [("Podgląd", 160), ("Standard", 280), ("Dokładna", 420), ("Bardzo dokładna", 600), ("Maksymalna", 900)] {
                if chip(ui, flow.resolution == r, l).clicked() {
                    flow.resolution = r;
                }
            }
        });
        ui.add(egui::Slider::new(&mut flow.resolution, 80..=1200).text("komórek na wysokość").step_by(10.0));
        let (nx, ny) = flow.grid();
        ui.label(
            RichText::new(format!("{nx} × {ny} = {} komórek (bez marginesów)", fmt_num((nx * ny) as f64, 0)))
                .size(11.5)
                .color(theme::TEXT_DIM),
        );
        if gpu.is_none() && flow.resolution > 420 {
            ui.label(
                RichText::new("Na procesorze tak gęsta siatka liczy się wolno — włącz GPU, jeśli jest dostępne.")
                    .size(11.5)
                    .color(theme::WARN),
            );
        }
        if flow.effective_resolution() < flow.resolution {
            ui.label(
                RichText::new("Rozdzielczość ograniczona limitem 3 mln komórek dla tak szerokiego obszaru.").size(11.5).color(theme::WARN),
            );
        }

        section(ui, "Parametry modelu");
        egui::Grid::new("solver_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label("Prędkość siatki u_lb");
            ui.add(egui::Slider::new(&mut flow.u_lattice, 0.02..=0.12).step_by(0.005)).on_hover_text(
                "Większa = szybsze liczenie w czasie fizycznym, ale większy błąd ściśliwości (∝ Ma²) i mniejsza stabilność.",
            );
            ui.end_row();
            ui.label("Stała Smagorinsky'ego");
            ui.add(egui::Slider::new(&mut flow.smagorinsky, 0.0..=0.3).step_by(0.01))
                .on_hover_text("Model podsiatkowy LES. 0 = bez modelu (tylko dla przepływów w pełni rozdzielczych).");
            ui.end_row();
        });

        section(ui, "Tempo");
        let mut s = self.prefs.speed;
        ui.horizontal(|ui| {
            ui.radio_value(&mut s, SpeedPref::Auto, "Płynne")
                .on_hover_text("Strumień przechodzi przez widok w ok. 8 s — to samo tempo na każdym komputerze, wygodne do oglądania.");
            ui.radio_value(&mut s, SpeedPref::Max, "Maksymalne")
                .on_hover_text("Tak szybko, jak pozwala sprzęt — szybsze ustalanie wyników.");
            let fixed = match s {
                SpeedPref::Fixed(n) => n,
                _ => 8,
            };
            ui.radio_value(&mut s, SpeedPref::Fixed(fixed), "Stałe");
        });
        if let SpeedPref::Fixed(mut n) = s {
            ui.add(egui::Slider::new(&mut n, 1..=200).text("kroków na klatkę"));
            s = SpeedPref::Fixed(n);
        }
        if s != self.prefs.speed {
            self.set_speed(s);
        }

        section(ui, "Uśrednianie w czasie");
        let mut avg = self.averaging;
        ui.horizontal(|ui| {
            if ui.checkbox(&mut avg, "Zbieraj pola średnie").changed() {
                self.set_averaging(avg);
            }
            if ui.small_button("od nowa").clicked() {
                self.sim.send(Cmd::ResetAveraging);
            }
        });
        if let Some(m) = self.frame.as_ref().and_then(|f| f.mean.as_ref()) {
            ui.label(RichText::new(format!("{} próbek, {}", m.samples, fmt_si(m.span_s, "s", 2))).size(11.5).color(theme::TEXT_DIM));
        }

        let units = self.scene.flow.units();
        if let Some(b) = self.main_body() {
            let l = b.ref_length();
            let re = units.reynolds(l);
            let a = assess(&units, l / units.dx, re);
            section(ui, "Wiarygodność");
            ui.label(RichText::new(a.fidelity.label()).strong().color(fidelity_color(a.fidelity)));
            ui.label(RichText::new(format!("Warstwa przyścienna ≈ {:.1} komórki", a.bl_cells)).size(11.5).color(theme::TEXT_DIM));
            for n in &a.notes {
                ui.label(RichText::new(format!("• {n}")).size(11.5).color(theme::TEXT_DIM));
            }
        }
        section(ui, "Jednostki siatki");
        egui::Grid::new("lattice_info").num_columns(2).spacing([10.0, 3.0]).show(ui, |ui| {
            let mono = |s: String| RichText::new(s).monospace();
            ui.label("Δx / Δt");
            ui.label(mono(format!("{} / {}", fmt_si(units.dx, "m", 3), fmt_si(units.dt, "s", 3))));
            ui.end_row();
            ui.label("τ");
            let tau_eff = 3.0 * units.nu_lb_effective() + 0.5;
            let t = if tau_eff > units.tau() + 1e-9 {
                format!("{:.5} → {:.4} (min.)", units.tau(), tau_eff)
            } else {
                format!("{:.5}", units.tau())
            };
            ui.label(mono(t));
            ui.end_row();
            ui.label("Ma siatki");
            ui.label(mono(fmt_num(units.mach_lb(), 3)));
            ui.end_row();
        });
        ui.add_space(6.0);
        if ui.button("Model fizyczny i ograniczenia…").clicked() {
            self.win.physics = true;
        }
    }
}

fn boundary_combo(ui: &mut egui::Ui, id: &str, b: &mut Boundary, bottom: bool) {
    egui::ComboBox::from_id_salt(id).selected_text(b.label()).show_ui(ui, |ui| {
        ui.selectable_value(b, Boundary::FreeStream, Boundary::FreeStream.label());
        ui.selectable_value(b, Boundary::NoSlipWall, Boundary::NoSlipWall.label());
        ui.selectable_value(b, Boundary::SlipWall, Boundary::SlipWall.label());
        if bottom {
            ui.selectable_value(b, Boundary::MovingGround, Boundary::MovingGround.label());
        }
    });
}

/// Ikony narzędzi rysowane wektorowo (niezależne od czcionek).
fn tool_icon(p: &egui::Painter, r: egui::Rect, t: Tool, c: Color32) {
    use egui::{pos2, Pos2, Shape as S, Stroke};
    let st = Stroke::new(1.6_f32, c);
    let at = |x: f32, y: f32| -> Pos2 { pos2(r.left() + x * r.width(), r.top() + y * r.height()) };
    match t {
        Tool::Select => {
            let pts = vec![at(0.15, 0.0), at(0.15, 0.85), at(0.38, 0.64), at(0.55, 1.0), at(0.7, 0.93), at(0.53, 0.58), at(0.85, 0.58)];
            p.add(S::convex_polygon(pts[..3].to_vec(), c, Stroke::NONE));
            p.add(S::closed_line(pts, st));
        }
        Tool::Airfoil => {
            let n = crate::physics::geometry::Naca::new(0.04, 0.4, 0.15).outline(24);
            let a = -0.25f32;
            let pts: Vec<Pos2> = n
                .iter()
                .map(|q| {
                    let (x, y) = (q[0] as f32 - 0.5, q[1] as f32);
                    let (xr, yr) = (x * a.cos() - y * a.sin(), x * a.sin() + y * a.cos());
                    at(0.5 + xr, 0.55 - yr)
                })
                .collect();
            let wp: Vec<[f64; 2]> = pts.iter().map(|q| [q.x as f64, q.y as f64]).collect();
            let mut mesh = egui::epaint::Mesh::default();
            for q in &pts {
                mesh.colored_vertex(*q, c);
            }
            for tr in crate::physics::geometry::triangulate(&wp) {
                mesh.add_triangle(tr[0] as u32, tr[1] as u32, tr[2] as u32);
            }
            p.add(S::mesh(mesh));
        }
        Tool::Circle => {
            p.circle_stroke(r.center(), r.width() * 0.45, st);
        }
        Tool::Rect => {
            p.rect_stroke(egui::Rect::from_min_max(at(0.0, 0.2), at(1.0, 0.8)), 1.0, st, egui::StrokeKind::Middle);
        }
        Tool::Ellipse => {
            let pts = (0..32)
                .map(|k| {
                    let a = std::f32::consts::TAU * k as f32 / 32.0;
                    at(0.5 + 0.5 * a.cos(), 0.5 + 0.3 * a.sin())
                })
                .collect();
            p.add(S::closed_line(pts, st));
        }
        Tool::Polygon => {
            let pts = vec![at(0.05, 0.6), at(0.25, 0.1), at(0.6, 0.25), at(0.95, 0.05), at(0.8, 0.7), at(0.45, 0.95)];
            p.add(S::closed_line(pts, st));
        }
        Tool::Brush => {
            let pts: Vec<Pos2> = (0..=16)
                .map(|k| {
                    let x = k as f32 / 16.0;
                    at(x, 0.5 + 0.3 * (x * 6.0).sin())
                })
                .collect();
            p.add(S::line(pts, Stroke::new(3.2_f32, c)));
        }
        Tool::Eraser => {
            let pts = vec![at(0.05, 0.62), at(0.5, 0.12), at(0.95, 0.5), at(0.52, 1.0)];
            p.add(S::convex_polygon(vec![at(0.05, 0.62), at(0.28, 0.37), at(0.73, 0.75), at(0.52, 1.0)], c, Stroke::NONE));
            p.add(S::closed_line(pts, st));
        }
        Tool::Emitter => {
            for k in 0..3 {
                let y0 = 0.2 + 0.3 * k as f32;
                let pts: Vec<Pos2> = (0..=12)
                    .map(|j| {
                        let x = j as f32 / 12.0;
                        at(x, y0 + 0.08 * (x * 9.0 + k as f32).sin())
                    })
                    .collect();
                p.add(S::line(pts, st));
            }
            p.circle_filled(at(0.0, 0.5), 2.5, c);
        }
        Tool::Probe => {
            p.circle_stroke(r.center(), r.width() * 0.3, st);
            p.line_segment([at(0.5, 0.0), at(0.5, 1.0)], st);
            p.line_segment([at(0.0, 0.5), at(1.0, 0.5)], st);
        }
    }
}
