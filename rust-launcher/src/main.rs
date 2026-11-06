//! Vestalis Launcher — Rust rewrite.
//!
//! The UI lives in `ui/app.slint`; this file is the composition root. It owns
//! the Slint event loop, starts the Tokio worker and translates worker events
//! into UI properties. All blocking work happens off the UI thread.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api;
mod discord;
mod files;
mod hwid;
mod launch;
mod layout;
mod model;
mod storage;
mod theme;
mod window;
mod worker;

slint::include_modules!();

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use base64::Engine;
use slint::{Model, ModelRc, VecModel};
use tracing::{error, info, warn};

use crate::model::{Persisted, Settings};
use crate::storage::load_state;
use crate::theme::ThemeItem;
use crate::worker::{Command, Event, ProfileView, Worker};

thread_local! {
    static THEME_CATALOG: RefCell<Vec<ThemeItem>> = const { RefCell::new(Vec::new()) };
    static SYSTEM_ACCENT: std::cell::Cell<Option<crate::theme::Rgb>> = const { std::cell::Cell::new(None) };
}

fn main() -> anyhow::Result<()> {
    window::set_app_user_model_id();

    let log_dir = model::local_app_dir().join("logs");
    let _ = std::fs::create_dir_all(&log_dir);
    let file_appender = tracing_appender::rolling::daily(&log_dir, "launcher.log");
    let (non_blocking, _log_guard) = tracing_appender::non_blocking(file_appender);
    tracing_subscriber::fmt()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    info!(
        version = env!("CARGO_PKG_VERSION"),
        "starting Vestalis launcher"
    );

    let mut state = load_state();
    // PL-1 is disabled: migrate a saved selection to RU-2 and let the normal
    // auto-login issue a fresh session on the working backend.
    if !model::PL1_AVAILABLE && model::server_index(&state.server_host) == 3 {
        state.server_host = model::SERVERS[1].1.to_string();
        state.user = None;
        state.auth_via_mirror = false;
        storage::save_state(&state);
    }

    let themes = crate::theme::load_all();
    let active = crate::theme::resolve(&state.settings.theme, &themes).clone();
    if state.settings.theme != active.name {
        state.settings.theme = active.name.clone();
        storage::save_state(&state);
    }
    state.settings.theme_mode =
        crate::theme::normalize_mode(&state.settings.theme_mode).to_string();
    let accent = crate::theme::system_accent();
    SYSTEM_ACCENT.with(|value| value.set(accent));
    info!(
        theme = %active.name,
        mode = %state.settings.theme_mode,
        accent = ?accent,
        "theme applied"
    );
    THEME_CATALOG.with(|catalog| *catalog.borrow_mut() = themes);

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let (command_tx, command_rx) = tokio::sync::mpsc::unbounded_channel();
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();

    let ui = AppWindow::new()?;
    ui.set_set_theme_mode(state.settings.theme_mode.clone().into());
    // Apply before the first frame so a non-dark theme never flashes.
    apply_item(&ui, &active, &state.settings.theme_mode);
    initialize_ui(&ui, &state);
    install_callbacks(&ui, command_tx.clone());

    runtime.spawn(Worker::new(state, event_tx).run(command_rx));

    // Animate the splash progress bar to the end, then keep it on screen for a
    // short beat instead of cutting away mid-animation.
    let splash_ui = ui.as_weak();
    let splash_started = std::time::Instant::now();
    let splash_duration = 1.8_f32;
    let splash_timer = slint::Timer::default();
    splash_timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(30),
        move || {
            let elapsed = splash_started.elapsed().as_secs_f32();
            let progress = (elapsed / splash_duration).clamp(0.0, 1.0);

            if let Some(ui) = splash_ui.upgrade() {
                ui.set_splash_progress(progress);
                // Auto-login can take a few seconds on a slow backend; keep the
                // branded screen until it finishes so the login form never
                // flashes, with a hard fallback for a hung request.
                if elapsed >= splash_duration + 0.25 && (ui.get_startup_ready() || elapsed >= 20.0)
                {
                    ui.set_splash(false);
                }
            }
        },
    );

    slint::Timer::single_shot(Duration::from_millis(350), || {
        if let Some(handle) = window::find_launcher_window() {
            window::apply_icon(handle);
            window::apply_rounded_corners(handle);
        }
    });

    ui.set_countdown_text(countdown_text().into());
    let countdown_ui = ui.as_weak();
    let countdown_timer = slint::Timer::default();
    countdown_timer.start(
        slint::TimerMode::Repeated,
        Duration::from_secs(30),
        move || {
            if let Some(ui) = countdown_ui.upgrade() {
                ui.set_countdown_text(countdown_text().into());
            }
        },
    );

    // Follow the Windows accent while Material 3 is active.
    let accent_ui = ui.as_weak();
    let accent_timer = slint::Timer::default();
    accent_timer.start(
        slint::TimerMode::Repeated,
        Duration::from_secs(20),
        move || {
            let Some(ui) = accent_ui.upgrade() else {
                return;
            };
            let accent = crate::theme::system_accent();
            let changed = SYSTEM_ACCENT.with(|value| {
                if value.get() != accent {
                    value.set(accent);
                    true
                } else {
                    false
                }
            });
            if changed && ui.get_set_theme_mode() == "material3" {
                apply_theme_by_name(&ui, &ui.get_set_theme());
            }
        },
    );

    let timer = slint::Timer::default();
    let ui_weak = ui.as_weak();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(40),
        move || {
            while let Ok(event) = event_rx.try_recv() {
                if let Some(ui) = ui_weak.upgrade() {
                    apply_event(&ui, event);
                }
            }
        },
    );

    ui.run()?;
    let _ = command_tx.send(Command::Shutdown);
    info!("launcher closed");
    Ok(())
}

fn initialize_ui(ui: &AppWindow, state: &Persisted) {
    ui.set_app_version(env!("CARGO_PKG_VERSION").into());
    ui.set_screen(0);
    ui.set_progress_visible(false);
    ui.set_show_2fa(false);
    let server_index = model::server_index(&state.server_host);
    ui.set_server_index(server_index as i32);
    ui.set_server_label(model::SERVERS[server_index].0.into());
    ui.set_server_host_label(state.server_host.clone().into());
    ui.set_pl1_available(model::PL1_AVAILABLE);
    if server_index == 3 {
        ui.set_main_status(
            "PL-1 — зеркало для просмотра. Для входа в игру выберите RU-1, RU-2 или EU-1.".into(),
        );
    }
    ui.set_is_kydasay(
        state
            .user
            .as_ref()
            .map(|session| session.name.eq_ignore_ascii_case("Kydasay"))
            .unwrap_or(false),
    );

    ui.set_themes(ModelRc::new(VecModel::from(theme_options())));
    ui.set_set_theme(state.settings.theme.clone().into());

    if let Some(saved) = &state.saved_login {
        ui.set_login(saved.login.clone().into());
    }

    // Show the cached head immediately on startup instead of the initial.
    if let Some(user) = &state.user {
        let path = model::avatar_path(&user.uuid);
        if path.exists() {
            if let Ok(image) = slint::Image::load_from_path(&path) {
                ui.set_user_avatar(image);
                ui.set_has_avatar(true);
            }
        }
    }

    apply_settings(ui, &state.settings, hwid::total_ram_mb());
}

#[link(name = "winmm")]
extern "system" {
    fn PlaySoundW(sound: *const u8, module: isize, flags: u32) -> i32;
}

enum Sound {
    Click,
    Toggle,
}

fn play_sound(ui: &AppWindow, sound: Sound) {
    if !ui.get_set_sounds() {
        return;
    }

    const SND_ASYNC: u32 = 0x0001;
    const SND_NODEFAULT: u32 = 0x0002;
    const SND_MEMORY: u32 = 0x0004;

    let data: &[u8] = match sound {
        Sound::Click => include_bytes!("../assets/click.wav"),
        Sound::Toggle => include_bytes!("../assets/toggle.wav"),
    };

    // The WAV bytes are embedded in the binary and live for the whole process.
    unsafe {
        PlaySoundW(data.as_ptr(), 0, SND_MEMORY | SND_ASYNC | SND_NODEFAULT);
    }
}

fn install_callbacks(ui: &AppWindow, tx: tokio::sync::mpsc::UnboundedSender<Command>) {
    ui.on_login_clicked({
        let tx = tx.clone();
        let ui = ui.as_weak();
        move || {
            let Some(ui) = ui.upgrade() else { return };
            play_sound(&ui, Sound::Click);
            let login = ui.get_login().to_string();
            let password = ui.get_password().to_string();
            let _ = tx.send(Command::Login { login, password });
        }
    });

    ui.on_verify_clicked({
        let tx = tx.clone();
        let ui = ui.as_weak();
        move || {
            let Some(ui) = ui.upgrade() else { return };
            play_sound(&ui, Sound::Click);
            let _ = tx.send(Command::Verify2Fa {
                code: ui.get_twofa().to_string(),
            });
        }
    });

    ui.on_offline_clicked({
        let tx = tx.clone();
        let ui = ui.as_weak();
        move || {
            if let Some(ui) = ui.upgrade() {
                play_sound(&ui, Sound::Click);
            }
            let _ = tx.send(Command::Offline);
        }
    });

    ui.on_hwid_toggled({
        let tx = tx.clone();
        let ui = ui.as_weak();
        move |enabled| {
            if let Some(ui) = ui.upgrade() {
                play_sound(&ui, Sound::Toggle);
            }
            let _ = tx.send(Command::SetHwid(enabled));
        }
    });

    ui.on_logout_clicked({
        let tx = tx.clone();
        let ui = ui.as_weak();
        move || {
            if let Some(ui) = ui.upgrade() {
                play_sound(&ui, Sound::Click);
            }
            let _ = tx.send(Command::Logout);
        }
    });

    ui.on_settings_clicked({
        let ui = ui.as_weak();
        let tx = tx.clone();
        move || {
            if let Some(ui) = ui.upgrade() {
                play_sound(&ui, Sound::Click);
                switch_screen(&ui, 2);
            }
            let _ = tx.send(Command::RefreshMirror);
        }
    });

    ui.on_back_clicked({
        let ui = ui.as_weak();
        move || {
            if let Some(ui) = ui.upgrade() {
                play_sound(&ui, Sound::Click);
                switch_screen(&ui, 1);
            }
        }
    });

    ui.on_play_clicked({
        let tx = tx.clone();
        let ui = ui.as_weak();
        move || {
            if let Some(ui) = ui.upgrade() {
                play_sound(&ui, Sound::Click);
            }
            let _ = tx.send(Command::Play);
        }
    });

    ui.on_refresh_clicked({
        let tx = tx.clone();
        let ui = ui.as_weak();
        move || {
            if let Some(ui) = ui.upgrade() {
                play_sound(&ui, Sound::Click);
            }
            let _ = tx.send(Command::Refresh);
        }
    });

    ui.on_mods_clicked({
        let tx = tx.clone();
        let ui = ui.as_weak();
        move || {
            if let Some(ui) = ui.upgrade() {
                play_sound(&ui, Sound::Click);
            }
            let _ = tx.send(Command::OpenMods);
        }
    });

    ui.on_mods_refresh_clicked({
        let tx = tx.clone();
        let ui = ui.as_weak();
        move || {
            if let Some(ui) = ui.upgrade() {
                play_sound(&ui, Sound::Click);
            }
            let _ = tx.send(Command::OpenMods);
        }
    });

    ui.on_mod_toggled({
        let tx = tx.clone();
        let ui = ui.as_weak();
        move |id, enabled| {
            if let Some(ui) = ui.upgrade() {
                play_sound(&ui, Sound::Toggle);
            }
            let _ = tx.send(Command::ToggleMod {
                id: id as usize,
                enabled,
            });
        }
    });

    ui.on_server_clicked({
        let tx = tx.clone();
        let ui = ui.as_weak();
        move |index| {
            if let Some(ui) = ui.upgrade() {
                play_sound(&ui, Sound::Click);
                ui.set_server_index(index);
                if let Some((label, host)) = model::SERVERS.get(index as usize) {
                    ui.set_server_label((*label).into());
                    ui.set_server_host_label((*host).into());
                }
            }
            let _ = tx.send(Command::SelectServer(index as usize));
        }
    });

    ui.on_profile_clicked({
        let tx = tx.clone();
        let ui = ui.as_weak();
        move |index| {
            if let Some(ui) = ui.upgrade() {
                play_sound(&ui, Sound::Click);
            }
            let _ = tx.send(Command::SelectProfile(index as usize));
        }
    });

    ui.on_adjust_xms({
        let ui = ui.as_weak();
        move |delta| {
            if let Some(ui) = ui.upgrade() {
                play_sound(&ui, Sound::Click);
                let xmx = ui.get_set_xmx();
                let value = (ui.get_set_xms() + delta).clamp(512, xmx);
                ui.set_set_xms(value);
            }
        }
    });

    ui.on_adjust_xmx({
        let ui = ui.as_weak();
        move |delta| {
            if let Some(ui) = ui.upgrade() {
                play_sound(&ui, Sound::Click);
                let max = ui.get_max_ram().max(1024);
                let value = (ui.get_set_xmx() + delta).clamp(1024, max);
                let value = value.max(ui.get_set_xms());
                ui.set_set_xmx(value);
            }
        }
    });

    ui.on_save_settings_clicked({
        let tx = tx.clone();
        let ui = ui.as_weak();
        move || {
            let Some(ui) = ui.upgrade() else { return };
            play_sound(&ui, Sound::Click);

            let width = ui
                .get_set_width_text()
                .parse::<u32>()
                .unwrap_or(900)
                .clamp(640, 7680);
            let height = ui
                .get_set_height_text()
                .parse::<u32>()
                .unwrap_or(600)
                .clamp(480, 4320);

            let settings = Settings {
                game_width: width,
                game_height: height,
                fullscreen: ui.get_set_fullscreen(),
                xms: ui.get_set_xms() as u32,
                xmx: ui.get_set_xmx() as u32,
                custom_jvm: ui.get_set_custom_jvm().to_string(),
                send_hwid: ui.get_set_send_hwid(),
                sounds: ui.get_set_sounds(),
                hide_on_launch: ui.get_set_hide_on_launch(),
                background_effect: ui.get_set_background_effect().to_string(),
                theme: ui.get_set_theme().to_string(),
                theme_mode: ui.get_set_theme_mode().to_string(),
                layout: ui_layout(&ui),
                install_dir: ui.get_set_install_dir().to_string(),
            };

            ui.set_settings_status("Сохранение…".into());
            let _ = tx.send(Command::SaveSettings(Box::new(settings)));
        }
    });

    ui.on_theme_clicked({
        let ui = ui.as_weak();
        move |name| {
            let Some(ui) = ui.upgrade() else { return };
            play_sound(&ui, Sound::Click);
            let mode = THEME_CATALOG.with(|catalog| {
                catalog
                    .borrow()
                    .iter()
                    .find(|item| item.name.eq_ignore_ascii_case(name.as_str()))
                    .map(|item| item.theme.mode.clone())
            });
            if let Some(mode) = mode {
                ui.set_set_theme_mode(mode.into());
            }
            apply_theme_by_name(&ui, name.as_str());
        }
    });

    ui.on_theme_mode_clicked({
        let ui = ui.as_weak();
        move |mode| {
            let Some(ui) = ui.upgrade() else { return };
            play_sound(&ui, Sound::Click);
            let mode = crate::theme::normalize_mode(mode.as_str());
            ui.set_set_theme_mode(mode.into());
            apply_theme_by_name(&ui, &ui.get_set_theme());
        }
    });

    ui.on_refresh_accent_clicked({
        let ui = ui.as_weak();
        move || {
            let Some(ui) = ui.upgrade() else { return };
            play_sound(&ui, Sound::Click);
            let accent = crate::theme::system_accent();
            SYSTEM_ACCENT.with(|value| value.set(accent));
            apply_theme_by_name(&ui, &ui.get_set_theme());
            ui.set_settings_status(
                match accent {
                    Some(rgb) if crate::theme::is_chromatic(rgb) => {
                        format!("Цвет Windows: #{:02X}{:02X}{:02X}", rgb.0, rgb.1, rgb.2)
                    }
                    Some((r, g, b)) => format!(
                        "Windows accent серый (#{r:02X}{g:02X}{b:02X}), использую accent темы"
                    ),
                    None => "Windows accent недоступен, использую accent темы".into(),
                }
                .into(),
            );
        }
    });

    ui.on_themes_refresh_clicked({
        let tx = tx.clone();
        let ui = ui.as_weak();
        move || {
            if let Some(ui) = ui.upgrade() {
                play_sound(&ui, Sound::Click);
                ui.set_settings_status("Обновление тем…".into());
            }
            let _ = tx.send(Command::RefreshThemes);
        }
    });

    ui.on_themes_open_folder_clicked({
        let tx = tx.clone();
        let ui = ui.as_weak();
        move || {
            if let Some(ui) = ui.upgrade() {
                play_sound(&ui, Sound::Click);
            }
            let _ = tx.send(Command::OpenThemesFolder);
        }
    });

    ui.on_theme_reset_clicked({
        let tx = tx.clone();
        let ui = ui.as_weak();
        move || {
            let Some(ui) = ui.upgrade() else { return };
            play_sound(&ui, Sound::Click);
            ui.set_set_theme("dark".into());
            ui.set_set_theme_mode("base".into());
            apply_theme_by_name(&ui, crate::theme::DEFAULT_THEME);
            let _ = tx.send(Command::SaveTheme(crate::theme::DEFAULT_THEME.into()));
        }
    });

    ui.on_layout_move({
        let tx = tx.clone();
        let ui = ui.as_weak();
        move |item, direction| {
            if let Some(ui) = ui.upgrade() {
                play_sound(&ui, Sound::Click);
            }
            let _ = tx.send(Command::MoveLayoutItem {
                item: item.to_string(),
                direction: direction.to_string(),
            });
        }
    });

    ui.on_layout_reset_clicked({
        let tx = tx.clone();
        let ui = ui.as_weak();
        move || {
            if let Some(ui) = ui.upgrade() {
                play_sound(&ui, Sound::Click);
            }
            let _ = tx.send(Command::ResetLayout);
        }
    });

    ui.on_mirror_save_key({
        let tx = tx.clone();
        let ui = ui.as_weak();
        move || {
            let Some(ui) = ui.upgrade() else { return };
            play_sound(&ui, Sound::Click);
            let _ = tx.send(Command::SaveMirrorKey(ui.get_mirror_key().to_string()));
        }
    });

    ui.on_mirror_clear_key({
        let tx = tx.clone();
        let ui = ui.as_weak();
        move || {
            if let Some(ui) = ui.upgrade() {
                play_sound(&ui, Sound::Click);
                ui.set_mirror_key("".into());
            }
            let _ = tx.send(Command::ClearMirrorKey);
        }
    });

    ui.on_mirror_mode_clicked({
        let tx = tx.clone();
        let ui = ui.as_weak();
        move |mode| {
            if let Some(ui) = ui.upgrade() {
                play_sound(&ui, Sound::Click);
            }
            let _ = tx.send(Command::SetMirrorMode(mode.to_string()));
        }
    });

    ui.on_mirror_tor_toggled({
        let tx = tx.clone();
        let ui = ui.as_weak();
        move |enabled| {
            if let Some(ui) = ui.upgrade() {
                play_sound(&ui, Sound::Toggle);
            }
            let _ = tx.send(Command::SetTor(enabled));
        }
    });

    ui.on_selftest_clicked({
        let tx = tx.clone();
        let ui = ui.as_weak();
        move || {
            if let Some(ui) = ui.upgrade() {
                play_sound(&ui, Sound::Click);
            }
            let _ = tx.send(Command::SelfTest);
        }
    });

    // Borderless windows maximize over the taskbar. Keep the window inside the
    // Windows work area instead and remember the previous rectangle.
    let restore_rect: Rc<RefCell<Option<window::Rect>>> = Rc::new(RefCell::new(None));
    ui.on_maximize_clicked({
        let ui = ui.as_weak();
        let restore_rect = restore_rect.clone();
        move || {
            let Some(ui) = ui.upgrade() else { return };
            play_sound(&ui, Sound::Click);

            let Some(handle) = window::find_launcher_window() else {
                return;
            };

            let previous = restore_rect.borrow_mut().take();

            if let Some(rect) = previous {
                if let Some(current) = window::window_rect(handle) {
                    window::animate_to(handle, current, rect);
                }
                ui.set_window_maximized(false);
            } else {
                let current = window::window_rect(handle);
                if let Some(current) = current {
                    *restore_rect.borrow_mut() = Some(current);
                }
                if let (Some(area), Some(current)) = (window::work_area(), current) {
                    window::animate_to(handle, current, area);
                    ui.set_window_maximized(true);
                }
            }
        }
    });
}

fn apply_event(ui: &AppWindow, event: Event) {
    match event {
        Event::Screen(screen) => switch_screen(ui, screen),
        Event::Busy(busy) => ui.set_busy(busy),
        Event::LoginError(message) => ui.set_error_text(message.into()),
        Event::TwoFa => {
            ui.set_show_2fa(true);
            ui.set_busy(false);
            ui.set_error_text("Введите код из приложения-аутентификатора".into());
        }
        Event::Auth { name, initial } => {
            ui.set_is_kydasay(name.eq_ignore_ascii_case("Kydasay"));
            ui.set_user_name(name.into());
            ui.set_user_initial(initial.into());
            ui.set_show_2fa(false);
            ui.set_error_text("".into());
        }
        Event::Avatar(path) => match slint::Image::load_from_path(std::path::Path::new(&path)) {
            Ok(image) => {
                ui.set_user_avatar(image);
                ui.set_has_avatar(true);
            }
            Err(error) => warn!(%error, "failed to load avatar"),
        },
        Event::Ready => ui.set_startup_ready(true),
        Event::Profiles { views, selected } => {
            let items: Vec<ProfileItem> = views.iter().map(profile_item).collect();
            ui.set_profiles(ModelRc::new(VecModel::from(items)));
            ui.set_selected_index(selected as i32);

            if let Some(view) = views.get(selected) {
                select_view(ui, view);
            } else {
                ui.set_selected_index(-1);
                ui.set_can_play(false);
                ui.set_selected_title("Профиль не выбран".into());
                ui.set_selected_description("".into());
                ui.set_selected_version("".into());
                ui.set_selected_online("".into());
            }
        }
        Event::Selected(view) => select_view(ui, &view),
        Event::Mods {
            views,
            enabled,
            total,
        } => {
            let items: Vec<ModItem> = views
                .into_iter()
                .map(|view| ModItem {
                    id: view.id as i32,
                    name: view.name.into(),
                    description: view.description.into(),
                    enabled: view.enabled,
                })
                .collect();

            ui.set_mods(ModelRc::new(VecModel::from(items)));
            ui.set_mods_enabled(enabled as i32);
            ui.set_mods_total(total as i32);
        }
        Event::CanPlay(value) => ui.set_can_play(value),
        Event::MirrorState {
            key,
            mode,
            upstream,
            can_control,
            tor_enabled,
            tor_available,
        } => {
            ui.set_mirror_key(key.into());
            ui.set_mirror_mode(mode.into());
            ui.set_mirror_upstream(upstream.into());
            ui.set_mirror_can_control(can_control);
            ui.set_mirror_tor(tor_enabled);
            ui.set_mirror_tor_available(tor_available);
        }
        Event::SelfTest { rows, summary } => {
            let items: Vec<SelfTestItem> = rows
                .into_iter()
                .map(|row| SelfTestItem {
                    name: row.name.into(),
                    status: row.status.into(),
                    message: row.message.into(),
                    duration: row.duration.into(),
                })
                .collect();
            ui.set_selftest(ModelRc::new(VecModel::from(items)));
            ui.set_selftest_summary(summary.into());
        }
        Event::News(news) => {
            let items: Vec<NewsItem> = news
                .into_iter()
                .map(|item| NewsItem {
                    title: item.title.into(),
                    body: item.body.into(),
                    date: item.date.into(),
                })
                .collect();
            ui.set_news(ModelRc::new(VecModel::from(items)));
        }
        Event::Progress {
            visible,
            value,
            title,
            text,
            speed,
        } => {
            ui.set_progress_visible(visible);
            ui.set_progress_value(value);
            ui.set_progress_title(title.into());
            ui.set_progress_text(text.into());
            ui.set_progress_speed(speed.into());
        }
        Event::Backend {
            label,
            color,
            visible,
        } => {
            ui.set_backend_mode(label.into());
            ui.set_backend_mode_visible(visible);
            ui.set_backend_mode_color(backend_color(&color));

            if visible {
                ui.set_backend_mode_expanded(true);
                let ui = ui.as_weak();
                slint::Timer::single_shot(Duration::from_secs(3), move || {
                    if let Some(ui) = ui.upgrade() {
                        ui.set_backend_mode_expanded(false);
                    }
                });
            }
        }
        Event::Themes { items, active } => {
            THEME_CATALOG.with(|catalog| *catalog.borrow_mut() = items);
            ui.set_themes(ModelRc::new(VecModel::from(theme_options())));
            ui.set_set_theme(active.clone().into());
            apply_theme_by_name(ui, &active);
            ui.set_settings_status("Темы обновлены".into());
        }
        Event::Layout(layout) => apply_layout(ui, &layout),
        Event::Settings { settings, max_ram } => {
            ui.set_set_theme_mode(settings.theme_mode.clone().into());
            apply_theme_by_name(ui, &settings.theme);
            apply_settings(ui, &settings, max_ram);
        }
        Event::SettingsStatus(status) => ui.set_settings_status(status.into()),
        Event::MainStatus(status) => ui.set_main_status(status.into()),
        Event::GameStarted { pid, hide } => {
            ui.set_main_status(format!("Игра запущена (PID {pid})").into());
            if hide {
                window::minimize_launcher();
            }
        }
        Event::Error(message) => {
            if !message.is_empty() {
                error!(%message, "worker error");
            }
            ui.set_error_text(message.into());
        }
        Event::LoggedOut => {
            ui.set_screen(0);
            ui.set_is_kydasay(false);
            ui.set_user_name("".into());
            ui.set_user_initial("?".into());
            ui.set_has_avatar(false);
            ui.set_user_avatar(slint::Image::default());
            ui.set_profiles(ModelRc::new(VecModel::from(Vec::<ProfileItem>::new())));
            ui.set_news(ModelRc::new(VecModel::from(Vec::<NewsItem>::new())));
            ui.set_selected_index(-1);
            ui.set_can_play(false);
            ui.set_error_text("".into());
            ui.set_main_status("".into());
            ui.set_show_2fa(false);
            ui.set_password("".into());
            ui.set_twofa("".into());
            ui.set_progress_visible(false);
        }
    }
}

fn countdown_text() -> String {
    use chrono::{Datelike, Local, TimeZone};

    let now = Local::now();
    let year = if now.month() == 12 {
        now.year() + 1
    } else {
        now.year()
    };
    let target = Local
        .with_ymd_and_hms(year, 12, 1, 0, 0, 0)
        .single()
        .unwrap_or(now);
    let diff = target - now;

    let days = diff.num_days();
    let hours = diff.num_hours() % 24;
    let minutes = diff.num_minutes() % 60;

    format!("До конца недрочабря: {days} д {hours} ч {minutes} мин")
}

fn switch_screen(ui: &AppWindow, screen: i32) {
    ui.set_screen_opacity(0.0);
    ui.set_screen(screen);

    let ui = ui.as_weak();
    slint::Timer::single_shot(Duration::from_millis(20), move || {
        if let Some(ui) = ui.upgrade() {
            ui.set_screen_opacity(1.0);
        }
    });
}

fn select_view(ui: &AppWindow, view: &ProfileView) {
    ui.set_selected_index(view.id as i32);
    ui.set_selected_title(view.display_name.clone().into());
    ui.set_selected_description(view.description.clone().into());
    ui.set_selected_version(view.subtitle.clone().into());
    ui.set_selected_online(view.online.clone().into());
    ui.set_selected_icon(decode_icon(view.icon.as_deref()));
    ui.set_can_play(true);
}

fn to_color(value: &str) -> slint::Color {
    crate::theme::parse_color(value).unwrap_or_else(|| slint::Color::from_rgb_u8(13, 13, 16))
}

fn theme_options() -> Vec<ThemeOption> {
    THEME_CATALOG.with(|catalog| {
        catalog
            .borrow()
            .iter()
            .map(|item| ThemeOption {
                name: item.name.clone().into(),
                builtin: item.builtin,
                accent: to_color(&item.theme.accent),
                surface: to_color(&item.theme.surface),
                text: to_color(&item.theme.text),
            })
            .collect()
    })
}

fn apply_item(ui: &AppWindow, item: &ThemeItem, mode: &str) {
    let accent = SYSTEM_ACCENT.with(|value| value.get());
    let theme = crate::theme::apply_mode(&item.theme, mode, accent);
    info!(
        theme = %item.name,
        mode,
        base_background = %item.theme.background,
        effective_background = %theme.background,
        "applying theme"
    );
    apply_theme(ui, &theme);
    apply_icons(ui, item, &theme);
    apply_texts(ui, item);
}

fn icon_tint<'a>(id: &str, theme: &'a crate::theme::Theme) -> &'a str {
    match id {
        "warning" => &theme.warning,
        "error" | "close" => &theme.danger,
        "success" => &theme.success,
        "chevron" | "minimize" | "maximize" | "restore" => &theme.text_muted,
        _ => &theme.accent,
    }
}

fn load_theme_icon(path: &str, tint: &str) -> Option<slint::Image> {
    use sha1::{Digest, Sha1};

    let source = std::path::Path::new(path);
    let is_svg = source
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.eq_ignore_ascii_case("svg"))
        .unwrap_or(false);

    if !is_svg {
        return slint::Image::load_from_path(source).ok();
    }

    let text = std::fs::read_to_string(source).ok()?;
    if !text.contains("currentColor") {
        return slint::Image::load_from_path(source).ok();
    }

    let replaced = text.replace("currentColor", tint);
    let mut hasher = Sha1::new();
    hasher.update(replaced.as_bytes());
    hasher.update(tint.as_bytes());
    let key = hex::encode(hasher.finalize());

    let dir = crate::model::local_app_dir().join("theme-icons");
    std::fs::create_dir_all(&dir).ok()?;
    let cache = dir.join(format!("{key}.svg"));
    if !cache.exists() {
        std::fs::write(&cache, replaced).ok()?;
    }

    slint::Image::load_from_path(&cache).ok()
}

fn apply_icons(ui: &AppWindow, item: &ThemeItem, theme: &crate::theme::Theme) {
    let icons = ui.global::<ThemeIcons>();

    macro_rules! set_icon {
        ($id:literal, $setter:ident, $has_setter:ident) => {
            match item
                .icons
                .get($id)
                .and_then(|path| load_theme_icon(path, icon_tint($id, theme)))
            {
                Some(image) => {
                    icons.$setter(image);
                    icons.$has_setter(true);
                }
                None => icons.$has_setter(false),
            }
        };
    }

    set_icon!("server", set_server, set_has_server);
    set_icon!("chevron", set_chevron, set_has_chevron);
    set_icon!("play", set_play, set_has_play);
    set_icon!("settings", set_settings, set_has_settings);
    set_icon!("mods", set_mods, set_has_mods);
    set_icon!("refresh", set_refresh, set_has_refresh);
    set_icon!("minimize", set_minimize, set_has_minimize);
    set_icon!("maximize", set_maximize, set_has_maximize);
    set_icon!("restore", set_restore, set_has_restore);
    set_icon!("close", set_close, set_has_close);
    set_icon!("warning", set_warning, set_has_warning);
    set_icon!("error", set_error, set_has_error);
    set_icon!("success", set_success, set_has_success);
}

fn apply_texts(ui: &AppWindow, item: &ThemeItem) {
    let texts = ui.global::<Texts>();
    let value = |key: &str, fallback: &str| -> slint::SharedString {
        item.texts
            .get(key)
            .cloned()
            .unwrap_or_else(|| fallback.to_string())
            .into()
    };

    texts.set_app_title(value("app.title", "Vestalis Launcher"));
    texts.set_app_subtitle(value("app.subtitle", "Femboy"));
    texts.set_login_title(value("login.title", "Вход в аккаунт"));
    texts.set_login_subtitle(value(
        "login.subtitle",
        "Войдите, чтобы играть на серверах Vestalis",
    ));
    texts.set_login_submit(value("login.submit", "Войти"));
    texts.set_login_offline(value("login.offline", "Оффлайн-режим"));
    texts.set_play(value("buttons.play", "Играть"));
    texts.set_settings(value("buttons.settings", "Настройки"));
    texts.set_logout(value("buttons.logout", "Выйти"));
    texts.set_refresh(value("buttons.refresh", "Обновить"));
    texts.set_mods(value("buttons.mods", "Моды"));
    texts.set_profiles(value("sections.profiles", "ПРОФИЛИ"));
    texts.set_news(value("sections.news", "НОВОСТИ"));
    texts.set_settings_title(value("settings.title", "Настройки"));
    texts.set_back(value("settings.back", "← Назад"));
}

fn apply_theme(ui: &AppWindow, theme: &crate::theme::Theme) {
    let global = ui.global::<Theme>();
    global.set_mode(theme.mode.clone().into());
    global.set_background(to_color(&theme.background));
    global.set_background_alt(to_color(&theme.background_alt));
    global.set_surface(to_color(&theme.surface));
    global.set_surface_hover(to_color(&theme.surface_hover));
    global.set_border(to_color(&theme.border));
    global.set_text(to_color(&theme.text));
    global.set_text_muted(to_color(&theme.text_muted));
    global.set_accent(to_color(&theme.accent));
    global.set_accent_hover(to_color(&theme.accent_hover));
    global.set_accent_text(to_color(&theme.accent_text));
    global.set_success(to_color(&theme.success));
    global.set_warning(to_color(&theme.warning));
    global.set_danger(to_color(&theme.danger));
    global.set_gradient_start(to_color(&theme.gradient_start));
    global.set_gradient_end(to_color(&theme.gradient_end));
    global.set_shadow(to_color(&theme.shadow));
    global.set_scrim(to_color(&theme.scrim));
    global.set_primary_container(to_color(&theme.primary_container));
    global.set_on_primary_container(to_color(&theme.on_primary_container));
    global.set_secondary(to_color(&theme.secondary));
    global.set_on_secondary(to_color(&theme.on_secondary));
    global.set_tertiary(to_color(&theme.tertiary));
    global.set_on_tertiary(to_color(&theme.on_tertiary));
    global.set_surface_low(to_color(&theme.surface_low));
    global.set_surface_high(to_color(&theme.surface_high));
    global.set_surface_variant(to_color(&theme.surface_variant));
    global.set_on_surface_variant(to_color(&theme.on_surface_variant));
    global.set_outline(to_color(&theme.outline));
    global.set_error(to_color(&theme.error));
    global.set_on_error(to_color(&theme.on_error));
    global.set_glass_tint(to_color(&theme.glass_tint));
    global.set_glass_border(to_color(&theme.glass_border));
    global.set_glass_highlight(to_color(&theme.glass_highlight));
    global.set_radius_xs(theme.radius_xs as f32);
    global.set_radius_sm(theme.radius_sm as f32);
    global.set_radius_md(theme.radius_md as f32);
    global.set_radius_lg(theme.radius_lg as f32);
    global.set_radius_pill(theme.radius_pill as f32);
    global.set_card_opacity(theme.card_opacity);
    global.set_font_family(theme.font_family.clone().into());
    global.set_base_font_weight(theme.base_font_weight);
    global.set_background_effect(theme.background_effect.clone().into());

    ui.set_background_effect(theme.background_effect.clone().into());
    ui.set_set_background_effect(theme.background_effect.clone().into());
}

fn apply_theme_by_name(ui: &AppWindow, name: &str) {
    let item = THEME_CATALOG.with(|catalog| {
        catalog
            .borrow()
            .iter()
            .find(|item| item.name.eq_ignore_ascii_case(name))
            .cloned()
    });

    if let Some(item) = item {
        apply_item(ui, &item, &ui.get_set_theme_mode());
    }
}

fn string_model(items: &[String]) -> ModelRc<slint::SharedString> {
    ModelRc::new(VecModel::from(
        items
            .iter()
            .map(|item| slint::SharedString::from(item.as_str()))
            .collect::<Vec<_>>(),
    ))
}

fn apply_layout(ui: &AppWindow, layout: &crate::layout::LayoutConfig) {
    ui.set_layout_sidebar_left(string_model(&layout.sidebar_left));
    ui.set_layout_sidebar_right(string_model(&layout.sidebar_right));
    ui.set_layout_header(string_model(&layout.header));
    ui.set_layout_content_top(string_model(&layout.content_top));
    ui.set_layout_content_bottom(string_model(&layout.content_bottom));
    ui.set_layout_footer(string_model(&layout.footer));
}

fn ui_layout(ui: &AppWindow) -> crate::layout::LayoutConfig {
    fn collect(model: &ModelRc<slint::SharedString>) -> Vec<String> {
        model.iter().map(|item| item.to_string()).collect()
    }

    crate::layout::LayoutConfig {
        sidebar_left: collect(&ui.get_layout_sidebar_left()),
        sidebar_right: collect(&ui.get_layout_sidebar_right()),
        header: collect(&ui.get_layout_header()),
        content_top: collect(&ui.get_layout_content_top()),
        content_bottom: collect(&ui.get_layout_content_bottom()),
        footer: collect(&ui.get_layout_footer()),
    }
}

fn apply_settings(ui: &AppWindow, settings: &Settings, max_ram: u64) {
    ui.set_set_install_dir(settings.install_dir.clone().into());
    ui.set_set_xms(settings.xms as i32);
    ui.set_set_xmx(settings.xmx as i32);
    ui.set_set_width_text(settings.game_width.to_string().into());
    ui.set_set_height_text(settings.game_height.to_string().into());
    ui.set_set_fullscreen(settings.fullscreen);
    ui.set_set_custom_jvm(settings.custom_jvm.clone().into());
    ui.set_set_send_hwid(settings.send_hwid);
    ui.set_set_sounds(settings.sounds);
    ui.set_set_hide_on_launch(settings.hide_on_launch);
    ui.set_set_theme(settings.theme.clone().into());
    ui.set_set_theme_mode(
        crate::theme::normalize_mode(&settings.theme_mode)
            .to_string()
            .into(),
    );
    apply_layout(ui, &settings.layout);

    let effect = match settings.background_effect.as_str() {
        "fireflies" | "leaves" | "snow" => settings.background_effect.clone(),
        _ => "none".to_string(),
    };
    ui.set_set_background_effect(effect.clone().into());
    ui.set_background_effect(effect.into());
    ui.set_max_ram(max_ram.min(i32::MAX as u64) as i32);
}

fn profile_item(view: &ProfileView) -> ProfileItem {
    ProfileItem {
        id: view.id as i32,
        name: view.name.clone().into(),
        display_name: view.display_name.clone().into(),
        initial: view.initial.clone().into(),
        subtitle: view.subtitle.clone().into(),
        description: view.description.clone().into(),
        online: 0,
        icon: decode_icon(view.icon.as_deref()),
        has_icon: view.has_icon,
    }
}

fn backend_color(value: &str) -> slint::Color {
    match value {
        "#2E9E5B" => slint::Color::from_rgb_u8(0x2E, 0x9E, 0x5B),
        "#D9822B" => slint::Color::from_rgb_u8(0xD9, 0x82, 0x2B),
        "#3B82F6" => slint::Color::from_rgb_u8(0x3B, 0x82, 0xF6),
        _ => slint::Color::from_rgb_u8(0x6B, 0x72, 0x80),
    }
}

fn decode_icon(value: Option<&str>) -> slint::Image {
    let Some(value) = value else {
        return slint::Image::default();
    };

    let payload = value
        .split(',')
        .next_back()
        .unwrap_or(value)
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();

    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(payload) else {
        return slint::Image::default();
    };
    let Ok(image) = image::load_from_memory(&bytes) else {
        return slint::Image::default();
    };
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    let buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
        rgba.as_raw(),
        width,
        height,
    );

    slint::Image::from_rgba8(buffer)
}
