use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use futures::FutureExt;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tracing::{error, info};

use crate::api::Backend;
use crate::files::{sync_profile, SyncProgress};
use crate::hwid;
use crate::launch;
use crate::model::{
    app_dir, ApiResponse, News, Persisted, Profile, ProfileInfoRequest, Settings, DEFAULT_HOST,
    SERVERS,
};
use crate::storage::{load_state, save_state};

#[derive(Debug)]
pub enum Command {
    Login { login: String, password: String },
    Verify2Fa { code: String },
    Offline,
    SetHwid(bool),
    Logout,
    SelectServer(usize),
    SelectProfile(usize),
    Play,
    Refresh,
    SaveSettings(Settings),
    CheckBackend,
    Shutdown,
}

#[derive(Clone)]
pub struct ProfileView {
    pub id: usize,
    pub name: String,
    pub display_name: String,
    pub initial: String,
    pub subtitle: String,
    pub description: String,
    pub online: String,
    pub icon: Option<String>,
    pub has_icon: bool,
}

#[derive(Clone)]
pub struct NewsView {
    pub title: String,
    pub body: String,
    pub date: String,
}

pub enum Event {
    Screen(i32),
    Busy(bool),
    LoginError(String),
    TwoFa,
    Auth {
        name: String,
        initial: String,
    },
    Profiles {
        views: Vec<ProfileView>,
        selected: usize,
    },
    Selected(ProfileView),
    News(Vec<NewsView>),
    CanPlay(bool),
    Progress {
        visible: bool,
        value: f32,
        title: String,
        text: String,
        speed: String,
    },
    Backend {
        label: String,
        color: String,
        visible: bool,
    },
    Settings {
        settings: Settings,
        max_ram: u64,
    },
    SettingsStatus(String),
    MainStatus(String),
    Error(String),
    LoggedOut,
}

pub struct Worker {
    tx: UnboundedSender<Event>,
    state: Persisted,
    backend: Backend,
    profiles: Vec<Profile>,
    views: Vec<ProfileView>,
    selected: usize,
    offline: bool,
    busy: bool,
    pending_login: (String, String),
}

impl Worker {
    pub fn new(state: Persisted, tx: UnboundedSender<Event>) -> Self {
        let backend = Backend::new(&state.server_host);
        Self {
            tx,
            state,
            backend,
            profiles: Vec::new(),
            views: Vec::new(),
            selected: 0,
            offline: false,
            busy: false,
            pending_login: (String::new(), String::new()),
        }
    }

    fn emit(&self, event: Event) {
        let _ = self.tx.send(event);
    }

    fn host(&self) -> &str {
        &self.state.server_host
    }

    fn token(&self) -> Option<String> {
        self.state
            .user
            .as_ref()
            .map(|session| session.access_token.clone())
            .filter(|token| !token.is_empty())
    }

    pub async fn run(mut self, mut rx: UnboundedReceiver<Command>) {
        self.emit(Event::Settings {
            settings: self.state.settings.clone(),
            max_ram: hwid::total_ram_mb(),
        });

        self.startup().await;

        let mut tick = tokio::time::interval(Duration::from_secs(45));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        tick.tick().await;

        loop {
            tokio::select! {
                command = rx.recv() => {
                    match command {
                        Some(Command::Login { login, password }) => self.login(login, password, String::new()).await,
                        Some(Command::Verify2Fa { code }) => {
                            let (login, password) = self.pending_login.clone();
                            self.login(login, password, code).await;
                        }
                        Some(Command::Offline) => self.enter_offline().await,
                        Some(Command::SetHwid(enabled)) => {
                            self.state.settings.send_hwid = enabled;
                            save_state(&self.state);
                        }
                        Some(Command::Logout) => self.logout(),
                        Some(Command::SelectServer(index)) => self.select_server(index).await,
                        Some(Command::SelectProfile(index)) => self.select_profile(index),
                        Some(Command::Play) => self.play().await,
                        Some(Command::Refresh) => {
                            if self.state.user.is_some() && !self.offline {
                                self.load_data().await;
                            } else {
                                self.spawn_check();
                            }
                        }
                        Some(Command::SaveSettings(settings)) => self.save_settings(settings),
                        Some(Command::CheckBackend) => self.spawn_check(),
                        Some(Command::Shutdown) | None => break,
                    }
                }
                _ = tick.tick() => {
                    if !self.offline {
                        self.spawn_check();
                    }
                }
            }
        }
    }

    async fn startup(&mut self) {
        self.spawn_check();

        if let Some(session) = self.state.user.clone() {
            if !session.is_expired() {
                let hwid = hwid::hwid_for(&self.state.settings);
                let check = tokio::time::timeout(
                    Duration::from_secs(15),
                    self.backend.check_token(&session.access_token, &hwid),
                )
                .await;

                if let Ok(Ok(outcome)) = check {
                    if let Some(mut fresh) = outcome.session {
                        if fresh.name.is_empty() {
                            fresh.name = session.name;
                        }
                        if fresh.uuid.is_empty() {
                            fresh.uuid = session.uuid;
                        }
                        self.state.user = Some(fresh);
                        save_state(&self.state);
                        if let Some(user) = self.state.user.clone() {
                            self.emit(Event::Auth {
                                name: user.name.clone(),
                                initial: user.initial(),
                            });
                        }
                        self.offline = false;
                        self.emit(Event::Screen(1));
                        self.load_data().await;
                        return;
                    }
                }

                self.state.user = None;
                save_state(&self.state);
            }
        }

        if let Some(saved) = self.state.saved_login.clone() {
            self.login(saved.login, saved.password, String::new()).await;
        }
    }

    async fn login(&mut self, login: String, password: String, code: String) {
        if self.busy {
            return;
        }

        self.busy = true;
        self.pending_login = (login.clone(), password.clone());
        self.emit(Event::Busy(true));
        self.emit(Event::LoginError(String::new()));

        let hwid = hwid::hwid_for(&self.state.settings);
        info!(login = %login, "sign-in requested");
        let result = self.backend.signin(&login, &password, &code, &hwid).await;

        if let Ok(outcome) = &result {
            info!(
                authenticated = outcome.session.is_some(),
                requires_2fa = outcome.requires_2fa,
                "sign-in finished"
            );
        }

        match result {
            Ok(outcome) if outcome.session.is_some() => {
                let session = outcome.session.expect("checked");
                self.state.user = Some(session.clone());
                self.state.saved_login = Some(crate::model::SavedLogin { login, password });
                save_state(&self.state);

                self.offline = false;
                self.busy = false;
                self.emit(Event::Busy(false));
                self.emit(Event::Auth {
                    name: session.name.clone(),
                    initial: session.initial(),
                });
                self.emit(Event::Screen(1));
                self.load_data().await;
            }
            Ok(outcome) if outcome.requires_2fa => {
                self.busy = false;
                self.emit(Event::Busy(false));
                self.emit(Event::TwoFa);
            }
            Ok(outcome) => {
                self.busy = false;
                self.emit(Event::Busy(false));
                let mut message = outcome.message;
                if message.is_empty() {
                    message = "Неверный логин или пароль".to_string();
                }
                if !outcome.errors.is_empty() {
                    message.push_str(&format!(": {}", outcome.errors.join(", ")));
                }
                self.emit(Event::LoginError(message));
            }
            Err(error) => {
                self.busy = false;
                self.emit(Event::Busy(false));
                self.emit(Event::LoginError(format!(
                    "Не удалось подключиться к серверу: {error}"
                )));
            }
        }
    }

    fn logout(&mut self) {
        self.state.user = None;
        self.state.saved_login = None;
        self.state.last_profile = None;
        save_state(&self.state);

        self.offline = false;
        self.profiles.clear();
        self.views.clear();
        self.selected = 0;
        self.emit(Event::LoggedOut);
        self.emit(Event::Screen(0));
    }

    async fn select_server(&mut self, index: usize) {
        let Some((_, host)) = SERVERS.get(index) else {
            return;
        };

        // Auth is bound to a backend, so switching drops the current session.
        self.state.server_host = (*host).to_string();
        self.state.user = None;
        self.state.saved_login = None;
        self.state.last_profile = None;
        save_state(&self.state);

        self.backend = Backend::new(host);
        self.offline = false;
        self.busy = false;
        self.profiles.clear();
        self.views.clear();
        self.selected = 0;

        self.emit(Event::Screen(0));
        self.emit(Event::LoggedOut);
        self.emit(Event::Busy(false));
        self.spawn_check();
    }

    /// The backend probe can take tens of seconds on a blocked endpoint. It must
    /// never sit in front of the command queue, or clicks appear to be ignored
    /// right after startup / a server switch.
    fn spawn_check(&self) {
        let backend = self.backend.clone();
        let offline = self.offline;
        let tx = self.tx.clone();

        tokio::spawn(async move {
            let event = if offline {
                Event::Backend {
                    label: "ОФФЛАЙН".into(),
                    color: "#6b7280".into(),
                    visible: true,
                }
            } else if !backend.is_reachable().await {
                Event::Backend {
                    label: String::new(),
                    color: "#6b7280".into(),
                    visible: false,
                }
            } else {
                let mode = backend.mirror_mode("").await;
                let (label, color) = match mode.as_deref() {
                    Some("local") => ("ЗЕРКАЛО", "#2E9E5B"),
                    Some("proxy") => ("ПРОКСИ", "#D9822B"),
                    Some("direct") => ("ОСНОВНОЙ", "#3B82F6"),
                    _ => ("ОНЛАЙН", "#2E9E5B"),
                };
                Event::Backend {
                    label: label.into(),
                    color: color.into(),
                    visible: true,
                }
            };

            let _ = tx.send(event);
        });
    }

    async fn load_data(&mut self) {
        if self.offline {
            self.load_offline().await;
            return;
        }

        let Some(token) = self.token() else {
            self.emit(Event::Screen(0));
            return;
        };

        let result = self.backend.profiles(&token).await;
        match result {
            Ok(profiles) => {
                self.apply_profiles(profiles);
                let news = self.backend.news().await;
                self.emit(Event::News(map_news(&news)));
            }
            Err(error) => {
                let text = error.to_string();
                if text.contains("401") || text.contains("403") || text.contains("Сессия") {
                    self.state.user = None;
                    save_state(&self.state);
                    self.emit(Event::Screen(0));
                    self.emit(Event::LoginError(
                        "Сессия истекла, войдите заново".to_string(),
                    ));
                } else {
                    self.emit(Event::Error(format!(
                        "Не удалось загрузить профили: {error}"
                    )));
                }
            }
        }
    }

    fn apply_profiles(&mut self, mut profiles: Vec<Profile>) {
        profiles.sort_by_key(|profile| std::cmp::Reverse(profile.priority));

        let selected = self
            .state
            .last_profile
            .as_ref()
            .and_then(|name| profiles.iter().position(|profile| &profile.name == name))
            .unwrap_or(0);

        self.views = profiles
            .iter()
            .enumerate()
            .map(|(id, profile)| view_for(id, profile))
            .collect();
        self.profiles = profiles;
        self.selected = selected.min(self.views.len().saturating_sub(1));

        self.emit(Event::Profiles {
            views: self.views.clone(),
            selected: self.selected,
        });

        if let Some(view) = self.views.get(self.selected) {
            self.emit(Event::Selected(view.clone()));
        }
    }

    async fn load_offline(&mut self) {
        let root = app_dir().join("offline-mode");
        let path = root.join("profiles.json");

        let Ok(text) = std::fs::read_to_string(&path) else {
            self.offline = false;
            self.emit(Event::LoginError(format!(
                "Нет оффлайн-профилей: {}",
                path.display()
            )));
            self.emit(Event::Screen(0));
            return;
        };

        let Ok(response) = serde_json::from_str::<ApiResponse<Vec<Profile>>>(&text) else {
            self.offline = false;
            self.emit(Event::Error(
                "Не удалось прочитать offline-mode/profiles.json".into(),
            ));
            self.emit(Event::Screen(0));
            return;
        };

        self.offline = true;
        self.emit(Event::Auth {
            name: "Оффлайн".into(),
            initial: "O".into(),
        });
        self.emit(Event::Screen(1));
        self.apply_profiles(response.data.unwrap_or_default());
        self.emit(Event::News(vec![NewsView {
            title: "Оффлайн-режим".into(),
            body: "Бэкенд не используется. Запуск идёт по сохранённому снимку профиля.".into(),
            date: String::new(),
        }]));
        self.emit(Event::MainStatus("Оффлайн-режим".into()));
    }

    fn select_profile(&mut self, index: usize) {
        let Some(view) = self.views.get(index).cloned() else {
            return;
        };

        self.selected = index;
        self.state.last_profile = Some(view.name.clone());
        save_state(&self.state);
        self.emit(Event::Selected(view));
    }

    async fn play(&mut self) {
        if self.busy {
            return;
        }

        self.busy = true;
        self.emit(Event::CanPlay(false));

        let result = std::panic::AssertUnwindSafe(self.play_inner())
            .catch_unwind()
            .await;

        if result.is_err() {
            error!("launch task panicked");
            self.emit(Event::Progress {
                visible: false,
                value: 0.0,
                title: String::new(),
                text: String::new(),
                speed: String::new(),
            });
            self.emit(Event::Error(
                "Внутренняя ошибка запуска. Подробности в логе лаунчера.".into(),
            ));
        }

        self.busy = false;
        self.emit(Event::CanPlay(true));
    }

    async fn play_inner(&mut self) {
        let Some(profile) = self.profiles.get(self.selected).cloned() else {
            self.emit(Event::Error("Профиль не выбран".into()));
            return;
        };

        let settings = self.state.settings.clone();
        let max_ram = hwid::total_ram_mb();
        info!(
            profile = %profile.name,
            offline = self.offline,
            "launch requested"
        );

        self.emit(Event::Error(String::new()));
        self.emit(Event::MainStatus(String::new()));
        self.emit(Event::Progress {
            visible: true,
            value: 0.0,
            title: "Подготовка".into(),
            text: "Получаем параметры профиля…".into(),
            speed: String::new(),
        });

        let info_result = if self.offline {
            load_offline_info(&profile.name)
        } else {
            let Some(token) = self.token() else {
                self.emit(Event::LoginError("Сессия истекла".into()));
                self.emit(Event::Screen(0));
                return;
            };
            let session = self.state.user.as_ref().expect("token implies user");
            let request = ProfileInfoRequest {
                user_name: &session.name,
                profile_name: &profile.name,
                user_access_token: &token,
                window_width: settings.game_width,
                window_height: settings.game_height,
                game_address: "",
                game_port: 0,
                is_full_screen: settings.fullscreen,
                ram_size: settings.xmx,
                user_uuid: &session.uuid,
                os_type: hwid::os_type(),
                os_architecture: hwid::os_architecture(),
            };

            match self.backend.profile_info(&token, &request).await {
                Ok(Some(info)) => Ok(info),
                Ok(None) => Err(anyhow::anyhow!("Сервер не вернул параметры профиля")),
                Err(error) => Err(error),
            }
        };

        let info = match info_result {
            Ok(info) => info,
            Err(error) => {
                self.emit(Event::Progress {
                    visible: false,
                    value: 0.0,
                    title: String::new(),
                    text: String::new(),
                    speed: String::new(),
                });
                self.emit(Event::Error(format!(
                    "Не удалось получить профиль: {error}"
                )));
                return;
            }
        };

        let safe_max = 32768.min((max_ram.saturating_sub(4096)).max(1024)) as u32;
        let xmx = settings.xmx.clamp(1024, safe_max);
        let xms = settings.xms.clamp(512, xmx);

        let install_dir = settings.install_path();
        let tx = self.tx.clone();
        let progress = Arc::new(move |event: SyncProgress| {
            let value = if event.total == 0 {
                0.0
            } else {
                (event.done as f32 / event.total as f32).clamp(0.0, 1.0)
            };
            let _ = tx.send(Event::Progress {
                visible: true,
                value,
                title: event.phase,
                text: format!("{} / {} · {}", event.done, event.total, event.current),
                speed: if event.speed_bps > 0 {
                    format!("{}/s", format_bytes(event.speed_bps))
                } else {
                    String::new()
                },
            });
        });

        if let Err(error) = sync_profile(&self.backend, &info, &install_dir, progress).await {
            self.emit(Event::Progress {
                visible: false,
                value: 0.0,
                title: String::new(),
                text: String::new(),
                speed: String::new(),
            });
            self.emit(Event::Error(format!(
                "Ошибка синхронизации файлов: {error}"
            )));
            return;
        }

        self.emit(Event::Progress {
            visible: true,
            value: 1.0,
            title: "Запуск".into(),
            text: "Запускаем Minecraft…".into(),
            speed: String::new(),
        });

        let spec = launch::build_spec(
            &info,
            &install_dir,
            &settings,
            !self.offline,
            self.host(),
            xms,
            xmx,
        );

        info!(files = info.files.len(), java = %info.java_path, "manifest ready");

        match spec.and_then(|spec| launch::spawn(&spec)) {
            Ok(pid) => {
                info!(pid, "game spawned");
                self.emit(Event::Progress {
                    visible: false,
                    value: 0.0,
                    title: String::new(),
                    text: String::new(),
                    speed: String::new(),
                });
                self.emit(Event::MainStatus(format!("Игра запущена (PID {pid})")));
            }
            Err(error) => {
                error!(%error, "launch failed");
                self.emit(Event::Progress {
                    visible: false,
                    value: 0.0,
                    title: String::new(),
                    text: String::new(),
                    speed: String::new(),
                });
                self.emit(Event::Error(format!("Не удалось запустить игру: {error}")));
            }
        }
    }

    fn save_settings(&mut self, mut settings: Settings) {
        let max_ram = hwid::total_ram_mb();
        let safe_max = 32768.min((max_ram.saturating_sub(4096)).max(1024)) as u32;

        settings.xmx = settings.xmx.clamp(1024, safe_max);
        settings.xms = settings.xms.clamp(512, settings.xmx);
        settings.game_width = settings.game_width.clamp(640, 7680);
        settings.game_height = settings.game_height.clamp(480, 4320);
        settings.install_dir = settings.install_dir.trim().to_string();

        if settings.install_dir.is_empty() {
            settings.install_dir = app_dir().to_string_lossy().into_owned();
        }

        self.state.settings = settings.clone();
        save_state(&self.state);

        self.emit(Event::Settings { settings, max_ram });
        self.emit(Event::SettingsStatus("Сохранено".into()));
    }

    async fn enter_offline(&mut self) {
        self.offline = true;
        self.load_offline().await;
        self.spawn_check();
    }
}

fn view_for(id: usize, profile: &Profile) -> ProfileView {
    let display_name = profile
        .display_name
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| profile.name.clone());

    let initial = display_name
        .chars()
        .next()
        .map(|value| value.to_uppercase().to_string())
        .unwrap_or_else(|| "?".into());

    let mut parts = Vec::new();
    if let Some(version) = profile
        .game_version
        .as_ref()
        .filter(|value| !value.is_empty())
    {
        parts.push(version.clone());
    }
    if let Some(launch) = profile
        .launch_version
        .as_ref()
        .filter(|value| !value.is_empty())
    {
        parts.push(launch.clone());
    }
    let subtitle = parts.join(" · ");

    let online_total: i64 = profile
        .servers
        .iter()
        .filter_map(|server| server.online)
        .sum();
    let max_total: i64 = profile
        .servers
        .iter()
        .filter_map(|server| server.max_online)
        .sum();

    let online = if online_total > 0 {
        if max_total > 0 {
            format!("{online_total} / {max_total} онлайн")
        } else {
            format!("{online_total} онлайн")
        }
    } else {
        String::new()
    };

    ProfileView {
        id,
        name: profile.name.clone(),
        display_name,
        initial,
        subtitle,
        description: profile.description.clone().unwrap_or_default(),
        online,
        has_icon: profile
            .icon_base64
            .as_ref()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false),
        icon: profile.icon_base64.clone(),
    }
}

fn map_news(news: &[News]) -> Vec<NewsView> {
    if news.is_empty() {
        return vec![NewsView {
            title: "Новостей пока нет".into(),
            body: "Как только администрация опубликует новости, они появятся здесь.".into(),
            date: String::new(),
        }];
    }

    news.iter()
        .map(|item| NewsView {
            title: item.title.clone(),
            body: plain_text(&item.content),
            date: format_date(item.date.as_deref()),
        })
        .collect()
}

fn plain_text(html: &str) -> String {
    let tags = regex::Regex::new(r"(?is)<[^>]*>").expect("valid regex");
    let text = tags.replace_all(html, " ");
    let text = text
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");

    let space = regex::Regex::new(r"\s+").expect("valid regex");
    let text = space.replace_all(&text, " ").trim().to_string();

    if text.chars().count() > 600 {
        let mut shortened: String = text.chars().take(600).collect();
        shortened.push('…');
        shortened
    } else {
        text
    }
}

fn format_date(value: Option<&str>) -> String {
    let Some(value) = value else {
        return String::new();
    };

    chrono::DateTime::parse_from_rfc3339(value)
        .map(|date| date.format("%d.%m.%Y").to_string())
        .unwrap_or_else(|_| value.to_string())
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["Б", "КБ", "МБ", "ГБ", "ТБ"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{} {}", bytes, UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn load_offline_info(profile_name: &str) -> Result<crate::model::ProfileInfo> {
    let path = app_dir()
        .join("offline-mode")
        .join(profile_name)
        .join("profileInfo.json");

    let text = std::fs::read_to_string(&path)
        .map_err(|error| anyhow::anyhow!("{}: {error}", path.display()))?;
    let response: ApiResponse<crate::model::ProfileInfo> = serde_json::from_str(&text)?;

    response
        .data
        .ok_or_else(|| anyhow::anyhow!("снимок профиля пуст"))
}

#[allow(dead_code)]
fn _default_host() -> &'static str {
    DEFAULT_HOST
}

#[allow(dead_code)]
fn _settings_path() -> PathBuf {
    crate::model::app_dir()
}

#[allow(dead_code)]
fn _state() -> Persisted {
    load_state()
}
