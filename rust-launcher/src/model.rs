use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Servers offered in the UI. The order matches the four chips in the Slint view.
pub const SERVERS: [(&str, &str); 4] = [
    ("RU-1", "https://gmlfrr.vestalis.xyz/"),
    ("RU-2", "https://msk1.gmlfr.vestalis.xyz/"),
    ("EU-1", "https://gmlfr.vestalis.xyz/"),
    ("PL-1", "https://201-58-180-2.sslip.io/"),
];

// The RU/EU deployments currently reject clients whose HTTP fingerprint is not
// the official C# build. PL-1 accepts the same API without that gate, so it is
// the safe default; other hosts stay selectable.
pub const DEFAULT_HOST: &str = SERVERS[3].1;

/// Local mirror used when a main deployment rejects sign-in with the
/// "official launcher required" gate. Its upstream is the RU-2 backend.
pub const MIRROR_HOST: &str = SERVERS[3].1;

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

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub game_width: u32,
    pub game_height: u32,
    pub fullscreen: bool,
    pub xms: u32,
    pub xmx: u32,
    pub custom_jvm: String,
    pub send_hwid: bool,
    pub sounds: bool,
    /// Minimize the launcher as soon as the game process starts.
    pub hide_on_launch: bool,
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
            send_hwid: true,
            sounds: true,
            hide_on_launch: true,
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
    /// The session was issued through the mirror because the selected host is
    /// gated for this client/IP.
    pub auth_via_mirror: bool,
    pub mirror_key: String,
}

impl Default for Persisted {
    fn default() -> Self {
        Self {
            settings: Settings::default(),
            server_host: DEFAULT_HOST.to_string(),
            user: None,
            saved_login: None,
            last_profile: None,
            auth_via_mirror: false,
            mirror_key: String::new(),
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MirrorResponse {
    #[serde(default)]
    pub mode: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MirrorState {
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub upstream: Option<String>,
    #[serde(default)]
    pub can_control: bool,
    #[serde(default)]
    pub tor_enabled: bool,
    #[serde(default)]
    pub tor_available: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfTestCheck {
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub duration_ms: i64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfTestReport {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub upstream: String,
    #[serde(default)]
    pub duration_ms: i64,
    #[serde(default)]
    pub checks: Vec<SelfTestCheck>,
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
