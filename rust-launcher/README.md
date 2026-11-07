# Vestalis Launcher — Rust rewrite

Native Rust implementation of the Vestalis launcher, kept in its own branch
(`feature/rust-launcher-rewrite`) and its own folder. The C#/Avalonia code in
`src/` is untouched.

## What works

- Sign-in with login/password, 2FA challenge, saved credentials and silent
  token validation (`/api/v1/integrations/auth/signin`, `/checkToken`).
- Official RU/EU backends: every request is signed with
  `X-Client-Signature` (HMAC-SHA256). The key is embedded in the build as
  XOR-obfuscated bytes and is not configurable — the old Settings field was
  removed because a mistyped key silently broke sessions.
- Server selector as a dropdown in the top-right corner (RU-1, RU-2, EU-1).
  PL-1 is always visible but disabled and labelled `W.I.P`
  (`model::PL1_AVAILABLE = false`); a saved PL-1 selection is migrated to RU-2
  on startup. Switching a server keeps the saved credentials and signs in again
  automatically; the branded start screen stays up until the auto-login
  finishes, so the login form never flashes.
- Real SignalR connection to `wss://<host>/ws/launcher?access_token=…`:
  negotiation, JSON handshake, `RefreshProfiles`, `RequestLauncherHash` ↔
  `ConfirmLauncherHash`, keep-alive pings and reconnect with exponential
  backoff. The 90-second poll remains only as a fallback.
- Profiles and news from the backend, with icons, version badges and online
  counters. The sidebar avatar is the player's real skin head (base layer plus
  hat overlay), cropped from `/api/v1/users/info/{name}` → texture endpoint and
  cached locally; the nick initial is only a fallback before the head loads.
- Offline mode using the snapshots in `%APPDATA%\Vestalis\offline-mode`
  (`profiles.json` + `<profile>/profileInfo.json`).
- File validation (SHA-1, size, mtime cache) and parallel downloads with
  progress, speed, ETA, retries, atomic replace and a «Отмена» button that
  interrupts validation/downloads before the game is spawned. The manifest `size` is checked
  before the hash, so a server-side replacement is picked up even if the hash
  is missing; the SHA-1 cache is only trusted while both the local file and
  the expected manifest hash are unchanged.
- Game launch: Java detection, `-Xms`/`-Xmx` injection, custom JVM flags,
  authlib endpoint rewrite for the active server, and removal of the authlib
  agent in offline mode.
- Settings: install folder, RAM, window size/fullscreen, JVM args, HWID
  sending, interface sounds, Discord Rich Presence toggle, language (RU/EN/UA),
  background effect, Liquid Glass opacity/blur imitation, manual accent for
  Material 3, an optional explicit Java executable (with the standard Windows
  file picker), an auto-login toggle and a «Скопировать HWID» action.
- Window size/position is remembered in `window.json` (written periodically and
  on close, clamped to the work area and the 1080×620 minimum); «Сбросить окно»
  centers it again. A named single-instance mutex prevents two launchers from
  overwriting the same state files.
- Maintenance actions: open the install folder, open the logs folder, validate
  the selected profile's files without launching, clear the SHA-1 validation
  cache and export a diagnostics ZIP.
- Mods page: optional mods from `/api/v1/profiles/{name}/mods/optionals`,
  search by name/description, open the mods folder, and enable/disable by
  renaming `.jar` ↔ `.jar.disabled`. Mods that were never
  downloaded get a «Скачать» button and a batch «Скачать отсутствующие»
  action: the file hash is resolved from the profile manifest, the existing
  downloader shows byte progress, retries and stores the new file as
  `.jar.disabled`. «Включить все» / «Выключить все» toggles every downloaded
  mod in one pass. The list updates without a game launch or a launcher
  restart.
- In-app toast notifications for new news (de-duplicated by title/date through
  `seenNews` in `state.json`) and for server-side profile list changes. No
  external notification service is used.
- Rounded UI typography: Nunito (Regular/SemiBold/Bold, Cyrillic included),
  embedded in the binary and used as the default font family.
- Visual themes: built-in `dark`, `light`, `femboy`, `ocean`, `Apple`,
  `Discord`, `Minecraft`, `Nord`, `Dracula`, `Gruvbox` and generated `Windows`
  (from the sampled Windows accent), plus user themes in
  `%APPDATA%\Vestalis\rust-launcher\themes`
  (old `name.json` and new `name/theme.json` layouts). Three presentation
  modes — `base`, `material3` and `liquid-glass` — switch instantly and apply to
  every screen. Material 3 accent priority is manual hex → Windows accent →
  theme accent. Settings can follow the Windows light/dark app mode with a
  separate theme for each, polled every 3 seconds without a restart. Theme
  changes cross-fade over ~280 ms. Liquid Glass uses translucent panels,
  specular edges and blurred colour fields; opacity and imitation strength are
  configurable (real backdrop blur is not available in Slint 1.18). Themes may
  override semantic icons and safe UI strings.
- Main-screen layout editor (Settings → «Layout»): six zones and seven movable
  blocks, drag-and-drop between zones and onto rows (arrow buttons remain as a
  fallback), per-block visibility, sidebar widths (240–420 px), and named layout
  profiles. The active layout is `settings.layout`, presets are
  `settings.layouts`, all persisted in `state.json`.
- Ambient background in Settings → «Фон лаунчера»: «Нет», «Светлячки»,
  «Листья» or «Снежинки». The effects are drawn in Slint and driven by
  `animation-tick()`, so they do not need a Rust timer.
- Interface sounds from the original assets, custom borderless window with
  hand-drawn minimize/maximize/close icons, a pill-style server selector,
  rounded DWM corners and a minimum window size of 1080×620, and an animated
  branded start screen.
  The maximize button animates to the Windows work area, so the taskbar stays
  usable. Screen changes, buttons, profile rows and the download bar animate.
- Discord Rich Presence with the same application ID, assets and texts as the
  C# launcher (`ApiProcedures.UpdateDiscordRpcState`): states «Сидит в
  лаунчере» / «Играет в "profile"», details «Продам гараж…», logo2/logo. The
  game process is awaited in a background thread; when it exits, the status
  returns to «Игра закрыта», Discord presence switches back to the launcher and
  a minimized launcher is restored when «Скрывать лаунчер при запуске игры» is
  enabled.
- Native taskbar/Explorer icon embedded into the executable, plus `WM_SETICON`
  and the same AppUserModelID as the C# launcher. The icon is generated from a
  hand-written vector cat face on a rounded gradient tile (no inner frame, no
  traced square), so it stays readable at 16–32 px.
- The settings tree is pre-built and fades in, so opening it does not wait for
  a first-time layout.
- Home screen countdown card («НЕДРОЧАБРЬ») with the user-supplied meme image
  and a live countdown to 1 December; «Проверить бэкенд» was removed from
  Settings.
- Fullscreen launches use the monitor's native resolution instead of the
  900×600 window size, so the game is not stretched.
- Optional «Скрывать лаунчер при запуске игры» in Settings → Окно игры
  (minimizes the launcher once the game process starts).
- Profiles are refreshed instantly by SignalR `RefreshProfiles`, with the 90 s
  poll only as a fallback. Neither path logs the player out, so a transient
  backend error cannot rotate the token and invalidate a running game session;
  the token is validated once at startup.
- Backend status badge (`ОНЛАЙН` / `ОФФЛАЙН`).
- Interface language RU/EN/UA with a settings switch; theme text overrides keep
  priority over the built-in translation within the allowlist.
- Diagnostic export (Settings → «Экспорт диагностики»): ZIP with logs, version,
  themes and sanitized state. Tokens, passwords, saved credentials and secrets
  are removed.
- `state.json` is written atomically and rotated to `.bak`; a partially broken
  file recovers every valid field instead of resetting all settings. Avatar and
  theme-icon caches use a 30-day TTL and file-count caps with atomic writes.

## Requirements (Windows)

```powershell
winget install --id Rustlang.Rustup -e
rustup default stable
rustup component add rustfmt clippy

# Linker: Visual Studio Build Tools with "Desktop development with C++"
# (or an existing VS installation with the VC tools workload).
```

## Build and run

```powershell
cd rust-launcher
cargo run                 # debug
cargo run -- --offline    # cached profiles, no backend
cargo build --release
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

`Cargo.lock` is committed because this is a binary crate.

## Layout

```
rust-launcher/
├─ ui/app.slint         Slint UI (login, main, settings)
├─ src/
│  ├─ main.rs           composition root, Slint callbacks, event pump
│  ├─ api.rs            reqwest backend client, auth, profiles, news
│  ├─ signalr.rs        minimal SignalR JSON client over WebSocket
│  ├─ worker.rs         async worker: commands, auth flow, launch orchestration
│  ├─ files.rs          manifest validation + parallel downloads
│  ├─ locale.rs         built-in RU/EN/UA interface strings
│  ├─ diagnostics.rs    sanitized support ZIP
│  ├─ launch.rs         JVM argument rewriting and process spawn
│  ├─ discord.rs        minimal Discord IPC rich presence
│  ├─ window.rs         Win32 work-area maximize for the borderless window
│  ├─ storage.rs        state.json + validation cache (legacy C# cache import)
│  ├─ theme.rs          theme tokens, built-ins and user theme loading
│  ├─ hwid.rs           Windows HWID, RAM, OS info
│  └─ model.rs          settings, session, API DTOs
└─ assets/              logo, background, meme, fonts/Nunito-*
```

Threading model: Slint owns the main thread; a Tokio multi-thread runtime runs
the worker. The UI sends `Command`s through an unbounded channel; the worker
sends `Event`s back and a 40 ms `slint::Timer` applies them on the UI thread.
No blocking I/O happens in a Slint callback.

## Data paths

| What | Where |
| --- | --- |
| Settings/session | `%APPDATA%\Vestalis\rust-launcher\state.json` (+ `.bak`) |
| Window size/position | `%APPDATA%\Vestalis\rust-launcher\window.json` |
| Game files | `%APPDATA%\Vestalis` (same as the C# launcher) |
| SHA-1 cache | `%LOCALAPPDATA%\Vestalis\validation-cache-rust.json` |
| Player head avatars | `%LOCALAPPDATA%\Vestalis\avatars\<uuid>.png` |
| Logs | `%LOCALAPPDATA%\Vestalis\logs\launcher.log.<date>` |
| Offline snapshots | `%APPDATA%\Vestalis\offline-mode\...` |
| Theme icon cache | `%LOCALAPPDATA%\Vestalis\theme-icons` |
| Diagnostics ZIP | Desktop, or `%LOCALAPPDATA%\Vestalis` when Desktop is absent |

The first validation imports `%LOCALAPPDATA%\Gml.Launcher\validation-cache-v1.json`,
so an existing C# installation does not re-hash its whole 16k-file client.

## Themes and modes

Built-in palettes live in `src/theme.rs`: `dark`, `light`, `femboy`, `ocean`,
`Apple`, `Discord`, `Minecraft`, `Nord`, `Dracula`, `Gruvbox` and `Windows`
(generated from the current Windows accent). The palette is stored as `settings.theme` and the presentation
mode as `settings.themeMode` in `state.json`. Settings → «Оформление» switches
both live; «Сохранить настройки» persists the choice. A manual accent
(`settings.accent`) overrides the Windows accent for Material 3.

Modes:

- `base` — the classic flat launcher surfaces.
- `material3` — tonal surfaces, larger radii, elevation shadows and Material 3
  state layers. The primary/tonal palette is generated in Rust from the exact
  Windows accent returned by WinRT `UISettings.GetColorValue(Accent)`, with a
  registry fallback (`AccentColor` ABGR, `ColorizationColor` ARGB and the
  Explorer `AccentPalette`; the most chromatic candidate wins). A gray or black
  accent is still a valid Windows setting and is used as-is; the palette accent
  is only a fallback when Windows exposes no value at all. «Тема Windows»
  reports the sampled hex. While Material 3 is active the accent is also
  polled every 20 seconds.
- `liquid-glass` — translucent panels, gradient specular edges, soft blooms
  behind the UI and larger rounded corners. Slint 1.18 has no real backdrop
  blur, so this is an approximation made of layered translucency, gradients
  and blurred colour fields; no network or GPU shader is involved. Opacity
  (25–90%) and imitation strength (0–100%) are configurable in Settings.

Modes are presentation layers over the selected palette. A user theme may set
a default `mode`; selecting the theme switches to it, and the mode buttons can
override it afterwards.

### User themes

Two layouts are supported:

- legacy: `%APPDATA%\Vestalis\rust-launcher\themes\name.json`
- folder: `%APPDATA%\Vestalis\rust-launcher\themes\name\theme.json` plus
  `icons/…` assets next to it

```json
{
  "name": "neon",
  "mode": "material3",
  "theme": {
    "background": "#071016",
    "backgroundAlt": "#03080c",
    "surface": "#0d1a22",
    "surfaceHover": "#152833",
    "border": "#23414f",
    "text": "#eaffff",
    "textMuted": "#9fc9d4",
    "accent": "#22d3ee",
    "accentHover": "#67e8f9",
    "accentText": "#041216",
    "gradientStart": "#06b6d4",
    "gradientEnd": "#d946ef",
    "cardOpacity": 0.9,
    "backgroundEffect": "fireflies"
  },
  "icons": {
    "play": "icons/play.svg",
    "server": "icons/server.svg",
    "close": "icons/close.svg"
  },
  "texts": {
    "app.title": "Vestalis Neon",
    "buttons.play": "ПОГНАЛИ",
    "sections.news": "НОВОСТИ СЕРВЕРА"
  }
}
```

Rules:

- Every theme token is optional and falls back to `dark`; every mode has
  complete defaults (M3/glass roles are generated even if the JSON omits them).
- Colors accept `#RRGGBB` or `#RRGGBBAA`. An invalid token is replaced by the
  `dark` value and logged; the rest of the theme still loads.
- `radius*` are logical pixels (0–64, pill 1–999), `cardOpacity` is clamped to
  0–1 and `baseFontWeight` to 100–900. `backgroundEffect` is `none`,
  `fireflies`, `leaves` or `snow`.
- Icons use semantic IDs: `server`, `chevron`, `play`, `settings`, `mods`,
  `refresh`, `minimize`, `maximize`, `restore`, `close`, `warning`, `error`,
  `success`, `download`, `folder`, `user`. Only SVG/PNG files up to 512 KiB,
  relative to the theme folder, are accepted. Absolute paths, `..`, symlinks
  and paths resolving outside the theme folder are rejected. Missing or broken
  icons fall back to the built-in vector icon. SVG files containing
  `currentColor` are tinted with the active theme colour and cached with a
  30-day TTL / 500-file cap in `%LOCALAPPDATA%\Vestalis\theme-icons`.
- Themeable text keys cover the main labels, settings cards, layout editor,
  language switcher and mods page (see `TEXT_KEYS` in `src/theme.rs`). The
  full list intentionally excludes error messages, warning details and
  window-control labels. Unknown keys are logged and ignored. Theme overrides
  take priority over the selected interface language.
- Only regular files directly inside the themes folder (or one level of theme
  folders) are read; themes are capped at 256 KiB. Invalid JSON, duplicate
  names and unreadable files are logged and skipped. A UTF-8 BOM is accepted.
- Unit tests enforce ≥ 4.5:1 contrast for `text` on `background`/`surface` for
  all built-ins and for generated Material 3 palettes, and ≥ 4.5:1 for
  `accentText` on `accent`.

## Layout editor

`settings.layout` is independent from the theme. The main screen is split into
six zones: `sidebarLeft`, `sidebarRight`, `header`, `contentTop`,
`contentBottom`, `footer`. Movable blocks are `user`, `profiles`, `countdown`,
`sidebar-actions`, `news` and `main-actions`; the server selector is pinned to
the header because the title bar owns its dropdown. Settings → «Layout» shows
each zone and supports drag-and-drop between zones and onto a specific row
(`DragArea`/`DropArea`, payload prepared from the hovered row), with ←/↑/↓/→
buttons as a fallback. Hidden blocks are removed from the main-screen layout
(not just made invisible), so they do not reserve height or spacing; their
configured height is kept and restored when the block is shown again. Block
heights are editable with −/+ in the editor, clamped to per-block min/max
values (for example 96–240 px for the countdown and 120–520 px for profiles).
Sidebar widths are clamped to 240–420 px. `sidebar-actions` is pinned to the
bottom of its sidebar: the other blocks live in a ScrollView above it, so the
buttons stay reachable when heights grow or blocks are hidden. An empty right
sidebar disappears completely. Named profiles are stored in `settings.layouts`,
the active one in `settings.layout` / `settings.layoutProfile`. Every change is
normalised (unknown/duplicate items removed, out-of-range heights clamped,
missing blocks returned to their defaults), saved immediately and applied
without a restart. There is no free canvas, which keeps the 1080×620 minimum
size and the existing scrolling/clicks intact.

## Backend notes

- The auth endpoints require `X-HWID` on every request. When the HWID toggle
  is off, a fresh random identifier is generated per request.
- Official hosts require a valid `X-Client-Signature`:
  `HMAC-SHA256(secret, "{unixSeconds}:{guidN}:{hwid}:Vestalis")`, sent as
  `unixSeconds:guidN:hexDigest`. The project key is embedded in the binary, so
  the host normally answers `200`. A `403 «Требуется запуск через официальный
  лаунчер»` is shown as-is: the old silent fallback to PL-1 is gone because a
  mirror-issued token cannot join a game server (`join` returns `401`, the
  server sees `204` from `hasJoined`, and Minecraft reports an invalid
  session). PL-1 stays visible in the dropdown only as a disabled `W.I.P`
  entry; selecting it does nothing.
- `OsArchitecture` must be `"64"` (not `"x64"`): the backend returns an empty
  profile with `state=Error` for `"x64"` but the full launch manifest for `"64"`.
- RU-2 is the default host. PL-1 mirror-control endpoints (`/api/v1/mirror/*`)
  and the service-access card were removed from the Rust launcher.
- The API speaks camelCase JSON; request bodies for sign-in keep the PascalCase
  names used by the C# client.
- The hub is negotiated at `/ws/launcher/negotiate`; the token is passed as the
  `access_token` query parameter, exactly like the C# client.

## Known gaps / next steps

1. Skin marketplace/page (the head avatar itself is already rendered) and Sentry.
2. The exact splash-to-window morph of the Avalonia build (the Rust build uses
   an animated branded start overlay).
3. Installer and code signing.
4. Background error/status strings from the backend stay in the language the
   backend returns; static launcher strings are localized in `src/locale.rs`.
