use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use futures::FutureExt;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tracing::{error, info};

use crate::api::Backend;
use crate::discord::DiscordRpc;
use crate::files::{sync_profile, SyncProgress};
use crate::hwid;
use crate::launch;
use crate::model::{
    app_dir, ApiResponse, News, Persisted, Profile, ProfileInfoRequest, Settings, DEFAULT_HOST,
    MIRROR_HOST, SERVERS,
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
    OpenMods,
    ToggleMod { id: usize, enabled: bool },
    RefreshMirror,
    SaveMirrorKey(String),
    ClearMirrorKey,
    SetMirrorMode(String),
    SetTor(bool),
    SelfTest,
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
pub struct ModView {
    pub id: usize,
    pub name: String,
    pub description: String,
    pub enabled: bool,
}

#[derive(Clone)]
pub struct SelfTestRowView {
    pub name: String,
    pub status: String,
    pub message: String,
    pub duration: String,
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
    Mods {
        views: Vec<ModView>,
        enabled: usize,
        total: usize,
    },
    CanPlay(bool),
    MirrorState {
        key: String,
        mode: String,
        upstream: String,
        can_control: bool,
        tor_enabled: bool,
        tor_available: bool,
    },
    SelfTest {
        rows: Vec<SelfTestRowView>,
        summary: String,
    },
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
    mirror: Backend,
    via_mirror: bool,
    profiles: Vec<Profile>,
    views: Vec<ProfileView>,
    mods: Vec<ModView>,
    selected: usize,
    offline: bool,
    busy: bool,
    pending_login: (String, String),
    discord: Option<DiscordRpc>,
}

impl Worker {
    pub fn new(state: Persisted, tx: UnboundedSender<Event>) -> Self {
        let secret = (!state.settings.client_secret.trim().is_empty())
            .then(|| state.settings.client_secret.clone())
            .or_else(|| std::env::var("VESTALIS_CLIENT_SECRET").ok())
            .filter(|value| !value.trim().is_empty());
        let backend = Backend::new(&state.server_host).with_client(secret.clone(), String::new());
        let mirror = Backend::new(MIRROR_HOST).with_client(secret, String::new());
        let via_mirror = state.auth_via_mirror;
        Self {
            tx,
            state,
            backend,
            mirror,
            via_mirror,
            profiles: Vec::new(),
            views: Vec::new(),
            mods: Vec::new(),
            selected: 0,
            offline: false,
            busy: false,
            pending_login: (String::new(), String::new()),
            discord: None,
        }
    }

    fn emit(&self, event: Event) {
        let _ = self.tx.send(event);
    }

    fn token(&self) -> Option<String> {
        self.state
            .user
            .as_ref()
            .map(|session| session.access_token.clone())
            .filter(|token| !token.is_empty())
    }

    /// Mirror control endpoints exist only on PL-1; RU/EU answer 404 there.
    fn is_pl_host(&self) -> bool {
        self.state
            .server_host
            .trim_end_matches('/')
            .eq_ignore_ascii_case(MIRROR_HOST.trim_end_matches('/'))
    }

    /// Data and token operations use the host that actually issued the session.
    fn data_backend(&self) -> &Backend {
        if self.via_mirror {
            &self.mirror
        } else {
            &self.backend
        }
    }

    async fn signin_with_fallback(
        &self,
        login: &str,
        password: &str,
        code: &str,
        hwid: &str,
    ) -> Result<(crate::api::AuthOutcome, bool)> {
        let first = self.backend.signin(login, password, code, hwid).await?;

        if first.session.is_none() && first.blocked && self.backend.base() != self.mirror.base() {
            info!("sign-in blocked by the selected host, retrying through the mirror");
            let second = self.mirror.signin(login, password, code, hwid).await?;
            if second.session.is_some() {
                return Ok((second, true));
            }
        }

        Ok((first, false))
    }

    async fn check_token_with_fallback(
        &self,
        token: &str,
        hwid: &str,
    ) -> Result<(crate::api::AuthOutcome, bool)> {
        let first = self.backend.check_token(token, hwid).await?;

        if first.session.is_none() && first.blocked && self.backend.base() != self.mirror.base() {
            let second = self.mirror.check_token(token, hwid).await?;
            if second.session.is_some() {
                return Ok((second, true));
            }
        }

        Ok((first, false))
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

        // Approximates the SignalR "RefreshProfiles" event of the C# client.
        let mut profiles_tick = tokio::time::interval(Duration::from_secs(90));
        profiles_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        profiles_tick.tick().await;

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
                        Some(Command::OpenMods) => self.open_mods().await,
                        Some(Command::ToggleMod { id, enabled }) => self.toggle_mod(id, enabled),
                        Some(Command::RefreshMirror) => self.refresh_mirror().await,
                        Some(Command::SaveMirrorKey(key)) => self.save_mirror_key(key).await,
                        Some(Command::ClearMirrorKey) => {
                            self.state.mirror_key.clear();
                            save_state(&self.state);
                            self.emit(Event::SettingsStatus("Ключ удалён".into()));
                            self.emit_mirror(None);
                        }
                        Some(Command::SetMirrorMode(mode)) => self.set_mirror_mode(mode).await,
                        Some(Command::SetTor(enabled)) => self.set_tor(enabled).await,
                        Some(Command::SelfTest) => self.run_self_test().await,
                        Some(Command::Shutdown) | None => break,
                    }
                }
                _ = tick.tick() => {
                    if !self.offline {
                        self.spawn_check();
                    }
                }
                _ = profiles_tick.tick() => {
                    if !self.offline && self.state.user.is_some() && !self.busy {
                        self.load_data().await;
                    }
                }
            }
        }
    }

    async fn startup(&mut self) {
        let hwid = hwid::hwid_for(&self.state.settings);
        self.backend.set_hwid(hwid.clone());
        self.mirror.set_hwid(hwid);
        self.spawn_check();

        if let Some(session) = self.state.user.clone() {
            if !session.is_expired() {
                let hwid = hwid::hwid_for(&self.state.settings);
                let check = tokio::time::timeout(
                    Duration::from_secs(15),
                    self.check_token_with_fallback(&session.access_token, &hwid),
                )
                .await;

                if let Ok(Ok((outcome, via_mirror))) = check {
                    if let Some(mut fresh) = outcome.session {
                        if fresh.name.is_empty() {
                            fresh.name = session.name;
                        }
                        if fresh.uuid.is_empty() {
                            fresh.uuid = session.uuid;
                        }
                        self.via_mirror = via_mirror;
                        self.state.auth_via_mirror = via_mirror;
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
        self.backend.set_hwid(hwid.clone());
        self.mirror.set_hwid(hwid.clone());
        info!(login = %login, "sign-in requested");
        let result = self
            .signin_with_fallback(&login, &password, &code, &hwid)
            .await;

        if let Ok((outcome, via_mirror)) = &result {
            info!(
                authenticated = outcome.session.is_some(),
                requires_2fa = outcome.requires_2fa,
                via_mirror,
                "sign-in finished"
            );
        }

        match result {
            Ok((outcome, via_mirror)) if outcome.session.is_some() => {
                let session = outcome.session.expect("checked");
                self.via_mirror = via_mirror;
                self.state.auth_via_mirror = via_mirror;
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
                if via_mirror {
                    self.emit(Event::MainStatus(
                        "Сессия зеркала: официальный хост отклонил клиент. Для RU/EU-серверов нужен доступ от их администратора.".into(),
                    ));
                }
                self.load_data().await;
            }
            Ok((outcome, _)) if outcome.requires_2fa => {
                self.busy = false;
                self.emit(Event::Busy(false));
                self.emit(Event::TwoFa);
            }
            Ok((outcome, _)) => {
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
        self.state.auth_via_mirror = false;
        save_state(&self.state);

        self.via_mirror = false;
        self.offline = false;
        self.profiles.clear();
        self.views.clear();
        self.selected = 0;
        self.emit(Event::LoggedOut);
        self.emit(Event::Screen(0));

        if let Some(rpc) = &self.discord {
            rpc.set_state("В лаунчере");
        }
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

        let secret = (!self.state.settings.client_secret.trim().is_empty())
            .then(|| self.state.settings.client_secret.clone())
            .or_else(|| std::env::var("VESTALIS_CLIENT_SECRET").ok())
            .filter(|value| !value.trim().is_empty());
        let hwid = hwid::hwid_for(&self.state.settings);
        self.backend = Backend::new(host).with_client(secret.clone(), hwid.clone());
        self.mirror.set_client(secret, hwid);
        self.via_mirror = false;
        self.state.auth_via_mirror = false;
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

        // A token issued through the mirror may still be accepted by the
        // selected host (the mirror proxies to the same upstream). Prefer the
        // real server's data when that works, so RU/EU labels are not cosmetic.
        if self.via_mirror && self.backend.base() != self.mirror.base() {
            if let Ok(profiles) = self.backend.profiles(&token).await {
                if !profiles.is_empty() {
                    info!("mirror token accepted by the selected host");
                    self.via_mirror = false;
                    self.state.auth_via_mirror = false;
                    save_state(&self.state);
                    self.apply_profiles(profiles);
                    let news = self.backend.news().await;
                    self.emit(Event::News(map_news(&news)));
                    return;
                }
            }
        }

        let backend = self.data_backend().clone();
        let mut result = backend.profiles(&token).await;

        // A mirror can briefly answer with an empty snapshot while syncing;
        // retry once instead of showing an empty sidebar.
        if matches!(&result, Ok(profiles) if profiles.is_empty()) {
            tokio::time::sleep(Duration::from_millis(600)).await;
            result = backend.profiles(&token).await;
        }

        match result {
            Ok(profiles) => {
                self.apply_profiles(profiles);
                let news = backend.news().await;
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

        self.ensure_discord();
        self.update_discord_presence();
    }

    fn ensure_discord(&mut self) {
        if self.discord.is_none() && !self.offline {
            self.discord = Some(DiscordRpc::start());
        }
    }

    fn update_discord_presence(&self) {
        let Some(rpc) = &self.discord else {
            return;
        };

        let state = self
            .profiles
            .get(self.selected)
            .map(|profile| {
                profile
                    .display_name
                    .clone()
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| profile.name.clone())
            })
            .map(|name| format!("В лаунчере: {name}"))
            .unwrap_or_else(|| "В лаунчере".to_string());

        rpc.set_state(state);
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
        self.update_discord_presence();
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

        if let Some(rpc) = &self.discord {
            rpc.set_state(format!(
                "Играет: {}",
                profile
                    .display_name
                    .clone()
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| profile.name.clone())
            ));
        }

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
            let (window_width, window_height) = if settings.fullscreen {
                hwid::screen_size()
            } else {
                (settings.game_width, settings.game_height)
            };
            info!(
                window_width,
                window_height,
                fullscreen = settings.fullscreen,
                "profile resolution"
            );
            let request = ProfileInfoRequest {
                user_name: &session.name,
                profile_name: &profile.name,
                user_access_token: &token,
                window_width,
                window_height,
                game_address: "",
                game_port: 0,
                is_full_screen: settings.fullscreen,
                ram_size: settings.xmx,
                user_uuid: &session.uuid,
                os_type: hwid::os_type(),
                os_architecture: hwid::os_architecture(),
            };

            let backend = self.data_backend().clone();
            match backend.profile_info(&token, &request).await {
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
        info!(xms, xmx, "heap configured");

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
            self.data_backend().base(),
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

    fn mods_dir(&self, profile: &str) -> PathBuf {
        self.state
            .settings
            .install_path()
            .join("game data")
            .join(profile)
            .join("mods")
    }

    fn mod_file_is_enabled(&self, profile: &str, name: &str) -> bool {
        let directory = self.mods_dir(profile);
        let Ok(entries) = std::fs::read_dir(&directory) else {
            return false;
        };
        let needle = name.to_lowercase();

        entries.flatten().any(|entry| {
            let file_name = entry.file_name().to_string_lossy().to_lowercase();
            file_name.contains(&needle)
                && file_name.ends_with(".jar")
                && !file_name.ends_with(".jar.disabled")
        })
    }

    async fn open_mods(&mut self) {
        let Some(profile) = self.profiles.get(self.selected).cloned() else {
            self.emit(Event::Error("Профиль не выбран".into()));
            return;
        };

        if self.offline {
            self.emit(Event::Error(
                "Список модов доступен только при подключённом бэкенде".into(),
            ));
            return;
        }

        let Some(token) = self.token() else {
            self.emit(Event::Screen(0));
            return;
        };

        self.emit(Event::MainStatus("Загрузка списка модов…".into()));
        let backend = self.data_backend().clone();
        let mods = backend.optional_mods(&profile.name, &token).await;
        let details = backend.optional_mods_details(&token).await;

        match mods {
            Ok(list) => {
                let mut views: Vec<ModView> = list
                    .into_iter()
                    .map(|item| {
                        let description = item
                            .description
                            .clone()
                            .filter(|value| !value.trim().is_empty())
                            .or_else(|| {
                                details
                                    .iter()
                                    .find(|detail| {
                                        detail.key.eq_ignore_ascii_case(&item.name)
                                            || detail.title.eq_ignore_ascii_case(&item.name)
                                    })
                                    .map(|detail| detail.description.clone())
                                    .filter(|value| !value.trim().is_empty())
                            })
                            .unwrap_or_default();

                        ModView {
                            id: 0,
                            name: item.name.clone(),
                            description,
                            enabled: self.mod_file_is_enabled(&profile.name, &item.name),
                        }
                    })
                    .collect();

                views.sort_by(|left, right| left.name.cmp(&right.name));
                for (index, view) in views.iter_mut().enumerate() {
                    view.id = index;
                }

                self.mods = views;
                self.emit_mods();
                self.emit(Event::MainStatus(String::new()));
                self.emit(Event::Screen(3));
            }
            Err(error) => {
                self.emit(Event::MainStatus(String::new()));
                self.emit(Event::Error(format!("Не удалось загрузить моды: {error}")));
            }
        }
    }

    fn toggle_mod(&mut self, id: usize, enabled: bool) {
        let Some(profile) = self.profiles.get(self.selected).cloned() else {
            return;
        };
        let Some(view) = self.mods.get(id).cloned() else {
            return;
        };

        let directory = self.mods_dir(&profile.name);
        let Ok(entries) = std::fs::read_dir(&directory) else {
            self.emit(Event::Error(format!(
                "Папка модов не найдена: {}",
                directory.display()
            )));
            return;
        };

        let needle = view.name.to_lowercase();
        let mut candidates: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                let name = path
                    .file_name()
                    .map(|value| value.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                name.contains(&needle) && name.contains(".jar")
            })
            .collect();

        candidates.sort_by_key(|path| {
            let name = path
                .file_name()
                .map(|value| value.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if name.starts_with(&needle) {
                0
            } else {
                1
            }
        });

        let Some(source) = candidates.into_iter().next() else {
            self.emit(Event::Error(format!(
                "Файл мода «{}» ещё не скачан. Запустите игру один раз.",
                view.name
            )));
            return;
        };

        let source_text = source.to_string_lossy().to_string();
        let target = if enabled {
            PathBuf::from(source_text.trim_end_matches(".disabled"))
        } else if source_text.ends_with(".disabled") {
            source.clone()
        } else {
            PathBuf::from(format!("{source_text}.disabled"))
        };

        if source != target {
            if target.exists() {
                let _ = std::fs::remove_file(&target);
            }
            if let Err(error) = std::fs::rename(&source, &target) {
                self.emit(Event::Error(format!(
                    "Не удалось переключить мод «{}»: {error}",
                    view.name
                )));
                return;
            }
        }

        if let Some(item) = self.mods.get_mut(id) {
            item.enabled = enabled;
        }
        self.emit_mods();
    }

    fn emit_mods(&self) {
        let enabled = self.mods.iter().filter(|item| item.enabled).count();
        self.emit(Event::Mods {
            views: self.mods.clone(),
            enabled,
            total: self.mods.len(),
        });
    }

    async fn refresh_mirror(&mut self) {
        if !self.is_pl_host() {
            self.emit_mirror(None);
            return;
        }

        let key = self.state.mirror_key.clone();
        let state = self.mirror.mirror_state(&key).await;
        self.emit_mirror(state);
    }

    fn emit_mirror(&self, state: Option<crate::model::MirrorState>) {
        let key = self.state.mirror_key.clone();
        match state {
            Some(state) => self.emit(Event::MirrorState {
                key,
                mode: state.mode,
                upstream: state.upstream.unwrap_or_default(),
                can_control: state.can_control,
                tor_enabled: state.tor_enabled,
                tor_available: state.tor_available,
            }),
            None => self.emit(Event::MirrorState {
                key,
                mode: String::new(),
                upstream: String::new(),
                can_control: false,
                tor_enabled: false,
                tor_available: false,
            }),
        }
    }

    async fn save_mirror_key(&mut self, key: String) {
        if !self.is_pl_host() {
            self.emit(Event::SettingsStatus(
                "Сервисный доступ доступен только на PL-1".into(),
            ));
            return;
        }

        let key = key.trim().to_string();
        if key.is_empty() {
            self.emit(Event::SettingsStatus("Введите сервисный ключ".into()));
            return;
        }

        let state = self.mirror.mirror_state(&key).await;

        if state
            .as_ref()
            .map(|value| value.can_control)
            .unwrap_or(false)
        {
            self.state.mirror_key = key;
            save_state(&self.state);
            self.emit(Event::SettingsStatus("Сервисный ключ принят".into()));
            self.emit_mirror(state);
        } else {
            self.emit(Event::SettingsStatus(
                "Сервисный ключ не принят сервером".into(),
            ));
        }
    }

    async fn set_mirror_mode(&mut self, mode: String) {
        if !self.is_pl_host() {
            self.emit(Event::SettingsStatus(
                "Сервисный доступ доступен только на PL-1".into(),
            ));
            return;
        }

        let key = self.state.mirror_key.clone();
        if key.is_empty() {
            self.emit(Event::SettingsStatus(
                "Сервисный ключ не подтверждён".into(),
            ));
            return;
        }

        let backend = self.mirror.clone();
        match backend.set_mirror_mode(&key, &mode).await {
            Ok(message) => {
                self.emit(Event::SettingsStatus(message));
                self.refresh_mirror().await;
            }
            Err(error) => self.emit(Event::SettingsStatus(error.to_string())),
        }
    }

    async fn set_tor(&mut self, enabled: bool) {
        if !self.is_pl_host() {
            self.emit(Event::SettingsStatus(
                "Сервисный доступ доступен только на PL-1".into(),
            ));
            return;
        }

        let key = self.state.mirror_key.clone();
        if key.is_empty() {
            self.emit(Event::SettingsStatus(
                "Сервисный ключ не подтверждён".into(),
            ));
            return;
        }

        let backend = self.mirror.clone();
        match backend.set_tor(&key, enabled).await {
            Ok(message) => {
                self.emit(Event::SettingsStatus(message));
                self.refresh_mirror().await;
            }
            Err(error) => self.emit(Event::SettingsStatus(error.to_string())),
        }
    }

    async fn run_self_test(&mut self) {
        if !self.is_pl_host() {
            self.emit(Event::SettingsStatus(
                "Сервисный доступ доступен только на PL-1".into(),
            ));
            return;
        }

        let key = self.state.mirror_key.clone();
        if key.is_empty() {
            self.emit(Event::SettingsStatus(
                "Сервисный ключ не подтверждён".into(),
            ));
            return;
        }

        self.emit(Event::SettingsStatus("Самодиагностика…".into()));
        let backend = self.mirror.clone();
        let report = backend.self_test(&key).await;

        match report {
            Some(report) => {
                let rows: Vec<SelfTestRowView> = report
                    .checks
                    .into_iter()
                    .map(|check| SelfTestRowView {
                        name: if check.name.is_empty() {
                            check.key
                        } else {
                            check.name
                        },
                        status: check.status,
                        message: check.message,
                        duration: format!("{} мс", check.duration_ms),
                    })
                    .collect();

                let summary = format!(
                    "Статус: {} · режим: {} · upstream: {} · {} мс",
                    report.status, report.mode, report.upstream, report.duration_ms
                );
                self.emit(Event::SelfTest { rows, summary });
                self.emit(Event::SettingsStatus("Самодиагностика завершена".into()));
            }
            None => self.emit(Event::SettingsStatus(
                "Самодиагностика недоступна (ключ не принят)".into(),
            )),
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

        let secret = (!settings.client_secret.trim().is_empty())
            .then(|| settings.client_secret.clone())
            .or_else(|| std::env::var("VESTALIS_CLIENT_SECRET").ok())
            .filter(|value| !value.trim().is_empty());
        let hwid = self.backend.client_hwid().to_string();
        self.backend.set_client(secret.clone(), hwid.clone());
        self.mirror.set_client(secret, hwid);

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
