# Vestalis Launcher — Rust rewrite

Native Rust implementation of the Vestalis launcher, kept in its own branch
(`feature/rust-launcher-rewrite`) and its own folder. The C#/Avalonia code in
`src/` is untouched.

## What works

- Sign-in with login/password, 2FA challenge, saved credentials and silent
  token validation (`/api/v1/integrations/auth/signin`, `/checkToken`).
- Official RU/EU backends: every request is signed with
  `X-Client-Signature` (HMAC-SHA256). The key is **not** in the repository; it
  is entered in Settings → «Ключ подписи клиента» and stored locally in
  `state.json`.
- Server selector (RU-1, RU-2, EU-1, PL-1). Switching a server drops the
  session, exactly like the C# launcher.
- Profiles and news from the backend, with icons, version badges and online
  counters.
- Offline mode using the snapshots in `%APPDATA%\Vestalis\offline-mode`
  (`profiles.json` + `<profile>/profileInfo.json`).
- File validation (SHA-1, size, mtime cache) and parallel downloads with
  progress, speed, retries and atomic replace.
- Game launch: Java detection, `-Xms`/`-Xmx` injection, custom JVM flags,
  authlib endpoint rewrite for the active server, and removal of the authlib
  agent in offline mode.
- Settings: install folder, RAM, window size/fullscreen, JVM args, HWID
  sending, interface sounds, backend check.
- Service access card (**PL-1 only**): service key, mirror/proxy switching,
  Tor routing and backend self-test (`/api/v1/mirror/*`).
- Mods page: optional mods from `/api/v1/profiles/{name}/mods/optionals`,
  enable/disable by renaming `.jar` ↔ `.jar.disabled`.
- Interface sounds from the original assets, custom borderless window with
  caption buttons and resize handles, and an animated branded start screen.
  The maximize button animates to the Windows work area, so the taskbar stays
  usable. Screen changes, buttons, profile rows and the download bar animate.
- Discord Rich Presence with the same application ID, assets and texts as the
  C# launcher (`ApiProcedures.UpdateDiscordRpcState`): states «Сидит в
  лаунчере» / «Играет в "profile"», details «Продам гараж…», logo2/logo.
- Native taskbar/Explorer icon embedded into the executable, plus `WM_SETICON`
  and the same AppUserModelID as the C# launcher.
- The settings tree is pre-built and fades in, so opening it does not wait for
  a first-time layout.
- Fullscreen launches use the monitor's native resolution instead of the
  900×600 window size, so the game is not stretched.
- Optional «Скрывать лаунчер при запуске игры» in Settings → Окно игры
  (minimizes the launcher once the game process starts).
- Profiles are refreshed every 90 s and asynchronously after the first load
  (a lightweight substitute for the C# SignalR `RefreshProfiles` event).
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
│  ├─ hwid.rs           Windows HWID, RAM, OS info
│  └─ model.rs          settings, session, API DTOs
└─ assets/              logo, background
```

Threading model: Slint owns the main thread; a Tokio multi-thread runtime runs
the worker. The UI sends `Command`s through an unbounded channel; the worker
sends `Event`s back and a 40 ms `slint::Timer` applies them on the UI thread.
No blocking I/O happens in a Slint callback.

## Data paths

| What | Where |
| --- | --- |
| Settings/session/client-signature key | `%APPDATA%\Vestalis\rust-launcher\state.json` |
| Game files | `%APPDATA%\Vestalis` (same as the C# launcher) |
| SHA-1 cache | `%LOCALAPPDATA%\Vestalis\validation-cache-rust.json` |
| Logs | `%LOCALAPPDATA%\Vestalis\logs\launcher.log.<date>` |
| Offline snapshots | `%APPDATA%\Vestalis\offline-mode\...` |

The first validation imports `%LOCALAPPDATA%\Gml.Launcher\validation-cache-v1.json`,
so an existing C# installation does not re-hash its whole 16k-file client.

## Backend notes

- The auth endpoints require `X-HWID` on every request. When the HWID toggle
  is off, a fresh random identifier is generated per request.
- Official hosts require a valid `X-Client-Signature`:
  `HMAC-SHA256(secret, "{unixSeconds}:{guidN}:{hwid}:Vestalis")`, sent as
  `unixSeconds:guidN:hexDigest`. Without the project-issued key the host answers
  `403 «Требуется запуск через официальный лаунчер»` before credential checks.
  In that case the launcher can fall back to the PL-1 mirror (badge `ЗЕРКАЛО`);
  a mirror session is not accepted by the official RU/EU servers.
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
3. Skins/marketplace pages and Sentry.
4. The exact splash-to-window morph of the Avalonia build (the Rust build uses
   an animated branded start overlay).
5. Installer and code signing.
6. Optional mods that are not downloaded yet: the mods page asks the user to
   launch the game once instead of downloading the missing file on the spot.
