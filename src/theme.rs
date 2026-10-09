use eframe::egui::epaint::text::VariationCoords;
use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontTweak, Stroke, Theme};
use std::path::PathBuf;
use std::sync::Arc;

pub const BG: Color32 = Color32::from_rgb(10, 10, 14);
pub const SURFACE: Color32 = Color32::from_rgb(22, 22, 30);
pub const PLAYER_BG: Color32 = Color32::from_rgb(15, 15, 21);
pub const SURFACE_HI: Color32 = Color32::from_rgb(32, 32, 44);
pub const TEXT: Color32 = Color32::from_rgb(242, 242, 247);
pub const MUTED: Color32 = Color32::from_rgb(140, 140, 158);
pub const FAINT: Color32 = Color32::from_rgb(78, 78, 94);
pub const ACCENT: Color32 =
    Color32::from_rgb(crate::icon::ACCENT_RGB[0], crate::icon::ACCENT_RGB[1], crate::icon::ACCENT_RGB[2]);
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(255, 120, 140);

pub const SEMIBOLD: &str = "semibold";
pub const BOLD: &str = "bold";

pub fn semibold() -> FontFamily {
    FontFamily::Name(SEMIBOLD.into())
}

pub fn bold() -> FontFamily {
    FontFamily::Name(BOLD.into())
}

pub fn install(ctx: &egui::Context) {
    install_fonts(ctx);
    ctx.set_theme(Theme::Dark);
    ctx.style_mut_of(Theme::Dark, |style| {
        let v = &mut style.visuals;
        v.panel_fill = BG;
        v.window_fill = SURFACE;
        v.extreme_bg_color = SURFACE;
        v.override_text_color = Some(TEXT);
        v.selection.bg_fill = ACCENT.gamma_multiply(0.45);
        v.selection.stroke = Stroke::new(1.0, ACCENT);
        v.text_cursor.stroke = Stroke::new(2.0, ACCENT);
        v.hyperlink_color = ACCENT_SOFT;
        for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active]
        {
            w.corner_radius = CornerRadius::same(10);
        }
        style.spacing.scroll.bar_width = 6.0;
        style.spacing.scroll.floating = true;
    });
}

struct Face {
    file: PathBuf,
    weight: Option<f32>,
}

/// System UI fonts per weight, best first: Segoe UI on Windows, San Francisco on macOS.
fn windows_fonts() -> PathBuf {
    std::env::var_os("WINDIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows")).join("Fonts")
}

fn system_faces(weight: f32) -> Vec<Face> {
    let segoe = match weight {
        w if w >= 700.0 => "segoeuib.ttf",
        w if w >= 600.0 => "seguisb.ttf",
        _ => "segoeui.ttf",
    };
    vec![
        Face { file: windows_fonts().join(segoe), weight: None },
        Face { file: PathBuf::from("/System/Library/Fonts/SFNS.ttf"), weight: Some(weight) },
    ]
}

fn load_face(faces: Vec<Face>) -> Option<FontData> {
    faces.into_iter().find_map(|face| {
        let data = FontData::from_owned(std::fs::read(&face.file).ok()?);
        Some(match face.weight {
            Some(w) => data.tweak(FontTweak { coords: VariationCoords::new([(b"wght", w)]), ..Default::default() }),
            None => data,
        })
    })
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let base = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    if let Ok(bytes) = std::fs::read(windows_fonts().join("seguisym.ttf")) {
        fonts.font_data.insert("symbols".into(), Arc::new(FontData::from_owned(bytes)));
    }
    let has_symbols = fonts.font_data.contains_key("symbols");

    for (family, key, weight) in [
        (FontFamily::Proportional, "ui-regular", 400.0),
        (semibold(), "ui-semibold", 600.0),
        (bold(), "ui-bold", 700.0),
    ] {
        let mut chain = Vec::new();
        if let Some(data) = load_face(system_faces(weight)) {
            fonts.font_data.insert(key.into(), Arc::new(data));
            chain.push(key.to_owned());
        }
        chain.extend(base.iter().cloned());
        if has_symbols {
            chain.push("symbols".into());
        }
        fonts.families.insert(family, chain);
    }
    ctx.set_fonts(fonts);
}

pub fn app_icon() -> egui::IconData {
    const N: usize = 64;
    egui::IconData { rgba: crate::icon::rgba(N), width: N as u32, height: N as u32 }
}
