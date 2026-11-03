# Vestalis Femboy edition

**Vestalis Femboy edition** is a community fork of the **Vestalis launcher**, which in turn is a
fork of [Gml Launcher (GamerVII Minecraft Launcher)](https://github.com/GamerVII-NET/minecraft-launcher).
Built with .NET 8 and Avalonia UI.

Lineage: **GML Launcher** (GamerVII-NET) → **Vestalis launcher** → **Vestalis Femboy edition** (this repo).

This repository is neither the GML Launcher nor the official Vestalis launcher. It is an
independent, experimental build: it may behave differently from the official release, and no
changelog of differences is maintained here.

> [!WARNING]
> **Vibecoded.** Large parts of this edition were written with AI assistance and reviewed only
> lightly. Expect rough edges, unfinished features and regressions. Use it at your own risk, and
> do not treat it as a stable or supported product.

> [!WARNING]
> **The PL1 server (`201-58-180-2.sslip.io`) is not an official Vestalis server.** It is a
> third-party/experimental host configured in this build. It is not run or endorsed by the Vestalis
> team, comes with no guarantees of uptime, data safety or moderation, and connecting to it is your
> own decision.

## About this edition

- Base: the **GML launcher** (not Vestalis launcher), with UI and client-side
  experiments layered on top.
- Not an official Vestalis release; for a supported launcher use the official one.
- Bugs found here should not be reported to the Vestalis team or to GML Launcher.

## What's new in this edition

- **Sound Effects** – click and toggle sounds added.
- **No Vestalis auto-updates** – the auto-updater script has been removed.
- **Custom Design** – ultra MEOW design in optional mods :3
- **Animations** – more animations for UI elements (may cause lag).
- **Hot Cache** – `javaw` starts faster after pressing the start button.
- **Auto Auth** – the launcher automatically sends an authentication request to the backend after a restart if you aren't logged in (!!! Not tested with 2FA !!!).
- **Random HWID** – enable this feature if you were banned for no reason via your HWID.
- **JVM Arguments & RAM settings** – the name says it all (credits to katran).
- **Custom Backend Server** – mirrors the EU server and is located in Poland/PL1 (W.I.P.).

## Requirements

- **Windows** (WPF-free Avalonia app, but the build and HWID collection target Windows).
- **.NET 8.0 SDK** — <https://dotnet.microsoft.com/download/dotnet/8.0>
- **Git** — <https://git-scm.com/downloads>
- The launcher must not be running while you build (otherwise MSB3027/MSB3021: file is locked).

## Build from source

The client code lives in the `src/Gml.Client` submodule, so clone recursively:

```bash
git clone --recursive <this-repo-url>
cd Gml.Launcher-2025.3.3
```

If you cloned without `--recursive`:

```bash
git submodule update --init --recursive
```

Then:

```powershell
# Stop a running instance first, otherwise the build cannot replace the exe
Stop-Process -Name Gml.Launcher -Force -ErrorAction SilentlyContinue

dotnet build Gml.Launcher.sln
dotnet test Gml.Launcher.sln --no-build
```

## Run

```powershell
Start-Process 'src\Gml.Launcher\bin\Debug\net8.0\Gml.Launcher.exe'
```

Startup takes ~35–50 seconds (splash screen, remote data loading).

## Configuration

Servers are defined in `src/Gml.Launcher/Assets/Resources/ResourceKeysDictionary.cs`
(`ServerRu1Host`, `ServerRu2Host`, `ServerEu1Host`, `ServerPl1Host`).

Local data is stored in `%APPDATA%\Vestalis\data.db` (SQLite, table `StorageItems`), with keys
such as `Settings`, `ServerHost`, `LastSelectedProfile`, `SavedCredentials` and `UserInfo`.

Debug artifacts:

- file validation cache — `%LOCALAPPDATA%\Gml.Launcher\validation-cache-v1.json`
- launch tracing (enabled with `GML_LAUNCH_TRACE=1`) — `%LOCALAPPDATA%\Gml.Launcher\launch-trace.log`

## Project structure

| Path | Contents |
| --- | --- |
| `Gml.Launcher.sln` | Solution file |
| `src/Gml.Launcher` | Avalonia UI, view models, services, assets |
| `src/Gml.Client` | Submodule: client logic, file sync, auth, SignalR |
| `src/GamerVII.Notification.Avalonia` | Submodule: in-app notifications |
| `dist/` | Local build/publish output (ignored by git) |

See [AGENTS.md](AGENTS.md) for a detailed code map and known pitfalls.

## Authors

**Vestalis Femboy edition**

- **Kydasayka** — fork maintenance (this edition)
- **Katran** — help, consulting and feedback (thank you!)

**Upstream / heritage**

- **Vestalis team** — original Vestalis launcher (direct base of this fork)
- **GamerVII** and the [Gml Launcher contributors](https://github.com/GamerVII-NET/minecraft-launcher)
  — original GML Launcher
- [GamerVII.Notification.Avalonia](https://github.com/GamerVII-NET/GamerVII.Notification.Avalonia)
  contributors — notification library

## Credits and license

This project is a derivative work: GML Launcher by GamerVII-NET → Vestalis launcher → Vestalis
Femboy edition. It keeps the architecture and large parts of the source code of both parents. No
separate license file is shipped with this edition; refer to the upstream repositories for
licensing terms and attribution requirements.

P.S. TK1 - Pidor
