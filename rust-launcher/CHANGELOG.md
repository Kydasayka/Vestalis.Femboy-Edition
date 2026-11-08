# Vestalis Launcher — Rust rewrite: update log

## 2026-11-09 — Rust-only branch cleanup

- Removed the C#/Avalonia launcher from the `rust` branch: `src/`, the solution,
  submodules, `load-repositories.*`, the .NET GitHub Actions workflow and the
  local `identifier.sqlite` artifact.
- Trimmed `.gitignore` and `.editorconfig` to the Rust/Windows project.
- Rewrote the root README and `AGENTS.md` around `rust-launcher/`; the legacy
  launcher is documented as a separate project.
- No Rust source behaviour changed. 47 tests pass, clippy and fmt are clean.

## 2026-11-08 — backend news icons, profile backgrounds and smoother UI

- News cards render the backend `iconUrl`; icons are downloaded and cached in
  `%LOCALAPPDATA%\Vestalis\news-icons` (SHA-1 URL key, png/jpg, atomic write,
  30-day cleanup, 300-file cap).
- Profile rows reveal the profile's personal backend `background` on hover and
  selection, under a theme-tinted gradient. Backgrounds are cached in
  `%LOCALAPPDATA%\Vestalis\profile-backgrounds` (200-file cap) and decoded once.
- Screen changes fade in (`screen-opacity`, 190 ms); the mods page switches
  immediately instead of waiting for the network; buttons, sidebar items,
  caption buttons, mini buttons, profile rows and text fields animate their
  hover/focus state.
- Smoothness: unchanged profile/news responses no longer rebuild the Slint
  models on the 90-second poll; decoded images are kept in a thread-local cache;
  ambient particles were reduced (fireflies 10, leaves 14, snow 22) and the
  firefly blur is cheaper.
- Tests: 47 passed, clippy clean.

## 2026-11-08 — UX pack (Catppuccin, Acrylic, autosave, telemetry, onboarding, picker)

- Catppuccin Mocha/Macchiato/Frappe/Latte with official palettes and contrast
  tests; readable names and existing swatch previews.
- Fourth presentation mode `acrylic`: denser translucency, bright rim, deep
  shadow, deterministic grain; explicitly documented as a no-backdrop-blur
  imitation. Reuses glass opacity/blur settings.
- Settings auto-save: Save button removed; controls and text fields persist
  with a 500 ms debounce, validation, and red/green status. Invalid snapshots
  are not written. Existing worker `SaveSettings` path is unchanged.
- Optional Sentry telemetry (`telemetryEnabled`, off by default), DSN from
  `VESTALIS_SENTRY_DSN`/`SENTRY_DSN` only, sanitized events (no tokens,
  passwords, HWID, logins, user paths) plus a short cleaned log tail.
- First-run onboarding after the splash: language, theme/mode, telemetry
  consent, install/Java check; skip/back/next/finish, once-only flag, and
  «Пройти обучение заново» in Settings.
- Custom colour picker: HSV field, hue slider, hex input, preview, recent
  colours, and a native screen eyedropper with a GDI magnifier. The standard
  `ChooseColorW` dialog is gone.
- New tests: Catppuccin accent contrast, Acrylic validity, HSV round-trip,
  telemetry sanitizer. 45 tests, clippy clean.

## 2026-11-07 — layout/profile hotfix (`ca0c074`)

- Profile list no longer clips the first/last row: the nested `ScrollView` was
  removed, the block now grows with `search + rows`, and the sidebar's outer
  ScrollView handles overflow.
- Hidden blocks are removed from the main-screen models, so they no longer
  reserve height or spacing.
- Block heights are configurable with `−/+` and clamped per block; hidden
  blocks keep their height for restore.
- `sidebar-actions` is pinned to the bottom of the sidebar; the right sidebar
  disappears when every block in it is hidden.
- Startup normalises old layout JSON (including the old 200 px profiles block).

## 2026-11-07 — polish branch (`57a7ba9` … `59be15d`)

- Window size/position and maximized state are persisted in `window.json`; the
  normal rectangle is kept separately from the maximized work-area rect.
- Single-instance mutex; a second launcher shows a message and exits.
- Cancel launch during file validation/downloading; `.tmp` is removed and the
  game is not spawned after cancellation.
- Game process is awaited in the background; status, Discord presence and the
  minimized launcher are restored when the game exits.
- Maintenance actions: open install/logs/mods folders, verify files, clear the
  SHA-1 cache, reset the window, export a sanitized diagnostics ZIP.
- Auto-login toggle and an optional custom Java executable with the system
  file picker.
- Profile and mods search with empty/not-found states; filters reset on
  logout/server switch.
- Mods page: open folder, download all missing mods, enable/disable all,
  per-action statuses/errors.
- Copy the real HWID to the clipboard; show-password toggle on login.
- Nord, Dracula and Gruvbox theme presets; sound test button.
- Settings status turns red for errors; new strings localized RU/EN/UA.
- Old logs pruned (30 days / 60 files); worker errors are visible in Settings.
- 40 unit tests; `cargo clippy --all-targets -- -D warnings` clean.

## 2026-11-07 — base feature set (`897b079`, `55e447a`)

- Real SignalR hub (`ws/launcher`): negotiation, JSON handshake,
  `RefreshProfiles`, launcher-hash handshake, keep-alive, reconnect/backoff;
  the 90 s poll remains only as a fallback.
- Optional mod downloads from the mods page with progress/retries and
  `.jar.disabled` handling.
- Themes: `Windows` (generated from the accent), `Apple`, `Discord`,
  `Minecraft`; manual accent with priority manual → Windows → theme;
  follow Windows light/dark with a theme pair; 280 ms theme cross-fade.
- Layout: drag-and-drop between zones/rows, named layout profiles, sidebar
  widths 240–420 px, block visibility.
- RU/EN/UA localization; in-app toasts for news/profile changes; loading
  stages, speed and ETA; Liquid Glass opacity/blur imitation settings.
- Cache TTL/cleanup for avatars and theme icons; diagnostics export; atomic
  `state.json` with `.bak` recovery; PL-1 service management removed while
  PL-1 stays visible as a disabled `W.I.P` entry.
- 36–39 tests at the time, clippy clean, release artefact rebuilt.

## Earlier Rust rewrite baseline

- Sign-in/2FA/HWID, saved credentials, official host signature, server
  selector, profiles/news/online, offline mode, SHA-1 file validation and
  parallel downloads, Java launch with authlib rewrite, Discord RPC, custom
  chrome/DWM corners, Nunito, background effects, theme modes
  (`base`/`material3`/`liquid-glass`), custom themes with icons/texts,
  per-machine state, logs and offline snapshots.
