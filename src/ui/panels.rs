//! Menu, pasek narzędzi, pasek stanu i panel boczny z ustawieniami.

use eframe::egui::{self, Color32, RichText};

use crate::app::{App, SideTab, Tool};
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

impl App {
    pub fn menu_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu").frame(egui::Frame::NONE.fill(theme::PANEL).inner_margin(egui::Margin::symmetric(8, 4))).show(
            ctx,
            |ui| {
                egui::MenuBar::new().ui(ui, |ui| {
                    ui.label(RichText::new("◆ AeroFlow").strong().color(theme::ACCENT));
                    ui.add_space(6.0);
                    ui.menu_button("Plik", |ui| {
                        if ui.button("Nowa pusta scena          Ctrl+N").clicked() {
                            self.load_preset("empty");
                            ui.close();
                        }
                        ui.menu_button("Scenariusze", |ui| self.preset_menu(ui));
                        ui.separator();
                        if ui.button("Otwórz…                         Ctrl+O").clicked() {
                            ui.close();
                            self.open_dialog();
                        }
                        let recent = self.prefs.recent_files.clone();
                        ui.add_enabled_ui(!recent.is_empty(), |ui| {
                            ui.menu_button("Ostatnie pliki", |ui| {
                                for p in recent {
                                    if ui.button(&p).clicked() {
                                        ui.close();
                                        self.open_path(p.into());
                                    }
                                }
                            });
                        });
                        if ui.button("Zapisz                            Ctrl+S").clicked() {
                            ui.close();
                            self.save(false);
                        }
                        if ui.button("Zapisz jako…            Ctrl+Shift+S").clicked() {
                            ui.close();
                            self.save(true);
                        }
                        ui.separator();
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
                        ui.separator();
                        if ui.button("Zakończ").clicked() {
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
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
                    ui.menu_button("Widok", |ui| {
                        ui.checkbox(&mut self.prefs.show_side, "Panel ustawień       S");
                        ui.checkbox(&mut self.prefs.show_bottom, "Wykresy                G");
                        ui.checkbox(&mut self.prefs.show_hud, "Informacje na widoku");
                        ui.checkbox(&mut self.prefs.show_legend, "Legenda");
                        ui.separator();
                        if ui.button("Dopasuj widok          Home").clicked() {
                            self.view.fit = true;
                            ui.close();
                        }
                        if ui.button("Ukryj interfejs          H").clicked() {
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
                    ui.menu_button("Symulacja", |ui| {
                        let label = if self.paused { "Wznów                 Spacja" } else { "Wstrzymaj           Spacja" };
                        if ui.button(label).clicked() {
                            self.set_paused(!self.paused);
                            ui.close();
                        }
                        if ui.button("Jeden krok              .").clicked() {
                            self.sim.send(Cmd::StepOnce);
                            ui.close();
                        }
                        if ui.button("Zresetuj przepływ    R").clicked() {
                            self.reset_flow();
                            ui.close();
                        }
                        ui.separator();
                        let mut avg = self.averaging;
                        if ui.checkbox(&mut avg, "Uśrednianie w czasie").changed() {
                            self.set_averaging(avg);
                        }
                        if ui.button("Biegunowa profilu…").clicked() {
                            self.chart_tab = crate::app::ChartTab::Polar;
                            self.prefs.show_bottom = true;
                            ui.close();
                        }
                    });
                    ui.menu_button("Pomoc", |ui| {
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
                        ui.separator();
                        if ui.button("O programie").clicked() {
                            self.win.about = true;
                            ui.close();
                        }
                    });

                    ui.separator();
                    // sterowanie symulacją
                    let (icon, tip) = if self.paused { ("▶", "Wznów (Spacja)") } else { ("⏸", "Wstrzymaj (Spacja)") };
                    if ui.button(RichText::new(icon).size(15.0)).on_hover_text(tip).clicked() {
                        self.set_paused(!self.paused);
                    }
                    if ui.button(RichText::new("⏭").size(15.0)).on_hover_text("Jeden krok (.)").clicked() {
                        self.sim.send(Cmd::StepOnce);
                    }
                    if ui.button(RichText::new("⟲").size(15.0)).on_hover_text("Zresetuj przepływ (R)").clicked() {
                        self.reset_flow();
                    }
                    ui.separator();
                    let mut field = self.prefs.field;
                    egui::ComboBox::from_id_salt("field").width(190.0).selected_text(field.label()).show_ui(ui, |ui| {
                        for f in FieldKind::ALL {
                            ui.selectable_value(&mut field, f, f.label());
                        }
                    });
                    if field != self.prefs.field {
                        self.set_field(field);
                    }
                    if ui.selectable_label(self.prefs.particles, "Cząstki").on_hover_text("Cząstki-znaczniki (P)").clicked() {
                        self.prefs.particles = !self.prefs.particles;
                    }
                    if ui.selectable_label(self.prefs.streamlines, "Linie prądu").on_hover_text("Linie prądu (L)").clicked() {
                        self.prefs.streamlines = !self.prefs.streamlines;
                    }
                    if ui.selectable_label(self.prefs.vectors, "Wektory").clicked() {
                        self.prefs.vectors = !self.prefs.vectors;
                    }
                    ui.separator();
                    ui.menu_button("📂 Scenariusze", |ui| self.preset_menu(ui));

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.selectable_label(self.prefs.show_side, "☰").on_hover_text("Panel ustawień (S)").clicked() {
                            self.prefs.show_side = !self.prefs.show_side;
                        }
                        if let State::Available(r) = self.updater.state() {
                            if ui.button(RichText::new(format!("⬆ Aktualizacja {}", r.version)).color(theme::GOOD)).clicked() {
                                self.win.update = true;
                            }
                        }
                        let title = match &self.file_path {
                            Some(p) => p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
                            None => self.scene.name.clone(),
                        };
                        let star = if self.file_path.is_some() && self.modified() { " •" } else { "" };
                        ui.label(RichText::new(format!("{title}{star}")).color(theme::TEXT_DIM));
                    });
                });
            },
        );
    }

    fn preset_menu(&mut self, ui: &mut egui::Ui) {
        let mut group = "";
        for p in presets() {
            if p.group != group {
                if !group.is_empty() {
                    ui.separator();
                }
                group = p.group;
                ui.label(RichText::new(group).small().color(theme::TEXT_DIM));
            }
            if ui.button(p.name).clicked() {
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

    pub fn status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status")
            .frame(egui::Frame::NONE.fill(theme::PANEL).inner_margin(egui::Margin::symmetric(10, 3)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let small = |s: String| RichText::new(s).size(11.5).color(theme::TEXT_DIM);
                    let flow = &self.scene.flow;
                    let (nx, ny) = flow.grid();
                    let units = flow.units();
                    ui.label(small(format!("Siatka {nx}×{ny}")));
                    ui.label(small(format!("Δx = {}", fmt_si(units.dx, "m", 2))));
                    ui.label(small(format!("Δt = {}", fmt_si(units.dt, "s", 2))));
                    if let Some(f) = &self.frame {
                        ui.label(small(format!("{:.0} MLUPS", f.mlups)));
                        ui.label(small(format!("{} kroków/klatkę", f.steps_per_frame)));
                        let r = f.realtime_ratio;
                        let rt = if r >= 1.0 {
                            format!("{}× szybciej niż w rzeczywistości", fmt_num(r, 2))
                        } else {
                            format!("{}× wolniej niż w rzeczywistości", fmt_num(1.0 / r.max(1e-12), 2))
                        };
                        if !self.paused && r > 0.0 {
                            ui.label(small(rt));
                        }
                    }
                    ui.label(small(format!("{:.0} FPS", self.fps)));
                    if let Some(sw) = &self.sweep {
                        if !sw.done() {
                            ui.add(
                                egui::ProgressBar::new(sw.progress())
                                    .desired_width(120.0)
                                    .text(format!("biegunowa α = {:.1}°", sw.current_angle().unwrap_or(0.0))),
                            );
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(small(format!("v{}", update::VERSION)));
                        if let Some(w) = self.hover {
                            ui.label(small(format!("x {}  y {}", fmt_si(w[0], "m", 3), fmt_si(w[1], "m", 3))));
                        }
                    });
                });
            });
    }

    pub fn tool_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("tools")
            .exact_width(46.0)
            .resizable(false)
            .frame(egui::Frame::NONE.fill(theme::PANEL).inner_margin(egui::Margin::symmetric(5, 8)))
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    for t in Tool::ALL {
                        let sel = self.tool == t;
                        let (rect, resp) = ui.allocate_exact_size(egui::vec2(36.0, 34.0), egui::Sense::click());
                        let bg = if sel {
                            theme::ACCENT_DIM
                        } else if resp.hovered() {
                            Color32::from_rgb(34, 42, 56)
                        } else {
                            theme::PANEL_2
                        };
                        ui.painter().rect_filled(rect, 6.0, bg);
                        let col = if sel {
                            Color32::WHITE
                        } else if resp.hovered() {
                            theme::TEXT
                        } else {
                            Color32::from_rgb(170, 182, 200)
                        };
                        tool_icon(ui.painter(), rect.shrink(9.0), t, col);
                        if resp.on_hover_text(format!("{}  ({})\n{}", t.label(), t.key(), t.hint())).clicked() {
                            self.tool = t;
                        }
                        if matches!(t, Tool::Select | Tool::Polygon | Tool::Eraser) {
                            ui.add_space(4.0);
                            ui.separator();
                            ui.add_space(4.0);
                        }
                    }
                });
            });
    }

    pub fn side_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("side").default_width(330.0).min_width(280.0).resizable(true).show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                for (t, l) in
                    [(SideTab::Flow, "Przepływ"), (SideTab::Objects, "Obiekty"), (SideTab::View, "Widok"), (SideTab::Solver, "Solver")]
                {
                    if ui.selectable_label(self.side_tab == t, l).clicked() {
                        self.side_tab = t;
                    }
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                match self.side_tab {
                    SideTab::Flow => self.flow_tab(ui),
                    SideTab::Objects => self.objects_tab(ui),
                    SideTab::View => self.view_tab(ui),
                    SideTab::Solver => self.solver_tab(ui),
                }
                ui.add_space(20.0);
            });
        });
    }

    fn flow_tab(&mut self, ui: &mut egui::Ui) {
        section(ui, "Scena");
        ui.add(egui::TextEdit::singleline(&mut self.scene.name).desired_width(f32::INFINITY));
        if !self.scene.description.is_empty() {
            ui.label(RichText::new(&self.scene.description).size(12.0).color(theme::TEXT_DIM));
        }

        section(ui, "Płyn");
        let flow = &mut self.scene.flow;
        let mut kind = flow.fluid.kind;
        egui::ComboBox::from_id_salt("fluid").selected_text(kind.label()).show_ui(ui, |ui| {
            for k in [FluidKind::Air, FluidKind::Water, FluidKind::Custom] {
                ui.selectable_value(&mut kind, k, k.label());
            }
        });
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
        egui::Grid::new("fluid_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| match flow.fluid.kind {
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
        });
        let fl = &flow.fluid;
        ui.label(
            RichText::new(format!(
                "ρ = {} kg/m³ · μ = {} Pa·s · ν = {} m²/s · c = {} m/s",
                fmt_num(fl.density(), 3),
                fmt_num(fl.dynamic_viscosity(), 3),
                fmt_num(fl.kinematic_viscosity(), 3),
                fmt_num(fl.speed_of_sound(), 0)
            ))
            .size(11.5)
            .color(theme::TEXT_DIM),
        );

        section(ui, "Strumień");
        egui::Grid::new("stream_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label("Prędkość U∞");
            let speed = (flow.velocity.abs() * 0.01).max(1e-5);
            ui.add(egui::DragValue::new(&mut flow.velocity).speed(speed).range(1e-4..=400.0).max_decimals(4).suffix(" m/s"));
            ui.end_row();
            ui.label("Turbulencja wlotu");
            let mut ti = flow.turbulence * 100.0;
            if ui.add(egui::Slider::new(&mut ti, 0.0..=10.0).suffix(" %").step_by(0.1)).changed() {
                flow.turbulence = ti / 100.0;
            }
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
            ui.label("Górna krawędź");
            boundary_combo(ui, "top", &mut flow.top, false);
            ui.end_row();
            ui.label("Dolna krawędź");
            boundary_combo(ui, "bottom", &mut flow.bottom, true);
            ui.end_row();
        });
        if matches!(flow.profile, InletProfile::Channel) && (flow.top == Boundary::FreeStream || flow.bottom == Boundary::FreeStream) {
            ui.label(RichText::new("Profil kanałowy ma sens tylko między ścianami.").size(11.5).color(theme::WARN));
        }

        section(ui, "Obszar");
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
        });
        flow.domain[0] = flow.domain[0].max(flow.domain[1] * 0.5).min(flow.domain[1] * 8.0);
        ui.horizontal(|ui| {
            ui.label("Skaluj całą scenę").on_hover_text("Obszar, ciała, sondy i źródła razem — zmienia liczbę Reynoldsa");
            for (l, k) in [("÷10", 0.1), ("÷2", 0.5), ("×2", 2.0), ("×10", 10.0)] {
                if ui.small_button(l).clicked() {
                    self.scale_scene(k);
                }
            }
        });

        // liczby podobieństwa
        section(ui, "Liczby podobieństwa");
        let units = self.scene.flow.units();
        if let Some(b) = self.main_body() {
            let l = b.ref_length();
            let re = units.reynolds(l);
            egui::Grid::new("sim_grid").num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
                ui.label(format!("Re ({}, {})", b.name, b.ref_length_label()));
                ui.label(RichText::new(fmt_num(re, 3)).monospace().strong());
                ui.end_row();
                ui.label(b.ref_length_label());
                ui.label(RichText::new(fmt_si(l, "m", 3)).monospace());
                ui.end_row();
                ui.label("Ma (fizyczna)");
                let ma = units.mach();
                ui.label(RichText::new(fmt_num(ma, 3)).monospace().color(if ma > 0.3 { theme::WARN } else { theme::TEXT }));
                ui.end_row();
                ui.label("q∞ = ½ρU²");
                ui.label(RichText::new(fmt_si(units.q_inf(), "Pa", 3)).monospace());
                ui.end_row();
                if let Some(pv) = self.scene.flow.fluid.vapor_pressure() {
                    let sigma = (self.scene.flow.fluid.pressure_kpa * 1000.0 - pv) / units.q_inf();
                    ui.label("Liczba kawitacji σ");
                    ui.label(RichText::new(fmt_num(sigma, 3)).monospace()).on_hover_text("Kawitacja grozi tam, gdzie Cp < −σ");
                    ui.end_row();
                }
            });
            ui.horizontal(|ui| {
                ui.label("Ustaw Re =");
                let sp = self.re_target * 0.01;
                ui.add(egui::DragValue::new(&mut self.re_target).speed(sp).range(1.0..=1e8));
                if ui.button("Zastosuj").on_hover_text("Dobiera prędkość strumienia dla zadanej liczby Reynoldsa").clicked() {
                    self.scene.flow.velocity = self.re_target * units.nu / l.max(1e-12);
                }
            });
        } else {
            ui.label(RichText::new("Dodaj ciało, aby zobaczyć liczbę Reynoldsa.").color(theme::TEXT_DIM));
        }
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

    fn objects_tab(&mut self, ui: &mut egui::Ui) {
        section(ui, "Obiekty w scenie");
        if self.scene.bodies.is_empty() {
            ui.label(RichText::new("Brak obiektów — narysuj je narzędziami z lewej.").color(theme::TEXT_DIM));
        }
        let mut action: Option<(u64, &str)> = None;
        for b in &self.scene.bodies {
            ui.horizontal(|ui| {
                let sel = self.selected == Some(b.id);
                let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                ui.painter().circle_filled(rect.center(), 4.0, crate::app::body_color(b.id));
                let label = format!("{}  ·  {}", b.name, b.shape.kind_label());
                if ui.selectable_label(sel, label).clicked() {
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
        match action {
            Some((id, "select")) => {
                self.selected = Some(id);
                self.chart_body = Some(id);
            }
            Some((id, "delete")) => {
                self.scene.bodies.retain(|b| b.id != id);
                if self.selected == Some(id) {
                    self.selected = None;
                }
            }
            Some((id, "lock")) => {
                if let Some(b) = self.scene.body_mut(id) {
                    b.locked = !b.locked;
                }
            }
            _ => {}
        }

        let Some(id) = self.selected else {
            ui.add_space(8.0);
            ui.label(RichText::new("Zaznacz obiekt, by edytować jego parametry.").color(theme::TEXT_DIM));
            return;
        };
        let units = self.scene.flow.units();
        let domain = self.scene.flow.domain;
        let Some(b) = self.scene.body_mut(id) else { return };
        section(ui, "Właściwości");
        egui::Grid::new("body_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label("Nazwa");
            ui.add(egui::TextEdit::singleline(&mut b.name).desired_width(170.0));
            ui.end_row();
            let sp = domain[1] * 0.002;
            ui.label("Położenie x");
            ui.add(egui::DragValue::new(&mut b.pos[0]).speed(sp).max_decimals(4).suffix(" m"));
            ui.end_row();
            ui.label("Położenie y");
            ui.add(egui::DragValue::new(&mut b.pos[1]).speed(sp).max_decimals(4).suffix(" m"));
            ui.end_row();
            if b.rotatable() {
                ui.label(if matches!(b.shape, Shape::Airfoil { .. }) { "Kąt natarcia α" } else { "Obrót" });
                ui.add(egui::DragValue::new(&mut b.angle).speed(0.1).range(-180.0..=180.0).max_decimals(2).suffix("°"));
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
                    ui.label("Położenie wygięcia p");
                    let mut p = naca.p * 10.0;
                    if ui.add(egui::Slider::new(&mut p, 1.0..=9.0).step_by(1.0).custom_formatter(|v, _| format!("{:.0}0 %", v))).changed() {
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
                        egui::DragValue::new(a).speed(domain[1] * 0.001).range(domain[1] * 0.003..=domain[0]).max_decimals(5).suffix(" m"),
                    );
                    ui.end_row();
                    ui.label("Półoś b");
                    ui.add(
                        egui::DragValue::new(bb).speed(domain[1] * 0.001).range(domain[1] * 0.003..=domain[0]).max_decimals(5).suffix(" m"),
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
        let re = units.reynolds(l);
        let a = assess(&units, l / units.dx, re);
        section(ui, "Charakterystyka");
        egui::Grid::new("body_info").num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
            ui.label(b.ref_length_label());
            ui.label(RichText::new(format!("{}  ({:.0} komórek)", fmt_si(l, "m", 3), l / units.dx)).monospace());
            ui.end_row();
            ui.label("Re");
            ui.label(RichText::new(fmt_num(re, 3)).monospace());
            ui.end_row();
            ui.label("Blokada obszaru");
            let bl = frontal / domain[1];
            ui.label(RichText::new(format!("{:.1} %", bl * 100.0)).monospace().color(if bl > 0.1 { theme::WARN } else { theme::TEXT }))
                .on_hover_text("Stosunek wysokości czołowej do wysokości obszaru. Powyżej ~10% ściany/brzegi zawyżają opór.");
            ui.end_row();
            ui.label("Wiarygodność");
            ui.label(RichText::new(a.fidelity.label()).color(fidelity_color(a.fidelity)));
            ui.end_row();
        });
        for n in &a.notes {
            ui.label(RichText::new(format!("• {n}")).size(11.5).color(theme::TEXT_DIM));
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.button("🗐 Duplikuj").clicked() {
                self.duplicate_selected();
            }
            if ui.button(RichText::new("🗑 Usuń").color(theme::BAD)).clicked() {
                self.delete_selected();
            }
        });
    }

    fn view_tab(&mut self, ui: &mut egui::Ui) {
        section(ui, "Pole");
        let mut field = self.prefs.field;
        egui::ComboBox::from_id_salt("field2").width(230.0).selected_text(field.label()).show_ui(ui, |ui| {
            for f in FieldKind::ALL {
                ui.selectable_value(&mut field, f, f.label());
            }
        });
        if field != self.prefs.field {
            self.set_field(field);
        }
        let f = self.prefs.field;
        let mut cm = self.prefs.colormap(f);
        egui::ComboBox::from_id_salt("cmap").width(230.0).selected_text(cm.label()).show_ui(ui, |ui| {
            for c in Colormap::ALL {
                ui.selectable_value(&mut cm, c, c.label());
            }
        });
        if cm != self.prefs.colormap(f) {
            self.prefs.set_colormap(f, cm);
            self.mark_view_dirty();
        }
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
        if f == FieldKind::Smoke {
            let mut rake = self.prefs.smoke_rake;
            if ui.checkbox(&mut rake, "Grzebień dymu na wlocie").changed() {
                self.prefs.smoke_rake = rake;
                self.sim.send(Cmd::Dye { rake, enabled: true });
            }
            if ui.button("Wyczyść dym").clicked() {
                self.sim.send(Cmd::ClearDye);
            }
            ui.label(RichText::new("Źródła dymu dodasz narzędziem ☁ (klawisz 9).").size(11.5).color(theme::TEXT_DIM));
        }
        if f.needs_mean() {
            ui.label(RichText::new("Pole średnie wymaga uśredniania w czasie (zakładka Solver).").size(11.5).color(theme::TEXT_DIM));
        }

        section(ui, "Nakładki");
        ui.checkbox(&mut self.prefs.particles, "Cząstki-znaczniki");
        if self.prefs.particles {
            ui.indent("p", |ui| {
                ui.add(egui::Slider::new(&mut self.prefs.particle_count, 200..=12000).logarithmic(true).text("liczba"));
                ui.add(egui::Slider::new(&mut self.prefs.trail_len, 1..=40).text("długość śladu"));
                ui.checkbox(&mut self.prefs.particle_speed_color, "Kolor wg prędkości");
            });
        }
        ui.checkbox(&mut self.prefs.streamlines, "Linie prądu");
        if self.prefs.streamlines {
            ui.indent("s", |ui| {
                ui.add(egui::Slider::new(&mut self.prefs.streamline_spacing, 4.0..=30.0).text("odstęp"));
            });
        }
        ui.checkbox(&mut self.prefs.vectors, "Wektory prędkości");
        if self.prefs.vectors {
            ui.indent("v", |ui| {
                ui.add(egui::Slider::new(&mut self.prefs.vector_spacing, 6..=40).text("odstęp [komórki]"));
            });
        }
        ui.checkbox(&mut self.prefs.show_forces, "Wektory sił na ciałach (D, L)");
        ui.checkbox(&mut self.prefs.show_probes, "Sondy i ich odczyty");
        ui.checkbox(&mut self.prefs.show_hud, "Informacje w rogu widoku");
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

    fn solver_tab(&mut self, ui: &mut egui::Ui) {
        section(ui, "Rozdzielczość");
        let flow = &mut self.scene.flow;
        ui.add(egui::Slider::new(&mut flow.resolution, 80..=900).text("komórek na wysokość").step_by(10.0));
        ui.horizontal(|ui| {
            for (l, r) in [("Podgląd", 160), ("Standard", 280), ("Dokładna", 420), ("Bardzo dokładna", 600)] {
                if ui.small_button(l).clicked() {
                    flow.resolution = r;
                }
            }
        });
        let (nx, ny) = flow.grid();
        ui.label(
            RichText::new(format!("{nx} × {ny} = {} komórek (bez marginesów)", fmt_num((nx * ny) as f64, 0)))
                .size(11.5)
                .color(theme::TEXT_DIM),
        );
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

        section(ui, "Tempo obliczeń");
        let mut s = self.prefs.speed;
        ui.horizontal(|ui| {
            ui.radio_value(&mut s, SpeedPref::Auto, "Maksymalne");
            let fixed = match s {
                SpeedPref::Fixed(n) => n,
                _ => 8,
            };
            ui.radio_value(&mut s, SpeedPref::Fixed(fixed), "Stałe");
        });
        if let SpeedPref::Fixed(mut n) = s {
            ui.add(egui::Slider::new(&mut n, 1..=100).text("kroków na klatkę"));
            s = SpeedPref::Fixed(n);
        }
        if s != self.prefs.speed {
            self.set_speed(s);
        }

        section(ui, "Uśrednianie w czasie");
        let mut avg = self.averaging;
        if ui.checkbox(&mut avg, "Zbieraj pola średnie").changed() {
            self.set_averaging(avg);
        }
        if let Some(m) = self.frame.as_ref().and_then(|f| f.mean.as_ref()) {
            ui.label(RichText::new(format!("{} próbek, {}", m.samples, fmt_si(m.span_s, "s", 2))).size(11.5).color(theme::TEXT_DIM));
        }
        if ui.button("Zacznij od nowa").clicked() {
            self.sim.send(Cmd::ResetAveraging);
        }

        section(ui, "Jednostki siatki");
        let units = self.scene.flow.units();
        egui::Grid::new("lattice_info").num_columns(2).spacing([10.0, 3.0]).show(ui, |ui| {
            let mono = |s: String| RichText::new(s).monospace();
            ui.label("Δx");
            ui.label(mono(fmt_si(units.dx, "m", 3)));
            ui.end_row();
            ui.label("Δt");
            ui.label(mono(fmt_si(units.dt, "s", 3)));
            ui.end_row();
            ui.label("ν siatki");
            ui.label(mono(fmt_num(units.nu_lb, 3)));
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
            ui.label("Re komórki");
            ui.label(mono(fmt_num(units.u_lb / units.nu_lb, 3)));
            ui.end_row();
        });
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
        ui.add_space(8.0);
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
