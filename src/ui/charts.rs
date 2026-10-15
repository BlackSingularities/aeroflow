//! Dolny panel z wykresami: siły w czasie, rozkład Cp, biegunowa, sondy.

use eframe::egui::{self, Color32, RichText};
use egui_plot::{HLine, Legend, Line, Plot, PlotPoints, Points};

use crate::analysis::{spectrum, stats, surface_curve, Sweep};
use crate::app::{App, ChartTab};
use crate::physics::geometry::Shape;
use crate::render::field::{fmt_num, fmt_si};
use crate::ui::theme;
use crate::ui::viewport::probe_color;

impl App {
    pub fn chart_panel(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("charts").resizable(true).default_height(250.0).min_height(160.0).show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                for (t, l) in [
                    (ChartTab::Forces, "Siły i współczynniki"),
                    (ChartTab::Surface, "Rozkład Cp"),
                    (ChartTab::Polar, "Biegunowa"),
                    (ChartTab::Probes, "Sondy"),
                ] {
                    if ui.selectable_label(self.chart_tab == t, l).clicked() {
                        self.chart_tab = t;
                    }
                }
                ui.separator();
                // wybór ciała
                let bodies: Vec<(u64, String)> =
                    self.scene.bodies.iter().filter(|b| !matches!(b.shape, Shape::Wall { .. })).map(|b| (b.id, b.name.clone())).collect();
                let cur = self.main_body().map(|b| b.id);
                if !bodies.is_empty() && self.chart_tab != ChartTab::Probes {
                    let name = cur.and_then(|id| bodies.iter().find(|b| b.0 == id)).map(|b| b.1.clone()).unwrap_or_default();
                    egui::ComboBox::from_id_salt("chart_body").selected_text(name).show_ui(ui, |ui| {
                        for (id, n) in &bodies {
                            if ui.selectable_label(cur == Some(*id), n).clicked() {
                                self.chart_body = Some(*id);
                                self.selected = Some(*id);
                            }
                        }
                    });
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("🗙").on_hover_text("Ukryj wykresy (G)").clicked() {
                        self.prefs.show_bottom = false;
                    }
                });
            });
            ui.separator();
            match self.chart_tab {
                ChartTab::Forces => self.forces_chart(ui),
                ChartTab::Surface => self.surface_chart(ui),
                ChartTab::Polar => self.polar_chart(ui),
                ChartTab::Probes => self.probes_chart(ui),
            }
        });
    }

    fn forces_chart(&mut self, ui: &mut egui::Ui) {
        let Some(b) = self.main_body().cloned() else {
            ui.label(RichText::new("Brak ciała w scenie.").color(theme::TEXT_DIM));
            return;
        };
        let units = self.scene.flow.units();
        let l = b.ref_length();
        let conv = l / units.u_inf;
        let series = self.history.get(b.id);
        ui.horizontal_top(|ui| {
            // statystyki
            ui.vertical(|ui| {
                ui.set_width(260.0);
                let Some(s) = series else {
                    ui.label(RichText::new("Zbieranie danych…").color(theme::TEXT_DIM));
                    return;
                };
                let window = (15.0 * conv).max(1e-9);
                let Some(st) = stats(s, &units, l, window) else {
                    ui.label(RichText::new("Zbieranie danych…").color(theme::TEXT_DIM));
                    return;
                };
                let span = self.scene.flow.span;
                let last = s.back().copied().unwrap_or_default();
                egui::Grid::new("force_stats").num_columns(2).spacing([12.0, 3.0]).show(ui, |ui| {
                    let row = |ui: &mut egui::Ui, k: &str, v: String, c: Color32| {
                        ui.label(RichText::new(k).color(theme::TEXT_DIM));
                        ui.label(RichText::new(v).monospace().color(c));
                        ui.end_row();
                    };
                    row(ui, "Cd średni", fmt_num(st.cd_mean, 4), theme::DRAG);
                    row(ui, "Cl średni", fmt_num(st.cl_mean, 4), theme::LIFT);
                    if matches!(b.shape, Shape::Airfoil { .. }) {
                        row(ui, "Cm (¼c) średni", fmt_num(st.cm_mean, 4), theme::TEXT);
                        let ld = st.cl_mean / st.cd_mean;
                        row(ui, "L/D", fmt_num(ld, 2), theme::TEXT);
                    }
                    if st.cl_amp > 0.005 {
                        row(ui, "Amplituda Cl", fmt_num(st.cl_amp, 3), theme::TEXT);
                    }
                    if let (Some(stv), Some(f)) = (st.strouhal, st.freq) {
                        row(ui, "Strouhal St", fmt_num(stv, 4), theme::TEXT);
                        row(ui, "Częstotliwość", fmt_si(f, "Hz", 3), theme::TEXT);
                    }
                    row(ui, "Opór D", format!("{} /m · {}", fmt_si(last.fx, "N", 3), fmt_si(last.fx * span, "N", 3)), theme::TEXT);
                    row(ui, "Siła nośna L", format!("{} /m · {}", fmt_si(last.fy, "N", 3), fmt_si(last.fy * span, "N", 3)), theme::TEXT);
                    if last.fy.abs() > 0.0 && span > 0.0 {
                        row(ui, "Unosi masę", format!("{} kg", fmt_num(last.fy * span / 9.80665, 3)), theme::TEXT_DIM);
                    }
                    row(ui, "Okno", format!("{} ({:.0} c/U)", fmt_si(st.window_s, "s", 2), st.window_s / conv), theme::TEXT_DIM);
                    row(
                        ui,
                        "Stan",
                        if st.settled { "ustalony".into() } else { "ustalanie…".into() },
                        if st.settled { theme::GOOD } else { theme::WARN },
                    );
                });
                if let Some(r) = &self.scene.reference {
                    ui.add_space(4.0);
                    ui.label(RichText::new(format!("Walidacja — {}", r.source)).size(11.0).color(theme::TEXT_DIM));
                    let cmp = |ui: &mut egui::Ui, name: &str, got: Option<f64>, refv: Option<[f64; 2]>| {
                        let (Some(g), Some([v, tol])) = (got, refv) else { return };
                        let ok = (g - v).abs() <= tol;
                        let err = (g - v) / v * 100.0;
                        ui.label(
                            RichText::new(format!(
                                "{name}: {} vs {} ± {} ({:+.1} %) {}",
                                fmt_num(g, 3),
                                fmt_num(v, 3),
                                fmt_num(tol, 2),
                                err,
                                if ok { "✔" } else { "✖" }
                            ))
                            .size(11.5)
                            .color(if ok { theme::GOOD } else { theme::WARN }),
                        );
                    };
                    cmp(ui, "Cd", Some(st.cd_mean), r.cd);
                    cmp(ui, "Cl amp.", (st.cl_amp > 0.005).then_some(st.cl_amp), r.cl_amp);
                    cmp(ui, "St", st.strouhal, r.strouhal);
                    if !st.settled {
                        ui.label(RichText::new("Poczekaj na ustalenie przepływu (≥ 50 c/U).").size(11.0).color(theme::TEXT_DIM));
                    }
                }
            });
            ui.separator();
            let Some(s) = series else { return };
            let n = s.len();
            let step = (n / 3000).max(1);
            let cd: PlotPoints = s.iter().step_by(step).map(|c| [c.t, c.cd]).collect();
            let cl: PlotPoints = s.iter().step_by(step).map(|c| [c.t, c.cl]).collect();
            let is_airfoil = matches!(b.shape, Shape::Airfoil { .. });
            Plot::new("forces_plot").legend(Legend::default()).x_axis_label("t [s]").allow_scroll(false).show(ui, |p| {
                p.line(Line::new("Cd", cd).color(theme::DRAG).width(1.6_f32));
                p.line(Line::new("Cl", cl).color(theme::LIFT).width(1.6_f32));
                if is_airfoil {
                    let cm: PlotPoints = s.iter().step_by(step).map(|c| [c.t, c.cm]).collect();
                    p.line(Line::new("Cm", cm).color(Color32::from_rgb(163, 230, 53)).width(1.2_f32));
                }
                p.hline(HLine::new("", 0.0).color(Color32::from_gray(70)).width(1.0_f32));
            });
        });
    }

    fn surface_chart(&mut self, ui: &mut egui::Ui) {
        let (Some(b), Some(f)) = (self.main_body().cloned(), self.frame.clone()) else { return };
        let Some(idx) = self.body_index(b.id) else { return };
        if f.scene_rev != self.rev {
            ui.label(RichText::new("Aktualizacja geometrii…").color(theme::TEXT_DIM));
            return;
        }
        let Some(c) = surface_curve(&f, idx, &b) else {
            ui.label(RichText::new("Brak punktów na powierzchni tego ciała.").color(theme::TEXT_DIM));
            return;
        };
        let sigma = self.scene.flow.fluid.vapor_pressure().map(|pv| {
            let u = self.scene.flow.units();
            (self.scene.flow.fluid.pressure_kpa * 1000.0 - pv) / u.q_inf()
        });
        let (upper_name, lower_name) = if matches!(b.shape, Shape::Airfoil { .. }) {
            ("strona górna (ssąca)", "strona dolna")
        } else {
            ("górna połowa", "dolna połowa")
        };
        Plot::new("cp_plot").legend(Legend::default()).x_axis_label(c.x_label).y_axis_label("−Cp").allow_scroll(false).show(ui, |p| {
            // konwencja aerodynamiczna: −Cp w górę
            let up: PlotPoints = c.upper.iter().map(|q| [q[0], -q[1]]).collect();
            let lo: PlotPoints = c.lower.iter().map(|q| [q[0], -q[1]]).collect();
            p.line(Line::new(upper_name, up).color(theme::LIFT).width(1.8_f32));
            p.line(Line::new(lower_name, lo).color(theme::DRAG).width(1.8_f32));
            p.hline(HLine::new("", 0.0).color(Color32::from_gray(70)));
            p.hline(
                HLine::new("punkt spiętrzenia (Cp = 1)", -1.0).color(Color32::from_gray(90)).style(egui_plot::LineStyle::dashed_dense()),
            );
            if let Some(s) = sigma {
                if s < 50.0 {
                    p.hline(HLine::new("próg kawitacji (−Cp = σ)", s).color(theme::BAD).style(egui_plot::LineStyle::dashed_loose()));
                }
            }
        });
    }

    fn polar_chart(&mut self, ui: &mut egui::Ui) {
        let airfoil = self.main_body().filter(|b| matches!(b.shape, Shape::Airfoil { .. })).cloned();
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(250.0);
                let running = self.sweep.as_ref().is_some_and(|s| !s.done());
                ui.add_enabled_ui(!running, |ui| {
                    egui::Grid::new("sweep_cfg").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                        let c = &mut self.sweep_cfg;
                        ui.label("Kąt od / do");
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(&mut c.from).speed(0.5).range(-30.0..=40.0).suffix("°"));
                            ui.add(egui::DragValue::new(&mut c.to).speed(0.5).range(-30.0..=40.0).suffix("°"));
                        });
                        ui.end_row();
                        ui.label("Krok");
                        ui.add(egui::DragValue::new(&mut c.step).speed(0.1).range(0.25..=10.0).suffix("°"));
                        ui.end_row();
                        ui.label("Ustalanie");
                        ui.add(egui::DragValue::new(&mut c.settle).speed(0.5).range(1.0..=100.0).suffix(" c/U"));
                        ui.end_row();
                        ui.label("Pomiar");
                        ui.add(egui::DragValue::new(&mut c.measure).speed(0.5).range(1.0..=100.0).suffix(" c/U"));
                        ui.end_row();
                    });
                });
                match (&airfoil, running) {
                    (None, _) => {
                        ui.label(RichText::new("Zaznacz profil, aby zmierzyć biegunową.").color(theme::TEXT_DIM));
                    }
                    (Some(b), false) => {
                        if ui.button("▶ Zmierz biegunową").clicked() {
                            let c = &self.sweep_cfg;
                            self.sweep = Some(Sweep::new(b.id, c.from, c.to, c.step, c.settle, c.measure));
                            if self.paused {
                                self.set_paused(false);
                            }
                        }
                    }
                    (Some(_), true) => {
                        if let Some(s) = &self.sweep {
                            ui.add(egui::ProgressBar::new(s.progress()).text(format!("α = {:.1}°", s.current_angle().unwrap_or(0.0))));
                            let phase = match s.phase {
                                crate::analysis::SweepPhase::Apply { .. } => "zmiana kąta",
                                crate::analysis::SweepPhase::Settle { .. } => "ustalanie przepływu",
                                crate::analysis::SweepPhase::Measure { .. } => "pomiar",
                            };
                            ui.label(RichText::new(phase).size(11.5).color(theme::TEXT_DIM));
                        }
                        if ui.button("⏹ Przerwij").clicked() {
                            if let Some(s) = self.sweep.as_mut() {
                                s.angles.truncate(s.idx);
                            }
                        }
                    }
                }
                let mut export = false;
                if let Some(s) = &self.sweep {
                    if !s.results.is_empty() {
                        export = ui.button("Eksport CSV").clicked();
                        let best = s.results.iter().max_by(|a, b| (a.cl / a.cd).partial_cmp(&(b.cl / b.cd)).unwrap_or(std::cmp::Ordering::Equal));
                        let clmax = s.results.iter().max_by(|a, b| a.cl.partial_cmp(&b.cl).unwrap_or(std::cmp::Ordering::Equal));
                        if let Some(p) = clmax {
                            ui.label(RichText::new(format!("Cl max = {} przy α = {:.1}°", fmt_num(p.cl, 3), p.alpha)).monospace().size(12.0));
                        }
                        if let Some(p) = best {
                            ui.label(RichText::new(format!("(L/D) max = {} przy α = {:.1}°", fmt_num(p.cl / p.cd, 3), p.alpha)).monospace().size(12.0));
                        }
                    }
                }
                if export {
                    self.export_csv("polar");
                }
                ui.label(RichText::new("Dla każdego kąta przepływ ustala się, a następnie współczynniki są uśredniane. c/U to czas przepływu strumienia wzdłuż cięciwy.").size(11.0).color(theme::TEXT_DIM));
            });
            ui.separator();
            let Some(s) = &self.sweep else {
                ui.label(RichText::new("Brak pomiarów.").color(theme::TEXT_DIM));
                return;
            };
            let w = (ui.available_width() / 2.0 - 6.0).max(120.0);
            let cl: Vec<[f64; 2]> = s.results.iter().map(|p| [p.alpha, p.cl]).collect();
            let cd: Vec<[f64; 2]> = s.results.iter().map(|p| [p.alpha, p.cd]).collect();
            let cm: Vec<[f64; 2]> = s.results.iter().map(|p| [p.alpha, p.cm]).collect();
            let drag_polar: Vec<[f64; 2]> = s.results.iter().map(|p| [p.cd, p.cl]).collect();
            Plot::new("polar_alpha").width(w).legend(Legend::default()).x_axis_label("α [°]").allow_scroll(false).show(ui, |p| {
                p.line(Line::new("Cl", PlotPoints::from(cl.clone())).color(theme::LIFT).width(1.8_f32));
                p.points(Points::new("", PlotPoints::from(cl)).color(theme::LIFT).radius(3.0_f32));
                p.line(Line::new("Cd", PlotPoints::from(cd.clone())).color(theme::DRAG).width(1.8_f32));
                p.points(Points::new("", PlotPoints::from(cd)).color(theme::DRAG).radius(3.0_f32));
                p.line(Line::new("Cm", PlotPoints::from(cm)).color(Color32::from_rgb(163, 230, 53)).width(1.3_f32));
                p.hline(HLine::new("", 0.0).color(Color32::from_gray(70)));
            });
            Plot::new("polar_drag").width(w).x_axis_label("Cd").y_axis_label("Cl").allow_scroll(false).show(ui, |p| {
                p.line(Line::new("Cl(Cd)", PlotPoints::from(drag_polar.clone())).color(Color32::from_rgb(232, 121, 249)).width(1.8_f32));
                p.points(Points::new("", PlotPoints::from(drag_polar)).color(Color32::from_rgb(232, 121, 249)).radius(3.0_f32));
            });
        });
    }

    fn probes_chart(&mut self, ui: &mut egui::Ui) {
        if self.scene.probes.is_empty() {
            ui.label(RichText::new("Brak sond. Wybierz narzędzie ✚ (klawisz 0) i kliknij w obszarze przepływu — sonda zapisuje prędkość i ciśnienie w czasie, a widmo pokazuje częstotliwości (np. odrywania wirów).").color(theme::TEXT_DIM));
            return;
        }
        self.selected_probe = self.selected_probe.min(self.scene.probes.len() - 1);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(220.0);
                for (k, p) in self.scene.probes.iter().enumerate() {
                    let s = self.probe_series.get(k);
                    let v = s.and_then(|s| s.speed.back()).copied();
                    let pr = s.and_then(|s| s.p.back()).copied();
                    let text = format!(
                        "S{}  ({}, {})\n    |u| {}   p {}",
                        k + 1,
                        fmt_si(p[0], "m", 2),
                        fmt_si(p[1], "m", 2),
                        v.map(|v| fmt_si(v, "m/s", 3)).unwrap_or("—".into()),
                        pr.map(|v| fmt_si(v, "Pa", 3)).unwrap_or("—".into())
                    );
                    if ui
                        .selectable_label(self.selected_probe == k, RichText::new(text).monospace().size(11.5).color(probe_color(k)))
                        .clicked()
                    {
                        self.selected_probe = k;
                    }
                }
                if ui.button("Eksport CSV").clicked() {
                    self.export_csv("probes");
                }
            });
            ui.separator();
            let w = (ui.available_width() / 2.0 - 6.0).max(120.0);
            Plot::new("probes_plot")
                .width(w)
                .legend(Legend::default())
                .x_axis_label("t [s]")
                .y_axis_label("|u| [m/s]")
                .allow_scroll(false)
                .show(ui, |p| {
                    for (k, s) in self.probe_series.iter().enumerate() {
                        let step = (s.t.len() / 3000).max(1);
                        let pts: PlotPoints = s.t.iter().zip(&s.speed).step_by(step).map(|(t, v)| [*t, *v]).collect();
                        p.line(Line::new(format!("S{}", k + 1), pts).color(probe_color(k)).width(1.4_f32));
                    }
                });
            let k = self.selected_probe;
            if let Some(s) = self.probe_series.get(k) {
                // widmo z ostatniej połowy przebiegu (bez rozbiegu)
                let half = s.t.len() / 2;
                let t: std::collections::VecDeque<f64> = s.t.iter().skip(half).copied().collect();
                let v: std::collections::VecDeque<f64> = s.uy.iter().skip(half).copied().collect();
                match spectrum(&t, &v, 512) {
                    Some((fr, amp)) => {
                        let peak =
                            amp.iter().enumerate().max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal)).map(|(i, _)| i);
                        let title = peak.map(|i| format!("widmo u_y S{} — szczyt {}", k + 1, fmt_si(fr[i], "Hz", 3))).unwrap_or_default();
                        let pts: PlotPoints = fr.iter().zip(&amp).map(|(f, a)| [*f, *a]).collect();
                        Plot::new("spectrum").width(w).legend(Legend::default()).x_axis_label("f [Hz]").allow_scroll(false).show(ui, |p| {
                            p.line(Line::new(title, pts).color(probe_color(k)).width(1.4_f32).fill(0.0_f32));
                        });
                    }
                    None => {
                        ui.label(RichText::new("Za mało danych do widma.").color(theme::TEXT_DIM));
                    }
                }
            }
        });
    }
}
