use std::time::Duration;

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use reqwest::header::{HeaderValue, AUTHORIZATION};
use reqwest::StatusCode;
use serde::de::DeserializeOwned;
use sha2::Sha256;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

use crate::model::{
    ApiResponse, LoginRequest, MirrorResponse, News, Player, Profile, ProfileInfo,
    ProfileInfoRequest, Session, TokenRequest,
};

#[derive(Clone)]
pub struct Backend {
    client: reqwest::Client,
    base: String,
    client_secret: Option<String>,
    hwid: String,
}

pub struct AuthOutcome {
    pub session: Option<Session>,
    pub requires_2fa: bool,
    /// The host refused the client with the "official launcher required" gate.
    pub blocked: bool,
    pub message: String,
    pub errors: Vec<String>,
}

impl AuthOutcome {
    fn failed(message: String, errors: Vec<String>, requires_2fa: bool, blocked: bool) -> Self {
        Self {
            session: None,
            requires_2fa,
            blocked,
            message,
            errors,
        }
    }
}

impl Backend {
    pub fn new(base: &str) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .user_agent(
                "Gml.Launcher-Client-GmlClientManager/1.0 \
                 (OS: Microsoft Windows 10.0.26200; OSArchitecture: X64; \
                 ProcessArchitecture: X64; FrameworkDescription: .NET 8.0.28; .NET: 8.0.28;)",
            )
            .build()
            .unwrap_or_default();

        Self {
            client,
            base: format!("{}/", base.trim_end_matches('/')),
            client_secret: None,
            hwid: String::new(),
        }
    }

    pub fn with_client(mut self, secret: Option<String>, hwid: String) -> Self {
        self.client_secret = secret.filter(|value| !value.trim().is_empty());
        self.hwid = hwid;
        self
    }

    pub fn set_client(&mut self, secret: Option<String>, hwid: String) {
        self.client_secret = secret.filter(|value| !value.trim().is_empty());
        self.hwid = hwid;
    }

    pub fn set_hwid(&mut self, hwid: String) {
        self.hwid = hwid;
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    pub fn client_hwid(&self) -> &str {
        &self.hwid
    }

    /// `X-Client-Signature` from the official client:
    /// HMAC-SHA256(secret, "{unixSeconds}:{guidN}:{hwid}:Vestalis"),
    /// sent as "{unixSeconds}:{guidN}:{hexDigest}".
    fn signature(&self, hwid: &str) -> Option<String> {
        let secret = self.client_secret.as_deref()?;
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_secs()
            .to_string();
        let nonce = Uuid::new_v4().simple().to_string();
        let message = format!("{timestamp}:{nonce}:{}:Vestalis", hwid.trim());

        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).ok()?;
        mac.update(message.as_bytes());
        let digest = hex::encode(mac.finalize().into_bytes());

        Some(format!("{timestamp}:{nonce}:{digest}"))
    }

    pub fn sign(&self, request: reqwest::RequestBuilder, hwid: &str) -> reqwest::RequestBuilder {
        match self.signature(hwid) {
            Some(value) => request.header("X-Client-Signature", value),
            None => request,
        }
    }

    pub fn client(&self) -> &reqwest::Client {
        &self.client
    }

    pub fn file_url(&self, hash: &str) -> String {
        self.url(&format!("api/v1/file/{hash}"))
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path.trim_start_matches('/'))
    }

    async fn send_json<T: DeserializeOwned>(
        &self,
        response: reqwest::Response,
    ) -> (StatusCode, Option<ApiResponse<T>>, String) {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        let parsed = serde_json::from_str::<ApiResponse<T>>(&text).ok();
        (status, parsed, text)
    }

    pub async fn signin(
        &self,
        login: &str,
        password: &str,
        code: &str,
        hwid: &str,
    ) -> Result<AuthOutcome> {
        let request = LoginRequest {
            login,
            password,
            access_token: "",
            two_factor_code: code,
        };

        let request = self
            .client
            .post(self.url("api/v1/integrations/auth/signin"))
            .header("X-HWID", hwid)
            .json(&request);
        let response = self.sign(request, hwid).send().await?;

        let (status, parsed, raw) = self.send_json::<Player>(response).await;
        let message = parsed
            .as_ref()
            .and_then(|value| value.message.clone())
            .unwrap_or_else(|| format!("HTTP {}", status.as_u16()));
        let errors = parsed
            .as_ref()
            .and_then(|value| value.errors.clone())
            .unwrap_or_default();

        if status.is_success() {
            if let Some(player) = parsed.and_then(|value| value.data) {
                if let Some(session) = session_from_player(player, login) {
                    return Ok(AuthOutcome {
                        session: Some(session),
                        requires_2fa: false,
                        blocked: false,
                        message: String::new(),
                        errors: Vec::new(),
                    });
                }
            }

            return Ok(AuthOutcome::failed(
                format!("Некорректный ответ сервера: {raw}"),
                errors,
                false,
                false,
            ));
        }

        let blocked = status == StatusCode::FORBIDDEN && message.contains("официальн");
        Ok(AuthOutcome::failed(
            message.clone(),
            errors,
            message.to_lowercase().contains("2fa"),
            blocked,
        ))
    }

    pub async fn check_token(&self, token: &str, hwid: &str) -> Result<AuthOutcome> {
        let request = self
            .client
            .post(self.url("api/v1/integrations/auth/checkToken"))
            .header("X-HWID", hwid)
            .json(&TokenRequest {
                access_token: token,
            });
        let response = self.sign(request, hwid).send().await?;

        let (status, parsed, raw) = self.send_json::<Player>(response).await;
        let message = parsed
            .as_ref()
            .and_then(|value| value.message.clone())
            .unwrap_or_else(|| format!("HTTP {}", status.as_u16()));
        let errors = parsed
            .as_ref()
            .and_then(|value| value.errors.clone())
            .unwrap_or_default();

        if status.is_success() {
            if let Some(player) = parsed.and_then(|value| value.data) {
                let session = session_from_player(player, "").or_else(|| {
                    Some(Session {
                        uuid: String::new(),
                        name: String::new(),
                        access_token: token.to_string(),
                        expired: None,
                        texture: None,
                    })
                });
                return Ok(AuthOutcome {
                    session,
                    requires_2fa: false,
                    blocked: false,
                    message: String::new(),
                    errors,
                });
            }

            return Ok(AuthOutcome::failed(
                format!("Некорректный ответ сервера: {raw}"),
                errors,
                false,
                false,
            ));
        }

        let blocked = status == StatusCode::FORBIDDEN && message.contains("официальн");
        Ok(AuthOutcome::failed(
            message.clone(),
            errors,
            message.to_lowercase().contains("2fa"),
            blocked,
        ))
    }

    pub async fn profiles(&self, token: &str) -> Result<Vec<Profile>> {
        let request = self
            .client
            .get(self.url("api/v1/profiles"))
            .bearer_auth(token);
        let response = self.sign(request, &self.hwid).send().await?;

        let (status, parsed, raw) = self.send_json::<Vec<Profile>>(response).await;

        if !status.is_success() {
            return Err(anyhow!(
                "{}",
                parsed
                    .as_ref()
                    .and_then(|value| value.message.clone())
                    .filter(|message| !message.is_empty())
                    .unwrap_or_else(|| format!(
                        "Не удалось загрузить профили: HTTP {} ({raw})",
                        status.as_u16()
                    ))
            ));
        }

        Ok(parsed.and_then(|value| value.data).unwrap_or_default())
    }

    pub async fn profile_info(
        &self,
        token: &str,
        request: &ProfileInfoRequest<'_>,
    ) -> Result<Option<ProfileInfo>> {
        let request = self
            .client
            .post(self.url("api/v1/profiles/info"))
            .header(
                AUTHORIZATION,
                HeaderValue::from_str(token).unwrap_or(HeaderValue::from_static("")),
            )
            .json(request);
        let response = self.sign(request, &self.hwid).send().await?;

        let (status, parsed, raw) = self.send_json::<ProfileInfo>(response).await;

        if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
            return Err(anyhow!("Сессия истекла, войдите заново"));
        }

        if !status.is_success() {
            return Err(anyhow!(
                "{}",
                parsed
                    .as_ref()
                    .and_then(|value| value.message.clone())
                    .filter(|message| !message.is_empty())
                    .unwrap_or_else(|| format!(
                        "Не удалось получить профиль: HTTP {} ({raw})",
                        status.as_u16()
                    ))
            ));
        }

        Ok(parsed.and_then(|value| value.data))
    }

    pub async fn news(&self) -> Vec<News> {
        let request = self.client.get(self.url("api/v1/integrations/news/list"));
        let Ok(response) = self.sign(request, &self.hwid).send().await else {
            return Vec::new();
        };

        let (status, parsed, _) = self.send_json::<Vec<News>>(response).await;
        if status.is_success() {
            parsed.and_then(|value| value.data).unwrap_or_default()
        } else {
            Vec::new()
        }
    }

    pub async fn optional_mods(
        &self,
        profile: &str,
        token: &str,
    ) -> Result<Vec<crate::model::OptionalMod>> {
        let request = self
            .client
            .get(self.url(&format!(
                "api/v1/profiles/{}/mods/optionals",
                urlencoding(profile)
            )))
            .bearer_auth(token);
        let response = self.sign(request, &self.hwid).send().await?;

        let (status, parsed, raw) = self
            .send_json::<Vec<crate::model::OptionalMod>>(response)
            .await;

        if !status.is_success() {
            return Err(anyhow!(
                "{}",
                parsed
                    .as_ref()
                    .and_then(|value| value.message.clone())
                    .filter(|message| !message.is_empty())
                    .unwrap_or_else(|| format!(
                        "Не удалось загрузить моды: HTTP {} ({raw})",
                        status.as_u16()
                    ))
            ));
        }

        Ok(parsed.and_then(|value| value.data).unwrap_or_default())
    }

    pub async fn optional_mods_details(&self, token: &str) -> Vec<crate::model::ModDetails> {
        let request = self
            .client
            .get(self.url("api/v1/mods/details"))
            .bearer_auth(token);
        let Ok(response) = self.sign(request, &self.hwid).send().await else {
            return Vec::new();
        };

        let (status, parsed, _) = self
            .send_json::<Vec<crate::model::ModDetails>>(response)
            .await;

        if status.is_success() {
            parsed.and_then(|value| value.data).unwrap_or_default()
        } else {
            Vec::new()
        }
    }

    pub async fn mirror_mode(&self, key: &str) -> Option<String> {
        let mut request = self.client.get(self.url("api/v1/mirror/mode"));
        if !key.trim().is_empty() {
            request = request.header("X-Mirror-Control-Key", key.trim());
        }

        let response = self.sign(request, &self.hwid).send().await.ok()?;
        if !response.status().is_success() {
            return None;
        }

        let parsed: ApiResponse<MirrorResponse> = response.json().await.ok()?;
        parsed.data.and_then(|value| value.mode)
    }

    pub async fn mirror_state(&self, key: &str) -> Option<crate::model::MirrorState> {
        let mut request = self.client.get(self.url("api/v1/mirror/mode"));
        if !key.trim().is_empty() {
            request = request.header("X-Mirror-Control-Key", key.trim());
        }

        let response = self.sign(request, &self.hwid).send().await.ok()?;
        if !response.status().is_success() {
            return None;
        }

        let parsed: ApiResponse<crate::model::MirrorState> = response.json().await.ok()?;
        parsed.data
    }

    pub async fn set_mirror_mode(&self, key: &str, mode: &str) -> Result<String> {
        let request = self
            .client
            .put(self.url("api/v1/mirror/mode"))
            .header("X-Mirror-Control-Key", key.trim())
            .json(&serde_json::json!({ "mode": mode }));
        let response = self.sign(request, &self.hwid).send().await?;

        self.mirror_action_result(response, "Не удалось переключить режим")
            .await
    }

    pub async fn set_tor(&self, key: &str, enabled: bool) -> Result<String> {
        let request = self
            .client
            .put(self.url("api/v1/mirror/tor"))
            .header("X-Mirror-Control-Key", key.trim())
            .json(&serde_json::json!({ "enabled": enabled }));
        let response = self.sign(request, &self.hwid).send().await?;

        self.mirror_action_result(response, "Не удалось изменить маршрутизацию через Tor")
            .await
    }

    async fn mirror_action_result(
        &self,
        response: reqwest::Response,
        fallback: &str,
    ) -> Result<String> {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        let message = serde_json::from_str::<ApiResponse<serde_json::Value>>(&text)
            .ok()
            .and_then(|value| value.message)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| fallback.to_string());

        if status.is_success() {
            Ok(message)
        } else if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
            Err(anyhow!("Сервисный ключ не принят сервером"))
        } else {
            Err(anyhow!(message))
        }
    }

    pub async fn self_test(&self, key: &str) -> Option<crate::model::SelfTestReport> {
        let request = self
            .client
            .get(self.url("api/v1/mirror/selftest"))
            .header("X-Mirror-Control-Key", key.trim())
            .timeout(Duration::from_secs(60));
        let response = self.sign(request, &self.hwid).send().await.ok()?;

        if !response.status().is_success() {
            return None;
        }

        let parsed: ApiResponse<crate::model::SelfTestReport> = response.json().await.ok()?;
        parsed.data
    }

    pub async fn is_reachable(&self) -> bool {
        let request = self.client.get(&self.base).timeout(Duration::from_secs(6));
        if let Ok(response) = self.sign(request, &self.hwid).send().await {
            if response.status().is_success() {
                return true;
            }
        }

        let request = self
            .client
            .get(self.url("health"))
            .timeout(Duration::from_secs(6));
        matches!(
            self.sign(request, &self.hwid)
                .send()
                .await
                .map(|response| response.status().is_success()),
            Ok(true)
        )
    }
}

fn urlencoding(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(byte as char)
            }
            b' ' => result.push_str("%20"),
            _ => result.push_str(&format!("%{byte:02X}")),
        }
    }
    result
}

fn session_from_player(player: Player, fallback_login: &str) -> Option<Session> {
    let token = player
        .access_token
        .filter(|value| !value.trim().is_empty())?;
    let name = player
        .name
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| fallback_login.to_string());

    let expired = normalize_expiry(player.expired_date);

    Some(Session {
        uuid: player.uuid.unwrap_or_default(),
        name,
        access_token: token,
        expired,
        texture: player.texture_skin_url,
    })
}

fn normalize_expiry(value: Option<DateTime<Utc>>) -> Option<DateTime<Utc>> {
    value.filter(|date| date.year() > 2000)
}

use chrono::Datelike;

#[allow(dead_code)]
fn _assert_send<T: Send>(_: T) {}
