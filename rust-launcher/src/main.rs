//! Vestalis Launcher — Rust rewrite.
//!
//! The UI lives in `ui/app.slint`; this file is the composition root. It owns
//! the Slint event loop, starts the Tokio worker and translates worker events
//! into UI properties. All blocking work happens off the UI thread.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api;
mod diagnostics;
mod discord;
mod files;
mod hwid;
mod launch;
mod layout;
mod locale;
mod model;
mod signalr;
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
    static CURRENT_THEME: RefCell<Option<crate::theme::Theme>> = const { RefCell::new(None) };
    static THEME_TRANSITION: RefCell<Option<ThemeTransition>> = const { RefCell::new(None) };
    static THEME_TIMER: slint::Timer = slint::Timer::default();
    static TOAST_TIMER: slint::Timer = slint::Timer::default();
    static LAYOUT_PROFILES: RefCell<Vec<crate::layout::LayoutProfile>> = const { RefCell::new(Vec::new()) };
    static DRAG_ITEM: RefCell<String> = const { RefCell::new(String::new()) };
    static WINDOW_GEOMETRY: std::cell::Cell<(u32, u32, i32, i32)> =
        const { std::cell::Cell::new((1180, 640, -1, -1)) };
}

struct ThemeTransition {
    from: crate::theme::Theme,
    to: crate::theme::Theme,
    started: std::time::Instant,
}

fn main() -> anyhow::Result<()> {
    window::set_app_user_model_id();
    if !window::ensure_single_instance() {
        return Ok(());
    }

    let log_dir = model::local_app_dir().join("logs");
    let _ = std::fs::create_dir_all(&log_dir);
    storage::cleanup_cache_dir(&log_dir, Duration::from_secs(30 * 24 * 60 * 60), 60);
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

    storage::cleanup_cache_dir(
        &model::local_app_dir().join("theme-icons"),
        Duration::from_secs(30 * 24 * 60 * 60),
        500,
    );

    let mut state = load_state();
    let saved_geometry = storage::load_window_geometry();
    WINDOW_GEOMETRY.with(|geometry| {
        geometry.set((
            saved_geometry.width,
            saved_geometry.height,
            saved_geometry.x,
            saved_geometry.y,
        ))
    });
    // PL-1 is disabled: migrate a saved selection to RU-2 and let the normal
    // auto-login issue a fresh session on the working backend.
    if !model::PL1_AVAILABLE && model::server_index(&state.server_host) == 3 {
        state.server_host = model::SERVERS[1].1.to_string();
        state.user = None;
        storage::save_state(&state);
    }

    let mut themes = crate::theme::load_all();
    state.settings.theme_mode =
        crate::theme::normalize_mode(&state.settings.theme_mode).to_string();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let (command_tx, command_rx) = tokio::sync::mpsc::unbounded_channel();
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();

    let ui = AppWindow::new()?;
    // winit initialises OLE/STA while creating the window, so the WinRT
    // accent lookup must run after this point on the UI thread.
    SYSTEM_ACCENT.with(|value| value.set(crate::theme::system_accent()));
    info!(
        accent = ?SYSTEM_ACCENT.with(|value| value.get()),
        "Windows accent sampled"
    );

    let accent = SYSTEM_ACCENT
        .with(|value| value.get())
        .unwrap_or((0, 120, 215));
    themes.retain(|item| !item.name.eq_ignore_ascii_case(crate::theme::WINDOWS_THEME));
    themes.push(windows_theme_item(accent));

    let selected = crate::theme::resolve(&state.settings.theme, &themes).clone();
    if state.settings.theme != selected.name {
        state.settings.theme = selected.name.clone();
        storage::save_state(&state);
    }
    let active = crate::theme::resolve_of_pair(
        state.settings.follow_windows_theme,
        &state.settings.light_theme,
        &state.settings.dark_theme,
        &selected.name,
        &themes,
    )
    .clone();
    info!(
        theme = %active.name,
        mode = %state.settings.theme_mode,
        follow_windows = state.settings.follow_windows_theme,
        "theme applied"
    );
    THEME_CATALOG.with(|catalog| *catalog.borrow_mut() = themes);

    ui.set_set_theme_mode(state.settings.theme_mode.clone().into());
    ui.set_set_accent(state.settings.accent.clone().into());
    ui.set_set_language(crate::locale::normalize(&state.settings.language).into());
    ui.set_set_glass_opacity(state.settings.glass_opacity.clamp(0.25, 0.9));
    ui.set_set_glass_blur(state.settings.glass_blur.clamp(0.0, 1.0));
    ui.set_follow_windows_theme(state.settings.follow_windows_theme);
    ui.set_light_theme(state.settings.light_theme.clone().into());
    ui.set_dark_theme(state.settings.dark_theme.clone().into());
    // Apply before the first frame so a non-dark theme never flashes.
    apply_item(&ui, &active, &state.settings.theme_mode);
    initialize_ui(&ui, &state);
    install_callbacks(&ui, command_tx.clone());

    runtime.spawn(Worker::new(state, event_tx).run(command_rx));

    // `--offline` lets the user (and UI tests) open the cached profiles
    // without touching the backend.
    if std::env::args().any(|argument| argument == "--offline") {
        let _ = command_tx.send(Command::Offline);
    }

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

    slint::Timer::single_shot(Duration::from_millis(350), move || {
        if let Some(handle) = window::find_launcher_window() {
            window::apply_icon(handle);
            window::apply_rounded_corners(handle);
            restore_window_geometry(handle, saved_geometry);
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

    // Follow the Windows accent for Material 3 and for the Windows palette.
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
            if changed {
                update_windows_theme(&ui, accent.unwrap_or((0, 120, 215)));
                let active = ui.get_set_theme();
                if ui.get_set_theme_mode() == "material3"
                    || active.eq_ignore_ascii_case(crate::theme::WINDOWS_THEME)
                {
                    apply_theme_by_name(&ui, &active);
                }
            }
        },
    );

    // Windows light/dark can change while the launcher is open. Poll the
    // personalization registry (cheap) and re-apply the paired theme.
    let follow_ui = ui.as_weak();
    let last_light = std::cell::Cell::new(crate::theme::system_uses_light_theme());
    let theme_timer = slint::Timer::default();
    theme_timer.start(
        slint::TimerMode::Repeated,
        Duration::from_secs(3),
        move || {
            let Some(ui) = follow_ui.upgrade() else {
                return;
            };
            if !ui.get_follow_windows_theme() {
                last_light.set(crate::theme::system_uses_light_theme());
                return;
            }

            let light = crate::theme::system_uses_light_theme();
            if last_light.replace(light) != light {
                apply_theme_by_name(&ui, &ui.get_set_theme());
            }
        },
    );

    // Persist the window rectangle periodically as well, so a crash or a
    // native Alt+F4 cannot lose the user's preferred size/position.
    let geometry_timer = slint::Timer::default();
    let last_geometry = std::cell::Cell::new((
        saved_geometry.width,
        saved_geometry.height,
        saved_geometry.x,
        saved_geometry.y,
    ));
    geometry_timer.start(
        slint::TimerMode::Repeated,
        Duration::from_secs(2),
        move || {
            let Some(handle) = window::find_launcher_window() else {
                return;
            };
            let Some(rect) = window::window_rect(handle) else {
                return;
            };
            let geometry = storage::WindowGeometry {
                width: (rect.right - rect.left).max(0) as u32,
                height: (rect.bottom - rect.top).max(0) as u32,
                x: rect.left,
                y: rect.top,
            };
            let current = (geometry.width, geometry.height, geometry.x, geometry.y);
            if last_geometry.replace(current) != current {
                storage::save_window_geometry(geometry);
            }
        },
    );

    let timer = slint::Timer::default();
    let ui_weak = ui.as_weak();
    let event_command_tx = command_tx.clone();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(40),
        move || {
            while let Ok(event) = event_rx.try_recv() {
                if let Some(ui) = ui_weak.upgrade() {
                    apply_event(&ui, event, &event_command_tx);
                }
            }
        },
    );

    ui.run()?;
    let _ = command_tx.send(Command::Shutdown);
    info!("launcher closed");
    Ok(())
}

/// Restores the last window size/position, clamped to the work area and the
/// 1080×620 minimum. A missing/negative position keeps the current placement.
fn restore_window_geometry(handle: isize, geometry: storage::WindowGeometry) {
    let Some(current) = window::window_rect(handle) else {
        return;
    };

    let width = geometry.width.clamp(1080, 3840) as i32;
    let height = geometry.height.clamp(620, 2160) as i32;
    let mut left = if geometry.x < 0 {
        current.left
    } else {
        geometry.x
    };
    let mut top = if geometry.y < 0 {
        current.top
    } else {
        geometry.y
    };

    if let Some(area) = window::work_area() {
        let max_left = (area.right - width).max(area.left);
        let max_top = (area.bottom - height).max(area.top);
        left = left.clamp(area.left, max_left);
        top = top.clamp(area.top, max_top);
    }

    WINDOW_GEOMETRY.with(|geometry| {
        geometry.set((width as u32, height as u32, left, top));
    });
    window::set_rect(
        handle,
        window::Rect {
            left,
            top,
            right: left + width,
            bottom: top + height,
        },
    );
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
        move || {
            if let Some(ui) = ui.upgrade() {
                play_sound(&ui, Sound::Click);
                switch_screen(&ui, 2);
            }
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

    ui.on_cancel_launch_clicked({
        let tx = tx.clone();
        let ui = ui.as_weak();
        move || {
            if let Some(ui) = ui.upgrade() {
                play_sound(&ui, Sound::Click);
                ui.set_main_status("Отмена запуска…".into());
            }
            let _ = tx.send(Command::CancelLaunch);
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

    ui.on_mods_filter_changed({
        let tx = tx.clone();
        move |filter| {
            let _ = tx.send(Command::SetModsFilter(filter.to_string()));
        }
    });

    ui.on_mods_download_missing_clicked({
        let tx = tx.clone();
        let ui = ui.as_weak();
        move || {
            if let Some(ui) = ui.upgrade() {
                play_sound(&ui, Sound::Click);
                ui.set_main_status("Скачивание модов…".into());
            }
            let _ = tx.send(Command::DownloadMissingMods);
        }
    });

    ui.on_mods_set_all_toggled({
        let tx = tx.clone();
        let ui = ui.as_weak();
        move |enabled| {
            if let Some(ui) = ui.upgrade() {
                play_sound(&ui, Sound::Toggle);
            }
            let _ = tx.send(Command::SetAllMods(enabled));
        }
    });

    ui.on_mods_open_folder_clicked({
        let tx = tx.clone();
        let ui = ui.as_weak();
        move || {
            if let Some(ui) = ui.upgrade() {
                play_sound(&ui, Sound::Click);
            }
            let _ = tx.send(Command::OpenModsFolder);
        }
    });

    ui.on_mod_download_clicked({
        let tx = tx.clone();
        let ui = ui.as_weak();
        move |id| {
            if let Some(ui) = ui.upgrade() {
                play_sound(&ui, Sound::Click);
            }
            let _ = tx.send(Command::DownloadMod(id as usize));
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
                custom_java: ui.get_set_custom_java().to_string(),
                auto_login: ui.get_set_auto_login(),
                send_hwid: ui.get_set_send_hwid(),
                sounds: ui.get_set_sounds(),
                hide_on_launch: ui.get_set_hide_on_launch(),
                discord_rpc: ui.get_set_discord_rpc(),
                background_effect: ui.get_set_background_effect().to_string(),
                theme: ui.get_set_theme().to_string(),
                theme_mode: ui.get_set_theme_mode().to_string(),
                language: ui.get_set_language().to_string(),
                glass_opacity: ui.get_set_glass_opacity(),
                glass_blur: ui.get_set_glass_blur(),
                accent: ui.get_set_accent().to_string(),
                follow_windows_theme: ui.get_follow_windows_theme(),
                light_theme: ui.get_light_theme().to_string(),
                dark_theme: ui.get_dark_theme().to_string(),
                layout: ui_layout(&ui),
                layouts: LAYOUT_PROFILES.with(|profiles| profiles.borrow().clone()),
                layout_profile: ui.get_layout_profile().to_string(),
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
            let rgb = accent.unwrap_or((0, 120, 215));
            update_windows_theme(&ui, rgb);
            ui.set_set_theme(crate::theme::WINDOWS_THEME.into());
            apply_theme_by_name(&ui, crate::theme::WINDOWS_THEME);
            ui.set_settings_status(
                match accent {
                    Some((r, g, b)) => {
                        format!("Тема Windows: #{r:02X}{g:02X}{b:02X}")
                    }
                    None => "Windows accent недоступен, тема Windows с синим accent".into(),
                }
                .into(),
            );
        }
    });

    ui.on_pick_accent_clicked({
        let ui = ui.as_weak();
        move || {
            let Some(ui) = ui.upgrade() else { return };
            play_sound(&ui, Sound::Click);
            let current = crate::theme::parse_hex_rgb(&ui.get_set_accent());
            if let Some((r, g, b)) = window::pick_color(current) {
                ui.set_set_accent(format!("#{r:02X}{g:02X}{b:02X}").into());
                apply_theme_by_name(&ui, &ui.get_set_theme());
                ui.set_settings_status("Ручной accent применён".into());
            }
        }
    });

    ui.on_accent_applied({
        let ui = ui.as_weak();
        move || {
            let Some(ui) = ui.upgrade() else { return };
            let value = ui.get_set_accent().trim().to_string();
            if value.is_empty() {
                ui.set_settings_status("Accent очищен, используется Windows".into());
                apply_theme_by_name(&ui, &ui.get_set_theme());
                return;
            }
            match crate::theme::parse_hex_rgb(&value) {
                Some((r, g, b)) => {
                    ui.set_set_accent(format!("#{r:02X}{g:02X}{b:02X}").into());
                    apply_theme_by_name(&ui, &ui.get_set_theme());
                    ui.set_settings_status(format!("Accent: #{r:02X}{g:02X}{b:02X}").into());
                }
                None => ui.set_settings_status("Некорректный HEX, нужен #RRGGBB".into()),
            }
        }
    });

    ui.on_follow_windows_toggled({
        let ui = ui.as_weak();
        move |enabled| {
            let Some(ui) = ui.upgrade() else { return };
            play_sound(&ui, Sound::Toggle);
            ui.set_follow_windows_theme(enabled);
            apply_theme_by_name(&ui, &ui.get_set_theme());
            ui.set_settings_status(
                if enabled {
                    "Тема будет следовать режиму Windows"
                } else {
                    "Используется выбранная тема"
                }
                .into(),
            );
        }
    });

    ui.on_open_install_clicked({
        let tx = tx.clone();
        let ui = ui.as_weak();
        move || {
            if let Some(ui) = ui.upgrade() {
                play_sound(&ui, Sound::Click);
            }
            let _ = tx.send(Command::OpenInstallFolder);
        }
    });

    ui.on_reset_window_clicked({
        let ui = ui.as_weak();
        move || {
            let Some(ui) = ui.upgrade() else { return };
            play_sound(&ui, Sound::Click);

            let Some(handle) = window::find_launcher_window() else {
                return;
            };
            let width = 1180;
            let height = 640;
            let (left, top) = window::work_area()
                .map(|area| {
                    (
                        area.left + ((area.right - area.left - width) / 2).max(0),
                        area.top + ((area.bottom - area.top - height) / 2).max(0),
                    )
                })
                .unwrap_or((120, 120));

            let geometry = storage::WindowGeometry {
                width: width as u32,
                height: height as u32,
                x: left,
                y: top,
            };
            WINDOW_GEOMETRY.with(|value| value.set((geometry.width, geometry.height, left, top)));
            storage::save_window_geometry(geometry);
            window::set_rect(
                handle,
                window::Rect {
                    left,
                    top,
                    right: left + width,
                    bottom: top + height,
                },
            );
        }
    });

    ui.on_open_logs_clicked({
        let tx = tx.clone();
        let ui = ui.as_weak();
        move || {
            if let Some(ui) = ui.upgrade() {
                play_sound(&ui, Sound::Click);
            }
            let _ = tx.send(Command::OpenLogsFolder);
        }
    });

    ui.on_copy_hwid_clicked({
        let ui = ui.as_weak();
        move || {
            let Some(ui) = ui.upgrade() else { return };
            play_sound(&ui, Sound::Click);

            // Always copy the real hardware ID: the toggle only controls what
            // is sent to the backend, not what support staff need.
            let mut settings = storage::load_state().settings;
            settings.send_hwid = true;
            let hwid = hwid::hwid_for(&settings);
            window::copy_text(&hwid);
            ui.set_settings_status("HWID скопирован в буфер обмена".into());
        }
    });

    ui.on_test_sound_clicked({
        let ui = ui.as_weak();
        move || {
            if let Some(ui) = ui.upgrade() {
                play_sound(&ui, Sound::Toggle);
            }
        }
    });

    ui.on_pick_java_clicked({
        let ui = ui.as_weak();
        move || {
            let Some(ui) = ui.upgrade() else { return };
            play_sound(&ui, Sound::Click);
            if let Some(path) = window::pick_file(
                "Выберите javaw.exe",
                "Java (*.exe)\0*.exe\0Все файлы (*.*)\0*.*\0",
            ) {
                ui.set_set_custom_java(path.to_string_lossy().into_owned().into());
            }
        }
    });

    ui.on_check_files_clicked({
        let tx = tx.clone();
        let ui = ui.as_weak();
        move || {
            if let Some(ui) = ui.upgrade() {
                play_sound(&ui, Sound::Click);
                ui.set_settings_status("Проверка файлов…".into());
            }
            let _ = tx.send(Command::CheckFiles);
        }
    });

    ui.on_clear_cache_clicked({
        let tx = tx.clone();
        let ui = ui.as_weak();
        move || {
            if let Some(ui) = ui.upgrade() {
                play_sound(&ui, Sound::Click);
                ui.set_settings_status("Очистка кэша…".into());
            }
            let _ = tx.send(Command::ClearValidationCache);
        }
    });

    ui.on_export_diagnostics_clicked({
        let tx = tx.clone();
        let ui = ui.as_weak();
        move || {
            if let Some(ui) = ui.upgrade() {
                play_sound(&ui, Sound::Click);
                ui.set_settings_status("Сбор диагностики…".into());
            }
            let _ = tx.send(Command::ExportDiagnostics);
        }
    });

    ui.on_toast_close({
        let ui = ui.as_weak();
        move || {
            if let Some(ui) = ui.upgrade() {
                ui.set_toast_visible(false);
            }
            TOAST_TIMER.with(|timer| timer.stop());
        }
    });

    ui.on_save_window_geometry({
        move || {
            let Some(handle) = window::find_launcher_window() else {
                return;
            };
            let Some(rect) = window::window_rect(handle) else {
                return;
            };

            let geometry = storage::WindowGeometry {
                width: (rect.right - rect.left).max(0) as u32,
                height: (rect.bottom - rect.top).max(0) as u32,
                x: rect.left,
                y: rect.top,
            };
            WINDOW_GEOMETRY
                .with(|value| value.set((geometry.width, geometry.height, geometry.x, geometry.y)));
            storage::save_window_geometry(geometry);
        }
    });

    ui.on_glass_changed({
        let ui = ui.as_weak();
        move || {
            let Some(ui) = ui.upgrade() else { return };
            apply_theme_by_name(&ui, &ui.get_set_theme());
        }
    });

    ui.on_language_clicked({
        let ui = ui.as_weak();
        move |code| {
            let Some(ui) = ui.upgrade() else { return };
            play_sound(&ui, Sound::Click);
            let code = crate::locale::normalize(&code);
            ui.set_set_language(code.into());
            apply_theme_by_name(&ui, &ui.get_set_theme());
            ui.set_settings_status("Язык изменён".into());
        }
    });

    ui.on_pair_theme_step({
        let ui = ui.as_weak();
        move |side, forward| {
            let Some(ui) = ui.upgrade() else { return };
            play_sound(&ui, Sound::Click);

            let names: Vec<String> = THEME_CATALOG.with(|catalog| {
                catalog
                    .borrow()
                    .iter()
                    .map(|item| item.name.clone())
                    .collect()
            });
            if names.is_empty() {
                return;
            }

            let current = if side.as_str() == "light" {
                ui.get_light_theme().to_string()
            } else {
                ui.get_dark_theme().to_string()
            };
            let index = names
                .iter()
                .position(|name| name.eq_ignore_ascii_case(&current))
                .unwrap_or(0);
            let next = if forward {
                (index + 1) % names.len()
            } else {
                (index + names.len() - 1) % names.len()
            };

            if side.as_str() == "light" {
                ui.set_light_theme(names[next].clone().into());
            } else {
                ui.set_dark_theme(names[next].clone().into());
            }
            apply_theme_by_name(&ui, &ui.get_set_theme());
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

    ui.on_layout_drop({
        let tx = tx.clone();
        let ui = ui.as_weak();
        move |item, zone, before| {
            if let Some(ui) = ui.upgrade() {
                play_sound(&ui, Sound::Click);
            }
            let _ = tx.send(Command::LayoutDrop {
                item: item.to_string(),
                zone: zone.to_string(),
                before: before.to_string(),
            });
        }
    });

    ui.on_layout_widths_changed({
        let tx = tx.clone();
        move |left, right| {
            let _ = tx.send(Command::LayoutWidths {
                left: left.max(0) as u32,
                right: right.max(0) as u32,
            });
        }
    });

    ui.on_layout_profile_save({
        let tx = tx.clone();
        let ui = ui.as_weak();
        move |name| {
            if let Some(ui) = ui.upgrade() {
                play_sound(&ui, Sound::Click);
            }
            let _ = tx.send(Command::LayoutProfileSave(name.to_string()));
        }
    });

    ui.on_layout_profile_switch({
        let tx = tx.clone();
        let ui = ui.as_weak();
        move |name| {
            if let Some(ui) = ui.upgrade() {
                play_sound(&ui, Sound::Click);
            }
            let _ = tx.send(Command::LayoutProfileSwitch(name.to_string()));
        }
    });

    ui.on_layout_profile_delete({
        let tx = tx.clone();
        let ui = ui.as_weak();
        move |name| {
            if let Some(ui) = ui.upgrade() {
                play_sound(&ui, Sound::Click);
            }
            let _ = tx.send(Command::LayoutProfileDelete(name.to_string()));
        }
    });

    ui.on_layout_profile_rename({
        let tx = tx.clone();
        let ui = ui.as_weak();
        move |old, new| {
            if let Some(ui) = ui.upgrade() {
                play_sound(&ui, Sound::Click);
            }
            let _ = tx.send(Command::LayoutProfileRename {
                old: old.to_string(),
                new: new.to_string(),
            });
        }
    });

    // LayoutState / LayoutDrag globals: visibility checks and the drag payload.
    ui.global::<ProfileFilter>().on_changed({
        let tx = tx.clone();
        move |filter| {
            let _ = tx.send(Command::SetProfileFilter(filter.to_string()));
        }
    });

    ui.global::<LayoutState>().on_resize_block({
        let tx = tx.clone();
        move |item, height| {
            let _ = tx.send(Command::LayoutHeight {
                item: item.to_string(),
                height: height.max(0) as u32,
            });
        }
    });

    ui.global::<LayoutState>().on_toggle_hidden({
        let tx = tx.clone();
        move |item, hidden| {
            let _ = tx.send(Command::LayoutToggle {
                item: item.to_string(),
                hidden,
            });
        }
    });

    ui.global::<LayoutDrag>().on_begin({
        let ui = ui.as_weak();
        move |item| {
            let changed = DRAG_ITEM.with(|current| {
                if current.borrow().as_str() == item.as_str() {
                    false
                } else {
                    *current.borrow_mut() = item.to_string();
                    true
                }
            });
            if !changed {
                return;
            }
            if let Some(ui) = ui.upgrade() {
                ui.global::<LayoutDrag>()
                    .set_payload(i_slint_core::data_transfer::DataTransfer::from(item));
            }
        }
    });
    ui.global::<LayoutDrag>()
        .on_transfer_text(|data| data.plain_text().ok().unwrap_or_default());

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

fn apply_event(
    ui: &AppWindow,
    event: Event,
    command_tx: &tokio::sync::mpsc::UnboundedSender<Command>,
) {
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
            missing,
        } => {
            let items: Vec<ModItem> = views
                .into_iter()
                .map(|view| ModItem {
                    id: view.id as i32,
                    name: view.name.into(),
                    description: view.description.into(),
                    enabled: view.enabled,
                    downloaded: view.downloaded,
                    downloading: view.downloading,
                })
                .collect();

            ui.set_mods(ModelRc::new(VecModel::from(items)));
            ui.set_mods_enabled(enabled as i32);
            ui.set_mods_total(total as i32);
            ui.set_mods_missing(missing as i32);
        }
        Event::CanPlay(value) => ui.set_can_play(value),
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
        Event::Notification { title, body } => {
            ui.set_toast_title(title.into());
            ui.set_toast_body(body.into());
            ui.set_toast_visible(true);

            let weak = ui.as_weak();
            TOAST_TIMER.with(|timer| {
                timer.start(
                    slint::TimerMode::SingleShot,
                    Duration::from_secs(6),
                    move || {
                        if let Some(ui) = weak.upgrade() {
                            ui.set_toast_visible(false);
                        }
                    },
                );
            });
        }
        Event::LayoutProfiles { profiles, active } => {
            ui.set_layout_profiles(string_model(
                &profiles
                    .iter()
                    .map(|profile| profile.name.clone())
                    .collect::<Vec<_>>(),
            ));
            ui.set_layout_profile(active.into());
            LAYOUT_PROFILES.with(|value| *value.borrow_mut() = profiles);
        }
        Event::Settings { settings, max_ram } => {
            apply_settings(ui, &settings, max_ram);
            apply_theme_by_name(ui, &settings.theme);
        }
        Event::SettingsStatus(status) => {
            ui.set_settings_status(status.into());
            ui.set_settings_status_error(false);
        }
        Event::MainStatus(status) => ui.set_main_status(status.into()),
        Event::GameStarted { pid, hide } => {
            ui.set_main_status(format!("Игра запущена (PID {pid})").into());
            if hide {
                window::minimize_launcher();
            }
        }
        Event::GameExited { pid } => {
            ui.set_main_status(format!("Игра закрыта (PID {pid})").into());
            if ui.get_set_hide_on_launch() {
                window::restore_launcher();
            }
            let _ = command_tx.send(Command::GameExited(pid));
        }
        Event::Error(message) => {
            if !message.is_empty() {
                error!(%message, "worker error");
                // Errors raised while Settings is open (maintenance actions,
                // file checks) must be visible there too.
                ui.set_settings_status(message.clone().into());
                ui.set_settings_status_error(true);
            }
            ui.set_error_text(message.into());
        }
        Event::LoggedOut => {
            ui.set_screen(0);
            ui.global::<ProfileFilter>().set_text("".into());
            ui.set_mods_filter("".into());
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

fn windows_theme_item(accent: crate::theme::Rgb) -> ThemeItem {
    ThemeItem {
        name: crate::theme::WINDOWS_THEME.into(),
        builtin: true,
        theme: crate::theme::windows_theme(accent),
        icons: std::collections::HashMap::new(),
        texts: std::collections::HashMap::new(),
    }
}

fn update_windows_theme(ui: &AppWindow, accent: crate::theme::Rgb) {
    let item = windows_theme_item(accent);
    THEME_CATALOG.with(|catalog| {
        let mut catalog = catalog.borrow_mut();
        match catalog
            .iter_mut()
            .find(|entry| entry.name.eq_ignore_ascii_case(crate::theme::WINDOWS_THEME))
        {
            Some(existing) => *existing = item,
            None => catalog.push(item),
        }
    });
    ui.set_themes(ModelRc::new(VecModel::from(theme_options())));
}

fn apply_item(ui: &AppWindow, item: &ThemeItem, mode: &str) {
    let manual = crate::theme::parse_hex_rgb(&ui.get_set_accent());
    let accent = manual.or_else(|| SYSTEM_ACCENT.with(|value| value.get()));
    let theme = crate::theme::apply_mode(
        &item.theme,
        mode,
        accent,
        ui.get_set_glass_opacity(),
        ui.get_set_glass_blur(),
    );
    info!(
        theme = %item.name,
        mode,
        base_background = %item.theme.background,
        effective_background = %theme.background,
        "applying theme"
    );
    apply_theme_transition(ui, &theme);
    apply_icons(ui, item, &theme);
    apply_texts(ui, item);
}

/// Cross-fades the palette over ~280 ms. Slint globals cannot declare
/// animations, so the interpolation is driven from Rust and pushed through the
/// same `apply_theme` path.
fn apply_theme_transition(ui: &AppWindow, target: &crate::theme::Theme) {
    let from = CURRENT_THEME.with(|value| value.borrow().clone());
    let Some(from) = from else {
        apply_theme(ui, target);
        return;
    };

    if from.background == target.background
        && from.surface == target.surface
        && from.accent == target.accent
        && from.mode == target.mode
    {
        apply_theme(ui, target);
        return;
    }

    THEME_TRANSITION.with(|state| {
        *state.borrow_mut() = Some(ThemeTransition {
            from,
            to: target.clone(),
            started: std::time::Instant::now(),
        });
    });

    let weak = ui.as_weak();
    let to = target.clone();
    THEME_TIMER.with(|timer| {
        timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(16),
            move || {
                let Some(ui) = weak.upgrade() else {
                    THEME_TIMER.with(|timer| timer.stop());
                    return;
                };

                let (from, to, progress) = THEME_TRANSITION.with(|state| {
                    let state = state.borrow();
                    match state.as_ref() {
                        Some(transition) => (
                            transition.from.clone(),
                            transition.to.clone(),
                            (transition.started.elapsed().as_secs_f32() / 0.28).clamp(0.0, 1.0),
                        ),
                        None => (to.clone(), to.clone(), 1.0),
                    }
                });

                if progress >= 1.0 {
                    apply_theme(&ui, &to);
                    THEME_TRANSITION.with(|state| *state.borrow_mut() = None);
                    THEME_TIMER.with(|timer| timer.stop());
                    return;
                }

                // smoothstep for an ease-in-out feel
                let eased = progress * progress * (3.0 - 2.0 * progress);
                let mixed = crate::theme::Theme::mix(&from, &to, eased);
                apply_theme(&ui, &mixed);
            },
        );
    });
}

fn icon_tint<'a>(id: &str, theme: &'a crate::theme::Theme) -> &'a str {
    match id {
        "warning" => &theme.warning,
        "error" | "close" => &theme.danger,
        "success" => &theme.success,
        "chevron" | "minimize" | "maximize" | "restore" | "folder" | "user" => &theme.text_muted,
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
        let temporary = dir.join(format!("{key}.tmp"));
        std::fs::write(&temporary, replaced).ok()?;
        std::fs::rename(&temporary, &cache).ok()?;
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
    set_icon!("download", set_download, set_has_download);
    set_icon!("folder", set_folder, set_has_folder);
    set_icon!("user", set_user, set_has_user);
}

fn apply_texts(ui: &AppWindow, item: &ThemeItem) {
    let lang = ui.get_set_language().to_string();
    let texts = ui.global::<Texts>();
    let value = |key: &str| -> slint::SharedString {
        item.texts
            .get(key)
            .cloned()
            .unwrap_or_else(|| crate::locale::get(&lang, key).to_string())
            .into()
    };

    macro_rules! set {
        ($key:literal, $setter:ident) => {
            texts.$setter(value($key))
        };
    }

    set!("app.title", set_app_title);
    set!("app.subtitle", set_app_subtitle);
    set!("login.title", set_login_title);
    set!("login.subtitle", set_login_subtitle);
    set!("login.submit", set_login_submit);
    set!("login.offline", set_login_offline);
    set!("buttons.play", set_play);
    set!("buttons.settings", set_settings);
    set!("buttons.logout", set_logout);
    set!("buttons.refresh", set_refresh);
    set!("buttons.mods", set_mods);
    set!("sections.profiles", set_profiles);
    set!("profiles.search", set_profiles_search);
    set!("profiles.not_found", set_profiles_not_found);
    set!("profiles.empty", set_profiles_empty);
    set!("settings.interface.test_sound", set_interface_test_sound);
    set!("settings.interface.copy_hwid", set_interface_copy_hwid);
    set!("sections.news", set_news);
    set!("settings.title", set_settings_title);
    set!("settings.back", set_back);

    set!("login.username", set_login_username);
    set!("login.password", set_login_password);
    set!("login.show_password", set_login_show_password);
    set!("login.hwid", set_login_hwid);
    set!("login.2fa", set_login_2fa);
    set!("login.2fa_submit", set_login_2fa_submit);
    set!("login.busy", set_login_busy);
    set!("login.checking", set_login_checking);
    set!("login.profile_select", set_profile_select);
    set!("buttons.save", set_save);
    set!("buttons.apply", set_apply);
    set!("buttons.pick", set_pick);
    set!("buttons.load", set_load);
    set!("buttons.delete", set_delete);
    set!("buttons.rename", set_rename);
    set!("buttons.download", set_download);
    set!("buttons.downloading", set_downloading);
    set!("buttons.cancel", set_cancel);
    set!("settings.interface.discord", set_interface_discord);
    set!("settings.utility.install", set_utility_install);
    set!("settings.utility.logs", set_utility_logs);
    set!("settings.utility.reset_window", set_utility_reset_window);
    set!("settings.utility.check_files", set_utility_check_files);
    set!("settings.utility.clear_cache", set_utility_clear_cache);

    set!("settings.memory.title", set_memory_title);
    set!("settings.memory.desc", set_memory_desc);
    set!("settings.memory.xms", set_memory_xms);
    set!("settings.memory.xmx", set_memory_xmx);
    set!("settings.jvm.title", set_jvm_title);
    set!("settings.jvm.desc", set_jvm_desc);
    set!("settings.game.title", set_game_title);
    set!("settings.game.fullscreen", set_game_fullscreen);
    set!("settings.game.resolution", set_game_resolution);
    set!("settings.game.java", set_game_java);
    set!("settings.interface.auto_login", set_interface_auto_login);
    set!("settings.interface.title", set_interface_title);
    set!("settings.interface.sounds", set_interface_sounds);
    set!("settings.interface.hwid", set_interface_hwid);
    set!("settings.interface.hide_on_launch", set_interface_hide);
    set!("settings.interface.background", set_interface_background);
    set!("settings.interface.effects", set_interface_effects);
    set!("settings.appearance.title", set_appearance_title);
    set!("settings.appearance.desc", set_appearance_desc);
    set!("settings.appearance.mode", set_appearance_mode);
    set!(
        "settings.appearance.accent_hint",
        set_appearance_accent_hint
    );
    set!("settings.appearance.windows", set_appearance_windows);
    set!("settings.appearance.manual", set_appearance_manual);
    set!("settings.appearance.follow", set_appearance_follow);
    set!("settings.appearance.light", set_appearance_light);
    set!("settings.appearance.dark", set_appearance_dark);
    set!("settings.appearance.builtin", set_appearance_builtin);
    set!("settings.appearance.custom", set_appearance_custom);
    set!("settings.appearance.refresh", set_appearance_refresh);
    set!("settings.appearance.open", set_appearance_open);
    set!("settings.appearance.reset", set_appearance_reset);
    set!("settings.language.title", set_language_title);
    set!("settings.language.hint", set_language_hint);

    set!("layout.title", set_layout_title);
    set!("layout.desc", set_layout_desc);
    set!("layout.zone_left", set_layout_zone_left);
    set!("layout.zone_right", set_layout_zone_right);
    set!("layout.zone_header", set_layout_zone_header);
    set!("layout.zone_content_top", set_layout_zone_content_top);
    set!("layout.zone_content_bottom", set_layout_zone_content_bottom);
    set!("layout.zone_footer", set_layout_zone_footer);
    set!("layout.widths", set_layout_widths);
    set!("layout.width_left", set_layout_width_left);
    set!("layout.width_right", set_layout_width_right);
    set!("layout.profiles", set_layout_profiles);
    set!("layout.active", set_layout_active);
    set!("layout.name_placeholder", set_layout_name_placeholder);
    set!("layout.save_profile", set_layout_save_profile);
    set!("layout.reset", set_layout_reset);
    set!("layout.server", set_layout_server);
    set!("layout.user", set_layout_user);
    set!("layout.profile_item", set_layout_profiles_item);
    set!("layout.countdown", set_layout_countdown);
    set!("layout.sidebar_actions", set_layout_sidebar_actions);
    set!("layout.news", set_layout_news);
    set!("layout.main_actions", set_layout_main_actions);
    set!("layout.pinned", set_layout_pinned);
    set!("layout.hidden", set_layout_hidden);
    set!("layout.visible", set_layout_visible);
    set!("layout.empty", set_layout_empty);

    set!("mods.title", set_mods_title);
    set!("mods.count", set_mods_count);
    set!("mods.refresh", set_mods_refresh);
    set!("mods.desc", set_mods_desc);
    set!("mods.empty", set_mods_empty);
    set!("mods.search", set_mods_search);
    set!("mods.folder", set_mods_folder);
    set!("mods.not_found", set_mods_not_found);
    set!("mods.download_missing", set_mods_download_missing);
    set!("mods.enable_all", set_mods_enable_all);
    set!("mods.disable_all", set_mods_disable_all);
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
    CURRENT_THEME.with(|value| *value.borrow_mut() = Some(theme.clone()));
}

fn apply_theme_by_name(ui: &AppWindow, name: &str) {
    let item = THEME_CATALOG.with(|catalog| {
        crate::theme::resolve_of_pair(
            ui.get_follow_windows_theme(),
            &ui.get_light_theme(),
            &ui.get_dark_theme(),
            name,
            &catalog.borrow(),
        )
        .clone()
    });

    apply_item(ui, &item, &ui.get_set_theme_mode());
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
    let mut normalized = layout.clone();
    normalized.normalize();
    ui.set_layout_sidebar_left(string_model(&normalized.sidebar_left));
    ui.set_layout_sidebar_right(string_model(&normalized.sidebar_right));
    ui.set_layout_header(string_model(&normalized.header));
    ui.set_layout_content_top(string_model(&normalized.content_top));
    ui.set_layout_content_bottom(string_model(&normalized.content_bottom));
    ui.set_layout_footer(string_model(&normalized.footer));
    let visible = |items: &[String]| -> Vec<String> {
        items
            .iter()
            .filter(|item| !normalized.is_hidden(item))
            .cloned()
            .collect()
    };
    let sidebar_body = |items: &[String]| -> Vec<String> {
        items
            .iter()
            .filter(|item| item.as_str() != "sidebar-actions" && !normalized.is_hidden(item))
            .cloned()
            .collect()
    };

    ui.set_layout_sidebar_left_visible(string_model(&sidebar_body(&normalized.sidebar_left)));
    ui.set_layout_sidebar_right_visible(string_model(&sidebar_body(&normalized.sidebar_right)));
    ui.set_layout_header_visible(string_model(&visible(&normalized.header)));
    ui.set_layout_content_top_visible(string_model(&visible(&normalized.content_top)));
    ui.set_layout_content_bottom_visible(string_model(&visible(&normalized.content_bottom)));
    ui.set_layout_footer_visible(string_model(&visible(&normalized.footer)));

    ui.set_layout_hidden(string_model(&normalized.hidden));
    ui.set_layout_sidebar_left_width(normalized.sidebar_left_width as i32);
    ui.set_layout_sidebar_right_width(normalized.sidebar_right_width as i32);

    let hidden = ui.global::<LayoutState>();
    hidden.set_hidden_user(normalized.is_hidden("user"));
    hidden.set_hidden_profiles(normalized.is_hidden("profiles"));
    hidden.set_hidden_countdown(normalized.is_hidden("countdown"));
    hidden.set_hidden_sidebar_actions(normalized.is_hidden("sidebar-actions"));
    hidden.set_hidden_news(normalized.is_hidden("news"));
    hidden.set_hidden_main_actions(normalized.is_hidden("main-actions"));
    hidden.set_hidden_server(normalized.is_hidden("server"));
    hidden.set_height_user(normalized.block_height("user") as i32);
    hidden.set_height_profiles(normalized.block_height("profiles") as i32);
    hidden.set_height_countdown(normalized.block_height("countdown") as i32);
    hidden.set_height_sidebar_actions(normalized.block_height("sidebar-actions") as i32);
    hidden.set_height_news(normalized.block_height("news") as i32);
    hidden.set_height_main_actions(normalized.block_height("main-actions") as i32);

    ui.set_sidebar_actions_left(
        normalized
            .sidebar_left
            .iter()
            .any(|item| item == "sidebar-actions"),
    );
    ui.set_sidebar_actions_right(
        normalized
            .sidebar_right
            .iter()
            .any(|item| item == "sidebar-actions"),
    );
    ui.set_sidebar_right_has_visible(
        normalized
            .sidebar_right
            .iter()
            .any(|item| !normalized.is_hidden(item)),
    );
}

fn ui_layout(ui: &AppWindow) -> crate::layout::LayoutConfig {
    fn collect(model: &ModelRc<slint::SharedString>) -> Vec<String> {
        model.iter().map(|item| item.to_string()).collect()
    }

    let state = ui.global::<LayoutState>();
    let mut block_heights = std::collections::BTreeMap::new();
    for (item, height) in [
        ("user", state.get_height_user()),
        ("profiles", state.get_height_profiles()),
        ("countdown", state.get_height_countdown()),
        ("sidebar-actions", state.get_height_sidebar_actions()),
        ("news", state.get_height_news()),
        ("main-actions", state.get_height_main_actions()),
    ] {
        block_heights.insert(item.to_string(), height.max(0) as u32);
    }

    crate::layout::LayoutConfig {
        sidebar_left: collect(&ui.get_layout_sidebar_left()),
        sidebar_right: collect(&ui.get_layout_sidebar_right()),
        header: collect(&ui.get_layout_header()),
        content_top: collect(&ui.get_layout_content_top()),
        content_bottom: collect(&ui.get_layout_content_bottom()),
        footer: collect(&ui.get_layout_footer()),
        hidden: collect(&ui.get_layout_hidden()),
        sidebar_left_width: ui.get_layout_sidebar_left_width().max(0) as u32,
        sidebar_right_width: ui.get_layout_sidebar_right_width().max(0) as u32,
        block_heights,
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
    ui.set_set_custom_java(settings.custom_java.clone().into());
    ui.set_set_auto_login(settings.auto_login);
    ui.set_set_send_hwid(settings.send_hwid);
    ui.set_set_sounds(settings.sounds);
    ui.set_set_hide_on_launch(settings.hide_on_launch);
    ui.set_set_discord_rpc(settings.discord_rpc);
    ui.set_set_theme(settings.theme.clone().into());
    ui.set_set_accent(settings.accent.clone().into());
    ui.set_follow_windows_theme(settings.follow_windows_theme);
    ui.set_light_theme(settings.light_theme.clone().into());
    ui.set_dark_theme(settings.dark_theme.clone().into());
    ui.set_set_language(crate::locale::normalize(&settings.language).into());
    ui.set_set_glass_opacity(settings.glass_opacity.clamp(0.25, 0.9));
    ui.set_set_glass_blur(settings.glass_blur.clamp(0.0, 1.0));
    ui.set_set_theme_mode(
        crate::theme::normalize_mode(&settings.theme_mode)
            .to_string()
            .into(),
    );
    apply_layout(ui, &settings.layout);
    LAYOUT_PROFILES.with(|profiles| *profiles.borrow_mut() = settings.layouts.clone());
    ui.set_layout_profiles(string_model(
        &settings
            .layouts
            .iter()
            .map(|profile| profile.name.clone())
            .collect::<Vec<_>>(),
    ));
    ui.set_layout_profile(settings.layout_profile.clone().into());

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
