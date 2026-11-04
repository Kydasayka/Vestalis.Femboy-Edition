# Vestalis Launcher — Rust rewrite

Native Rust implementation of the Vestalis launcher, kept in its own branch
(`feature/rust-launcher-rewrite`) and its own folder. The C#/Avalonia code in
`src/` is untouched.

## What works

- Sign-in with login/password, 2FA challenge, saved credentials and silent
  token validation (`/api/v1/integrations/auth/signin`, `/checkToken`).
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
  sending, interface sounds flag.
- Backend mode badge (`ЗЕРКАЛО` / `ПРОКСИ` / `ОСНОВНОЙ` / `ОНЛАЙН`), mirror
  mode probe, and a manual backend check.

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
| Settings/session | `%APPDATA%\Vestalis\rust-launcher\state.json` |
| Game files | `%APPDATA%\Vestalis` (same as the C# launcher) |
| SHA-1 cache | `%LOCALAPPDATA%\Vestalis\validation-cache-rust.json` |
| Logs | `%LOCALAPPDATA%\Vestalis\logs\launcher.log.<date>` |
| Offline snapshots | `%APPDATA%\Vestalis\offline-mode\...` |

The first validation imports `%LOCALAPPDATA%\Gml.Launcher\validation-cache-v1.json`,
so an existing C# installation does not re-hash its whole 16k-file client.

## Backend notes

- The auth endpoints require `X-HWID` on every request. When the HWID toggle
  is off, a fresh random identifier is generated per request.
- The RU/EU deployments currently answer unauthenticated/non-official clients
  with `403` and an "unofficial client" message. PL-1 accepts the same API and
  is therefore the default. Other hosts remain selectable.
- The API speaks camelCase JSON; request bodies for sign-in keep the PascalCase
  names used by the C# client.
- Mirror control (`X-Mirror-Control-Key`, mode switching, Tor, selftest) is
  read-only in this MVP: the badge reads `/api/v1/mirror/mode`, but there is no
  service-key UI yet.

## Known gaps / next steps

1. SignalR live profile refresh (currently manual “Обновить” or the 45 s
   backend probe; profiles are loaded on sign-in/refresh).
2. Optional-mods page and `.disabled` toggle UI (the download layer already
   respects `-optional-mod.jar` and keeps new optional mods disabled).
3. Sound playback (the setting is persisted but `rodio` is not wired yet).
4. Skins/marketplace pages, Discord RPC, Sentry, custom window chrome and the
   splash morph from the Avalonia build.
5. Mirror service key, mode switch, Tor toggle and selftest.
6. Installer and code signing.
