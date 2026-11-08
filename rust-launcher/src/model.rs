use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Servers offered in the UI. The order matches the dropdown items.
pub const SERVERS: [(&str, &str); 4] = [
    ("RU-1", "https://gmlfrr.vestalis.xyz/"),
    ("RU-2", "https://msk1.gmlfr.vestalis.xyz/"),
    ("EU-1", "https://gmlfr.vestalis.xyz/"),
    ("PL-1", "https://201-58-180-2.sslip.io/"),
];

pub const DEFAULT_HOST: &str = SERVERS[1].1;

/// PL-1 is a read-only mirror and is shown as a disabled W.I.P entry. The host
/// entry stays in the list so it can be re-enabled later without a rewrite.
pub const PL1_AVAILABLE: bool = false;

pub fn server_index(host: &str) -> usize {
    let normalized = host.trim_end_matches('/');
    SERVERS
        .iter()
        .position(|(_, url)| url.trim_end_matches('/') == normalized)
        .unwrap_or(1)
}

pub fn app_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Vestalis")
}

pub fn local_app_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Vestalis")
}

/// Cached head render for the signed-in player. The key is the account UUID,
/// so two accounts with the same name cannot overwrite each other.
pub fn avatar_path(uuid: &str) -> PathBuf {
    let safe: String = uuid
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    let key = if safe.is_empty() {
        "player".to_string()
    } else {
        safe
    };
    local_app_dir().join("avatars").join(format!("{key}.png"))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub game_width: u32,
    pub game_height: u32,
    pub fullscreen: bool,
    pub xms: u32,
    pub xmx: u32,
    pub custom_jvm: String,
    /// Optional explicit Java executable. Empty means "use the manifest path".
    pub custom_java: String,
    /// Sign in automatically with the saved credentials on startup.
    pub auto_login: bool,
    pub send_hwid: bool,
    pub sounds: bool,
    /// Minimize the launcher as soon as the game process starts.
    pub hide_on_launch: bool,
    /// Ambient background: "none", "fireflies", "leaves" or "snow".
    pub background_effect: String,
    /// Active visual theme name (built-in or a JSON file in the themes folder).
    pub theme: String,
    /// Presentation mode: "base", "material3" or "liquid-glass".
    pub theme_mode: String,
    /// Interface language: "ru", "en" or "ua".
    pub language: String,
    /// Optional manual accent (#RRGGBB) overriding the Windows accent for M3.
    pub accent: String,
    /// Liquid Glass panel opacity (imitation: Slint 1.18 has no backdrop blur).
    pub glass_opacity: f32,
    /// Strength of the layered glass imitation (specular/shadow depth).
    pub glass_blur: f32,
    /// Discord Rich Presence enabled.
    pub discord_rpc: bool,
    /// Anonymous Sentry telemetry. Off by default until the user opts in.
    pub telemetry_enabled: bool,
    /// First-run onboarding has been completed or skipped.
    pub onboarding_completed: bool,
    /// Recently applied manual accents, newest first (#RRGGBB).
    pub accent_history: Vec<String>,
    /// Follow the Windows light/dark app mode instead of a fixed theme.
    pub follow_windows_theme: bool,
    /// Theme applied when Windows is in light mode.
    pub light_theme: String,
    /// Theme applied when Windows is in dark mode.
    pub dark_theme: String,
    /// Main-screen zone layout, independent from the selected theme.
    pub layout: crate::layout::LayoutConfig,
    /// Named layout presets.
    pub layouts: Vec<crate::layout::LayoutProfile>,
    /// Name of the preset the active layout was loaded from.
    pub layout_profile: String,
    pub install_dir: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            game_width: 900,
            game_height: 600,
            fullscreen: false,
            xms: 2048,
            xmx: 8192,
            custom_jvm: String::new(),
            custom_java: String::new(),
            auto_login: true,
            send_hwid: true,
            sounds: true,
            hide_on_launch: true,
            background_effect: "none".to_string(),
            theme: "dark".to_string(),
            theme_mode: "base".to_string(),
            language: "ru".to_string(),
            accent: String::new(),
            glass_opacity: 0.55,
            glass_blur: 0.5,
            discord_rpc: true,
            telemetry_enabled: false,
            onboarding_completed: false,
            accent_history: Vec::new(),
            follow_windows_theme: false,
            light_theme: "light".to_string(),
            dark_theme: "dark".to_string(),
            layout: crate::layout::LayoutConfig::default(),
            layouts: Vec::new(),
            layout_profile: "По умолчанию".to_string(),
            install_dir: app_dir().to_string_lossy().into_owned(),
        }
    }
}

impl Settings {
    pub fn install_path(&self) -> PathBuf {
        let value = self.install_dir.trim();
        if value.is_empty() {
            app_dir()
        } else {
            PathBuf::from(value)
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub uuid: String,
    pub name: String,
    pub access_token: String,
    pub expired: Option<DateTime<Utc>>,
    pub texture: Option<String>,
}

impl Session {
    pub fn is_expired(&self) -> bool {
        self.expired.map(|date| date <= Utc::now()).unwrap_or(false)
    }

    pub fn initial(&self) -> String {
        self.name
            .chars()
            .next()
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_else(|| "?".into())
    }
}

/// `GET /api/v1/users/info/{name}` payload with the player's skin location.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerTexture {
    #[serde(default)]
    pub texture_skin_guid: String,
    #[serde(default)]
    pub external_texture_skin_url: Option<String>,
    #[serde(default)]
    pub full_skin_url: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedLogin {
    pub login: String,
    pub password: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Persisted {
    pub settings: Settings,
    pub server_host: String,
    pub user: Option<Session>,
    pub saved_login: Option<SavedLogin>,
    pub last_profile: Option<String>,
    /// News items already shown to the user, used for toast de-duplication.
    pub seen_news: Vec<String>,
}

impl Default for Persisted {
    fn default() -> Self {
        Self {
            settings: Settings::default(),
            server_host: DEFAULT_HOST.to_string(),
            user: None,
            saved_login: None,
            last_profile: None,
            seen_news: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Backend DTOs (the API speaks camelCase JSON).
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ApiResponse<T> {
    pub data: Option<T>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub errors: Option<Vec<String>>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub name: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub game_version: Option<String>,
    #[serde(default)]
    pub launch_version: Option<String>,
    #[serde(default)]
    pub icon_base64: Option<String>,
    /// Personal profile background URL returned by the backend.
    #[serde(default)]
    pub background: Option<String>,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub servers: Vec<ServerInfo>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    #[serde(default)]
    pub online: Option<i64>,
    #[serde(default)]
    pub max_online: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileFile {
    pub name: String,
    pub directory: String,
    #[serde(default)]
    pub hash: Option<String>,
    /// Server-side size. A mismatch is a fast "must update" signal even when
    /// the hash is missing or stale in the manifest.
    #[serde(default)]
    pub size: i64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileInfo {
    pub java_path: String,
    #[serde(rename = "releativePath", alias = "relativePath")]
    pub relative_path: String,
    #[serde(default)]
    pub arguments: String,
    #[serde(default)]
    pub files: Vec<ProfileFile>,
    #[serde(default)]
    pub white_list_files: Vec<ProfileFile>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct News {
    pub title: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub date: Option<String>,
    /// News icon URL returned by the backend.
    #[serde(default)]
    pub icon_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Player {
    #[serde(default)]
    pub uuid: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub access_token: Option<String>,
    #[serde(default)]
    pub expired_date: Option<DateTime<Utc>>,
    #[serde(default)]
    pub texture_skin_url: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OptionalMod {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModDetails {
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscordConfig {
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub details: String,
    #[serde(default)]
    pub large_image_key: String,
    #[serde(default)]
    pub large_image_text: String,
    #[serde(default)]
    pub small_image_key: String,
    #[serde(default)]
    pub small_image_text: String,
}

impl Default for DiscordConfig {
    fn default() -> Self {
        // Exactly the values used by the C# launcher in ApiProcedures.
        Self {
            client_id: "1558556890605162577".to_string(),
            details: "Продам гараж с чулочками, по поводу покупки - пишите в ЛС. Myaw >w<"
                .to_string(),
            large_image_key: "logo2".to_string(),
            large_image_text: "~Nya~".to_string(),
            small_image_key: "logo".to_string(),
            small_image_text: "Vestalis FemBoy Edition".to_string(),
        }
    }
}

#[derive(Serialize)]
pub struct LoginRequest<'a> {
    #[serde(rename = "Login")]
    pub login: &'a str,
    #[serde(rename = "Password")]
    pub password: &'a str,
    #[serde(rename = "AccessToken")]
    pub access_token: &'a str,
    #[serde(rename = "TwoFactorCode")]
    pub two_factor_code: &'a str,
}

#[derive(Serialize)]
pub struct TokenRequest<'a> {
    #[serde(rename = "AccessToken")]
    pub access_token: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct ProfileInfoRequest<'a> {
    pub user_name: &'a str,
    pub profile_name: &'a str,
    pub user_access_token: &'a str,
    pub window_width: u32,
    pub window_height: u32,
    pub game_address: &'a str,
    pub game_port: u32,
    pub is_full_screen: bool,
    pub ram_size: u32,
    pub user_uuid: &'a str,
    pub os_type: &'a str,
    pub os_architecture: &'a str,
}
