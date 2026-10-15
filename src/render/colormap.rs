//! Mapy kolorów (tablice 256 kolorów).

use eframe::egui::Color32;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Colormap {
    Turbo,
    Viridis,
    Inferno,
    Ocean,
    CoolWarm,
    IceFire,
    Gray,
}

impl Colormap {
    pub const ALL: [Colormap; 7] =
        [Colormap::Turbo, Colormap::Viridis, Colormap::Inferno, Colormap::Ocean, Colormap::CoolWarm, Colormap::IceFire, Colormap::Gray];

    pub fn label(self) -> &'static str {
        match self {
            Colormap::Turbo => "Turbo",
            Colormap::Viridis => "Viridis",
            Colormap::Inferno => "Inferno",
            Colormap::Ocean => "Ocean",
            Colormap::CoolWarm => "Coolwarm (rozbieżna)",
            Colormap::IceFire => "Ice-Fire (rozbieżna)",
            Colormap::Gray => "Skala szarości",
        }
    }

    pub fn lut(self) -> [Color32; 256] {
        let mut out = [Color32::BLACK; 256];
        for (i, c) in out.iter_mut().enumerate() {
            let t = i as f32 / 255.0;
            *c = match self {
                Colormap::Turbo => turbo(t),
                Colormap::Viridis => stops(
                    t,
                    &[
                        [68, 1, 84],
                        [71, 44, 122],
                        [59, 81, 139],
                        [44, 113, 142],
                        [33, 144, 141],
                        [39, 173, 129],
                        [92, 200, 99],
                        [170, 220, 50],
                        [253, 231, 37],
                    ],
                ),
                Colormap::Inferno => stops(
                    t,
                    &[
                        [0, 0, 4],
                        [27, 12, 65],
                        [74, 12, 107],
                        [120, 28, 109],
                        [165, 44, 96],
                        [207, 68, 70],
                        [237, 105, 37],
                        [251, 155, 6],
                        [247, 209, 61],
                        [252, 255, 164],
                    ],
                ),
                Colormap::Ocean => stops(
                    t,
                    &[
                        [4, 6, 16],
                        [9, 24, 58],
                        [14, 52, 108],
                        [20, 90, 150],
                        [30, 132, 182],
                        [70, 182, 214],
                        [150, 226, 240],
                        [240, 252, 255],
                    ],
                ),
                Colormap::CoolWarm => stops(
                    t,
                    &[
                        [59, 76, 192],
                        [98, 130, 234],
                        [141, 176, 254],
                        [184, 208, 249],
                        [221, 221, 221],
                        [245, 196, 173],
                        [244, 154, 123],
                        [222, 96, 77],
                        [180, 4, 38],
                    ],
                ),
                Colormap::IceFire => stops(
                    t,
                    &[
                        [180, 240, 255],
                        [60, 150, 230],
                        [24, 60, 140],
                        [12, 16, 34],
                        [8, 8, 12],
                        [60, 14, 18],
                        [170, 40, 30],
                        [240, 120, 40],
                        [255, 230, 150],
                    ],
                ),
                Colormap::Gray => {
                    let v = (t * 255.0) as u8;
                    Color32::from_rgb(v, v, v)
                }
            };
        }
        out
    }
}

fn stops(t: f32, s: &[[u8; 3]]) -> Color32 {
    let x = t.clamp(0.0, 1.0) * (s.len() - 1) as f32;
    let i = (x as usize).min(s.len() - 2);
    let f = x - i as f32;
    let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * f).round() as u8;
    Color32::from_rgb(mix(s[i][0], s[i + 1][0]), mix(s[i][1], s[i + 1][1]), mix(s[i][2], s[i + 1][2]))
}

/// Przybliżenie wielomianowe mapy Turbo (Google, 2019).
fn turbo(t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0) as f64;
    let r = 0.13572138 + t * (4.61539260 + t * (-42.66032258 + t * (132.13108234 + t * (-152.94239396 + t * 59.28637943))));
    let g = 0.09140261 + t * (2.19418839 + t * (4.84296658 + t * (-14.18503333 + t * (4.27729857 + t * 2.82956604))));
    let b = 0.10667330 + t * (12.64194608 + t * (-60.58204836 + t * (110.36276771 + t * (-89.90310912 + t * 27.34824973))));
    let c = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgb(c(r), c(g), c(b))
}
