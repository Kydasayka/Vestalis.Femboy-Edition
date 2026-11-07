# Architecture — Rust launcher

## Goals

- Keep the UI responsive while the network and disk work runs.
- Make backend/file/process code testable without a Slint window.
- Reuse the existing game installation and API contracts.

## Layers

```
┌──────────────────────────── ui/app.slint ────────────────────────────┐
│ login · main (profiles/news/progress) · settings                     │
└───────────────▲───────────────────────────────────┬──────────────────┘
                │ Event (40 ms Timer)               │ Command
┌───────────────┴───────────────────────────────────▼──────────────────┐
│ worker.rs — application service / orchestrator                       │
│ auth · profile loading · offline mode · launch · settings · layout   │
└───────┬───────────────┬───────────────────┬──────────────────────────┘
        │               │                   │
   api.rs          files.rs             launch.rs
   reqwest         SHA-1 + downloads    JVM args + process
        │               │
   signalr.rs      diagnostics.rs
   hub events      sanitized ZIP
        │
        └── model.rs / storage.rs / hwid.rs / theme.rs / locale.rs / layout.rs ──
```

- `main.rs` is the composition root: logging, runtime, Slint callbacks, event
  pump, image decoding. It contains no business rules.
- `worker.rs` owns mutable application state (`Persisted`, current profile,
  offline flag, busy flag) and is the only place that mutates it.
- `api.rs` is stateless apart from the HTTP client and base URL; it returns
  typed results and never touches the UI.
- `files.rs` performs validation and downloads. Progress is reported through a
  `Send + Sync` callback; the worker converts it into `Event::Progress`.
- `launch.rs` is pure argument manipulation plus a process spawn. Its regex
  logic is covered by unit tests.
- `signalr.rs` owns the WebSocket hub task. It reconnects with backoff and only
  emits `RefreshProfiles`; the worker aborts the task on logout/server switch.
- `theme.rs` owns palettes, modes, icon/text overrides and colour mixing;
  `locale.rs` owns RU/EN/UA labels; `layout.rs` owns zones, profiles and
  normalisation. None of them touch the network.
- `diagnostics.rs` builds the support ZIP from logs and sanitized state.
- `storage.rs` is also the single place for atomic state/cache writes, `.bak`
  recovery and cache TTL cleanup.

## Threading

- Slint runs on the main thread (`ui.run()`), as required on Windows.
- `tokio::runtime::Builder::new_multi_thread()` runs the worker task. Do **not**
  use `#[tokio::main]`: it would take the main thread away from Slint.
- UI → worker: `tokio::sync::mpsc::UnboundedSender<Command>`.
- Worker → UI: `UnboundedSender<Event>` plus a `slint::Timer` on the UI thread.
  This avoids sending non-`Send` Slint handles across threads.
- The hub runs as a separate Tokio task and sends `HubEvent::RefreshProfiles`
  through its own channel, so a slow backend never blocks the command queue.
- Theme cross-fades and toast auto-hide use `slint::Timer` on the UI thread;
  no Rust thread touches Slint handles directly.
- `DragArea` payloads are `DataTransfer` values created on the UI thread from a
  hovered row; `LayoutDrag.payload` is read declaratively by `DragArea`.

## Error model

- `anyhow::Result` on the async boundary; user-facing messages are Russian.
- The worker catches panics from the launch pipeline (`catch_unwind`) so a
  bad argument can never leave the Play button permanently disabled.
- HTTP 401/403 on profile loading clears the session and returns to login.

## Launch pipeline

1. `POST /api/v1/profiles/info` with the token in the `Authorization` header
   (raw token, matching the C# client).
2. Validate the manifest: optional mods may live as `.jar.disabled`; files
   ≥100 MiB are trusted by presence; SHA-1 results are cached by size+mtime.
3. Download changed files with 32-way concurrency, retries and a temporary
   file + rename.
4. Rewrite `{localPath}`, replace any cached authlib endpoint with the active
   host, remove the authlib agent offline, remove old `-Xms/-Xmx` and inject
   the selected values before `-cp`/`-jar`.
5. Spawn `javaw.exe` with `CREATE_NO_WINDOW` and the original command line
   (`raw_arg`), so quoting from the backend is preserved.

## Security notes

- Tokens and saved credentials live in `state.json` in the user profile. This
  matches the C# launcher's local storage; moving them to the Windows
  Credential Manager is a future improvement. `state.json.bak` contains the
  same data and must be treated as sensitive too.
- The diagnostics ZIP scrubs `token`, `password`, `secret`, `credential`, `user`
  and `savedLogin` keys recursively before writing.
- Tokens and HWIDs must never be logged. Login logs only the login name and the
  outcome. The hub keeps the token in the WebSocket query string, mirroring the
  C# client; logs only report connection state.
- Paths from the manifest are sanitized (`..`, drive letters and absolute
  paths are rejected) before joining them to the install directory.
