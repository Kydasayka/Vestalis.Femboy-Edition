use std::time::Duration;

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use reqwest::header::{HeaderValue, AUTHORIZATION};
use reqwest::StatusCode;
use serde::de::DeserializeOwned;

use crate::model::{
    ApiResponse, LoginRequest, MirrorResponse, News, Player, Profile, ProfileInfo,
    ProfileInfoRequest, Session, TokenRequest,
};

#[derive(Clone)]
pub struct Backend {
    client: reqwest::Client,
    base: String,
}

pub struct AuthOutcome {
    pub session: Option<Session>,
    pub requires_2fa: bool,
    pub message: String,
    pub errors: Vec<String>,
}

impl AuthOutcome {
    fn failed(message: String, errors: Vec<String>, requires_2fa: bool) -> Self {
        Self {
            session: None,
            requires_2fa,
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
                 (OS: Microsoft Windows 10.0.26100; OSArchitecture: X64; \
                 ProcessArchitecture: X64; FrameworkDescription: .NET 8.0; .NET: 8.0;)",
            )
            .build()
            .unwrap_or_default();

        Self {
            client,
            base: format!("{}/", base.trim_end_matches('/')),
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

        let response = self
            .client
            .post(self.url("api/v1/integrations/auth/signin"))
            .header("X-HWID", hwid)
            .json(&request)
            .send()
            .await?;

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
                        message: String::new(),
                        errors: Vec::new(),
                    });
                }
            }

            return Ok(AuthOutcome::failed(
                format!("Некорректный ответ сервера: {raw}"),
                errors,
                false,
            ));
        }

        Ok(AuthOutcome::failed(
            message.clone(),
            errors,
            message.to_lowercase().contains("2fa"),
        ))
    }

    pub async fn check_token(&self, token: &str, hwid: &str) -> Result<AuthOutcome> {
        let response = self
            .client
            .post(self.url("api/v1/integrations/auth/checkToken"))
            .header("X-HWID", hwid)
            .json(&TokenRequest {
                access_token: token,
            })
            .send()
            .await?;

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
                    message: String::new(),
                    errors,
                });
            }

            return Ok(AuthOutcome::failed(
                format!("Некорректный ответ сервера: {raw}"),
                errors,
                false,
            ));
        }

        Ok(AuthOutcome::failed(
            message.clone(),
            errors,
            message.to_lowercase().contains("2fa"),
        ))
    }

    pub async fn profiles(&self, token: &str) -> Result<Vec<Profile>> {
        let response = self
            .client
            .get(self.url("api/v1/profiles"))
            .bearer_auth(token)
            .send()
            .await?;

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
        let response = self
            .client
            .post(self.url("api/v1/profiles/info"))
            .header(
                AUTHORIZATION,
                HeaderValue::from_str(token).unwrap_or(HeaderValue::from_static("")),
            )
            .json(request)
            .send()
            .await?;

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
        let Ok(response) = self
            .client
            .get(self.url("api/v1/integrations/news/list"))
            .send()
            .await
        else {
            return Vec::new();
        };

        let (status, parsed, _) = self.send_json::<Vec<News>>(response).await;
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

        let response = request.send().await.ok()?;
        if !response.status().is_success() {
            return None;
        }

        let parsed: ApiResponse<MirrorResponse> = response.json().await.ok()?;
        parsed.data.and_then(|value| value.mode)
    }

    pub async fn is_reachable(&self) -> bool {
        if let Ok(response) = self
            .client
            .get(&self.base)
            .timeout(Duration::from_secs(6))
            .send()
            .await
        {
            if response.status().is_success() {
                return true;
            }
        }

        matches!(
            self.client
                .get(self.url("health"))
                .timeout(Duration::from_secs(6))
                .send()
                .await
                .map(|response| response.status().is_success()),
            Ok(true)
        )
    }
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
