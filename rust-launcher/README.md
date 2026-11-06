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
  PL-1 is temporarily hidden (`model::PL1_AVAILABLE = false`) because the mirror
  backend is offline; a saved PL-1 selection is migrated to RU-2 on startup.
  Switching a server keeps the saved credentials and signs in again
  automatically; the branded start screen stays up until the auto-login
  finishes, so the login form never flashes.
- Profiles and news from the backend, with icons, version badges and online
  counters. The sidebar avatar is the player's real skin head (base layer plus
  hat overlay), cropped from `/api/v1/users/info/{name}` → texture endpoint and
  cached locally; the nick initial is only a fallback before the head loads.
- Offline mode using the snapshots in `%APPDATA%\Vestalis\offline-mode`
  (`profiles.json` + `<profile>/profileInfo.json`).
- File validation (SHA-1, size, mtime cache) and parallel downloads with
  progress, speed, retries and atomic replace. The manifest `size` is checked
  before the hash, so a server-side replacement is picked up even if the hash
  is missing; the SHA-1 cache is only trusted while both the local file and
  the expected manifest hash are unchanged.
- Game launch: Java detection, `-Xms`/`-Xmx` injection, custom JVM flags,
  authlib endpoint rewrite for the active server, and removal of the authlib
  agent in offline mode.
- Settings: install folder, RAM, window size/fullscreen, JVM args, HWID
  sending, interface sounds.
- Service access card (**PL-1 only, and only for the Kydasay account**):
  service key, mirror/proxy switching, Tor routing and backend self-test
  (`/api/v1/mirror/*`).
- Mods page: optional mods from `/api/v1/profiles/{name}/mods/optionals`,
  enable/disable by renaming `.jar` ↔ `.jar.disabled`.
- Rounded UI typography: Nunito (Regular/SemiBold/Bold, Cyrillic included),
  embedded in the binary and used as the default font family.
- Visual themes: built-in `dark`, `light`, `femboy`, `ocean` plus user themes
  in `%APPDATA%\Vestalis\rust-launcher\themes` (old `name.json` and new
  `name/theme.json` layouts). Three presentation modes — `base`, `material3`
  and `liquid-glass` — switch instantly and apply to every screen. Material 3
  derives its tonal palette from the Windows accent (with a theme fallback);
  Liquid Glass uses translucent panels, specular edges and blurred colour
  fields. Themes may override semantic icons and safe UI strings.
- Main-screen layout editor (Settings → «Layout»): six zones and seven movable
  blocks, persisted separately as `settings.layout` in `state.json`.
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
  лаунчере» / «Играет в "profile"», details «Продам гараж…», logo2/logo.
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
- Profiles are refreshed every 90 s and asynchronously after the first load
  (a lightweight substitute for the C# SignalR `RefreshProfiles` event). The
  periodic refresh never logs the player out, so a transient backend error
  cannot rotate the token and invalidate a running game session; the token is
  validated once at startup.
- Backend mode badge (`ЗЕРКАЛО` / `ПРОКСИ` / `ОСНОВНОЙ` / `ОНЛАЙН` / `ОФФЛАЙН`).

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
│  ├─ api.rs            reqwest backend client, auth, profiles, news, mirror
│  ├─ worker.rs         async worker: commands, auth flow, launch orchestration
│  ├─ files.rs          manifest validation + parallel downloads
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
| Settings/session | `%APPDATA%\Vestalis\rust-launcher\state.json` |
| Game files | `%APPDATA%\Vestalis` (same as the C# launcher) |
| SHA-1 cache | `%LOCALAPPDATA%\Vestalis\validation-cache-rust.json` |
| Player head avatars | `%LOCALAPPDATA%\Vestalis\avatars\<uuid>.png` |
| Logs | `%LOCALAPPDATA%\Vestalis\logs\launcher.log.<date>` |
| Offline snapshots | `%APPDATA%\Vestalis\offline-mode\...` |

The first validation imports `%LOCALAPPDATA%\Gml.Launcher\validation-cache-v1.json`,
so an existing C# installation does not re-hash its whole 16k-file client.

## Themes and modes

Built-in palettes live in `src/theme.rs`: `dark`, `light`, `femboy`, `ocean`.
The palette is stored as `settings.theme` and the presentation mode as
`settings.themeMode` in `state.json`. Settings → «Оформление» switches both
live; «Сохранить настройки» persists the choice.

Modes:

- `base` — the classic flat launcher surfaces.
- `material3` — tonal surfaces, larger radii, elevation shadows and Material 3
  state layers. The primary/tonal palette is generated in Rust from the Windows
  accent colour (`HKCU\Software\Microsoft\Windows\DWM\AccentColor`, ABGR,
  falling back to `ColorizationColor`, ARGB). If neither is usable, the
  palette accent is used. «Взять цвет Windows» refreshes it manually; while
  Material 3 is active the accent is also polled every 20 seconds.
- `liquid-glass` — translucent panels, gradient specular edges, soft blooms
  behind the UI and larger rounded corners. Slint 1.18 has no real backdrop
  blur, so this is an approximation made of layered translucency, gradients
  and blurred colour fields; no network or GPU shader is involved.

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
  `success`. Only SVG/PNG files up to 512 KiB, relative to the theme folder,
  are accepted. Absolute paths, `..`, symlinks and paths resolving outside the
  theme folder are rejected. Missing or broken icons fall back to the built-in
  vector icon. SVG files containing `currentColor` are tinted with the active
  theme colour and cached in `%LOCALAPPDATA%\Vestalis\theme-icons`.
- Themeable text keys are limited to `app.title`, `app.subtitle`,
  `login.title`, `login.subtitle`, `login.submit`, `login.offline`,
  `buttons.play`, `buttons.settings`, `buttons.logout`, `buttons.refresh`,
  `buttons.mods`, `sections.profiles`, `sections.news`, `settings.title` and
  `settings.back`. Unknown keys are logged and ignored. Error messages, warning
  details and window-control labels are deliberately not themeable.
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
each zone and moves blocks with ←/↑/↓/→ buttons. Every move is normalised
(unknown/duplicate items removed, missing blocks returned to their defaults),
saved immediately and applied without a restart. There is no free canvas, which
keeps the 1080×620 minimum size and the existing scrolling/clicks intact.

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
  session). Selecting PL-1 explicitly is still possible, but for browsing only.
- `OsArchitecture` must be `"64"` (not `"x64"`): the backend returns an empty
  profile with `state=Error` for `"x64"` but the full launch manifest for `"64"`.
- PL-1 accepts the same API and is the default host. The client-signature key
  is launcher-wide, not per server.
- The API speaks camelCase JSON; request bodies for sign-in keep the PascalCase
  names used by the C# client.
- Mirror control is intentionally limited to PL-1: RU/EU answer `404` for
  `/api/v1/mirror/*`, so the card is hidden for them in the UI and guarded in
  the worker.

## Known gaps / next steps

1. True SignalR connection (the 90 s refresh is a lightweight substitute).
2. Localisation (ru/en/zh): the Rust UI currently ships Russian strings.
3. Skin marketplace/page (the head avatar itself is already rendered) and Sentry.
4. The exact splash-to-window morph of the Avalonia build (the Rust build uses
   an animated branded start overlay).
5. Installer and code signing.
6. Optional mods that are not downloaded yet: the mods page asks the user to
   launch the game once instead of downloading the missing file on the spot.
