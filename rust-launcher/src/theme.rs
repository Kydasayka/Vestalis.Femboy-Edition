//! Theme tokens, built-in palettes and user theme loading.
//!
//! Themes are plain JSON files in `%APPDATA%\Vestalis\rust-launcher\themes`.
//! Every token is optional; missing values fall back to the dark palette so a
//! half-written theme still produces a usable UI.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::model::app_dir;

pub const DEFAULT_THEME: &str = "dark";
pub const THEME_DIR: &str = "themes";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Theme {
    pub background: String,
    pub background_alt: String,
    pub surface: String,
    pub surface_hover: String,
    pub border: String,
    pub text: String,
    pub text_muted: String,
    pub accent: String,
    pub accent_hover: String,
    pub accent_text: String,
    pub success: String,
    pub warning: String,
    pub danger: String,
    pub gradient_start: String,
    pub gradient_end: String,
    pub radius_xs: u32,
    pub radius_sm: u32,
    pub radius_md: u32,
    pub radius_lg: u32,
    pub radius_pill: u32,
    pub card_opacity: f32,
    pub font_family: String,
    pub base_font_weight: i32,
    pub background_effect: String,
    /// Extra tokens used for window shadows and translucent overlays.
    pub shadow: String,
    pub scrim: String,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            background: "#0d0d10".into(),
            background_alt: "#09090c".into(),
            surface: "#16161c".into(),
            surface_hover: "#202028".into(),
            border: "#2a2a33".into(),
            text: "#f2f2f7".into(),
            text_muted: "#b9b9c4".into(),
            accent: "#a21caf".into(),
            accent_hover: "#d946ef".into(),
            accent_text: "#ffffff".into(),
            success: "#4ade80".into(),
            warning: "#fbbf24".into(),
            danger: "#f87171".into(),
            gradient_start: "#a21caf".into(),
            gradient_end: "#6d28d9".into(),
            radius_xs: 3,
            radius_sm: 8,
            radius_md: 11,
            radius_lg: 14,
            radius_pill: 99,
            card_opacity: 0.92,
            font_family: "Nunito".into(),
            base_font_weight: 600,
            background_effect: "none".into(),
            shadow: "#00000080".into(),
            scrim: "#00000000".into(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ThemeItem {
    pub name: String,
    pub builtin: bool,
    pub theme: Theme,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct ThemeFile {
    name: String,
    theme: Theme,
}

/// Parses `#RRGGBB` or `#RRGGBBAA` (Slint's alpha-last convention).
pub fn parse_color(value: &str) -> Option<slint::Color> {
    let hex = value.trim().strip_prefix('#')?;
    let byte = |slice: &str| u8::from_str_radix(slice, 16).ok();

    match hex.len() {
        6 => Some(slint::Color::from_rgb_u8(
            byte(&hex[0..2])?,
            byte(&hex[2..4])?,
            byte(&hex[4..6])?,
        )),
        8 => Some(slint::Color::from_argb_u8(
            byte(&hex[6..8])?,
            byte(&hex[0..2])?,
            byte(&hex[2..4])?,
            byte(&hex[4..6])?,
        )),
        _ => None,
    }
}

impl Theme {
    /// Replaces invalid tokens with the dark-theme value instead of failing the
    /// whole theme. The origin is only used for logging.
    pub fn validated(mut self, origin: &str) -> Self {
        let defaults = Theme::default();

        macro_rules! check_color {
            ($field:ident) => {
                if parse_color(&self.$field).is_none() {
                    warn!(
                        theme = origin,
                        field = stringify!($field),
                        value = %self.$field,
                        "invalid theme color, using dark"
                    );
                    self.$field = defaults.$field.clone();
                }
            };
        }

        check_color!(background);
        check_color!(background_alt);
        check_color!(surface);
        check_color!(surface_hover);
        check_color!(border);
        check_color!(text);
        check_color!(text_muted);
        check_color!(accent);
        check_color!(accent_hover);
        check_color!(accent_text);
        check_color!(success);
        check_color!(warning);
        check_color!(danger);
        check_color!(gradient_start);
        check_color!(gradient_end);
        check_color!(shadow);
        check_color!(scrim);

        self.radius_xs = self.radius_xs.min(64);
        self.radius_sm = self.radius_sm.min(64);
        self.radius_md = self.radius_md.min(64);
        self.radius_lg = self.radius_lg.min(64);
        self.radius_pill = self.radius_pill.clamp(1, 999);
        self.card_opacity = self.card_opacity.clamp(0.0, 1.0);
        self.base_font_weight = self.base_font_weight.clamp(100, 900);

        if !matches!(
            self.background_effect.as_str(),
            "none" | "fireflies" | "leaves" | "snow"
        ) {
            warn!(
                theme = origin,
                value = %self.background_effect,
                "unknown background effect, using none"
            );
            self.background_effect = defaults.background_effect;
        }

        if self.font_family.trim().is_empty() {
            self.font_family = defaults.font_family;
        }

        self
    }
}

fn dark_theme() -> Theme {
    Theme::default()
}

fn light_theme() -> Theme {
    Theme {
        background: "#f4f1f7".into(),
        background_alt: "#ebe5f1".into(),
        surface: "#ffffff".into(),
        surface_hover: "#f0eaf6".into(),
        border: "#d8cfe3".into(),
        text: "#1c1724".into(),
        text_muted: "#5b5566".into(),
        accent: "#a21caf".into(),
        accent_hover: "#c026d3".into(),
        accent_text: "#ffffff".into(),
        success: "#15803d".into(),
        warning: "#b45309".into(),
        danger: "#b91c1c".into(),
        gradient_start: "#a21caf".into(),
        gradient_end: "#6d28d9".into(),
        card_opacity: 0.98,
        background_effect: "none".into(),
        shadow: "#00000026".into(),
        ..Theme::default()
    }
}

fn femboy_theme() -> Theme {
    Theme {
        background: "#140b1a".into(),
        background_alt: "#0f0714".into(),
        surface: "#1e1026".into(),
        surface_hover: "#2a1634".into(),
        border: "#3a2145".into(),
        text: "#fff0fb".into(),
        text_muted: "#d8b4e6".into(),
        accent: "#d946ef".into(),
        accent_hover: "#e879f9".into(),
        accent_text: "#2a0a33".into(),
        success: "#86efac".into(),
        warning: "#fbbf24".into(),
        danger: "#fb7185".into(),
        gradient_start: "#a21caf".into(),
        gradient_end: "#ec4899".into(),
        card_opacity: 0.92,
        background_effect: "fireflies".into(),
        shadow: "#00000080".into(),
        ..Theme::default()
    }
}

fn ocean_theme() -> Theme {
    Theme {
        background: "#06101a".into(),
        background_alt: "#040b12".into(),
        surface: "#0c1c2b".into(),
        surface_hover: "#122a40".into(),
        border: "#1c3a54".into(),
        text: "#e6f2ff".into(),
        text_muted: "#9cc2df".into(),
        accent: "#0ea5e9".into(),
        accent_hover: "#38bdf8".into(),
        accent_text: "#04121f".into(),
        success: "#34d399".into(),
        warning: "#fbbf24".into(),
        danger: "#fb7185".into(),
        gradient_start: "#0ea5e9".into(),
        gradient_end: "#2563eb".into(),
        card_opacity: 0.92,
        background_effect: "snow".into(),
        shadow: "#00000080".into(),
        ..Theme::default()
    }
}

pub fn builtin_themes() -> Vec<ThemeItem> {
    vec![
        ThemeItem {
            name: "dark".into(),
            builtin: true,
            theme: dark_theme(),
        },
        ThemeItem {
            name: "light".into(),
            builtin: true,
            theme: light_theme(),
        },
        ThemeItem {
            name: "femboy".into(),
            builtin: true,
            theme: femboy_theme(),
        },
        ThemeItem {
            name: "ocean".into(),
            builtin: true,
            theme: ocean_theme(),
        },
    ]
}

pub fn themes_dir() -> PathBuf {
    app_dir().join("rust-launcher").join(THEME_DIR)
}

pub fn ensure_themes_dir() -> std::io::Result<PathBuf> {
    let dir = themes_dir();
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Loads built-ins plus every valid `*.json` directly inside the themes folder.
/// Symlinks and files outside the folder are ignored.
pub fn load_all() -> Vec<ThemeItem> {
    let mut items = builtin_themes();
    let dir = themes_dir();

    let Ok(entries) = fs::read_dir(&dir) else {
        return items;
    };

    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_file() {
            continue;
        }

        let path = entry.path();
        let is_json = path
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| value.eq_ignore_ascii_case("json"))
            .unwrap_or(false);
        if !is_json {
            continue;
        }

        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(error) => {
                warn!(file = %path.display(), %error, "theme file is not readable");
                continue;
            }
        };
        if metadata.len() > 256 * 1024 {
            warn!(file = %path.display(), "theme file is too large, skipped");
            continue;
        }

        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) => {
                warn!(file = %path.display(), %error, "failed to read theme file");
                continue;
            }
        };

        // Notepad and PowerShell commonly write a UTF-8 BOM.
        let parsed: ThemeFile = match serde_json::from_str(text.trim_start_matches('\u{feff}')) {
            Ok(parsed) => parsed,
            Err(error) => {
                warn!(file = %path.display(), %error, "invalid theme json, skipped");
                continue;
            }
        };

        let stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("custom")
            .to_string();
        let name = if parsed.name.trim().is_empty() {
            stem
        } else {
            parsed.name.trim().to_string()
        };

        if items
            .iter()
            .any(|item| item.name.eq_ignore_ascii_case(&name))
        {
            warn!(
                file = %path.display(),
                theme = %name,
                "theme name collides with an existing theme, skipped"
            );
            continue;
        }

        let theme = parsed.theme.validated(&name);
        info!(file = %path.display(), theme = %name, "user theme loaded");
        items.push(ThemeItem {
            name,
            builtin: false,
            theme,
        });
    }

    items
}

pub fn resolve<'a>(name: &str, items: &'a [ThemeItem]) -> &'a ThemeItem {
    items
        .iter()
        .find(|item| item.name.eq_ignore_ascii_case(name.trim()))
        .unwrap_or_else(|| {
            warn!(requested = name, "unknown theme, using dark");
            items
                .iter()
                .find(|item| item.name == DEFAULT_THEME)
                .unwrap_or(&items[0])
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(color: &str) -> f32 {
        let parsed = parse_color(color).expect("valid color");
        let rgba = parsed.to_argb_u8();
        let channel = |value: u8| {
            let value = value as f32 / 255.0;
            if value <= 0.03928 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(rgba.red) + 0.7152 * channel(rgba.green) + 0.0722 * channel(rgba.blue)
    }

    fn contrast(a: &str, b: &str) -> f32 {
        let (first, second) = (luminance(a), luminance(b));
        let (lighter, darker) = if first > second {
            (first, second)
        } else {
            (second, first)
        };
        (lighter + 0.05) / (darker + 0.05)
    }

    #[test]
    fn builtin_text_is_readable() {
        for item in builtin_themes() {
            for background in [&item.theme.background, &item.theme.surface] {
                let ratio = contrast(&item.theme.text, background);
                assert!(
                    ratio >= 4.5,
                    "{} text on {} is only {:.2}:1",
                    item.name,
                    background,
                    ratio
                );
            }
        }
    }

    #[test]
    fn invalid_colors_fall_back_to_dark() {
        let theme = Theme {
            background: "not-a-color".into(),
            accent: "#123".into(),
            ..Theme::default()
        }
        .validated("test");

        assert_eq!(theme.background, Theme::default().background);
        assert_eq!(theme.accent, Theme::default().accent);
    }

    #[test]
    fn color_parser_supports_rgb_and_rgba() {
        assert!(parse_color("#0d0d10").is_some());
        assert!(parse_color("#0d0d10cc").is_some());
        assert!(parse_color("0d0d10").is_none());
        assert!(parse_color("#0d0d1").is_none());
    }

    #[test]
    fn every_builtin_has_a_valid_effect() {
        for item in builtin_themes() {
            assert!(matches!(
                item.theme.background_effect.as_str(),
                "none" | "fireflies" | "leaves" | "snow"
            ));
        }
    }
}
