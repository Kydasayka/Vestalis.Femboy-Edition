use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use futures::FutureExt;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tracing::{error, info};

use crate::api::Backend;
use crate::discord::DiscordRpc;
use crate::files::{sync_profile, ProgressFn, SyncProgress};
use crate::hwid;
use crate::launch;
use crate::layout::{LayoutConfig, LayoutProfile};
use crate::model::{
    app_dir, avatar_path, local_app_dir, ApiResponse, News, Persisted, Profile, ProfileInfoRequest,
    Settings, DEFAULT_HOST, SERVERS,
};
use crate::signalr::{self, HubEvent};
use crate::storage::{load_state, save_state};
use crate::theme::{self, ThemeItem};

#[derive(Debug)]
pub enum Command {
    Login {
        login: String,
        password: String,
    },
    Verify2Fa {
        code: String,
    },
    Offline,
    SetHwid(bool),
    Logout,
    SelectServer(usize),
    SelectProfile(usize),
    Play,
    Refresh,
    SaveSettings(Box<Settings>),
    RefreshThemes,
    OpenThemesFolder,
    OpenInstallFolder,
    OpenLogsFolder,
    ClearValidationCache,
    CheckFiles,
    CancelLaunch,
    GameExited(u32),
    SaveTheme(String),
    MoveLayoutItem {
        item: String,
        direction: String,
    },
    LayoutDrop {
        item: String,
        zone: String,
        before: String,
    },
    LayoutToggle {
        item: String,
        hidden: bool,
    },
    LayoutWidths {
        left: u32,
        right: u32,
    },
    LayoutHeight {
        item: String,
        height: u32,
    },
    LayoutProfileSave(String),
    LayoutProfileSwitch(String),
    LayoutProfileDelete(String),
    LayoutProfileRename {
        old: String,
        new: String,
    },
    ResetLayout,
    OpenMods,
    ToggleMod {
        id: usize,
        enabled: bool,
    },
    DownloadMod(usize),
    DownloadMissingMods,
    SetModsFilter(String),
    SetAllMods(bool),
    SetProfileFilter(String),
    OpenModsFolder,
    ExportDiagnostics,
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
    pub downloaded: bool,
    pub downloading: bool,
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
    Ready,
    LoginError(String),
    TwoFa,
    Auth {
        name: String,
        initial: String,
    },
    Avatar(String),
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
        missing: usize,
    },
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
    Themes {
        items: Vec<ThemeItem>,
        active: String,
    },
    Layout(LayoutConfig),
    LayoutProfiles {
        profiles: Vec<LayoutProfile>,
        active: String,
    },
    Settings {
        settings: Box<Settings>,
        max_ram: u64,
    },
    SettingsStatus(String),
    MainStatus(String),
    GameStarted {
        pid: u32,
        hide: bool,
    },
    GameExited {
        pid: u32,
    },
    Error(String),
    Notification {
        title: String,
        body: String,
    },
    LoggedOut,
}

pub struct Worker {
    tx: UnboundedSender<Event>,
    state: Persisted,
    backend: Backend,
    profiles: Vec<Profile>,
    views: Vec<ProfileView>,
    mods: Vec<ModView>,
    mods_filter: String,
    profile_filter: String,
    selected: usize,
    offline: bool,
    busy: bool,
    pending_login: (String, String),
    discord: Option<DiscordRpc>,
    profiles_signature: String,
    launch_cancel: Arc<AtomicBool>,
    hub: Option<tokio::task::JoinHandle<()>>,
    hub_tx: Option<UnboundedSender<HubEvent>>,
}

impl Worker {
    pub fn new(state: Persisted, tx: UnboundedSender<Event>) -> Self {
        let secret = client_secret();
        let backend = Backend::new(&state.server_host).with_client(Some(secret), String::new());
        Self {
            tx,
            state,
            backend,
            profiles: Vec::new(),
            views: Vec::new(),
            mods: Vec::new(),
            mods_filter: String::new(),
            profile_filter: String::new(),
            selected: 0,
            offline: false,
            busy: false,
            pending_login: (String::new(), String::new()),
            discord: None,
            profiles_signature: String::new(),
            launch_cancel: Arc::new(AtomicBool::new(false)),
            hub: None,
            hub_tx: None,
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

    fn data_backend(&self) -> &Backend {
        &self.backend
    }

    async fn signin(
        &self,
        login: &str,
        password: &str,
        code: &str,
        hwid: &str,
    ) -> Result<(crate::api::AuthOutcome, bool)> {
        Ok((
            self.backend.signin(login, password, code, hwid).await?,
            false,
        ))
    }

    async fn check_session_token(
        &self,
        token: &str,
        hwid: &str,
    ) -> Result<(crate::api::AuthOutcome, bool)> {
        Ok((self.backend.check_token(token, hwid).await?, false))
    }

    pub async fn run(mut self, mut rx: UnboundedReceiver<Command>) {
        self.ensure_layout_profiles();
        self.emit(Event::Settings {
            settings: Box::new(self.state.settings.clone()),
            max_ram: hwid::total_ram_mb(),
        });
        self.emit_layout_profiles();

        let (hub_tx, mut hub_rx) = tokio::sync::mpsc::unbounded_channel();
        self.hub_tx = Some(hub_tx);

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
                        Some(Command::SaveSettings(settings)) => self.save_settings(*settings),
                        Some(Command::RefreshThemes) => self.refresh_themes(),
                        Some(Command::OpenThemesFolder) => self.open_themes_folder(),
                        Some(Command::OpenInstallFolder) => self.open_install_folder(),
                        Some(Command::OpenLogsFolder) => self.open_logs_folder(),
                        Some(Command::ClearValidationCache) => self.clear_validation_cache(),
                        Some(Command::CheckFiles) => self.check_files().await,
                        Some(Command::CancelLaunch) => {
                            self.launch_cancel.store(true, Ordering::Relaxed);
                            self.emit(Event::SettingsStatus("Отмена запуска…".into()));
                        }
                        Some(Command::GameExited(pid)) => {
                            if let Some(rpc) = &self.discord {
                                rpc.set_state("В лаунчере");
                            }
                            self.emit(Event::MainStatus(format!("Игра закрыта (PID {pid})")));
                        }
                        Some(Command::SaveTheme(name)) => self.save_theme(name),
                        Some(Command::MoveLayoutItem { item, direction }) => {
                            self.move_layout_item(&item, &direction)
                        }
                        Some(Command::LayoutDrop { item, zone, before }) => {
                            self.drop_layout_item(&item, &zone, &before)
                        }
                        Some(Command::LayoutToggle { item, hidden }) => {
                            self.toggle_layout_item(&item, hidden)
                        }
                        Some(Command::LayoutWidths { left, right }) => {
                            self.set_layout_widths(left, right)
                        }
                        Some(Command::LayoutHeight { item, height }) => {
                            self.set_layout_height(&item, height)
                        }
                        Some(Command::LayoutProfileSave(name)) => self.save_layout_profile(&name),
                        Some(Command::LayoutProfileSwitch(name)) => {
                            self.switch_layout_profile(&name)
                        }
                        Some(Command::LayoutProfileDelete(name)) => {
                            self.delete_layout_profile(&name)
                        }
                        Some(Command::LayoutProfileRename { old, new }) => {
                            self.rename_layout_profile(&old, &new)
                        }
                        Some(Command::ResetLayout) => self.save_layout(LayoutConfig::default()),
                        Some(Command::OpenMods) => self.open_mods().await,
                        Some(Command::ToggleMod { id, enabled }) => self.toggle_mod(id, enabled),
                        Some(Command::DownloadMod(id)) => self.download_mod(id).await,
                        Some(Command::DownloadMissingMods) => self.download_missing_mods().await,
                        Some(Command::SetModsFilter(filter)) => {
                            self.mods_filter = filter;
                            self.emit_mods();
                        }
                        Some(Command::SetAllMods(enabled)) => self.set_all_mods(enabled),
                        Some(Command::SetProfileFilter(filter)) => {
                            self.profile_filter = filter;
                            self.emit_profiles();
                        }
                        Some(Command::OpenModsFolder) => self.open_mods_folder(),
                        Some(Command::ExportDiagnostics) => self.export_diagnostics(),
                        Some(Command::Shutdown) | None => break,
                    }
                }
                _ = tick.tick() => {
                    if !self.offline {
                        self.spawn_check();
                    }
                }
                event = hub_rx.recv() => {
                    if let Some(HubEvent::RefreshProfiles) = event {
                        if !self.offline && self.state.user.is_some() && !self.busy {
                            self.load_data_inner(false).await;
                        }
                    }
                }
                _ = profiles_tick.tick() => {
                    if !self.offline && self.state.user.is_some() && !self.busy {
                        // Fallback poll only; the SignalR hub normally delivers
                        // RefreshProfiles immediately. A transient backend error
                        // must not log the player out and rotate the token while
                        // the game is running (that makes the in-game session
                        // invalid). The token is validated once at startup.
                        self.load_data_inner(false).await;
                    }
                }
            }
        }

        self.stop_hub();
    }

    /// (Re)starts the SignalR listener for the current backend and session.
    fn start_hub(&mut self) {
        self.stop_hub();

        if self.offline {
            return;
        }

        let Some(token) = self.token() else {
            return;
        };
        let name = self
            .state
            .user
            .as_ref()
            .map(|session| session.name.clone())
            .unwrap_or_default();
        let Some(tx) = self.hub_tx.clone() else {
            return;
        };

        let base = self.backend.base().to_string();
        self.hub = Some(tokio::spawn(signalr::run(base, token, name, tx)));
    }

    fn stop_hub(&mut self) {
        if let Some(handle) = self.hub.take() {
            handle.abort();
        }
    }

    async fn startup(&mut self) {
        crate::storage::cleanup_cache_dir(
            &local_app_dir().join("avatars"),
            Duration::from_secs(30 * 24 * 60 * 60),
            200,
        );
        let hwid = hwid::hwid_for(&self.state.settings);
        self.backend.set_hwid(hwid);
        self.spawn_check();

        if let Some(session) = self.state.user.clone() {
            if !session.is_expired() {
                let hwid = hwid::hwid_for(&self.state.settings);
                let check = tokio::time::timeout(
                    Duration::from_secs(15),
                    self.check_session_token(&session.access_token, &hwid),
                )
                .await;

                if let Ok(Ok((outcome, _))) = check {
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
                        self.start_hub();
                        self.load_data().await;
                        self.load_avatar().await;
                        self.emit(Event::Ready);
                        return;
                    }
                }

                self.state.user = None;
                save_state(&self.state);
            }
        }

        if self.state.settings.auto_login {
            if let Some(saved) = self.state.saved_login.clone() {
                self.login(saved.login, saved.password, String::new()).await;
            }
        }

        self.emit(Event::Ready);
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
        info!(login = %login, "sign-in requested");
        let result = self.signin(&login, &password, &code, &hwid).await;

        if let Ok((outcome, _)) = &result {
            info!(
                authenticated = outcome.session.is_some(),
                requires_2fa = outcome.requires_2fa,
                "sign-in finished"
            );
        }

        match result {
            Ok((outcome, _)) if outcome.session.is_some() => {
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
                self.start_hub();
                self.load_data().await;
                self.load_avatar().await;
            }
            Ok((outcome, _)) if outcome.requires_2fa => {
                self.busy = false;
                self.emit(Event::Busy(false));
                self.emit(Event::TwoFa);
                self.emit(Event::Ready);
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
                if outcome.blocked {
                    message.push_str(
                        ". Официальный хост отклонил подпись клиента; проверьте версию лаунчера",
                    );
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

        self.emit(Event::Ready);
    }

    fn logout(&mut self) {
        self.stop_hub();
        self.profile_filter.clear();
        self.mods_filter.clear();
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

        if let Some(rpc) = &self.discord {
            rpc.set_state("В лаунчере");
        }
    }

    async fn select_server(&mut self, index: usize) {
        if index == 3 && !crate::model::PL1_AVAILABLE {
            return;
        }
        let Some((_, host)) = SERVERS.get(index) else {
            return;
        };

        // Auth is bound to a backend, so switching drops the current session.
        // The saved credentials are kept so the launcher can sign in again.
        self.stop_hub();
        self.profile_filter.clear();
        self.mods_filter.clear();
        let saved_login = self.state.saved_login.clone();
        self.state.server_host = (*host).to_string();
        self.state.user = None;
        self.state.last_profile = None;
        save_state(&self.state);

        let secret = client_secret();
        let hwid = hwid::hwid_for(&self.state.settings);
        self.backend = Backend::new(host).with_client(Some(secret), hwid);
        self.offline = false;
        self.busy = false;
        self.profiles.clear();
        self.views.clear();
        self.selected = 0;

        self.emit(Event::Screen(0));
        self.emit(Event::LoggedOut);
        self.emit(Event::Busy(false));
        self.spawn_check();

        self.emit(Event::MainStatus(String::new()));

        if let Some(saved) = saved_login {
            self.login(saved.login, saved.password, String::new()).await;
        } else {
            self.emit(Event::Ready);
        }
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
                Event::Backend {
                    label: "ОНЛАЙН".into(),
                    color: "#2E9E5B".into(),
                    visible: true,
                }
            };

            let _ = tx.send(event);
        });
    }

    /// Downloads the player's Minecraft skin and renders just the head (with
    /// the hat overlay) into a cached PNG for the sidebar avatar.
    async fn load_avatar(&mut self) {
        let Some(user) = self.state.user.clone() else {
            return;
        };
        let Some(url) = self.data_backend().player_skin(&user.name).await else {
            return;
        };
        let Ok(response) = self.data_backend().client().get(&url).send().await else {
            return;
        };
        if !response.status().is_success() {
            return;
        }
        let Ok(bytes) = response.bytes().await else {
            return;
        };
        if bytes.len() > 2 * 1024 * 1024 {
            return;
        }
        let Ok(image) = image::load_from_memory(&bytes) else {
            return;
        };
        let image = image.to_rgba8();
        if image.width() < 16 || image.height() < 16 {
            return;
        }

        let mut head = image::imageops::crop_imm(&image, 8, 8, 8, 8).to_image();
        if image.width() >= 64 && image.height() >= 16 {
            let hat = image::imageops::crop_imm(&image, 40, 8, 8, 8).to_image();
            image::imageops::overlay(&mut head, &hat, 0, 0);
        }
        let head = image::imageops::resize(&head, 96, 96, image::imageops::FilterType::Nearest);

        let path = avatar_path(&user.uuid);
        if let Some(parent) = path.parent() {
            if tokio::fs::create_dir_all(parent).await.is_err() {
                return;
            }
        }
        // Write through a temporary file so a crash mid-save cannot leave a
        // truncated PNG in the cache.
        let temporary = path.with_extension("tmp.png");
        let save_path = temporary.clone();
        let saved = tokio::task::spawn_blocking(move || head.save(save_path))
            .await
            .map(|result| result.is_ok())
            .unwrap_or(false)
            && std::fs::rename(&temporary, &path).is_ok();
        if saved {
            info!(player = %user.name, "avatar rendered");
            self.emit(Event::Avatar(path.to_string_lossy().into_owned()));
        }
    }

    async fn load_data(&mut self) {
        self.load_data_inner(true).await;
    }

    async fn load_data_inner(&mut self, logout_on_auth_error: bool) {
        if self.offline {
            self.load_offline().await;
            return;
        }

        let Some(token) = self.token() else {
            self.emit(Event::Screen(0));
            return;
        };

        let backend = self.backend.clone();
        let mut result = backend.profiles(&token).await;

        // The backend can briefly answer with an empty snapshot while syncing;
        // retry once instead of showing an empty sidebar.
        if matches!(&result, Ok(profiles) if profiles.is_empty()) {
            tokio::time::sleep(Duration::from_millis(600)).await;
            result = backend.profiles(&token).await;
        }

        match result {
            Ok(profiles) => {
                self.apply_profiles(profiles);
                self.ensure_discord().await;
                let news = backend.news(&token).await;
                self.emit(Event::News(map_news(&news)));
                self.notify_news(&news);
            }
            Err(error) => {
                let text = error.to_string();
                let auth_error =
                    text.contains("401") || text.contains("403") || text.contains("Сессия");
                if auth_error && logout_on_auth_error {
                    self.state.user = None;
                    save_state(&self.state);
                    self.emit(Event::Screen(0));
                    self.emit(Event::LoginError(
                        "Сессия истекла, войдите заново".to_string(),
                    ));
                } else if !auth_error {
                    self.emit(Event::Error(format!(
                        "Не удалось загрузить профили: {error}"
                    )));
                }
            }
        }
    }

    fn apply_profiles(&mut self, mut profiles: Vec<Profile>) {
        profiles.sort_by_key(|profile| std::cmp::Reverse(profile.priority));

        let signature = profiles
            .iter()
            .map(|profile| profile.name.as_str())
            .collect::<Vec<_>>()
            .join("|");
        if !self.profiles_signature.is_empty() && signature != self.profiles_signature {
            self.emit(Event::Notification {
                title: "Профили обновлены".into(),
                body: "Список профилей сервера изменился".into(),
            });
        }
        self.profiles_signature = signature;

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

        self.emit_profiles();

        if let Some(view) = self.views.get(self.selected) {
            self.emit(Event::Selected(view.clone()));
        }

        self.update_discord_presence();
    }

    /// Shows a toast only for news that were not seen before. The first load
    /// marks everything as seen, so a fresh install is not spammed.
    fn notify_news(&mut self, news: &[News]) {
        if news.is_empty() {
            return;
        }

        let keys: Vec<String> = news
            .iter()
            .map(|item| {
                format!(
                    "{}|{}",
                    item.title,
                    item.date.as_deref().unwrap_or_default()
                )
            })
            .collect();

        let first_run = self.state.seen_news.is_empty();
        let unseen: Vec<&News> = news
            .iter()
            .zip(keys.iter())
            .filter(|(_, key)| !self.state.seen_news.contains(key))
            .map(|(item, _)| item)
            .collect();

        for key in keys {
            if !self.state.seen_news.contains(&key) {
                self.state.seen_news.push(key);
            }
        }
        if self.state.seen_news.len() > 200 {
            let excess = self.state.seen_news.len() - 200;
            self.state.seen_news.drain(..excess);
        }
        save_state(&self.state);

        if first_run || unseen.is_empty() {
            return;
        }

        let title = if unseen.len() == 1 {
            "Новая новость".to_string()
        } else {
            format!("Новые новости: {}", unseen.len())
        };
        let body = unseen
            .first()
            .map(|item| item.title.clone())
            .unwrap_or_default();

        self.emit(Event::Notification { title, body });
    }

    fn emit_profiles(&self) {
        let needle = self.profile_filter.trim().to_lowercase();
        let views = if needle.is_empty() {
            self.views.clone()
        } else {
            self.views
                .iter()
                .filter(|view| {
                    view.display_name.to_lowercase().contains(&needle)
                        || view.name.to_lowercase().contains(&needle)
                })
                .cloned()
                .collect()
        };

        self.emit(Event::Profiles {
            views,
            selected: self.selected,
        });
    }

    async fn ensure_discord(&mut self) {
        if !self.state.settings.discord_rpc {
            self.discord = None;
            return;
        }
        if self.discord.is_some() || self.offline {
            return;
        }

        self.discord = Some(DiscordRpc::start(crate::model::DiscordConfig::default()));
        self.update_discord_presence();
    }

    fn update_discord_presence(&self) {
        let Some(rpc) = &self.discord else {
            return;
        };

        rpc.set_state("Сидит в лаунчере");
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
        self.launch_cancel.store(false, Ordering::Relaxed);
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
            rpc.set_state(format!("Играет в \"{}\"", profile.name));
        }

        self.emit(Event::Error(String::new()));
        self.emit(Event::MainStatus(String::new()));
        self.emit(Event::Progress {
            visible: true,
            value: 0.0,
            title: "Этап 1/3 · Подготовка".into(),
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
        let progress = progress_callback(self.tx.clone());

        if let Err(error) = sync_profile(
            &self.backend,
            &info,
            &install_dir,
            progress,
            self.launch_cancel.clone(),
        )
        .await
        {
            self.emit(Event::Progress {
                visible: false,
                value: 0.0,
                title: String::new(),
                text: String::new(),
                speed: String::new(),
            });
            if self.launch_cancel.load(Ordering::Relaxed) {
                self.emit(Event::MainStatus("Запуск отменён".into()));
            } else {
                self.emit(Event::Error(format!(
                    "Ошибка синхронизации файлов: {error}"
                )));
            }
            return;
        }

        if self.launch_cancel.load(Ordering::Relaxed) {
            self.emit(Event::Progress {
                visible: false,
                value: 0.0,
                title: String::new(),
                text: String::new(),
                speed: String::new(),
            });
            self.emit(Event::MainStatus("Запуск отменён".into()));
            return;
        }

        self.emit(Event::Progress {
            visible: true,
            value: 1.0,
            title: "Этап 3/3 · Запуск".into(),
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
            Ok(mut child) => {
                let pid = child.id();
                let tx = self.tx.clone();
                std::thread::spawn(move || {
                    let _ = child.wait();
                    let _ = tx.send(Event::GameExited { pid });
                });
                info!(pid, "game spawned");
                self.emit(Event::Progress {
                    visible: false,
                    value: 0.0,
                    title: String::new(),
                    text: String::new(),
                    speed: String::new(),
                });
                self.emit(Event::GameStarted {
                    pid,
                    hide: settings.hide_on_launch,
                });
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

    /// Local files that belong to the optional mod, best match first. Mojang
    /// file names and display names differ (dashes, versions, spaces), so the
    /// comparison is done on alphanumerics only.
    fn mod_paths(&self, profile: &str, name: &str) -> Vec<PathBuf> {
        let directory = self.mods_dir(profile);
        let Ok(entries) = std::fs::read_dir(&directory) else {
            return Vec::new();
        };

        let needle = normalize_name(name);
        if needle.is_empty() {
            return Vec::new();
        }

        let mut matches: Vec<(u8, PathBuf)> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter_map(|path| {
                let file_name = path.file_name()?.to_string_lossy().to_lowercase();
                if !file_name.contains(".jar") {
                    return None;
                }

                let base = file_name
                    .trim_end_matches(".disabled")
                    .trim_end_matches(".jar");
                let normalized = normalize_name(base);
                if normalized.is_empty() {
                    return None;
                }

                let rank = if normalized == needle {
                    0
                } else if normalized.contains(&needle) || needle.contains(&normalized) {
                    1
                } else {
                    return None;
                };

                Some((rank, path))
            })
            .collect();

        matches.sort_by_key(|(rank, _)| *rank);
        matches.into_iter().map(|(_, path)| path).collect()
    }

    fn mod_file_state(&self, profile: &str, name: &str) -> (bool, bool) {
        let paths = self.mod_paths(profile, name);
        let downloaded = !paths.is_empty();
        let enabled = paths
            .iter()
            .any(|path| !path.to_string_lossy().to_lowercase().ends_with(".disabled"));

        (downloaded, enabled)
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

                        let (downloaded, enabled) = self.mod_file_state(&profile.name, &item.name);

                        ModView {
                            id: 0,
                            name: item.name.clone(),
                            description,
                            enabled,
                            downloaded,
                            downloading: false,
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

    /// Renames the mod file to `.jar` or `.jar.disabled`. Returns false when
    /// the file was never downloaded.
    fn set_mod_enabled(
        &self,
        profile: &str,
        view: &ModView,
        enabled: bool,
    ) -> Result<bool, String> {
        let Some(source) = self.mod_paths(profile, &view.name).into_iter().next() else {
            return Ok(false);
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
            std::fs::rename(&source, &target)
                .map_err(|error| format!("Не удалось переключить мод «{}»: {error}", view.name))?;
        }

        Ok(true)
    }

    fn toggle_mod(&mut self, id: usize, enabled: bool) {
        let Some(profile) = self.profiles.get(self.selected).cloned() else {
            return;
        };
        let Some(view) = self.mods.get(id).cloned() else {
            return;
        };

        match self.set_mod_enabled(&profile.name, &view, enabled) {
            Ok(true) => {
                if let Some(item) = self.mods.get_mut(id) {
                    item.enabled = enabled;
                }
                self.emit_mods();
            }
            Ok(false) => self.emit(Event::Error(format!(
                "Файл мода «{}» ещё не скачан. Нажмите «Скачать».",
                view.name
            ))),
            Err(error) => self.emit(Event::Error(error)),
        }
    }

    fn set_all_mods(&mut self, enabled: bool) {
        let Some(profile) = self.profiles.get(self.selected).cloned() else {
            return;
        };
        let views = self.mods.clone();
        let mut changed = 0usize;
        let mut failed = 0usize;

        for view in views.iter().filter(|view| view.downloaded) {
            if view.enabled == enabled {
                continue;
            }
            match self.set_mod_enabled(&profile.name, view, enabled) {
                Ok(true) => changed += 1,
                Ok(false) => {}
                Err(error) => {
                    failed += 1;
                    error!(%error, mod_name = %view.name, "failed to toggle mod");
                }
            }
        }

        for item in &mut self.mods {
            if item.downloaded {
                item.enabled = enabled;
            }
        }
        self.emit_mods();
        self.emit(Event::SettingsStatus(if failed == 0 {
            format!("Моды переключены: {changed}")
        } else {
            format!("Переключено: {changed}, ошибок: {failed}")
        }));
    }

    async fn download_mod(&mut self, id: usize) {
        if self.offline {
            self.emit(Event::Error(
                "Скачивание модов доступно только при подключённом бэкенде".into(),
            ));
            return;
        }

        let Some(profile) = self.profiles.get(self.selected).cloned() else {
            self.emit(Event::Error("Профиль не выбран".into()));
            return;
        };
        let Some(view) = self.mods.get(id).cloned() else {
            return;
        };

        if view.downloading {
            return;
        }
        if view.downloaded {
            self.emit(Event::MainStatus(format!("Мод «{}» уже скачан", view.name)));
            return;
        }

        if let Some(item) = self.mods.get_mut(id) {
            item.downloading = true;
        }
        self.emit_mods();
        self.emit(Event::MainStatus(format!(
            "Скачивание мода «{}»…",
            view.name
        )));

        let result = async {
            let info = self.fetch_profile_info(&profile).await?;
            let file = find_optional_file(&info, &view.name)
                .ok_or_else(|| anyhow::anyhow!("Сервер не отдал файл для мода «{}»", view.name))?;
            let hash = file
                .hash
                .as_deref()
                .filter(|hash| !hash.is_empty())
                .ok_or_else(|| anyhow::anyhow!("В манифесте нет хэша для мода"))?;

            let root = self.state.settings.install_path();
            let target = crate::storage::resolve_below(&root, &file.directory)
                .ok_or_else(|| anyhow::anyhow!("Некорректный путь мода: {}", file.directory))?;

            if target.exists() {
                return Ok::<(), anyhow::Error>(());
            }

            // New optional mods stay disabled, exactly like the first launch
            // sync does; the user enables them with the toggle afterwards.
            let target = PathBuf::from(format!("{}.disabled", target.display()));
            if target.exists() {
                return Ok(());
            }

            crate::files::download_optional(
                &self.backend,
                hash,
                &target,
                file.size.max(0) as u64,
                &file.name,
                progress_callback(self.tx.clone()),
            )
            .await
        }
        .await;

        if let Some(item) = self.mods.get_mut(id) {
            item.downloading = false;
        }

        match result {
            Ok(()) => {
                let (downloaded, enabled) = self.mod_file_state(&profile.name, &view.name);
                if let Some(item) = self.mods.get_mut(id) {
                    item.downloaded = downloaded;
                    item.enabled = enabled;
                }
                self.emit_mods();
                self.emit(Event::Progress {
                    visible: false,
                    value: 0.0,
                    title: String::new(),
                    text: String::new(),
                    speed: String::new(),
                });
                self.emit(Event::MainStatus(format!(
                    "Мод «{}» скачан (выключен)",
                    view.name
                )));
            }
            Err(error) => {
                self.emit_mods();
                self.emit(Event::Progress {
                    visible: false,
                    value: 0.0,
                    title: String::new(),
                    text: String::new(),
                    speed: String::new(),
                });
                self.emit(Event::Error(format!(
                    "Не удалось скачать мод «{}»: {error}",
                    view.name
                )));
            }
        }
    }

    /// Profile manifest used by both launch and mod downloads.
    async fn fetch_profile_info(&self, profile: &Profile) -> Result<crate::model::ProfileInfo> {
        if self.offline {
            return load_offline_info(&profile.name);
        }

        let token = self
            .token()
            .ok_or_else(|| anyhow::anyhow!("Сессия истекла"))?;
        let session = self
            .state
            .user
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Сессия истекла"))?;
        let settings = &self.state.settings;
        let (window_width, window_height) = if settings.fullscreen {
            hwid::screen_size()
        } else {
            (settings.game_width, settings.game_height)
        };

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

        match self.backend.profile_info(&token, &request).await? {
            Some(info) => Ok(info),
            None => Err(anyhow::anyhow!("Сервер не вернул параметры профиля")),
        }
    }

    fn emit_mods(&self) {
        let enabled = self.mods.iter().filter(|item| item.enabled).count();
        let missing = self.mods.iter().filter(|item| !item.downloaded).count();
        let needle = self.mods_filter.trim().to_lowercase();
        let views = if needle.is_empty() {
            self.mods.clone()
        } else {
            self.mods
                .iter()
                .filter(|item| {
                    item.name.to_lowercase().contains(&needle)
                        || item.description.to_lowercase().contains(&needle)
                })
                .cloned()
                .collect()
        };

        self.emit(Event::Mods {
            views,
            enabled,
            total: self.mods.len(),
            missing,
        });
    }

    async fn download_missing_mods(&mut self) {
        if self.offline {
            self.emit(Event::Error(
                "Скачивание модов доступно только при подключённом бэкенде".into(),
            ));
            return;
        }

        let ids: Vec<usize> = self
            .mods
            .iter()
            .filter(|item| !item.downloaded)
            .map(|item| item.id)
            .collect();

        if ids.is_empty() {
            self.emit(Event::MainStatus("Все моды уже скачаны".into()));
            return;
        }

        self.emit(Event::MainStatus(format!(
            "Скачивание отсутствующих модов: {}…",
            ids.len()
        )));
        for id in ids {
            self.download_mod(id).await;
        }
        self.emit(Event::MainStatus("Скачивание модов завершено".into()));
    }

    fn open_mods_folder(&mut self) {
        let Some(profile) = self.profiles.get(self.selected) else {
            self.emit(Event::Error("Профиль не выбран".into()));
            return;
        };
        let dir = self.mods_dir(&profile.name);
        let _ = std::fs::create_dir_all(&dir);
        info!(path = %dir.display(), "opening mods folder");
        if let Err(error) = std::process::Command::new("explorer").arg(&dir).spawn() {
            self.emit(Event::Error(format!(
                "Не удалось открыть папку модов: {error}"
            )));
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
        settings.custom_java = settings.custom_java.trim().to_string();

        if settings.install_dir.is_empty() {
            settings.install_dir = app_dir().to_string_lossy().into_owned();
        }

        let themes = theme::load_all();
        settings.theme = theme::resolve(&settings.theme, &themes).name.clone();
        settings.light_theme = theme::resolve(&settings.light_theme, &themes).name.clone();
        settings.dark_theme = theme::resolve(&settings.dark_theme, &themes).name.clone();
        settings.accent = if theme::parse_hex_rgb(&settings.accent).is_some() {
            settings.accent.trim().to_uppercase()
        } else {
            String::new()
        };
        settings.theme_mode = theme::normalize_mode(&settings.theme_mode).to_string();
        settings.language = crate::locale::normalize(&settings.language).to_string();
        settings.glass_opacity = settings.glass_opacity.clamp(0.25, 0.9);
        settings.glass_blur = settings.glass_blur.clamp(0.0, 1.0);
        settings.layout.normalize();

        self.state.settings = settings;
        if !self.state.settings.discord_rpc {
            self.discord = None;
        }
        self.ensure_layout_profiles();
        save_state(&self.state);
        let settings = self.state.settings.clone();

        let secret = client_secret();
        let hwid = self.backend.client_hwid().to_string();
        self.backend.set_client(Some(secret), hwid);

        self.emit(Event::Settings {
            settings: Box::new(settings),
            max_ram,
        });
        self.emit(Event::SettingsStatus("Сохранено".into()));
    }

    fn refresh_themes(&mut self) {
        let items = theme::load_all();
        let active = theme::resolve(&self.state.settings.theme, &items)
            .name
            .clone();
        self.state.settings.theme = active.clone();
        save_state(&self.state);
        info!(theme = %active, themes = items.len(), "themes refreshed");
        self.emit(Event::Themes { items, active });
    }

    fn save_theme(&mut self, name: String) {
        let items = theme::load_all();
        let item = theme::resolve(&name, &items).clone();
        self.state.settings.theme = item.name.clone();
        self.state.settings.theme_mode = item.theme.mode.clone();
        self.state.settings.background_effect = item.theme.background_effect.clone();
        save_state(&self.state);
        info!(theme = %item.name, "theme selected");
        self.emit(Event::Themes {
            items,
            active: item.name,
        });
    }

    fn open_install_folder(&mut self) {
        let dir = self.state.settings.install_path();
        let _ = std::fs::create_dir_all(&dir);
        info!(path = %dir.display(), "opening install folder");
        if let Err(error) = std::process::Command::new("explorer").arg(&dir).spawn() {
            self.emit(Event::Error(format!(
                "Не удалось открыть папку установки: {error}"
            )));
        }
    }

    fn open_logs_folder(&mut self) {
        let dir = local_app_dir().join("logs");
        let _ = std::fs::create_dir_all(&dir);
        info!(path = %dir.display(), "opening logs folder");
        if let Err(error) = std::process::Command::new("explorer").arg(&dir).spawn() {
            self.emit(Event::Error(format!(
                "Не удалось открыть папку логов: {error}"
            )));
        }
    }

    /// Validates (and repairs) the selected profile without launching the game.
    /// Runs through the normal file pipeline, so progress, retries and the
    /// validation cache behave exactly like a launch.
    async fn check_files(&mut self) {
        if self.busy {
            return;
        }
        if self.offline {
            self.emit(Event::SettingsStatus(
                "Проверка файлов недоступна в оффлайн-режиме".into(),
            ));
            return;
        }
        let Some(profile) = self.profiles.get(self.selected).cloned() else {
            self.emit(Event::Error("Профиль не выбран".into()));
            return;
        };

        self.busy = true;
        self.emit(Event::SettingsStatus("Проверка файлов…".into()));
        let result = async {
            let info = self.fetch_profile_info(&profile).await?;
            let root = self.state.settings.install_path();
            crate::files::sync_profile(
                &self.backend,
                &info,
                &root,
                progress_callback(self.tx.clone()),
                Arc::new(AtomicBool::new(false)),
            )
            .await
        }
        .await;
        self.busy = false;

        self.emit(Event::Progress {
            visible: false,
            value: 0.0,
            title: String::new(),
            text: String::new(),
            speed: String::new(),
        });

        match result {
            Ok(()) => self.emit(Event::SettingsStatus(
                "Файлы профиля проверены и актуальны".into(),
            )),
            Err(error) => self.emit(Event::Error(format!("Не удалось проверить файлы: {error}"))),
        }
    }

    fn clear_validation_cache(&mut self) {
        let path = crate::storage::cache_path();
        match std::fs::remove_file(&path) {
            Ok(()) => self.emit(Event::SettingsStatus("Кэш проверки файлов очищен".into())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.emit(Event::SettingsStatus("Кэш проверки уже пуст".into()))
            }
            Err(error) => self.emit(Event::Error(format!(
                "Не удалось очистить кэш проверки: {error}"
            ))),
        }
    }

    fn open_themes_folder(&mut self) {
        match theme::ensure_themes_dir() {
            Ok(dir) => {
                info!(path = %dir.display(), "opening themes folder");
                if let Err(error) = std::process::Command::new("explorer").arg(&dir).spawn() {
                    error!(%error, "failed to open themes folder");
                    self.emit(Event::Error(format!(
                        "Не удалось открыть папку тем: {error}"
                    )));
                }
            }
            Err(error) => {
                error!(%error, "failed to create themes folder");
                self.emit(Event::Error(format!(
                    "Не удалось создать папку тем: {error}"
                )));
            }
        }
    }

    fn move_layout_item(&mut self, item: &str, direction: &str) {
        let mut layout = self.state.settings.layout.clone();
        layout.move_item(item, direction);
        layout.normalize();
        self.state.settings.layout = layout;
        save_state(&self.state);
        self.emit(Event::Layout(self.state.settings.layout.clone()));
        self.emit(Event::SettingsStatus("Layout обновлён".into()));
    }

    fn save_layout(&mut self, mut layout: LayoutConfig) {
        layout.normalize();
        self.state.settings.layout = layout;
        save_state(&self.state);
        self.emit(Event::Layout(self.state.settings.layout.clone()));
        self.emit(Event::SettingsStatus("Layout сохранён".into()));
    }

    fn drop_layout_item(&mut self, item: &str, zone: &str, before: &str) {
        let mut layout = self.state.settings.layout.clone();
        let before = if before.is_empty() {
            None
        } else {
            Some(before)
        };
        layout.move_to_zone(item, zone, before);
        layout.normalize();
        self.state.settings.layout = layout;
        save_state(&self.state);
        self.emit(Event::Layout(self.state.settings.layout.clone()));
        self.emit(Event::SettingsStatus("Layout обновлён".into()));
    }

    fn toggle_layout_item(&mut self, item: &str, hidden: bool) {
        let mut layout = self.state.settings.layout.clone();
        layout.set_hidden(item, hidden);
        layout.normalize();
        self.state.settings.layout = layout;
        save_state(&self.state);
        self.emit(Event::Layout(self.state.settings.layout.clone()));
    }

    fn set_layout_height(&mut self, item: &str, height: u32) {
        let mut layout = self.state.settings.layout.clone();
        layout.set_block_height(item, height);
        layout.normalize();
        self.state.settings.layout = layout;
        save_state(&self.state);
        self.emit(Event::Layout(self.state.settings.layout.clone()));
    }

    fn set_layout_widths(&mut self, left: u32, right: u32) {
        let mut layout = self.state.settings.layout.clone();
        layout.set_widths(left, right);
        self.state.settings.layout = layout;
        save_state(&self.state);
        self.emit(Event::Layout(self.state.settings.layout.clone()));
    }

    fn ensure_layout_profiles(&mut self) {
        if self.state.settings.layouts.is_empty() {
            self.state.settings.layouts.push(LayoutProfile {
                name: "По умолчанию".into(),
                layout: LayoutConfig::default(),
            });
        }
        let active_exists = self
            .state
            .settings
            .layouts
            .iter()
            .any(|profile| profile.name == self.state.settings.layout_profile);
        if !active_exists {
            self.state.settings.layout_profile = self.state.settings.layouts[0].name.clone();
        }
    }

    fn emit_layout_profiles(&self) {
        self.emit(Event::LayoutProfiles {
            profiles: self.state.settings.layouts.clone(),
            active: self.state.settings.layout_profile.clone(),
        });
    }

    fn save_layout_profile(&mut self, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            self.emit(Event::SettingsStatus(
                "Введите название профиля layout".into(),
            ));
            return;
        }

        self.ensure_layout_profiles();
        let mut layout = self.state.settings.layout.clone();
        layout.normalize();

        if let Some(profile) = self
            .state
            .settings
            .layouts
            .iter_mut()
            .find(|profile| profile.name == name)
        {
            profile.layout = layout;
        } else {
            self.state.settings.layouts.push(LayoutProfile {
                name: name.to_string(),
                layout,
            });
        }

        self.state.settings.layout_profile = name.to_string();
        save_state(&self.state);
        self.emit(Event::Layout(self.state.settings.layout.clone()));
        self.emit_layout_profiles();
        self.emit(Event::SettingsStatus(format!(
            "Профиль layout «{name}» сохранён"
        )));
    }

    fn switch_layout_profile(&mut self, name: &str) {
        self.ensure_layout_profiles();
        let Some(profile) = self
            .state
            .settings
            .layouts
            .iter()
            .find(|profile| profile.name == name)
            .cloned()
        else {
            self.emit(Event::SettingsStatus("Профиль layout не найден".into()));
            return;
        };

        let mut layout = profile.layout;
        layout.normalize();
        self.state.settings.layout = layout;
        self.state.settings.layout_profile = profile.name.clone();
        save_state(&self.state);
        self.emit(Event::Layout(self.state.settings.layout.clone()));
        self.emit_layout_profiles();
        self.emit(Event::SettingsStatus(format!(
            "Загружен профиль «{}»",
            profile.name
        )));
    }

    fn delete_layout_profile(&mut self, name: &str) {
        self.ensure_layout_profiles();
        if self.state.settings.layouts.len() <= 1 {
            self.emit(Event::SettingsStatus(
                "Нельзя удалить последний профиль layout".into(),
            ));
            return;
        }

        self.state
            .settings
            .layouts
            .retain(|profile| profile.name != name);

        if self.state.settings.layout_profile == name {
            let first = self.state.settings.layouts[0].clone();
            self.state.settings.layout = first.layout;
            self.state.settings.layout_profile = first.name;
        }

        save_state(&self.state);
        self.emit(Event::Layout(self.state.settings.layout.clone()));
        self.emit_layout_profiles();
    }

    fn rename_layout_profile(&mut self, old: &str, new: &str) {
        let new = new.trim();
        if new.is_empty() {
            self.emit(Event::SettingsStatus(
                "Введите новое название профиля".into(),
            ));
            return;
        }
        self.ensure_layout_profiles();
        if self
            .state
            .settings
            .layouts
            .iter()
            .any(|profile| profile.name == new && profile.name != old)
        {
            self.emit(Event::SettingsStatus(
                "Профиль с таким названием уже есть".into(),
            ));
            return;
        }

        let mut renamed = false;
        for profile in &mut self.state.settings.layouts {
            if profile.name == old {
                profile.name = new.to_string();
                renamed = true;
            }
        }
        if !renamed {
            self.emit(Event::SettingsStatus("Профиль layout не найден".into()));
            return;
        }
        if self.state.settings.layout_profile == old {
            self.state.settings.layout_profile = new.to_string();
        }

        save_state(&self.state);
        self.emit_layout_profiles();
        self.emit(Event::SettingsStatus(format!(
            "Профиль переименован в «{new}»"
        )));
    }

    fn export_diagnostics(&mut self) {
        match crate::diagnostics::export(&self.state.settings) {
            Ok(path) => self.emit(Event::SettingsStatus(format!(
                "Диагностика сохранена: {}",
                path.display()
            ))),
            Err(error) => self.emit(Event::Error(format!(
                "Не удалось сохранить диагностику: {error}"
            ))),
        }
    }

    async fn enter_offline(&mut self) {
        self.stop_hub();
        self.offline = true;
        self.load_offline().await;
        self.spawn_check();
    }
}

/// Client signature key, embedded in the build as XOR-obfuscated bytes so the
/// launcher works out of the box and the key is not a plain string in the exe.
/// The previous Settings override was removed: a mistyped key silently broke
/// sessions on the official hosts.
fn client_secret() -> String {
    const OBFUSCATED: &[u8] = &[
        241, 194, 212, 211, 198, 203, 206, 212, 248, 224, 210, 198, 213, 195, 248, 209, 149, 151,
        149, 145, 248, 244, 194, 196, 210, 213, 194, 239, 234, 230, 228, 248, 230, 201, 211, 206,
        229, 222, 215, 198, 212, 212, 248, 158, 158, 147, 150,
    ];

    OBFUSCATED
        .iter()
        .map(|byte| (byte ^ 0xA7) as char)
        .collect()
}

fn normalize_name(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(|character| character.to_lowercase())
        .collect()
}

/// Picks the manifest entry that belongs to an optional mod shown in the UI.
fn find_optional_file<'a>(
    info: &'a crate::model::ProfileInfo,
    name: &str,
) -> Option<&'a crate::model::ProfileFile> {
    let needle = normalize_name(name);
    if needle.is_empty() {
        return None;
    }

    let mut best: Option<(u8, &crate::model::ProfileFile)> = None;

    for file in info
        .files
        .iter()
        .filter(|file| file.directory.contains("-optional-mod.jar"))
    {
        let base = file
            .name
            .trim_end_matches(".disabled")
            .trim_end_matches(".jar");
        let normalized = normalize_name(base);
        if normalized.is_empty() {
            continue;
        }

        let rank = if normalized == needle {
            0
        } else if normalized.contains(&needle) || needle.contains(&normalized) {
            1
        } else {
            continue;
        };

        if best.map(|(current, _)| rank < current).unwrap_or(true) {
            best = Some((rank, file));
        }
    }

    best.map(|(_, file)| file)
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

fn progress_callback(tx: UnboundedSender<Event>) -> ProgressFn {
    Arc::new(move |event: SyncProgress| {
        // A single-file download reports bytes; the regular sync reports files.
        let value = if event.total_bytes > 0 {
            (event.bytes as f32 / event.total_bytes as f32).clamp(0.0, 1.0)
        } else if event.total == 0 {
            0.0
        } else {
            (event.done as f32 / event.total as f32).clamp(0.0, 1.0)
        };

        let mut text = if event.total_bytes > 0 {
            format!(
                "{} / {} · {}",
                format_bytes(event.bytes),
                format_bytes(event.total_bytes),
                event.current
            )
        } else {
            format!("{} / {} · {}", event.done, event.total, event.current)
        };
        if !event.eta.is_empty() {
            text.push_str(&format!(" · осталось {}", event.eta));
        }

        let _ = tx.send(Event::Progress {
            visible: true,
            value,
            title: event.phase,
            text,
            speed: if event.speed_bps > 0 {
                format!("{}/s", format_bytes(event.speed_bps))
            } else {
                String::new()
            },
        });
    })
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
