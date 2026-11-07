//! Theme tokens, built-in palettes and user theme loading.
//!
//! Themes are plain JSON files in `%APPDATA%\Vestalis\rust-launcher\themes`.
//! Every token is optional; missing values fall back to the dark palette so a
//! half-written theme still produces a usable UI.

use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

use crate::model::app_dir;

pub const DEFAULT_THEME: &str = "dark";
pub const WINDOWS_THEME: &str = "Windows";
pub const THEME_DIR: &str = "themes";
pub const MODES: [&str; 3] = ["base", "material3", "liquid-glass"];

pub const ICON_IDS: &[&str] = &[
    "server", "chevron", "play", "settings", "mods", "refresh", "minimize", "maximize", "restore",
    "close", "warning", "error", "success",
];

/// Only user-facing strings are themeable. Errors, status details and window
/// controls stay in the launcher so a theme cannot break critical messages.
pub const TEXT_KEYS: &[&str] = &[
    "app.title",
    "app.subtitle",
    "login.title",
    "login.subtitle",
    "login.submit",
    "login.offline",
    "buttons.play",
    "buttons.settings",
    "buttons.logout",
    "buttons.refresh",
    "buttons.mods",
    "sections.profiles",
    "profiles.search",
    "profiles.not_found",
    "sections.news",
    "settings.title",
    "settings.back",
    "login.username",
    "login.password",
    "login.hwid",
    "login.2fa",
    "login.2fa_submit",
    "login.busy",
    "login.checking",
    "login.profile_select",
    "buttons.save",
    "buttons.apply",
    "buttons.pick",
    "buttons.load",
    "buttons.delete",
    "buttons.rename",
    "buttons.download",
    "buttons.downloading",
    "buttons.cancel",
    "settings.interface.discord",
    "settings.utility.install",
    "settings.utility.logs",
    "settings.utility.reset_window",
    "settings.utility.check_files",
    "settings.utility.clear_cache",
    "settings.memory.title",
    "settings.memory.desc",
    "settings.memory.xms",
    "settings.memory.xmx",
    "settings.jvm.title",
    "settings.jvm.desc",
    "settings.game.title",
    "settings.game.fullscreen",
    "settings.game.resolution",
    "settings.game.java",
    "settings.interface.auto_login",
    "settings.interface.title",
    "settings.interface.sounds",
    "settings.interface.hwid",
    "settings.interface.hide_on_launch",
    "settings.interface.background",
    "settings.interface.effects",
    "settings.appearance.title",
    "settings.appearance.desc",
    "settings.appearance.mode",
    "settings.appearance.accent_hint",
    "settings.appearance.windows",
    "settings.appearance.manual",
    "settings.appearance.follow",
    "settings.appearance.light",
    "settings.appearance.dark",
    "settings.appearance.builtin",
    "settings.appearance.custom",
    "settings.appearance.refresh",
    "settings.appearance.open",
    "settings.appearance.reset",
    "settings.language.title",
    "settings.language.hint",
    "layout.title",
    "layout.desc",
    "layout.zone_left",
    "layout.zone_right",
    "layout.zone_header",
    "layout.zone_content_top",
    "layout.zone_content_bottom",
    "layout.zone_footer",
    "layout.widths",
    "layout.width_left",
    "layout.width_right",
    "layout.profiles",
    "layout.profile_item",
    "layout.active",
    "layout.name_placeholder",
    "layout.save_profile",
    "layout.reset",
    "layout.server",
    "layout.user",
    "layout.profiles",
    "layout.countdown",
    "layout.sidebar_actions",
    "layout.news",
    "layout.main_actions",
    "layout.pinned",
    "layout.hidden",
    "layout.visible",
    "layout.empty",
    "mods.title",
    "mods.count",
    "mods.refresh",
    "mods.desc",
    "mods.empty",
    "mods.search",
    "mods.not_found",
    "mods.folder",
];

/// sRGB triplet, 0..=255 per channel.
pub type Rgb = (u8, u8, u8);

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Theme {
    pub mode: String,
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

    // ---- Material 3 tonal roles ---------------------------------------
    pub primary_container: String,
    pub on_primary_container: String,
    pub secondary: String,
    pub on_secondary: String,
    pub tertiary: String,
    pub on_tertiary: String,
    pub surface_low: String,
    pub surface_high: String,
    pub surface_variant: String,
    pub on_surface_variant: String,
    pub outline: String,
    pub error: String,
    pub on_error: String,

    // ---- Liquid Glass -------------------------------------------------
    pub glass_tint: String,
    pub glass_border: String,
    pub glass_highlight: String,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            mode: "base".into(),
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

            primary_container: "#4a044e".into(),
            on_primary_container: "#f5d0fe".into(),
            secondary: "#c084fc".into(),
            on_secondary: "#1a0a20".into(),
            tertiary: "#f472b6".into(),
            on_tertiary: "#2a0a18".into(),
            surface_low: "#101015".into(),
            surface_high: "#202029".into(),
            surface_variant: "#26262e".into(),
            on_surface_variant: "#c4c4cf".into(),
            outline: "#4a4a55".into(),
            error: "#f87171".into(),
            on_error: "#2a0a0a".into(),

            glass_tint: "#ffffff14".into(),
            glass_border: "#ffffff2e".into(),
            glass_highlight: "#ffffff1f".into(),
        }
    }
}

impl Theme {
    /// Intermediate palette used by the theme cross-fade. Colors are mixed in
    /// straight sRGB, which is what the eye expects for short UI transitions.
    pub fn mix(from: &Theme, to: &Theme, t: f32) -> Theme {
        let t = t.clamp(0.0, 1.0);
        let mix = |a: &str, b: &str| mix_color(a, b, t);
        let radius = |a: u32, b: u32| -> u32 {
            (a as f32 + (b as f32 - a as f32) * t).round().max(0.0) as u32
        };

        Theme {
            mode: if t >= 1.0 {
                to.mode.clone()
            } else {
                from.mode.clone()
            },
            background: mix(&from.background, &to.background),
            background_alt: mix(&from.background_alt, &to.background_alt),
            surface: mix(&from.surface, &to.surface),
            surface_hover: mix(&from.surface_hover, &to.surface_hover),
            border: mix(&from.border, &to.border),
            text: mix(&from.text, &to.text),
            text_muted: mix(&from.text_muted, &to.text_muted),
            accent: mix(&from.accent, &to.accent),
            accent_hover: mix(&from.accent_hover, &to.accent_hover),
            accent_text: mix(&from.accent_text, &to.accent_text),
            success: mix(&from.success, &to.success),
            warning: mix(&from.warning, &to.warning),
            danger: mix(&from.danger, &to.danger),
            gradient_start: mix(&from.gradient_start, &to.gradient_start),
            gradient_end: mix(&from.gradient_end, &to.gradient_end),
            radius_xs: radius(from.radius_xs, to.radius_xs),
            radius_sm: radius(from.radius_sm, to.radius_sm),
            radius_md: radius(from.radius_md, to.radius_md),
            radius_lg: radius(from.radius_lg, to.radius_lg),
            radius_pill: radius(from.radius_pill, to.radius_pill),
            card_opacity: from.card_opacity + (to.card_opacity - from.card_opacity) * t,
            font_family: to.font_family.clone(),
            base_font_weight: to.base_font_weight,
            background_effect: to.background_effect.clone(),
            shadow: mix(&from.shadow, &to.shadow),
            scrim: mix(&from.scrim, &to.scrim),
            primary_container: mix(&from.primary_container, &to.primary_container),
            on_primary_container: mix(&from.on_primary_container, &to.on_primary_container),
            secondary: mix(&from.secondary, &to.secondary),
            on_secondary: mix(&from.on_secondary, &to.on_secondary),
            tertiary: mix(&from.tertiary, &to.tertiary),
            on_tertiary: mix(&from.on_tertiary, &to.on_tertiary),
            surface_low: mix(&from.surface_low, &to.surface_low),
            surface_high: mix(&from.surface_high, &to.surface_high),
            surface_variant: mix(&from.surface_variant, &to.surface_variant),
            on_surface_variant: mix(&from.on_surface_variant, &to.on_surface_variant),
            outline: mix(&from.outline, &to.outline),
            error: mix(&from.error, &to.error),
            on_error: mix(&from.on_error, &to.on_error),
            glass_tint: mix(&from.glass_tint, &to.glass_tint),
            glass_border: mix(&from.glass_border, &to.glass_border),
            glass_highlight: mix(&from.glass_highlight, &to.glass_highlight),
        }
    }
}

/// Mixes `#RRGGBB` / `#RRGGBBAA` values and keeps the alpha channel when any
/// input is translucent. Invalid values switch at the end of the transition.
fn mix_color(from: &str, to: &str, t: f32) -> String {
    let parse = |value: &str| -> Option<(u8, u8, u8, u8)> {
        let hex = value.trim().strip_prefix('#')?;
        let byte = |slice: &str| u8::from_str_radix(slice, 16).ok();
        let alpha = if hex.len() == 8 {
            byte(hex.get(6..8)?)?
        } else {
            255
        };
        Some((
            byte(hex.get(0..2)?)?,
            byte(hex.get(2..4)?)?,
            byte(hex.get(4..6)?)?,
            alpha,
        ))
    };

    let (Some(a), Some(b)) = (parse(from), parse(to)) else {
        return if t >= 1.0 { to } else { from }.to_string();
    };

    let channel = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
    let (r, g, bl, al) = (
        channel(a.0, b.0),
        channel(a.1, b.1),
        channel(a.2, b.2),
        channel(a.3, b.3),
    );

    if a.3 == 255 && b.3 == 255 {
        format!("#{r:02X}{g:02X}{bl:02X}")
    } else {
        format!("#{r:02X}{g:02X}{bl:02X}{al:02X}")
    }
}

#[derive(Clone, Debug)]
pub struct ThemeItem {
    pub name: String,
    pub builtin: bool,
    pub theme: Theme,
    /// Resolved absolute paths for semantic icon IDs.
    pub icons: HashMap<String, String>,
    /// Validated user-facing text overrides.
    pub texts: HashMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct ThemeFile {
    name: String,
    mode: String,
    theme: Theme,
    icons: HashMap<String, String>,
    texts: HashMap<String, String>,
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
        check_color!(primary_container);
        check_color!(on_primary_container);
        check_color!(secondary);
        check_color!(on_secondary);
        check_color!(tertiary);
        check_color!(on_tertiary);
        check_color!(surface_low);
        check_color!(surface_high);
        check_color!(surface_variant);
        check_color!(on_surface_variant);
        check_color!(outline);
        check_color!(error);
        check_color!(on_error);
        check_color!(glass_tint);
        check_color!(glass_border);
        check_color!(glass_highlight);

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

        if !MODES.contains(&self.mode.as_str()) {
            warn!(
                theme = origin,
                value = %self.mode,
                "unknown theme mode, using base"
            );
            self.mode = "base".into();
        }

        self
    }
}

fn dark_theme() -> Theme {
    Theme::default()
}

fn apple_theme() -> Theme {
    Theme {
        background: "#f5f5f7".into(),
        background_alt: "#ececee".into(),
        surface: "#ffffff".into(),
        surface_hover: "#f0f0f3".into(),
        border: "#d2d2d7".into(),
        text: "#1d1d1f".into(),
        text_muted: "#5f5f66".into(),
        accent: "#007aff".into(),
        accent_hover: "#0a84ff".into(),
        accent_text: "#ffffff".into(),
        success: "#1f9d55".into(),
        warning: "#b45309".into(),
        danger: "#c62828".into(),
        gradient_start: "#0a84ff".into(),
        gradient_end: "#5e5ce6".into(),
        radius_xs: 6,
        radius_sm: 12,
        radius_md: 18,
        radius_lg: 24,
        card_opacity: 0.98,
        background_effect: "none".into(),
        shadow: "#0000001F".into(),
        ..Theme::default()
    }
}

fn discord_theme() -> Theme {
    Theme {
        background: "#1e1f22".into(),
        background_alt: "#17181a".into(),
        surface: "#2b2d31".into(),
        surface_hover: "#35373c".into(),
        border: "#3f4147".into(),
        text: "#f2f3f5".into(),
        text_muted: "#b5bac1".into(),
        accent: "#5865f2".into(),
        accent_hover: "#7289da".into(),
        accent_text: "#ffffff".into(),
        success: "#23a55a".into(),
        warning: "#f0b232".into(),
        danger: "#f23f43".into(),
        gradient_start: "#5865f2".into(),
        gradient_end: "#9b59b6".into(),
        card_opacity: 0.94,
        background_effect: "none".into(),
        ..Theme::default()
    }
}

fn minecraft_theme() -> Theme {
    Theme {
        background: "#14180f".into(),
        background_alt: "#0e120a".into(),
        surface: "#1e2617".into(),
        surface_hover: "#27331d".into(),
        border: "#34452a".into(),
        text: "#f2f7ea".into(),
        text_muted: "#b8c8a7".into(),
        accent: "#78b84a".into(),
        accent_hover: "#9bd86a".into(),
        accent_text: "#10170b".into(),
        success: "#78b84a".into(),
        warning: "#d8a63c".into(),
        danger: "#d9534f".into(),
        gradient_start: "#4f7a2e".into(),
        gradient_end: "#2f5d7c".into(),
        card_opacity: 0.93,
        background_effect: "leaves".into(),
        ..Theme::default()
    }
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
            icons: HashMap::new(),
            texts: HashMap::new(),
        },
        ThemeItem {
            name: "light".into(),
            builtin: true,
            theme: light_theme(),
            icons: HashMap::new(),
            texts: HashMap::new(),
        },
        ThemeItem {
            name: "femboy".into(),
            builtin: true,
            theme: femboy_theme(),
            icons: HashMap::new(),
            texts: HashMap::new(),
        },
        ThemeItem {
            name: "ocean".into(),
            builtin: true,
            theme: ocean_theme(),
            icons: HashMap::new(),
            texts: HashMap::new(),
        },
        ThemeItem {
            name: "Apple".into(),
            builtin: true,
            theme: apple_theme(),
            icons: HashMap::new(),
            texts: HashMap::new(),
        },
        ThemeItem {
            name: "Discord".into(),
            builtin: true,
            theme: discord_theme(),
            icons: HashMap::new(),
            texts: HashMap::new(),
        },
        ThemeItem {
            name: "Minecraft".into(),
            builtin: true,
            theme: minecraft_theme(),
            icons: HashMap::new(),
            texts: HashMap::new(),
        },
    ]
}

/// Builds the special `Windows` palette from a sampled accent. It is a normal
/// dark theme whose primary/accent roles follow the Windows colour, so it is
/// useful outside Material 3 as well.
pub fn windows_theme(accent: Rgb) -> Theme {
    let mut theme = Theme::default();
    let value = hex(accent);
    let (_, chroma, hue) = rgb_to_oklch(&value).unwrap_or((0.5, 0.1, 280.0));
    let chroma = chroma.max(0.02);

    theme.accent = value.clone();
    theme.accent_hover = tone(hue, chroma, 0.82);
    theme.accent_text = if relative_luminance(accent) > 0.45 {
        "#0b0b0e".into()
    } else {
        "#ffffff".into()
    };
    theme.gradient_start = value;
    theme.gradient_end = tone(hue + 35.0, chroma * 0.9, 0.56);
    theme.primary_container = tone(hue, chroma * 0.7, 0.34);
    theme.on_primary_container = tone(hue, 0.03, 0.90);
    theme.secondary = tone(hue + 10.0, chroma * 0.4, 0.78);
    theme.on_secondary = "#111116".into();
    theme.tertiary = tone(hue + 55.0, chroma * 0.7, 0.76);
    theme.on_tertiary = "#111116".into();
    theme
}

fn relative_luminance(rgb: Rgb) -> f64 {
    let channel = |value: u8| {
        let value = value as f64 / 255.0;
        if value <= 0.039_28 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(rgb.0) + 0.7152 * channel(rgb.1) + 0.0722 * channel(rgb.2)
}

pub fn parse_hex_rgb(value: &str) -> Option<Rgb> {
    parse_rgb(value)
}

fn parse_rgb(value: &str) -> Option<Rgb> {
    let hex = value.trim().strip_prefix('#')?;
    let byte = |slice: &str| u8::from_str_radix(slice, 16).ok();
    Some((
        byte(hex.get(0..2)?)?,
        byte(hex.get(2..4)?)?,
        byte(hex.get(4..6)?)?,
    ))
}

fn hex(rgb: Rgb) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb.0, rgb.1, rgb.2)
}

fn with_alpha(value: &str, alpha: u8) -> String {
    match parse_rgb(value) {
        Some((r, g, b)) => format!("#{r:02X}{g:02X}{b:02X}{alpha:02X}"),
        None => value.to_string(),
    }
}

fn srgb_to_linear(value: f64) -> f64 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(value: f64) -> f64 {
    if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
}

fn rgb_to_oklch(value: &str) -> Option<(f64, f64, f64)> {
    let (r, g, b) = parse_rgb(value)?;
    let r = srgb_to_linear(r as f64 / 255.0);
    let g = srgb_to_linear(g as f64 / 255.0);
    let b = srgb_to_linear(b as f64 / 255.0);

    let l = 0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_995 * b;
    let m = 0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b;
    let s = 0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b;

    let l_root = l.cbrt();
    let m_root = m.cbrt();
    let s_root = s.cbrt();

    let lightness = 0.210_454_26 * l_root + 0.793_617_8 * m_root - 0.004_072_047 * s_root;
    let a = 1.977_998_5 * l_root - 2.428_592_2 * m_root + 0.450_593_7 * s_root;
    let b = 0.025_904_037 * l_root + 0.782_771_77 * m_root - 0.808_675_77 * s_root;

    let chroma = (a * a + b * b).sqrt();
    let hue = b.atan2(a).to_degrees().rem_euclid(360.0);
    Some((lightness, chroma, hue))
}

fn oklch_to_rgb(lightness: f64, chroma: f64, hue: f64) -> Rgb {
    let angle = hue.to_radians();
    let a = chroma * angle.cos();
    let b = chroma * angle.sin();

    let l_root = lightness + 0.396_337_78 * a + 0.215_803_76 * b;
    let m_root = lightness - 0.105_561_346 * a - 0.063_854_17 * b;
    let s_root = lightness - 0.089_484_18 * a - 1.291_485_5 * b;

    let l = l_root * l_root * l_root;
    let m = m_root * m_root * m_root;
    let s = s_root * s_root * s_root;

    let r = linear_to_srgb(4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s);
    let g = linear_to_srgb(-1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s);
    let blue = linear_to_srgb(-0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s);

    (
        (r.clamp(0.0, 1.0) * 255.0).round() as u8,
        (g.clamp(0.0, 1.0) * 255.0).round() as u8,
        (blue.clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}

fn tone(hue: f64, chroma: f64, lightness: f64) -> String {
    hex(oklch_to_rgb(lightness, chroma.max(0.0), hue))
}

/// Windows stores `AccentColor` as ABGR and `ColorizationColor` as ARGB.
/// The DWM values are black when accent colouring is disabled, so the Explorer
/// accent palette is also consulted. The most chromatic candidate wins; if the
/// system accent is genuinely gray, the callers fall back to the theme accent
/// while still being able to display the sampled Windows colour.
#[cfg(windows)]
fn winrt_accent() -> Option<Rgb> {
    use windows::UI::ViewManagement::{UIColorType, UISettings};

    // COM/WinRT is initialised by winit before this is called from the UI
    // thread. Do not initialise MTA here: winit uses OLE/STA and would fail
    // with RPC_E_CHANGED_MODE.
    let settings = UISettings::new().ok()?;
    let color = settings.GetColorValue(UIColorType::Accent).ok()?;
    Some((color.R, color.G, color.B))
}

#[cfg(not(windows))]
fn winrt_accent() -> Option<Rgb> {
    None
}

/// Reads the Windows "app mode" preference. `AppsUseLightTheme` matches the
/// look of the shell and is preferred; `SystemUsesLightTheme` is the fallback.
pub fn system_uses_light_theme() -> bool {
    let Ok(personalize) = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
    else {
        return false;
    };

    for name in ["AppsUseLightTheme", "SystemUsesLightTheme"] {
        if let Ok(value) = personalize.get_value::<u32, _>(name) {
            return value != 0;
        }
    }

    false
}

pub fn system_accent() -> Option<Rgb> {
    // The WinRT setting is the exact colour shown in Windows Settings. It is
    // authoritative and also works when DWM colourisation is disabled.
    if let Some(rgb) = winrt_accent() {
        return Some(rgb);
    }

    let mut candidates: Vec<Rgb> = Vec::new();

    if let Ok(dwm) =
        RegKey::predef(HKEY_CURRENT_USER).open_subkey(r"Software\Microsoft\Windows\DWM")
    {
        if let Ok(value) = dwm.get_value::<u32, _>("AccentColor") {
            candidates.push((
                (value & 0xFF) as u8,
                ((value >> 8) & 0xFF) as u8,
                ((value >> 16) & 0xFF) as u8,
            ));
        }
        if let Ok(value) = dwm.get_value::<u32, _>("ColorizationColor") {
            candidates.push((
                ((value >> 16) & 0xFF) as u8,
                ((value >> 8) & 0xFF) as u8,
                (value & 0xFF) as u8,
            ));
        }
    }

    if let Ok(accent) = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Accent")
    {
        if let Ok(value) = accent.get_value::<u32, _>("StartColorMenu") {
            candidates.push((
                (value & 0xFF) as u8,
                ((value >> 8) & 0xFF) as u8,
                ((value >> 16) & 0xFF) as u8,
            ));
        }

        // The palette is eight BGRA entries (light to dark tones).
        if let Ok(palette) = accent.get_raw_value("AccentPalette") {
            if palette.vtype == winreg::enums::REG_BINARY {
                for entry in palette.bytes.chunks_exact(4) {
                    let (b, g, r, a) = (entry[0], entry[1], entry[2], entry[3]);
                    if a > 0 {
                        candidates.push((r, g, b));
                    }
                }
            }
        }
    }

    candidates
        .into_iter()
        .filter(|rgb| *rgb != (0, 0, 0))
        .max_by(|left, right| {
            let left = rgb_to_oklch(&hex(*left)).map(|(_, c, _)| c).unwrap_or(0.0);
            let right = rgb_to_oklch(&hex(*right)).map(|(_, c, _)| c).unwrap_or(0.0);
            left.total_cmp(&right)
        })
}

pub fn normalize_mode(value: &str) -> &'static str {
    match value {
        "material3" => "material3",
        "liquid-glass" => "liquid-glass",
        _ => "base",
    }
}

/// Applies a presentation mode on top of a palette. Material 3 derives its
/// tonal roles from the Windows accent (falling back to the palette accent).
pub fn apply_mode(
    base: &Theme,
    mode: &str,
    accent: Option<Rgb>,
    glass_opacity: f32,
    glass_blur: f32,
) -> Theme {
    let mode = normalize_mode(mode);
    let mut theme = base.clone();
    theme.mode = mode.to_string();

    match mode {
        "material3" => apply_material3(&mut theme, accent),
        "liquid-glass" => apply_glass(&mut theme, glass_opacity, glass_blur),
        _ => {}
    }

    theme
}

fn apply_material3(theme: &mut Theme, accent: Option<Rgb>) {
    let theme_accent = parse_rgb(&theme.accent).unwrap_or((162, 28, 175));
    // A gray/black Windows accent is still a real user choice; use it. The
    // palette accent is only a fallback when Windows exposes no value at all.
    let accent = accent.unwrap_or(theme_accent);
    let (_, accent_chroma, hue) = rgb_to_oklch(&hex(accent)).unwrap_or((0.6, 0.18, 300.0));
    let chroma = accent_chroma.clamp(0.05, 0.22);
    let dark = rgb_to_oklch(&theme.background)
        .map(|(l, _, _)| l < 0.5)
        .unwrap_or(true);

    let (bg, surface, low, high, variant, text, muted, outline) = if dark {
        (
            tone(hue, 0.012, 0.19),
            tone(hue, 0.015, 0.24),
            tone(hue, 0.012, 0.21),
            tone(hue, 0.02, 0.29),
            tone(hue, 0.025, 0.32),
            tone(hue, 0.01, 0.90),
            tone(hue, 0.025, 0.71),
            tone(hue, 0.03, 0.49),
        )
    } else {
        (
            tone(hue, 0.01, 0.96),
            tone(hue, 0.012, 0.985),
            tone(hue, 0.012, 0.93),
            tone(hue, 0.02, 0.88),
            tone(hue, 0.025, 0.90),
            tone(hue, 0.01, 0.22),
            tone(hue, 0.03, 0.46),
            tone(hue, 0.04, 0.58),
        )
    };

    let primary = tone(hue, chroma, if dark { 0.76 } else { 0.48 });
    let on_primary = tone(hue, 0.03, if dark { 0.20 } else { 0.99 });
    let primary_container = tone(hue, chroma * 0.75, if dark { 0.38 } else { 0.86 });
    let on_primary_container = tone(hue, 0.03, if dark { 0.87 } else { 0.22 });

    let secondary = tone(hue + 8.0, chroma * 0.35, if dark { 0.76 } else { 0.48 });
    let on_secondary = tone(hue, 0.02, if dark { 0.21 } else { 0.99 });
    let tertiary = tone(hue + 55.0, chroma * 0.55, if dark { 0.79 } else { 0.48 });
    let on_tertiary = tone(hue + 55.0, 0.02, if dark { 0.21 } else { 0.99 });
    let error = tone(25.0, 0.18, if dark { 0.67 } else { 0.47 });
    let on_error = tone(25.0, 0.02, if dark { 0.19 } else { 0.99 });

    theme.background = bg;
    theme.background_alt = low.clone();
    theme.surface = surface.clone();
    theme.surface_hover = high.clone();
    theme.border = outline.clone();
    theme.text = text;
    theme.text_muted = muted.clone();
    theme.accent = primary.clone();
    theme.accent_hover = tone(hue, chroma, if dark { 0.86 } else { 0.34 });
    theme.accent_text = on_primary;
    theme.gradient_start = primary;
    theme.gradient_end = tertiary.clone();
    theme.success = tone(150.0, 0.15, if dark { 0.78 } else { 0.40 });
    theme.warning = tone(75.0, 0.16, if dark { 0.82 } else { 0.50 });
    theme.danger = error.clone();

    theme.surface_low = low;
    theme.surface_high = high;
    theme.surface_variant = variant;
    theme.on_surface_variant = muted;
    theme.outline = outline;
    theme.primary_container = primary_container;
    theme.on_primary_container = on_primary_container;
    theme.secondary = secondary;
    theme.on_secondary = on_secondary;
    theme.tertiary = tertiary;
    theme.on_tertiary = on_tertiary;
    theme.error = error;
    theme.on_error = on_error;

    theme.radius_xs = 4;
    theme.radius_sm = 10;
    theme.radius_md = 16;
    theme.radius_lg = 22;
    theme.card_opacity = if dark { 0.94 } else { 0.98 };
    theme.shadow = if dark { "#00000099" } else { "#00000024" }.into();
    theme.glass_tint = with_alpha(&theme.surface, 0x8C);
    theme.glass_border = with_alpha(&theme.outline, 0x66);
    theme.glass_highlight = with_alpha("#FFFFFF", 0x1A);
}

fn apply_glass(theme: &mut Theme, opacity: f32, blur: f32) {
    let light = rgb_to_oklch(&theme.background)
        .map(|(l, _, _)| l >= 0.5)
        .unwrap_or(false);

    // Slint 1.18 has no backdrop blur, so "blur strength" only changes how
    // soft the layered imitation looks: stronger specular/highlight layers and
    // a deeper ambient shadow. It is a documented imitation, not real blur.
    let opacity = opacity.clamp(0.25, 0.9);
    let blur = blur.clamp(0.0, 1.0);
    let alpha = |base: u8| -> u8 {
        ((base as f32) * (0.55 + opacity * 0.75))
            .round()
            .clamp(0.0, 255.0) as u8
    };

    theme.glass_tint = with_alpha(&theme.surface, alpha(if light { 0x8C } else { 0x73 }));
    theme.glass_border = with_alpha(
        if light { "#000000" } else { "#FFFFFF" },
        alpha(if light { 0x2E } else { 0x47 }),
    );
    theme.glass_highlight = with_alpha(
        "#FFFFFF",
        ((if light { 0x80u8 } else { 0x40 }) as f32 * (0.6 + blur * 0.8))
            .round()
            .clamp(0.0, 255.0) as u8,
    );
    theme.card_opacity = opacity;
    theme.radius_xs = 5;
    theme.radius_sm = 12;
    theme.radius_md = 18;
    theme.radius_lg = 26;
    theme.shadow = with_alpha(
        "#000000",
        ((if light { 0x33u8 } else { 0x66 }) as f32 * (0.55 + blur))
            .round()
            .clamp(0.0, 255.0) as u8,
    );
}

pub fn themes_dir() -> PathBuf {
    app_dir().join("rust-launcher").join(THEME_DIR)
}

pub fn ensure_themes_dir() -> std::io::Result<PathBuf> {
    let dir = themes_dir();
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Loads built-ins plus user themes. Both layouts are supported:
/// `themes/name.json` (legacy) and `themes/name/theme.json` (with assets).
/// Symlinks and files outside the themes folder are ignored.
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
        if file_type.is_symlink() {
            warn!(path = %entry.path().display(), "theme symlink skipped");
            continue;
        }

        let (path, base, stem) = if file_type.is_dir() {
            let path = entry.path().join("theme.json");
            if !path.is_file() {
                continue;
            }
            (
                path,
                entry.path(),
                entry.file_name().to_string_lossy().to_string(),
            )
        } else if file_type.is_file() {
            let path = entry.path();
            let is_json = path
                .extension()
                .and_then(|value| value.to_str())
                .map(|value| value.eq_ignore_ascii_case("json"))
                .unwrap_or(false);
            if !is_json {
                continue;
            }
            let stem = path
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("custom")
                .to_string();
            (path, dir.clone(), stem)
        } else {
            continue;
        };

        let metadata = match fs::metadata(&path) {
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

        let mode = if parsed.mode.trim().is_empty() {
            parsed.theme.mode.clone()
        } else {
            parsed.mode.clone()
        };
        let mut theme = parsed.theme;
        theme.mode = normalize_mode(&mode).to_string();
        let theme = theme.validated(&name);

        let mut icons = HashMap::new();
        for (id, relative) in parsed.icons {
            if !ICON_IDS.contains(&id.as_str()) {
                warn!(theme = %name, icon = %id, "unknown icon id, skipped");
                continue;
            }
            match resolve_asset(&base, &relative) {
                Some(path) => {
                    icons.insert(id, path.to_string_lossy().into_owned());
                }
                None => warn!(
                    theme = %name,
                    icon = %id,
                    path = %relative,
                    "invalid theme icon path, skipped"
                ),
            }
        }

        let mut texts = HashMap::new();
        for (key, value) in parsed.texts {
            if !TEXT_KEYS.contains(&key.as_str()) {
                warn!(theme = %name, key = %key, "unknown text key, skipped");
                continue;
            }
            texts.insert(key, value);
        }

        info!(
            file = %path.display(),
            theme = %name,
            icons = icons.len(),
            texts = texts.len(),
            "user theme loaded"
        );
        items.push(ThemeItem {
            name,
            builtin: false,
            theme,
            icons,
            texts,
        });
    }

    items
}

/// Resolves a relative asset path below the theme folder. Rejects absolute
/// paths, `..`, symlinks and files outside the canonical theme root.
fn resolve_asset(base: &Path, relative: &str) -> Option<PathBuf> {
    let candidate = Path::new(relative.trim());
    if candidate.as_os_str().is_empty() || candidate.is_absolute() {
        return None;
    }
    for component in candidate.components() {
        match component {
            Component::Normal(_) => {}
            _ => return None,
        }
    }

    let path = base.join(candidate);
    let metadata = fs::symlink_metadata(&path).ok()?;
    if !metadata.file_type().is_file() || metadata.len() > 512 * 1024 {
        return None;
    }
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    if extension != "svg" && extension != "png" {
        return None;
    }

    let canonical = path.canonicalize().ok()?;
    let root = base.canonicalize().ok()?;
    if !canonical.starts_with(&root) {
        return None;
    }
    Some(canonical)
}

/// Picks the theme for the current Windows app mode when following the
/// system, otherwise resolves the explicitly selected theme.
pub fn resolve_of_pair<'a>(
    follow_windows: bool,
    light_theme: &str,
    dark_theme: &str,
    selected: &str,
    items: &'a [ThemeItem],
) -> &'a ThemeItem {
    if follow_windows {
        let name = if system_uses_light_theme() {
            light_theme
        } else {
            dark_theme
        };
        resolve(name, items)
    } else {
        resolve(selected, items)
    }
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
    fn material3_keeps_the_palette_lightness() {
        for item in builtin_themes() {
            let dark = luminance(&item.theme.background) < 0.5;
            let themed = apply_mode(&item.theme, "material3", None, 0.55, 0.5);
            let generated_dark = luminance(&themed.background) < 0.5;
            assert_eq!(
                dark, generated_dark,
                "{} M3 background is {}",
                item.name, themed.background
            );
        }
    }

    #[test]
    fn mix_interpolates_endpoints_and_alpha() {
        let from = Theme {
            background: "#000000".into(),
            shadow: "#00000000".into(),
            ..Theme::default()
        };
        let to = Theme {
            background: "#FFFFFF".into(),
            shadow: "#FFFFFF80".into(),
            ..Theme::default()
        };

        assert_eq!(Theme::mix(&from, &to, 0.0).background, "#000000");
        assert_eq!(Theme::mix(&from, &to, 1.0).background, "#FFFFFF");
        assert_eq!(Theme::mix(&from, &to, 0.0).shadow, "#00000000");
        assert_eq!(Theme::mix(&from, &to, 1.0).shadow, "#FFFFFF80");
        assert!(Theme::mix(&from, &to, 0.5).background.starts_with('#'));
    }

    #[test]
    fn glass_settings_are_clamped_and_affect_depth() {
        let base = Theme::default();
        let translucent = apply_mode(&base, "liquid-glass", None, 0.25, 0.0);
        let solid = apply_mode(&base, "liquid-glass", None, 0.9, 1.0);

        assert!(translucent.card_opacity >= 0.25);
        assert!(solid.card_opacity <= 0.9);
        assert!(solid.card_opacity > translucent.card_opacity);

        let clamped = apply_mode(&base, "liquid-glass", None, -5.0, 5.0);
        assert!(clamped.card_opacity >= 0.25 && clamped.card_opacity <= 0.9);
    }

    #[test]
    fn theme_text_allowlist_does_not_include_critical_errors() {
        assert!(TEXT_KEYS.contains(&"buttons.play"));
        assert!(TEXT_KEYS.contains(&"settings.appearance.title"));
        assert!(!TEXT_KEYS.contains(&"errors.session_expired"));
        assert!(!TEXT_KEYS.contains(&"window.close"));
    }

    #[test]
    fn theme_asset_paths_stay_inside_the_theme_folder() {
        let root =
            std::env::temp_dir().join(format!("vestalis-theme-assets-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("icons")).unwrap();
        fs::write(root.join("icons/play.svg"), "<svg/>").unwrap();

        assert!(resolve_asset(&root, "icons/play.svg").is_some());
        assert!(resolve_asset(&root, "../evil.svg").is_none());
        assert!(resolve_asset(&root, "C:\\Windows\\evil.svg").is_none());
        assert!(resolve_asset(&root, "icons/missing.svg").is_none());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn windows_theme_uses_the_sampled_accent() {
        for accent in [(220, 20, 60), (0, 120, 215), (63, 63, 63)] {
            let theme = windows_theme(accent);
            assert_eq!(theme.accent, hex(accent));
            let ratio = contrast(&theme.accent_text, &theme.accent);
            // A sampled accent can land exactly on the 4.5 boundary; keep the
            // code point for the exact colour and allow 0.01 rounding slack.
            assert!(
                ratio >= 4.49,
                "accent {:?} text {} ratio {:.2}",
                accent,
                theme.accent_text,
                ratio
            );
        }
    }

    #[test]
    fn status_colors_are_readable() {
        for item in builtin_themes() {
            for background in [&item.theme.background, &item.theme.surface] {
                assert!(
                    contrast(&item.theme.warning, background) >= 3.0,
                    "{} warning on {}",
                    item.name,
                    background
                );
                assert!(
                    contrast(&item.theme.danger, background) >= 3.0,
                    "{} danger on {}",
                    item.name,
                    background
                );
            }
        }
    }

    #[test]
    fn material3_accent_palettes_stay_readable() {
        for accent in [(0, 120, 215), (220, 20, 60), (0, 200, 110), (107, 107, 107)] {
            for base in [Theme::default(), light_theme()] {
                let theme = apply_mode(&base, "material3", Some(accent), 0.55, 0.5);
                assert!(contrast(&theme.text, &theme.background) >= 4.5);
                assert!(contrast(&theme.text, &theme.surface) >= 4.5);
                assert!(contrast(&theme.accent_text, &theme.accent) >= 4.5);
                assert!(contrast(&theme.on_error, &theme.error) >= 4.5);
            }
        }
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
