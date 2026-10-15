//! Widok symulacji: pole, nakładki, ciała, narzędzia edycji, HUD i legenda.

use eframe::egui::{
    self, epaint::Mesh, Align2, Color32, CornerRadius, FontId, PointerButton, Pos2, Rect, Sense, Shape as EShape, Stroke, StrokeKind, Vec2,
};

use crate::analysis::stats;
use crate::app::{wrap_angle, App, Drag, Tool};
use crate::physics::geometry::{simplify, triangulate, Body, Naca, Shape, Stroke as WStroke, V2};
use crate::physics::lattice::{Boundary, InletProfile};
use crate::physics::units::{assess, Fidelity};
use crate::render::field::{fmt_num, fmt_si, FieldKind};
use crate::render::flowviz::sample;
use crate::ui::theme;

pub fn fidelity_color(f: Fidelity) -> Color32 {
    match f {
        Fidelity::Resolved => theme::GOOD,
        Fidelity::Marginal => theme::WARN,
        Fidelity::Qualitative => Color32::from_rgb(251, 146, 60),
    }
}

impl App {
    pub fn to_screen(&self, rect: Rect, w: V2) -> Pos2 {
        Pos2::new(
            rect.center().x + ((w[0] - self.view.center[0]) * self.view.ppm) as f32,
            rect.center().y - ((w[1] - self.view.center[1]) * self.view.ppm) as f32,
        )
    }

    pub fn to_world(&self, rect: Rect, p: Pos2) -> V2 {
        [
            self.view.center[0] + (p.x - rect.center().x) as f64 / self.view.ppm,
            self.view.center[1] - (p.y - rect.center().y) as f64 / self.view.ppm,
        ]
    }

    fn pick(&self, w: V2) -> Option<u64> {
        self.scene.bodies.iter().rev().find(|b| b.contains(w)).map(|b| b.id)
    }

    fn handle_pos(&self, rect: Rect, b: &Body) -> Pos2 {
        let a = -b.angle.to_radians();
        let dir = Vec2::new(a.cos() as f32, -(a.sin() as f32));
        let pivot = self.to_screen(rect, b.pos);
        let len = match &b.shape {
            Shape::Airfoil { chord, .. } => (0.75 * chord * self.view.ppm) as f32 + 22.0,
            _ => {
                let p = b.prepare();
                let half = 0.5 * ((p.bbox[2] - p.bbox[0]).max(p.bbox[3] - p.bbox[1]));
                (half * self.view.ppm) as f32 + 22.0
            }
        };
        pivot + dir * len.max(30.0)
    }

    pub fn viewport(&mut self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        self.viewport_rect = rect;
        let resp = ui.interact(rect, ui.id().with("viewport"), Sense::click_and_drag());
        let dom = self.scene.flow.domain;
        if self.view.fit {
            self.view.ppm = (rect.width() as f64 / dom[0]).min(rect.height() as f64 / dom[1]) * 0.94;
            self.view.center = [dom[0] * 0.5, dom[1] * 0.5];
        }

        // powiększanie (kółko) i obrót (Alt + kółko)
        if resp.hovered() {
            let (scroll, mods, zoom, pos) = ui.input(|i| (i.raw_scroll_delta.y, i.modifiers, i.zoom_delta(), i.pointer.hover_pos()));
            let rotatable = self.selected.and_then(|id| self.scene.body(id)).is_some_and(|b| b.rotatable());
            if scroll != 0.0 && mods.alt && rotatable {
                let step = if mods.shift { 0.1 } else { 1.0 };
                self.rotate_selected(scroll.signum() as f64 * step);
            } else if scroll != 0.0 || zoom != 1.0 {
                let k = zoom as f64 * (scroll as f64 * 0.0018).exp();
                if let Some(p) = pos {
                    let before = self.to_world(rect, p);
                    self.view.ppm = (self.view.ppm * k).clamp(1e-3, 1e9);
                    let after = self.to_world(rect, p);
                    self.view.center[0] += before[0] - after[0];
                    self.view.center[1] += before[1] - after[1];
                    self.view.fit = false;
                }
            }
        }

        let painter = ui.painter_at(rect);
        let p0 = self.to_screen(rect, [0.0, dom[1]]);
        let p1 = self.to_screen(rect, [dom[0], 0.0]);
        let dom_rect = Rect::from_min_max(p0, p1);
        let frame = self.frame.clone();

        // pole
        if let Some(t) = &self.field_tex {
            painter.image(t.id(), dom_rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        } else {
            painter.rect_filled(dom_rect, 0.0, theme::FIELD_OFF);
            painter.text(dom_rect.center(), Align2::CENTER_CENTER, "Przygotowanie siatki…", FontId::proportional(16.0), theme::TEXT_DIM);
        }
        self.draw_boundaries(&painter, rect, dom_rect);

        let dx = self.scene.flow.dx();
        let cell_to_screen = |me: &App, c: [f32; 2]| me.to_screen(rect, [c[0] as f64 * dx, c[1] as f64 * dx]);

        // nakładki przepływu
        if let Some(f) = frame.as_ref().filter(|f| f.scene_rev + 2 >= self.rev) {
            if self.prefs.streamlines {
                for line in &self.stream_cache {
                    if line.len() < 2 {
                        continue;
                    }
                    let pts: Vec<Pos2> = line.iter().step_by(2).map(|c| cell_to_screen(self, *c)).collect();
                    painter.add(EShape::line(pts, Stroke::new(1.1_f32, Color32::from_rgba_unmultiplied(235, 245, 255, 120))));
                }
            }
            if self.prefs.vectors {
                let k = self.prefs.vector_spacing.max(4);
                let uref = f.units.u_lb as f32;
                let cell_px = (dx * self.view.ppm) as f32;
                let mut y = k / 2;
                while y < f.h {
                    let mut x = k / 2;
                    while x < f.w {
                        let c = [x as f32 + 0.5, y as f32 + 0.5];
                        if let Some((u, v)) = sample(f, c[0], c[1]) {
                            let a = cell_to_screen(self, c);
                            let len = (u.hypot(v) / uref).min(2.0) * k as f32 * cell_px * 0.85;
                            let dir = Vec2::new(u, -v).normalized();
                            if len > 1.5 {
                                let b = a + dir * len;
                                let col = Color32::from_rgba_unmultiplied(255, 255, 255, 170);
                                painter.line_segment([a, b], Stroke::new(1.0_f32, col));
                                let h = (len * 0.3).min(6.0);
                                let n = Vec2::new(-dir.y, dir.x);
                                painter.add(EShape::convex_polygon(
                                    vec![b, b - dir * h + n * h * 0.45, b - dir * h - n * h * 0.45],
                                    col,
                                    Stroke::NONE,
                                ));
                            }
                        }
                        x += k;
                    }
                    y += k;
                }
            }
            if self.prefs.particles {
                let lut = self.prefs.colormap(FieldKind::Speed).lut();
                let by_speed = self.prefs.particle_speed_color;
                let umax = f.units.u_lb as f32 * 1.8;
                let mesh = self.particles.mesh(
                    |c| cell_to_screen(self, c),
                    1.3,
                    |c| {
                        if by_speed {
                            let s = sample(f, c[0], c[1]).map(|(u, v)| u.hypot(v) / umax).unwrap_or(0.0);
                            lut[(s.clamp(0.0, 1.0) * 255.0) as usize].gamma_multiply(0.95)
                        } else {
                            Color32::from_rgba_unmultiplied(240, 248, 255, 200)
                        }
                    },
                );
                painter.add(EShape::mesh(mesh));
            }
        }

        // ciała (przycięte do obszaru — części w marginesach są niewidoczne)
        let body_painter = painter.with_clip_rect(dom_rect.intersect(rect));
        let hovered_body = resp.hover_pos().and_then(|p| self.pick(self.to_world(rect, p)));
        for b in &self.scene.bodies {
            let sel = self.selected == Some(b.id);
            let hov = hovered_body == Some(b.id) && self.tool == Tool::Select;
            if let Shape::Wall { strokes } = &b.shape {
                // ściana bez gumki: gładki rysunek wektorowy (kapsuły); z gumką — raster z pola
                if strokes.iter().all(|s| !s.erase) {
                    let p = b.prepare();
                    for s in strokes {
                        let pts: Vec<Pos2> = s.pts.iter().map(|q| self.to_screen(rect, p.to_world(*q))).collect();
                        let w = (2.0 * s.r * self.view.ppm) as f32;
                        for q in &pts {
                            body_painter.circle_filled(*q, 0.5 * w, theme::BODY_FILL);
                        }
                        if pts.len() > 1 {
                            body_painter.add(EShape::line(pts, Stroke::new(w, theme::BODY_FILL)));
                        }
                    }
                }
                if sel || hov {
                    let p = b.prepare();
                    let r = Rect::from_two_pos(self.to_screen(rect, [p.bbox[0], p.bbox[1]]), self.to_screen(rect, [p.bbox[2], p.bbox[3]]));
                    body_painter.rect_stroke(
                        r.expand(3.0),
                        3.0,
                        Stroke::new(1.2_f32, if sel { theme::ACCENT } else { theme::TEXT_DIM }),
                        StrokeKind::Outside,
                    );
                }
                continue;
            }
            let pts: Vec<Pos2> = b.outline_world().into_iter().map(|w| self.to_screen(rect, w)).collect();
            if pts.len() < 3 {
                continue;
            }
            let wpts: Vec<V2> = pts.iter().map(|p| [p.x as f64, p.y as f64]).collect();
            let mut mesh = Mesh::default();
            for p in &pts {
                mesh.colored_vertex(*p, theme::BODY_FILL);
            }
            for t in triangulate(&wpts) {
                mesh.add_triangle(t[0] as u32, t[1] as u32, t[2] as u32);
            }
            body_painter.add(EShape::mesh(mesh));
            let edge = if sel {
                Stroke::new(2.0_f32, theme::ACCENT)
            } else if hov {
                Stroke::new(1.6_f32, Color32::from_rgb(200, 220, 240))
            } else {
                Stroke::new(1.1_f32, theme::BODY_EDGE)
            };
            body_painter.add(EShape::closed_line(pts, edge));
            if sel || hov {
                let c = self.to_screen(rect, b.pos);
                body_painter.circle_filled(c, 3.0, theme::ACCENT);
            }
        }

        // wektory sił
        if self.prefs.show_forces {
            if let Some(f) = frame.as_ref() {
                if f.scene_rev == self.rev && !f.forces.is_empty() {
                    for b in &self.scene.bodies {
                        if b.locked || matches!(b.shape, Shape::Wall { .. }) {
                            continue;
                        }
                        let Some(series) = self.history.get(b.id) else { continue };
                        let n = series.len().min(20);
                        if n == 0 {
                            continue;
                        }
                        let (cd, cl) = series.iter().rev().take(n).fold((0.0, 0.0), |a, c| (a.0 + c.cd / n as f64, a.1 + c.cl / n as f64));
                        let c = self.to_screen(rect, b.pos);
                        let k = 46.0;
                        arrow(&painter, c, Vec2::new((cd.clamp(-3.0, 3.0) * k) as f32, 0.0), theme::DRAG, "D");
                        arrow(&painter, c, Vec2::new(0.0, -(cl.clamp(-3.0, 3.0) * k) as f32), theme::LIFT, "L");
                    }
                }
            }
        }

        // źródła dymu i sondy
        for e in &self.scene.emitters {
            let p = self.to_screen(rect, *e);
            painter.circle_filled(p, 5.0, Color32::from_rgb(230, 230, 235));
            painter.circle_stroke(p, 7.5, Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 120)));
        }
        if self.prefs.show_probes {
            for (k, pr) in self.scene.probes.iter().enumerate() {
                let p = self.to_screen(rect, *pr);
                let col = probe_color(k);
                painter.line_segment([p - Vec2::new(7.0, 0.0), p + Vec2::new(7.0, 0.0)], Stroke::new(1.5_f32, col));
                painter.line_segment([p - Vec2::new(0.0, 7.0), p + Vec2::new(0.0, 7.0)], Stroke::new(1.5_f32, col));
                painter.circle_stroke(p, 4.0, Stroke::new(1.5_f32, col));
                let mut label = format!("S{}", k + 1);
                if let Some(s) = self.probe_series.get(k) {
                    if let Some(v) = s.speed.back() {
                        label += &format!("  {}", fmt_si(*v, "m/s", 2));
                    }
                }
                painter.text(p + Vec2::new(9.0, -9.0), Align2::LEFT_BOTTOM, label, FontId::monospace(11.5), col);
            }
        }

        // uchwyt obrotu
        let mut on_handle = false;
        if self.tool == Tool::Select {
            if let Some(b) = self.selected.and_then(|id| self.scene.body(id)) {
                if b.rotatable() && !b.locked {
                    let h = self.handle_pos(rect, b);
                    let c = self.to_screen(rect, b.pos);
                    on_handle = resp.hover_pos().is_some_and(|p| p.distance(h) < 9.0) || matches!(self.drag, Some(Drag::Rotate { .. }));
                    painter.add(EShape::dashed_line(&[c, h], Stroke::new(1.0_f32, theme::ACCENT), 4.0, 3.0));
                    painter.circle_filled(h, if on_handle { 7.0 } else { 5.5 }, if on_handle { Color32::WHITE } else { theme::ACCENT });
                    painter.text(
                        h + Vec2::new(10.0, 0.0),
                        Align2::LEFT_CENTER,
                        format!("{:.1}°", b.angle),
                        FontId::monospace(12.0),
                        theme::TEXT,
                    );
                }
            }
        }

        self.interact(ui, &resp, rect, on_handle);
        self.draw_tool_preview(&painter, rect, &resp);
        self.draw_hud(ui, rect);
        if self.prefs.show_legend {
            self.draw_legend(&painter, rect);
        }
        self.draw_hover(&painter, rect, &resp);

        // podpowiedź narzędzia
        if !self.hide_ui {
            let hint = self.tool.hint();
            let pos = Pos2::new(rect.left() + 12.0, rect.bottom() - 12.0);
            let galley = painter.layout(hint.to_string(), FontId::proportional(12.0), theme::TEXT_DIM, (rect.width() * 0.55).max(200.0));
            let r = Rect::from_min_size(pos - Vec2::new(0.0, galley.size().y), galley.size()).expand(6.0);
            painter.rect_filled(r, 6.0, Color32::from_rgba_unmultiplied(10, 13, 19, 200));
            painter.galley(r.min + Vec2::splat(6.0), galley, theme::TEXT_DIM);
        }
        if self.paused {
            let r = Rect::from_center_size(Pos2::new(rect.center().x, rect.top() + 26.0), Vec2::new(150.0, 28.0));
            painter.rect_filled(r, 14.0, Color32::from_rgba_unmultiplied(250, 204, 21, 40));
            painter.text(r.center(), Align2::CENTER_CENTER, "⏸  WSTRZYMANO", FontId::proportional(13.0), theme::WARN);
        }
    }

    fn draw_boundaries(&self, painter: &egui::Painter, rect: Rect, d: Rect) {
        let f = &self.scene.flow;
        let wall = |y: f32, b: Boundary, top: bool| {
            let col = Color32::from_rgb(120, 136, 160);
            match b {
                Boundary::FreeStream => {
                    painter.add(EShape::dashed_line(
                        &[Pos2::new(d.left(), y), Pos2::new(d.right(), y)],
                        Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(150, 170, 200, 60)),
                        6.0,
                        6.0,
                    ));
                }
                Boundary::SlipWall | Boundary::NoSlipWall | Boundary::MovingGround => {
                    let s = if top { -1.0 } else { 1.0 };
                    painter.line_segment([Pos2::new(d.left(), y), Pos2::new(d.right(), y)], Stroke::new(2.0_f32, col));
                    let mut x = d.left();
                    let step = if b == Boundary::SlipWall { 16.0 } else { 9.0 };
                    while x < d.right() {
                        if b == Boundary::MovingGround {
                            painter.line_segment([Pos2::new(x, y + s * 6.0), Pos2::new(x + 7.0, y + s * 6.0)], Stroke::new(1.0_f32, col));
                            painter.line_segment(
                                [Pos2::new(x + 7.0, y + s * 6.0), Pos2::new(x + 4.0, y + s * 4.0)],
                                Stroke::new(1.0_f32, col),
                            );
                            x += 22.0;
                        } else {
                            painter.line_segment([Pos2::new(x, y + s * 7.0), Pos2::new(x + 6.0, y)], Stroke::new(1.0_f32, col));
                            x += step;
                        }
                    }
                }
            }
        };
        wall(d.top(), f.top, true);
        wall(d.bottom(), f.bottom, false);
        // strzałki profilu wlotu
        let n = 9;
        let max_len = 26.0;
        for k in 0..n {
            let s = (k as f64 + 0.5) / n as f64;
            let kf = match f.profile {
                InletProfile::Uniform => 1.0,
                InletProfile::Channel => 6.0 * s * (1.0 - s) / 1.5,
                InletProfile::PowerLaw { alpha } => s.powf(alpha),
            } as f32;
            let y = d.bottom() - d.height() * s as f32;
            let x0 = d.left() - max_len - 6.0;
            let x1 = x0 + max_len * kf;
            if x0 < rect.left() {
                break;
            }
            let col = Color32::from_rgba_unmultiplied(160, 200, 240, 130);
            painter.line_segment([Pos2::new(x0, y), Pos2::new(x1, y)], Stroke::new(1.2_f32, col));
            painter.add(EShape::convex_polygon(
                vec![Pos2::new(x1 + 4.0, y), Pos2::new(x1 - 2.0, y - 3.0), Pos2::new(x1 - 2.0, y + 3.0)],
                col,
                Stroke::NONE,
            ));
        }
    }

    fn interact(&mut self, ui: &egui::Ui, resp: &egui::Response, rect: Rect, on_handle: bool) {
        let (origin, latest, mods) = ui.input(|i| (i.pointer.press_origin(), i.pointer.latest_pos(), i.modifiers));
        let world_at = |me: &App, p: Option<Pos2>| p.map(|p| me.to_world(rect, p));
        self.hover = if resp.hovered() { world_at(self, resp.hover_pos()) } else { None };
        let dx = self.scene.flow.dx();

        // przesuwanie widoku środkowym przyciskiem
        if resp.dragged_by(PointerButton::Middle) {
            let d = resp.drag_delta();
            self.view.center[0] -= d.x as f64 / self.view.ppm;
            self.view.center[1] += d.y as f64 / self.view.ppm;
            self.view.fit = false;
        }
        if resp.double_clicked_by(PointerButton::Middle) {
            self.view.fit = true;
        }

        // prawy przycisk: zaznacz ciało pod kursorem i pokaż menu
        if resp.secondary_clicked() {
            if let Some(w) = world_at(self, latest) {
                self.selected = self.pick(w);
                ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("ctx_world"), w));
            }
        }
        resp.context_menu(|ui| self.context_menu(ui));

        let primary_started = resp.drag_started_by(PointerButton::Primary);
        let primary_dragging = resp.dragged_by(PointerButton::Primary);
        let primary_stopped = resp.drag_stopped_by(PointerButton::Primary);
        let clicked = resp.clicked();
        let cur = world_at(self, latest);
        let start = world_at(self, origin);

        match self.tool {
            Tool::Select => {
                if clicked {
                    self.selected = cur.and_then(|w| self.pick(w));
                    if self.selected.is_some() {
                        self.chart_body = self.selected;
                    }
                }
                if primary_started {
                    if on_handle {
                        if let Some(id) = self.selected {
                            self.drag = Some(Drag::Rotate { id });
                        }
                    } else if let Some(s) = start {
                        match self.pick(s) {
                            Some(id) => {
                                self.selected = Some(id);
                                self.chart_body = Some(id);
                                let locked = self.scene.body(id).is_some_and(|b| b.locked);
                                self.drag = if locked { None } else { Some(Drag::Move { id, last: s }) };
                                if locked {
                                    self.notify("Obiekt jest zablokowany — odblokuj go w menu kontekstowym.");
                                }
                            }
                            None => self.drag = Some(Drag::Pan { last: origin.unwrap_or_default() }),
                        }
                    }
                }
            }
            Tool::Airfoil | Tool::Circle | Tool::Rect | Tool::Ellipse | Tool::Polygon => {
                if clicked {
                    self.selected = cur.and_then(|w| self.pick(w));
                }
                if primary_started {
                    if let Some(s) = start {
                        self.drag = Some(Drag::Create { start: s, cur: s, pts: vec![s] });
                    }
                }
            }
            Tool::Brush => {
                if primary_started || clicked {
                    if let Some(s) = if clicked { cur } else { start } {
                        self.begin_paint(s);
                    }
                }
            }
            Tool::Eraser => {
                if primary_started || clicked {
                    self.drag = Some(Drag::Erase);
                    if let Some(s) = if clicked { cur } else { start } {
                        self.erase_at(s, true);
                    }
                }
            }
            Tool::Emitter | Tool::Probe => {
                if clicked {
                    if let Some(w) = cur {
                        let list = if self.tool == Tool::Emitter { &mut self.scene.emitters } else { &mut self.scene.probes };
                        let tol = 10.0 / self.view.ppm;
                        let near = list.iter().position(|p| (p[0] - w[0]).hypot(p[1] - w[1]) < tol);
                        match near {
                            Some(i) => {
                                list.remove(i);
                                if self.tool == Tool::Probe && i < self.probe_series.len() {
                                    self.probe_series.remove(i);
                                }
                            }
                            None => {
                                if w[0] >= 0.0 && w[1] >= 0.0 && w[0] <= self.scene.flow.domain[0] && w[1] <= self.scene.flow.domain[1] {
                                    list.push(w);
                                    if self.tool == Tool::Emitter && self.prefs.field != crate::render::field::FieldKind::Smoke {
                                        self.set_field(crate::render::field::FieldKind::Smoke);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // przeciąganie w toku
        if primary_dragging {
            if let Some(c) = cur {
                match self.drag.as_mut() {
                    Some(Drag::Move { id, last }) => {
                        let mut d = [c[0] - last[0], c[1] - last[1]];
                        if mods.shift {
                            // ruch tylko w pionie (np. zmiana położenia nad podłogą)
                            d[0] = 0.0;
                        }
                        *last = [last[0] + d[0], last[1] + d[1]];
                        let id = *id;
                        if let Some(b) = self.scene.body_mut(id) {
                            b.translate(d);
                        }
                    }
                    Some(Drag::Rotate { id }) => {
                        let id = *id;
                        if let Some(b) = self.scene.body_mut(id) {
                            let mut a = -(c[1] - b.pos[1]).atan2(c[0] - b.pos[0]).to_degrees();
                            a = if mods.shift { (a / 5.0).round() * 5.0 } else { (a * 10.0).round() / 10.0 };
                            b.angle = wrap_angle(a);
                        }
                    }
                    Some(Drag::Pan { last }) => {
                        if let Some(p) = latest {
                            let d = p - *last;
                            *last = p;
                            self.view.center[0] -= d.x as f64 / self.view.ppm;
                            self.view.center[1] += d.y as f64 / self.view.ppm;
                            self.view.fit = false;
                        }
                    }
                    Some(Drag::Create { cur: cc, pts, .. }) => {
                        *cc = c;
                        if self.tool == Tool::Polygon {
                            let l = *pts.last().unwrap();
                            if (l[0] - c[0]).hypot(l[1] - c[1]) > 0.6 * dx {
                                pts.push(c);
                            }
                        }
                    }
                    Some(Drag::Paint { id }) => {
                        let id = *id;
                        let r = self.prefs.brush_cells as f64 * dx;
                        if let Some(b) = self.scene.body_mut(id) {
                            let l = b.prepare().to_local(c);
                            if let Shape::Wall { strokes } = &mut b.shape {
                                if let Some(s) = strokes.last_mut() {
                                    if s.pts.last().is_none_or(|p| (p[0] - l[0]).hypot(p[1] - l[1]) > 0.35 * r) {
                                        s.pts.push(l);
                                    }
                                }
                            }
                        }
                    }
                    Some(Drag::Erase) => self.erase_at(c, false),
                    None => {}
                }
            }
        }
        if primary_stopped || (!primary_dragging && !ui.input(|i| i.pointer.primary_down()) && self.drag.is_some()) {
            if let Some(Drag::Create { start, cur, pts }) = self.drag.take() {
                self.finish_create(start, cur, pts);
            }
            self.drag = None;
        }
    }

    fn begin_paint(&mut self, w: V2) {
        let r = self.prefs.brush_cells as f64 * self.scene.flow.dx();
        // kontynuuj ścianę pod kursorem lub zaznaczoną ścianę
        let target = self
            .scene
            .bodies
            .iter()
            .rev()
            .find(|b| matches!(b.shape, Shape::Wall { .. }) && (b.contains(w) || Some(b.id) == self.selected))
            .map(|b| b.id);
        let id = match target {
            Some(id) => id,
            None => self.scene.add(Shape::Wall { strokes: Vec::new() }, w, 0.0),
        };
        if let Some(b) = self.scene.body_mut(id) {
            let l = b.prepare().to_local(w);
            if let Shape::Wall { strokes } = &mut b.shape {
                strokes.push(WStroke { pts: vec![l], r, erase: false });
            }
        }
        self.selected = Some(id);
        self.drag = Some(Drag::Paint { id });
    }

    fn erase_at(&mut self, w: V2, new_stroke: bool) {
        let r = self.prefs.brush_cells as f64 * self.scene.flow.dx() * 1.5;
        let ids: Vec<u64> = self
            .scene
            .bodies
            .iter()
            .filter(|b| matches!(b.shape, Shape::Wall { .. }))
            .filter(|b| {
                let bb = b.prepare().bbox;
                w[0] >= bb[0] - r && w[0] <= bb[2] + r && w[1] >= bb[1] - r && w[1] <= bb[3] + r
            })
            .map(|b| b.id)
            .collect();
        for id in ids {
            if let Some(b) = self.scene.body_mut(id) {
                let l = b.prepare().to_local(w);
                if let Shape::Wall { strokes } = &mut b.shape {
                    match strokes.last_mut() {
                        Some(s) if s.erase && !new_stroke => s.pts.push(l),
                        _ => strokes.push(WStroke { pts: vec![l], r, erase: true }),
                    }
                }
            }
        }
    }

    fn finish_create(&mut self, start: V2, cur: V2, pts: Vec<V2>) {
        let dx = self.scene.flow.dx();
        let d = [cur[0] - start[0], cur[1] - start[1]];
        let len = d[0].hypot(d[1]);
        let min = 2.5 * dx;
        let id = match self.tool {
            Tool::Airfoil if len > min => {
                let naca = Naca::parse(&self.prefs.naca).unwrap_or(Naca::new(0.02, 0.4, 0.12));
                let angle = -(d[1].atan2(d[0])).to_degrees();
                let pivot = [start[0] + 0.25 * d[0], start[1] + 0.25 * d[1]];
                Some(self.scene.add(Shape::Airfoil { chord: len, naca }, pivot, (angle * 10.0).round() / 10.0))
            }
            Tool::Circle if len > 0.5 * min => Some(self.scene.add(Shape::Circle { r: len }, start, 0.0)),
            Tool::Rect | Tool::Ellipse if d[0].abs() > 0.5 * min && d[1].abs() > 0.5 * min => {
                let c = [start[0] + 0.5 * d[0], start[1] + 0.5 * d[1]];
                let (w, h) = (d[0].abs(), d[1].abs());
                let shape =
                    if self.tool == Tool::Rect { Shape::Rect { w, h, radius: 0.0 } } else { Shape::Ellipse { a: 0.5 * w, b: 0.5 * h } };
                Some(self.scene.add(shape, c, 0.0))
            }
            Tool::Polygon if pts.len() >= 3 => {
                let pts = simplify(&pts, 0.7 * dx);
                let area = crate::physics::geometry::signed_area(&pts).abs();
                if pts.len() >= 3 && area > 4.0 * dx * dx {
                    let cx = pts.iter().map(|p| p[0]).sum::<f64>() / pts.len() as f64;
                    let cy = pts.iter().map(|p| p[1]).sum::<f64>() / pts.len() as f64;
                    let local = pts.iter().map(|p| [p[0] - cx, p[1] - cy]).collect();
                    Some(self.scene.add(Shape::Polygon { pts: local }, [cx, cy], 0.0))
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(id) = id {
            self.selected = Some(id);
            self.chart_body = Some(id);
        }
    }

    fn draw_tool_preview(&self, painter: &egui::Painter, rect: Rect, resp: &egui::Response) {
        let stroke = Stroke::new(1.5_f32, Color32::from_rgb(125, 211, 252));
        let fill = Color32::from_rgba_unmultiplied(125, 211, 252, 40);
        if let Some(Drag::Create { start, cur, pts }) = &self.drag {
            let d = [cur[0] - start[0], cur[1] - start[1]];
            let len = d[0].hypot(d[1]);
            let units = self.scene.flow.units();
            let (shape_pts, label): (Vec<V2>, String) = match self.tool {
                Tool::Airfoil => {
                    let naca = Naca::parse(&self.prefs.naca).unwrap_or(Naca::new(0.02, 0.4, 0.12));
                    let a = d[1].atan2(d[0]);
                    let (s, c) = a.sin_cos();
                    let p = naca
                        .outline(50)
                        .into_iter()
                        .map(|q| [start[0] + (q[0] * c - q[1] * s) * len, start[1] + (q[0] * s + q[1] * c) * len])
                        .collect();
                    (
                        p,
                        format!(
                            "NACA {} · c = {} · α = {:.1}° · Re = {}",
                            naca.designation(),
                            fmt_si(len, "m", 2),
                            -a.to_degrees(),
                            fmt_num(units.reynolds(len), 2)
                        ),
                    )
                }
                Tool::Circle => {
                    let p = (0..64).map(|k| {
                        let t = std::f64::consts::TAU * k as f64 / 64.0;
                        [start[0] + len * t.cos(), start[1] + len * t.sin()]
                    });
                    (p.collect(), format!("D = {} · Re = {}", fmt_si(2.0 * len, "m", 2), fmt_num(units.reynolds(2.0 * len), 2)))
                }
                Tool::Rect => (
                    vec![*start, [cur[0], start[1]], *cur, [start[0], cur[1]]],
                    format!("{} × {}", fmt_si(d[0].abs(), "m", 2), fmt_si(d[1].abs(), "m", 2)),
                ),
                Tool::Ellipse => {
                    let c = [start[0] + 0.5 * d[0], start[1] + 0.5 * d[1]];
                    let p = (0..64).map(|k| {
                        let t = std::f64::consts::TAU * k as f64 / 64.0;
                        [c[0] + 0.5 * d[0].abs() * t.cos(), c[1] + 0.5 * d[1].abs() * t.sin()]
                    });
                    (p.collect(), format!("{} × {}", fmt_si(d[0].abs(), "m", 2), fmt_si(d[1].abs(), "m", 2)))
                }
                Tool::Polygon => (pts.clone(), String::new()),
                _ => (Vec::new(), String::new()),
            };
            let sp: Vec<Pos2> = shape_pts.iter().map(|w| self.to_screen(rect, *w)).collect();
            if sp.len() >= 2 {
                if self.tool == Tool::Polygon {
                    painter.add(EShape::line(sp, stroke));
                } else {
                    let wp: Vec<V2> = sp.iter().map(|p| [p.x as f64, p.y as f64]).collect();
                    let mut mesh = Mesh::default();
                    for p in &sp {
                        mesh.colored_vertex(*p, fill);
                    }
                    for t in triangulate(&wp) {
                        mesh.add_triangle(t[0] as u32, t[1] as u32, t[2] as u32);
                    }
                    painter.add(EShape::mesh(mesh));
                    painter.add(EShape::closed_line(sp, stroke));
                }
            }
            if !label.is_empty() {
                let p = self.to_screen(rect, *cur) + Vec2::new(14.0, 14.0);
                let g = painter.layout_no_wrap(label, FontId::monospace(12.0), theme::TEXT);
                let r = Rect::from_min_size(p, g.size()).expand(5.0);
                painter.rect_filled(r, 5.0, Color32::from_rgba_unmultiplied(10, 13, 19, 220));
                painter.galley(p, g, theme::TEXT);
            }
        }
        if matches!(self.tool, Tool::Brush | Tool::Eraser) {
            if let Some(p) = resp.hover_pos() {
                let mut r = self.prefs.brush_cells as f64 * self.scene.flow.dx() * self.view.ppm;
                if self.tool == Tool::Eraser {
                    r *= 1.5;
                }
                let col = if self.tool == Tool::Eraser { theme::BAD } else { Color32::WHITE };
                painter.circle_stroke(p, r as f32, Stroke::new(1.2_f32, col));
            }
        }
    }

    fn draw_hud(&self, ui: &egui::Ui, rect: Rect) {
        if !self.prefs.show_hud {
            return;
        }
        let units = self.scene.flow.units();
        let frame = self.frame.as_ref();
        egui::Area::new(egui::Id::new("hud"))
            .fixed_pos(rect.min + Vec2::new(12.0, 12.0))
            .interactable(false)
            .order(egui::Order::Middle)
            .show(ui.ctx(), |ui| {
                egui::Frame::NONE
                    .fill(Color32::from_rgba_unmultiplied(10, 13, 19, 205))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(egui::Margin::symmetric(10, 8))
                    .show(ui, |ui| {
                        ui.set_max_width(330.0);
                        ui.label(egui::RichText::new(&self.scene.name).strong().size(14.0));
                        let fluid = format!(
                            "{} {:.0} °C · U∞ = {}",
                            self.scene.flow.fluid.kind.label(),
                            self.scene.flow.fluid.temperature_c,
                            fmt_si(self.scene.flow.velocity, "m/s", 2)
                        );
                        ui.label(egui::RichText::new(fluid).color(theme::TEXT_DIM).size(12.0));
                        let Some(b) = self.main_body() else { return };
                        let l = b.ref_length();
                        let re = units.reynolds(l);
                        let a = assess(&units, l / units.dx, re);
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(format!("{} · Re = {}", b.name, fmt_num(re, 2))).monospace());
                            ui.label(egui::RichText::new(a.fidelity.label()).size(11.0).color(fidelity_color(a.fidelity)));
                        });
                        let re_eff = units.reynolds_effective(l);
                        if re_eff < 0.95 * re {
                            ui.label(
                                egui::RichText::new(format!("siatka rozdziela Re ≈ {}", fmt_num(re_eff, 2)))
                                    .size(11.5)
                                    .color(theme::TEXT_DIM),
                            )
                            .on_hover_text(
                                "Lepkość numeryczna potrzebna do stabilności ogranicza efektywną liczbę Reynoldsa. Wynik jest jakościowy.",
                            );
                        }
                        if let Some(s) = self.history.get(b.id) {
                            let conv = l / units.u_inf;
                            if let Some(st) = stats(s, &units, l, (12.0 * conv).max(1e-9)) {
                                let ld = if st.cd_mean.abs() > 1e-6 { st.cl_mean / st.cd_mean } else { f64::NAN };
                                let mut t = format!("Cd {}   Cl {}", fmt_num(st.cd_mean, 3), fmt_num(st.cl_mean, 3));
                                if matches!(b.shape, Shape::Airfoil { .. }) {
                                    t += &format!("   Cm {}   L/D {}", fmt_num(st.cm_mean, 3), fmt_num(ld, 1));
                                }
                                ui.label(egui::RichText::new(t).monospace().color(theme::TEXT));
                                if let (Some(stv), Some(fr)) = (st.strouhal, st.freq) {
                                    ui.label(
                                        egui::RichText::new(format!("St {}   f = {}", fmt_num(stv, 3), fmt_si(fr, "Hz", 2))).monospace(),
                                    );
                                }
                            }
                        }
                        if let Some(f) = frame {
                            let conv = l / units.u_inf;
                            let tc = f.time / conv.max(1e-12);
                            let mut t = format!("t = {}  ({} c/U)", fmt_si(f.time, "s", 2), fmt_num(tc, 1));
                            if tc < 3.0 {
                                t += " · rozbieg";
                            }
                            ui.label(egui::RichText::new(t).size(11.5).color(theme::TEXT_DIM));
                        }
                    });
            });
    }

    fn draw_legend(&self, painter: &egui::Painter, rect: Rect) {
        let kind = self.prefs.field;
        if self.field_values.is_none() {
            if kind.needs_mean() {
                let p = Pos2::new(rect.right() - 16.0, rect.bottom() - 16.0);
                painter.text(p, Align2::RIGHT_BOTTOM, "Zbieranie średnich…", FontId::proportional(12.0), theme::TEXT_DIM);
            }
            return;
        }
        let (lo, hi) = self.field_range;
        let lut = self.prefs.colormap(kind).lut();
        let h = 170.0f32.min(rect.height() * 0.4);
        let bar = Rect::from_min_size(Pos2::new(rect.right() - 34.0, rect.bottom() - h - 34.0), Vec2::new(14.0, h));
        let bg = bar.expand2(Vec2::new(70.0, 26.0)).translate(Vec2::new(-52.0, -4.0));
        painter.rect_filled(bg, 8.0, Color32::from_rgba_unmultiplied(10, 13, 19, 205));
        let mut mesh = Mesh::default();
        let n = 48;
        for k in 0..=n {
            let t = k as f32 / n as f32;
            let y = bar.bottom() - t * bar.height();
            let c = lut[(t * 255.0) as usize];
            mesh.colored_vertex(Pos2::new(bar.left(), y), c);
            mesh.colored_vertex(Pos2::new(bar.right(), y), c);
            if k > 0 {
                let i = (k * 2) as u32;
                mesh.add_triangle(i - 2, i - 1, i);
                mesh.add_triangle(i - 1, i + 1, i);
            }
        }
        painter.add(EShape::mesh(mesh));
        painter.rect_stroke(bar, 0.0, Stroke::new(1.0_f32, Color32::from_gray(80)), StrokeKind::Outside);
        let unit = kind.unit();
        for k in 0..=4 {
            let t = k as f32 / 4.0;
            let v = lo + (hi - lo) * t;
            let y = bar.bottom() - t * bar.height();
            painter.line_segment([Pos2::new(bar.left() - 4.0, y), Pos2::new(bar.left(), y)], Stroke::new(1.0_f32, theme::TEXT_DIM));
            painter.text(
                Pos2::new(bar.left() - 6.0, y),
                Align2::RIGHT_CENTER,
                fmt_si(v as f64, unit, 2),
                FontId::monospace(10.5),
                theme::TEXT,
            );
        }
        painter.text(Pos2::new(bar.right(), bar.top() - 8.0), Align2::RIGHT_BOTTOM, kind.short(), FontId::proportional(11.5), theme::TEXT);
    }

    fn draw_hover(&self, painter: &egui::Painter, rect: Rect, resp: &egui::Response) {
        let (Some(w), Some(f), Some(pos)) = (self.hover, self.frame.as_ref(), resp.hover_pos()) else { return };
        if self.drag.is_some() || resp.context_menu_opened() {
            return;
        }
        let u = &f.units;
        let x = (w[0] / u.dx).floor();
        let y = (w[1] / u.dx).floor();
        if x < 0.0 || y < 0.0 || x >= f.w as f64 || y >= f.h as f64 {
            return;
        }
        let i = f.idx(x as usize, y as usize);
        let mut lines = vec![format!("x = {}  y = {}", fmt_si(w[0], "m", 3), fmt_si(w[1], "m", 3))];
        if f.labels[i] != 0 {
            let name = self.scene.bodies.get(f.labels[i] as usize - 1).map(|b| b.name.clone()).unwrap_or_default();
            lines.push(format!("ciało stałe: {name}"));
        } else {
            let vel = u.dx / u.dt;
            let (ux, uy) = (f.ux[i] as f64 * vel, f.uy[i] as f64 * vel);
            let p = u.pressure(3.0 * f.p[i] as f64);
            lines.push(format!("|u| = {}  ({:.2} U∞)", fmt_si(ux.hypot(uy), "m/s", 3), ux.hypot(uy) / u.u_inf));
            lines.push(format!("p − p∞ = {}   Cp = {}", fmt_si(p, "Pa", 3), fmt_num(p / u.q_inf(), 3)));
            if let Some(v) = &self.field_values {
                if !matches!(self.prefs.field, FieldKind::Speed | FieldKind::Pressure | FieldKind::Cp) {
                    lines.push(format!("{} = {}", self.prefs.field.short(), fmt_si(v[i] as f64, self.prefs.field.unit(), 3)));
                }
            }
        }
        let text = lines.join("\n");
        let g = painter.layout_no_wrap(text, FontId::monospace(11.5), theme::TEXT);
        let mut p = pos + Vec2::new(16.0, 16.0);
        if p.x + g.size().x > rect.right() - 8.0 {
            p.x = pos.x - g.size().x - 16.0;
        }
        if p.y + g.size().y > rect.bottom() - 8.0 {
            p.y = pos.y - g.size().y - 16.0;
        }
        painter.rect_filled(Rect::from_min_size(p, g.size()).expand(6.0), 6.0, Color32::from_rgba_unmultiplied(10, 13, 19, 225));
        painter.galley(p, g, theme::TEXT);
    }

    fn context_menu(&mut self, ui: &mut egui::Ui) {
        let w: Option<V2> = ui.ctx().data(|d| d.get_temp(egui::Id::new("ctx_world")));
        if let Some(id) = self.selected {
            let Some(b) = self.scene.body(id).cloned() else { return };
            ui.label(egui::RichText::new(format!("{} — {}", b.name, b.shape.kind_label())).strong());
            ui.separator();
            if b.rotatable() {
                let mut a = b.angle;
                ui.horizontal(|ui| {
                    ui.label("Kąt");
                    if ui.add(egui::DragValue::new(&mut a).speed(0.1).suffix("°").range(-180.0..=180.0)).changed() {
                        if let Some(bm) = self.scene.body_mut(id) {
                            bm.angle = a;
                        }
                    }
                    for d in [-5.0, -1.0, 1.0, 5.0] {
                        if ui.small_button(format!("{d:+}°")).clicked() {
                            self.rotate_selected(d);
                        }
                    }
                });
            }
            if ui.button("🗐  Duplikuj  (Ctrl+D)").clicked() {
                self.duplicate_selected();
                ui.close();
            }
            if ui.button("⇅  Odbij lustrzanie").clicked() {
                if let Some(bm) = self.scene.body_mut(id) {
                    bm.mirror = !bm.mirror;
                    bm.angle = -bm.angle;
                }
                ui.close();
            }
            if ui.button("↕  Wyśrodkuj w pionie").clicked() {
                let h = self.scene.flow.domain[1];
                if let Some(bm) = self.scene.body_mut(id) {
                    let p = bm.prepare();
                    let c = 0.5 * (p.bbox[1] + p.bbox[3]);
                    bm.translate([0.0, 0.5 * h - c]);
                }
                ui.close();
            }
            let lock = if b.locked { "🔓  Odblokuj" } else { "🔒  Zablokuj położenie" };
            if ui.button(lock).clicked() {
                if let Some(bm) = self.scene.body_mut(id) {
                    bm.locked = !bm.locked;
                }
                ui.close();
            }
            if ui.button("📈  Pokaż na wykresach").clicked() {
                self.chart_body = Some(id);
                self.prefs.show_bottom = true;
                ui.close();
            }
            if ui.button("⚙  Właściwości…").clicked() {
                self.side_tab = crate::app::SideTab::Objects;
                self.prefs.show_side = true;
                ui.close();
            }
            ui.separator();
            if ui.button(egui::RichText::new("🗑  Usuń  (Del)").color(theme::BAD)).clicked() {
                self.delete_selected();
                ui.close();
            }
        } else {
            if let Some(w) = w {
                if ui.button("✚  Postaw sondę tutaj").clicked() {
                    self.scene.probes.push(w);
                    ui.close();
                }
                if ui.button("☁  Dodaj źródło dymu tutaj").clicked() {
                    self.scene.emitters.push(w);
                    self.set_field(FieldKind::Smoke);
                    ui.close();
                }
            }
            if ui.button("⛶  Dopasuj widok  (Home)").clicked() {
                self.view.fit = true;
                ui.close();
            }
            if ui.button("⟲  Zresetuj przepływ  (R)").clicked() {
                self.reset_flow();
                ui.close();
            }
        }
    }
}

fn arrow(painter: &egui::Painter, from: Pos2, v: Vec2, col: Color32, label: &str) {
    let len = v.length();
    if len < 4.0 {
        return;
    }
    let dir = v / len;
    let to = from + v;
    let h = 9.0f32.min(len * 0.6);
    let n = Vec2::new(-dir.y, dir.x);
    painter.line_segment([from, to - dir * h * 0.6], Stroke::new(2.6_f32, col));
    painter.add(EShape::convex_polygon(vec![to, to - dir * h + n * h * 0.5, to - dir * h - n * h * 0.5], col, Stroke::NONE));
    painter.text(to + dir * 6.0 + Vec2::new(3.0, 0.0), Align2::LEFT_CENTER, label, FontId::proportional(12.0), col);
}

pub fn probe_color(k: usize) -> Color32 {
    const C: [Color32; 6] = [
        Color32::from_rgb(250, 204, 21),
        Color32::from_rgb(244, 114, 182),
        Color32::from_rgb(52, 211, 153),
        Color32::from_rgb(167, 139, 250),
        Color32::from_rgb(251, 146, 60),
        Color32::from_rgb(96, 165, 250),
    ];
    C[k % C.len()]
}
