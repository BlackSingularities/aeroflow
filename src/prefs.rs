//! Preferencje użytkownika zapisywane między uruchomieniami.

use serde::{Deserialize, Serialize};

use crate::render::colormap::Colormap;
use crate::render::field::FieldKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpeedPref {
    Auto,
    Fixed(usize),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub field: FieldKind,
    pub colormaps: Vec<(FieldKind, Colormap)>,
    pub manual_ranges: Vec<(FieldKind, [f32; 2])>,
    pub particles: bool,
    pub particle_count: usize,
    pub trail_len: usize,
    pub particle_speed_color: bool,
    pub streamlines: bool,
    pub streamline_spacing: f32,
    pub vectors: bool,
    pub vector_spacing: usize,
    pub show_forces: bool,
    pub show_legend: bool,
    pub show_hud: bool,
    pub show_probes: bool,
    pub smoke_rake: bool,
    pub speed: SpeedPref,
    pub brush_cells: f32,
    pub naca: String,
    pub show_side: bool,
    pub show_bottom: bool,
    pub ui_scale: f32,
    pub check_updates: bool,
    pub skipped_version: String,
    pub last_version: String,
    pub last_scene: Option<String>,
    pub recent_files: Vec<String>,
    /// Opis zmian pobranej aktualizacji (wersja, tekst) — pokazywany po instalacji.
    pub update_notes: Option<(String, String)>,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            field: FieldKind::Speed,
            colormaps: Vec::new(),
            manual_ranges: Vec::new(),
            particles: true,
            particle_count: 2500,
            trail_len: 10,
            particle_speed_color: false,
            streamlines: false,
            streamline_spacing: 10.0,
            vectors: false,
            vector_spacing: 14,
            show_forces: true,
            show_legend: true,
            show_hud: true,
            show_probes: true,
            smoke_rake: true,
            speed: SpeedPref::Auto,
            brush_cells: 3.0,
            naca: "2412".into(),
            show_side: true,
            show_bottom: true,
            ui_scale: 1.0,
            check_updates: true,
            skipped_version: String::new(),
            last_version: String::new(),
            last_scene: None,
            recent_files: Vec::new(),
            update_notes: None,
        }
    }
}

impl Prefs {
    pub fn colormap(&self, f: FieldKind) -> Colormap {
        self.colormaps.iter().find(|(k, _)| *k == f).map(|(_, c)| *c).unwrap_or(f.default_colormap())
    }

    pub fn set_colormap(&mut self, f: FieldKind, c: Colormap) {
        self.colormaps.retain(|(k, _)| *k != f);
        if c != f.default_colormap() {
            self.colormaps.push((f, c));
        }
    }

    pub fn manual_range(&self, f: FieldKind) -> Option<[f32; 2]> {
        self.manual_ranges.iter().find(|(k, _)| *k == f).map(|(_, r)| *r)
    }

    pub fn set_manual_range(&mut self, f: FieldKind, r: Option<[f32; 2]>) {
        self.manual_ranges.retain(|(k, _)| *k != f);
        if let Some(r) = r {
            self.manual_ranges.push((f, r));
        }
    }

    pub fn add_recent(&mut self, path: &str) {
        self.recent_files.retain(|p| p != path);
        self.recent_files.insert(0, path.to_string());
        self.recent_files.truncate(8);
    }
}
