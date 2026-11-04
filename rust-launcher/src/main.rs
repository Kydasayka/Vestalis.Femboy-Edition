//! Vestalis Launcher — Rust rewrite.
//!
//! The UI lives in `ui/app.slint`; this file is the composition root. It owns
//! the Slint event loop, starts the Tokio worker and translates worker events
//! into UI properties. All blocking work happens off the UI thread.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api;
mod files;
mod hwid;
mod launch;
mod model;
mod storage;
mod worker;

slint::include_modules!();

use std::time::Duration;

use base64::Engine;
use slint::{ModelRc, VecModel};
use tracing::{error, info};

use crate::model::{Persisted, Settings};
use crate::storage::load_state;
use crate::worker::{Command, Event, ProfileView, Worker};

fn main() -> anyhow::Result<()> {
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

    let state = load_state();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let (command_tx, command_rx) = tokio::sync::mpsc::unbounded_channel();
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();

    let ui = AppWindow::new()?;
    initialize_ui(&ui, &state);
    install_callbacks(&ui, command_tx.clone());

    runtime.spawn(Worker::new(state, event_tx).run(command_rx));

    let splash_ui = ui.as_weak();
    slint::Timer::single_shot(Duration::from_millis(1400), move || {
        if let Some(ui) = splash_ui.upgrade() {
            ui.set_splash(false);
        }
    });

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
    ui.set_server_index(model::server_index(&state.server_host) as i32);
    ui.set_server_host_label(state.server_host.clone().into());

    if let Some(saved) = &state.saved_login {
        ui.set_login(saved.login.clone().into());
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
                ui.set_screen(2);
            }
            let _ = tx.send(Command::RefreshMirror);
        }
    });

    ui.on_back_clicked({
        let ui = ui.as_weak();
        move || {
            if let Some(ui) = ui.upgrade() {
                play_sound(&ui, Sound::Click);
                ui.set_screen(1);
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
                ui.set_server_host_label(
                    model::SERVERS
                        .get(index as usize)
                        .map(|(_, host)| (*host).to_string())
                        .unwrap_or_default()
                        .into(),
                );
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
                install_dir: ui.get_set_install_dir().to_string(),
            };

            ui.set_settings_status("Сохранение…".into());
            let _ = tx.send(Command::SaveSettings(settings));
        }
    });

    ui.on_check_backend_clicked({
        let tx = tx.clone();
        let ui = ui.as_weak();
        move || {
            if let Some(ui) = ui.upgrade() {
                play_sound(&ui, Sound::Click);
            }
            let _ = tx.send(Command::CheckBackend);
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
}

fn apply_event(ui: &AppWindow, event: Event) {
    match event {
        Event::Screen(screen) => ui.set_screen(screen),
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
        }
        Event::Settings { settings, max_ram } => {
            apply_settings(ui, &settings, max_ram);
        }
        Event::SettingsStatus(status) => ui.set_settings_status(status.into()),
        Event::MainStatus(status) => ui.set_main_status(status.into()),
        Event::Error(message) => {
            if !message.is_empty() {
                error!(%message, "worker error");
            }
            ui.set_error_text(message.into());
        }
        Event::LoggedOut => {
            ui.set_screen(0);
            ui.set_user_name("".into());
            ui.set_user_initial("?".into());
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

fn select_view(ui: &AppWindow, view: &ProfileView) {
    ui.set_selected_index(view.id as i32);
    ui.set_selected_title(view.display_name.clone().into());
    ui.set_selected_description(view.description.clone().into());
    ui.set_selected_version(view.subtitle.clone().into());
    ui.set_selected_online(view.online.clone().into());
    ui.set_selected_icon(decode_icon(view.icon.as_deref()));
    ui.set_can_play(true);
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
