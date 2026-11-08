# Vestalis Femboy edition — Rust launcher

**Vestalis Femboy edition** is a community fork of the **Vestalis launcher**, which in turn is a
fork of [Gml Launcher (GamerVII Minecraft Launcher)](https://github.com/GamerVII-NET/minecraft-launcher).
This branch ships a native **Rust + Slint** implementation in `rust-launcher/`; the original .NET 8
and Avalonia launcher remains in `src/` and is left untouched.

Lineage: **GML Launcher** (GamerVII-NET) → **Vestalis launcher** → **Vestalis Femboy edition** (this repo).

This repository is neither the GML Launcher nor the official Vestalis launcher. The Rust build is
an independent, experimental implementation: it may behave differently from the official release,
and no changelog of differences is maintained here.

> [!WARNING]
> **Vibecoded.** Large parts of this edition were written with AI assistance and reviewed only
> lightly. Expect rough edges, unfinished features and regressions. Use it at your own risk, and
> do not treat it as a stable or supported product.

> [!WARNING]
> **The PL1 server (`201-58-180-2.sslip.io`) is not an official Vestalis server.** It is a
> third-party/experimental host configured in this build. It is not run or endorsed by the Vestalis
> team, comes with no guarantees of uptime, data safety or moderation, and connecting to it is your
> own decision. In the Rust build PL-1 is shown as a disabled `W.I.P` entry.

## About this edition

- Native Rust rewrite in `rust-launcher/`, using Slint for the UI. The C#/Avalonia launcher in
  `src/` is still present and is not modified by this branch.
- Standalone executable: no .NET runtime is required to run the Rust launcher.
- Uses the same backend API and the same game/data folders as the C# launcher, and imports the
  existing `%LOCALAPPDATA%\Gml.Launcher\validation-cache-v1.json` on the first validation.
- Not an official Vestalis release; for a supported launcher use the official one.
- Bugs found here should not be reported to the Vestalis team or to GML Launcher.

## What's new in this edition

- **Native Rust + Slint UI** — no .NET runtime, custom borderless window with DWM rounding,
  its own caption buttons and a minimum size of 1080×620.
- **Authentication** — login/password, 2FA, saved credentials, auto-login, silent token validation,
  real HWID and `X-Client-Signature` for the official RU/EU hosts.
- **Profiles and news** — backend profiles, version badges, online counters, personal profile
  backgrounds on hover/selection, backend news icons, and the player's real skin head as the
  sidebar avatar. Images are downloaded and cached locally.
- **File sync** — SHA-1 validation with a size/mtime cache, parallel downloads, progress, speed,
  ETA, retries, atomic replace and a working «Отмена» button.
- **Game launch** — Java detection, `-Xms`/`-Xmx`, custom JVM arguments, authlib endpoint rewrite,
  native fullscreen resolution, game-exit tracking and optional launcher minimization.
- **Mods page** — optional mods from the backend, search, download of missing mods, bulk
  enable/disable and `.jar` ↔ `.jar.disabled` switching without a restart.
- **Themes** — `dark`, `light`, `femboy`, `ocean`, `Apple`, `Discord`, `Minecraft`, `Nord`,
  `Dracula`, `Gruvbox`, Catppuccin Mocha/Macchiato/Frappe/Latte and a generated Windows accent
  theme. Four presentation modes: `base`, `material3`, `liquid-glass` and `acrylic` (the last two
  are imitations, because Slint 1.18 has no backdrop blur). User themes can override colours,
  safe UI strings and semantic icons.
- **Colour picker** — built-in HSV picker with recent colours and a native screen eyedropper.
- **Layout editor** — six zones and seven movable blocks, drag-and-drop, per-block visibility and
  heights, resizable sidebars and named layout presets.
- **Settings** — auto-save with validation (no Save button), install folder, RAM, resolution,
  JVM args, HWID toggle, sounds, Discord RPC, language, background effects, Java path and
  maintenance actions.
- **First-run onboarding** — language, theme/mode, telemetry consent and install/Java check;
  it can be skipped and restarted from Settings.
- **Optional telemetry** — Sentry, off by default, DSN supplied only through environment
  variables. Events are sanitized (no tokens, passwords, HWID, logins or user paths).
- **Quality of life** — SignalR profile refreshes, toast notifications, RU/EN/UA localization,
  interface sounds, Discord Rich Presence, single-instance guard, remembered window geometry and
  a sanitized diagnostics ZIP.
- **Smooth UI** — fading screen transitions, animated buttons/rows, locally cached images and
  profile/news models that are rebuilt only when the backend data actually changes.

## Requirements

- **Windows 10/11 x64**.
- **Rust stable** — <https://rustup.rs/>
- **Visual Studio Build Tools** with the **Desktop development with C++** workload (the MSVC
  linker is required; an existing Visual Studio installation with the VC tools works too).
- **Git** — <https://git-scm.com/downloads>
- The .NET 8 SDK is **not** required for the Rust build.

```powershell
winget install --id Rustlang.Rustup -e
rustup default stable
rustup component add rustfmt clippy
```

## Build from source

```powershell
git clone <this-repo-url>
cd Vestalis.Femboy-Edition\rust-launcher

cargo build --release
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

`Cargo.lock` is committed because this is a binary crate. Submodules are only needed for the C#
launcher, not for the Rust build.

## Run

```powershell
cd rust-launcher
cargo run                 # debug
cargo run -- --offline    # cached profiles, no backend
```

Or start the release build directly:

```powershell
Start-Process 'rust-launcher\target\release\vestalis-launcher.exe'
```

The branded start screen stays up until the saved session and profile data are ready, so the login
form does not flash on startup.

### Portable build

```powershell
cd rust-launcher
cargo build --release
Copy-Item target\release\vestalis-launcher.exe ..\dist\rust\vestalis-launcher.exe -Force
```

The result is a single `dist\rust\vestalis-launcher.exe` (ignored by git). Fonts, icons, the logo
and the background are embedded into the binary; no side-by-side asset folder is required.

## Configuration

Servers are defined in `rust-launcher/src/model.rs` (`RU-1`, `RU-2`, `EU-1`, `PL-1`). RU-2 is the
default host. PL-1 is a read-only third-party mirror and is disabled in the selector
(`model::PL1_AVAILABLE = false`); a saved PL-1 selection is migrated to RU-2 on startup.

The official RU/EU hosts require `X-HWID` and `X-Client-Signature`
(`HMAC-SHA256(secret, "{unixSeconds}:{guidN}:{hwid}:Vestalis")`, sent as
`unixSeconds:guidN:hexDigest`). The project key is embedded in the binary in obfuscated form and is
not configurable. A `403 «Требуется запуск через официальный лаунчер»` is shown as-is; the old
silent fallback to PL-1 was removed because a mirror token cannot join a game server.

Local data:

| What | Where |
| --- | --- |
| Settings/session | `%APPDATA%\Vestalis\rust-launcher\state.json` (+ `.bak`) |
| Window size/position | `%APPDATA%\Vestalis\rust-launcher\window.json` |
| Game files | `%APPDATA%\Vestalis` (same as the C# launcher) |
| Offline snapshots | `%APPDATA%\Vestalis\offline-mode\...` |
| User themes | `%APPDATA%\Vestalis\rust-launcher\themes` |
| SHA-1 cache | `%LOCALAPPDATA%\Vestalis\validation-cache-rust.json` |
| Player avatars | `%LOCALAPPDATA%\Vestalis\avatars\<uuid>.png` |
| News icons | `%LOCALAPPDATA%\Vestalis\news-icons` |
| Profile backgrounds | `%LOCALAPPDATA%\Vestalis\profile-backgrounds` |
| Theme icon cache | `%LOCALAPPDATA%\Vestalis\theme-icons` |
| Logs | `%LOCALAPPDATA%\Vestalis\logs\launcher.log.<date>` |
| Diagnostics ZIP | Desktop, or `%LOCALAPPDATA%\Vestalis` when Desktop is absent |

`state.json` is written atomically and backed up to `.bak`; a partially broken file recovers every
valid field instead of resetting all settings. The first validation imports the C# cache
`%LOCALAPPDATA%\Gml.Launcher\validation-cache-v1.json`, so an existing installation does not
re-hash its whole client.

### Themes and modes

Built-in palettes live in `rust-launcher/src/theme.rs`; the active theme is `settings.theme` and the
presentation mode is `settings.themeMode`. User themes can use either
`themes\name.json` or `themes\name\theme.json` with an `icons/` folder next to it. Theme fields are
optional, colours accept `#RRGGBB`/`#RRGGBBAA`, and invalid values fall back to `dark` with a log
entry. Icons use semantic IDs (`play`, `settings`, `mods`, `close`, …), only SVG/PNG files up to
512 KiB inside the theme folder are accepted, and icon paths cannot escape that folder.

Modes are presentation layers over the selected palette:

- `base` — flat launcher surfaces.
- `material3` — tonal surfaces, elevation shadows and state layers. The palette is generated from
  the exact Windows accent (WinRT `UISettings.GetColorValue(Accent)`, with registry fallbacks).
- `liquid-glass` — translucent panels, specular edges and blurred colour fields; an imitation,
  because Slint 1.18 has no backdrop blur. Opacity and strength are configurable.
- `acrylic` — denser translucency, a bright rim, a deep shadow and a deterministic grain overlay.
  Also an imitation, using the same opacity/blur controls.

### Layout editor

`settings.layout` is independent from the theme. The main screen has six zones
(`sidebarLeft`, `sidebarRight`, `header`, `contentTop`, `contentBottom`, `footer`) and seven movable
blocks (`user`, `profiles`, `countdown`, `sidebar-actions`, `news`, `main-actions`; the server
selector is pinned to the header). Settings → «Layout» supports drag-and-drop with arrow-button
fallback, per-block visibility, per-block heights and sidebar widths (240–420 px). Hidden blocks
do not reserve space. Named presets are stored in `settings.layouts`; changes are normalised,
saved immediately and applied without a restart.

### Backend notes

- Every auth request carries `X-HWID`. With the HWID toggle off, a fresh random identifier is
  generated per request; with it on, the real hardware ID is sent.
- `OsArchitecture` must be `"64"` (not `"x64"`): the backend returns an empty profile for
  `"x64"` and the full manifest for `"64"`.
- SignalR is negotiated at `/ws/launcher/negotiate`; the token is passed as the `access_token`
  query parameter. `RefreshProfiles` updates the list instantly, and a 90-second poll remains as a
  fallback.
- The API speaks camelCase JSON; sign-in request bodies keep the PascalCase names used by the C#
  client.
- The mirror-control endpoints (`/api/v1/mirror/*`) and the service-access card are not part of the
  Rust launcher.

## Project structure

| Path | Contents |
| --- | --- |
| `rust-launcher/ui/app.slint` | Slint UI: login, main, settings, mods, onboarding, themes |
| `rust-launcher/src/main.rs` | Composition root, Slint callbacks and event pump |
| `rust-launcher/src/worker.rs` | Async worker: commands, auth, profiles, mods, launch flow |
| `rust-launcher/src/api.rs` | `reqwest` backend client, auth, profiles, news |
| `rust-launcher/src/files.rs` | Manifest validation and parallel downloads |
| `rust-launcher/src/launch.rs` | JVM argument rewriting and process spawn |
| `rust-launcher/src/signalr.rs` | Minimal SignalR JSON client over WebSocket |
| `rust-launcher/src/theme.rs` | Theme tokens, built-ins, user themes, Material 3 |
| `rust-launcher/src/layout.rs` | Main-screen layout zones, presets and normalisation |
| `rust-launcher/src/locale.rs` | Built-in RU/EN/UA strings |
| `rust-launcher/src/telemetry.rs` | Optional Sentry client and event sanitizer |
| `rust-launcher/src/diagnostics.rs` | Sanitized support ZIP |
| `rust-launcher/src/screen_picker.rs` | Native GDI screen eyedropper |
| `rust-launcher/src/window.rs` | Win32 window, DWM corners, single-instance guard |
| `rust-launcher/src/discord.rs` | Discord IPC rich presence |
| `rust-launcher/src/storage.rs` | `state.json`, backups and validation cache |
| `rust-launcher/src/hwid.rs` | Windows HWID, RAM and OS information |
| `rust-launcher/src/model.rs` | Settings, session, API DTOs and server list |
| `rust-launcher/assets/` | Logo, meme image, Nunito fonts and icons |
| `src/` | Original C#/Avalonia launcher (not part of the Rust build) |
| `dist/` | Local build/publish output (ignored by git) |

Threading model: Slint owns the main thread, a Tokio multi-thread runtime runs the worker, and
commands/events travel through channels. No blocking I/O happens in a Slint callback.

See [AGENTS.md](AGENTS.md) for a detailed code map, build notes and known pitfalls. The full
technical reference for the Rust build lives in [rust-launcher/README.md](rust-launcher/README.md).

## Authors

**Vestalis Femboy edition**

- **Kydasayka** — fork maintenance, Rust rewrite (this edition)
- **Katran** — help, consulting and feedback (thank you!)

**Upstream / heritage**

- **Vestalis team** — original Vestalis launcher (direct base of this fork)
- **GamerVII** and the [Gml Launcher contributors](https://github.com/GamerVII-NET/minecraft-launcher)
  — original GML Launcher
- [GamerVII.Notification.Avalonia](https://github.com/GamerVII-NET/GamerVII.Notification.Avalonia)
  contributors — notification library
- **Slint** — UI toolkit used by the Rust launcher
- **Nunito** — UI font, licensed under the SIL Open Font License

## Credits and license

This project is a derivative work: GML Launcher by GamerVII-NET → Vestalis launcher → Vestalis
Femboy edition. It keeps the architecture and large parts of the source code of both parents. The
Rust launcher is an additional original implementation of the same client behaviour. No separate
license file is shipped with this edition; refer to the upstream repositories for licensing terms
and attribution requirements, and to the bundled font/toolkit licenses for their respective terms.
