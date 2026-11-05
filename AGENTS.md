# AGENTS.md — рабочие заметки (Vestalis Femboy edition)

Короткая карта проекта, чтобы не перечитывать весь код. Держать этот файл в актуальном
состоянии: сюда дописываются решения, подводные камни и проверенные команды.

## Что это

- **Vestalis Femboy edition** — форк лаунчера Vestalis, который сам форк GML Launcher
  (GamerVII-NET). Avalonia 11.3.3, .NET 8, Windows.
- Решение: `Gml.Launcher.sln`; UI: `src/Gml.Launcher`.
- Клиентская логика (авторизация, загрузка файлов, SignalR) — в сабмодуле `src/Gml.Client`
  (`src/Gml.Client/src/Gml.Client`, интерфейсы `src/Gml.Client/src/Gml.Client.Interfaces`).
- Уведомления — сабмодуль `src/GamerVII.Notification.Avalonia`.
- Тесты клиента: `src/Gml.Client/Tests/Gml.Client.Tests` (8 тестов).
- CI: `.github/workflows/dotnet.yml` — `ubuntu-latest`, `dotnet restore/build/test`,
  сабмодули обязательны (`git submodule update --init --recursive`).

## Состояние репозитория (проверять перед работой)

```powershell
git status --short; git log --oneline -6; git log --oneline origin/main..main
git -C src/Gml.Client status --short; git -C src/Gml.Client log --oneline -3
```

- Корень: ветка `main`, origin — `https://github.com/Kydasayka/Vestalis.Femboy-Edition.git`.
  Локально бывают незапушенные коммиты — не считать `origin/main` актуальным.
- `src/Gml.Client` — ветка `master`, origin — форк `Kydasayka/Gml.Client` (upstream —
  `GamerVII-NET/Gml.Client`). Коммит `4690172` `Faster Game loading`: параллельный SHA-1,
  `ValidationCache`, `.disabled`-моды, `LaunchTrace` (см. раздел про подводные камни).
- `src/GamerVII.Notification.Avalonia` — ветка `master`, upstream `f063e0b`, не трогается.
- Сабмодули — отдельные клоны: коммитятся отдельно, потом в корне обновляется указатель
  (`git add src/Gml.Client`).

## Открытые вопросы (не забыть в следующей сессии)

- Бэкенд отвечает `400 «Пользователь заблокирован!»` даже с рандомным HWID — это бан
  аккаунта/IP на стороне сервера, а не баг лаунчера. HWID-часть уже сделана.
- Тексты карточки «Сервисный доступ» и сообщения `MirrorControlService` захардкожены
  по-русски (локализация не сделана, см. ниже).
- `dist/portable/Gml.Launcher.exe` — локальный артефакт; после правок его надо пересобрать,
  в git он не попадает.
- Корневые коммиты могут быть не запушены: перед PR проверить `git log origin/main..main`.

## Сборка, тесты, запуск

```powershell
# Обязательно: остановить запущенный лаунчер, иначе MSB3027/MSB3021 (файл занят)
Stop-Process -Name Gml.Launcher -Force -ErrorAction SilentlyContinue

dotnet build Gml.Launcher.sln -v q 2>&1 | Select-Object -Last 4
dotnet test Gml.Launcher.sln --no-build -v q 2>&1 | Select-Object -Last 2

# Запуск (старт ~35–50 с: сплэш, загрузка данных)
Start-Process 'src\Gml.Launcher\bin\Debug\net8.0\Gml.Launcher.exe'
```

Портативная single-file сборка (результат gitignored, лежит в `dist/portable`):

```powershell
dotnet publish 'src\Gml.Launcher\Gml.Launcher.csproj' -c Release -r win-x64 `
  --self-contained true -p:PublishSingleFile=true `
  -p:IncludeNativeLibrariesForSelfExtract=true -o dist\portable
```

`IncludeNativeLibrariesForSelfExtract` обязателен, иначе рядом с exe окажутся нативные
`libSkiaSharp`/`libHarfBuzzSharp`/`e_sqlite3`/`av_libglesv2`.

Известные предупреждения сборки (не ошибки, не связаны с текущими правками):
`CS4014` в `LoginPageViewModel` и `CS8600` в `SkinService`.

## Локальные данные и артефакты

- `%APPDATA%\Vestalis\data.db` — SQLite, таблица `StorageItems` (`Key`, `Value` — JSON).
  Ключи: `Settings`, `ServerHost`, `LastSelectedProfile`, `SavedCredentials`, `UserInfo`,
  `InstallationDirectory`, `MirrorControlKey` (сервисный ключ зеркала, в репозиторий его
  не класть; у обычного пользователя его нет).
- Пока лаунчер запущен, БД читать только копией. Дамп:
  `python %LOCALAPPDATA%\Temp\opencode\read_db.py`.
- `%LOCALAPPDATA%\Gml.Launcher\validation-cache-v1.json` — кэш SHA-1 (можно удалять).
- `%LOCALAPPDATA%\Gml.Launcher\launch-trace.log` — трассировка запуска, только при
  `GML_LAUNCH_TRACE=1`.
- `.gitignore` игнорирует `dist/`, `*.db-wal/-shm/-journal`, `launch-trace.log`,
  `validation-cache*.json`.

## Серверы и режимы бэкенда

Хосты — `src/Gml.Launcher/Assets/Resources/ResourceKeysDictionary.cs`:

- `ServerRu1Host` — `https://gmlfrr.vestalis.xyz/`
- `ServerRu2Host` — `https://msk1.gmlfr.vestalis.xyz/`
- `ServerEu1Host` — `https://gmlfr.vestalis.xyz/`
- `ServerPl1Host` / `Host` — `https://201-58-180-2.sslip.io/` (сторонний, см. README)

Режимы зеркала (`GET /api/v1/mirror/mode`), бейдж в правом верхнем углу:

| `data.mode` | Бейдж | Цвет | Смысл |
| --- | --- | --- | --- |
| `local` | `ЗЕРКАЛО` | `#2E9E5B` зелёный | ответы из локального кэша VDS |
| `proxy` | `ПРОКСИ` | `#D9822B` оранжевый | запросы напрямую на основной бэкенд |
| `direct` | `ОСНОВНОЙ` | `#3B82F6` синий | основного бэкенда, зеркальных эндпоинтов нет |
| `null`/ошибка | бейдж скрыт | — | бэкенд недоступен или режим неизвестен |

## Карта кода (главное)

| Что | Где |
| --- | --- |
| Точка входа, DI | `Program.cs`, `App.axaml.cs`, `Core/Extensions/ServiceLocator.cs` |
| Сплэш, старт, валидация токена с HWID | `ViewModels/SplashScreenViewModel.cs` |
| Главное окно: сервер, бейдж режима | `ViewModels/MainWindowViewModel.cs`, `Views/MainWindow.axaml` |
| Глобальное состояние (режим, доступ) | `ViewModels/GlobalVariables.cs` |
| Логин, 2FA, HWID | `ViewModels/Pages/LoginPageViewModel.cs`, `Core/Services/AutoLoginService.cs` |
| Настройки (RAM/JVM/звук/HWID/ключ) | `ViewModels/Pages/SettingsPageViewModel.cs`, `Views/Pages/SettingsPageView.axaml` |
| Проверка бэкенда и режим | `Core/Services/BackendChecker.cs`, `IBackendChecker.cs` |
| Сервисный ключ, смена режима, selftest | `Core/Services/MirrorControlService.cs`, `IMirrorControlService.cs` |
| Модель строки самодиагностики | `Models/SelfTestRow.cs` |
| Настройки/хранилище | `Core/Services/SettingsService.cs`, `LocalStorageService.cs`, `StorageConstants.cs` |
| Главная, профили, новости | `ViewModels/Pages/OverviewPageViewModel.cs` |
| Список профилей | `ViewModels/Components/ListViewModel.cs`, `Views/Components/ServerListComponent.axaml` |
| Выбор сервера | `Views/Components/ServerSelectorComponent.axaml`, `Models/GmlServer.cs` |
| Звуки интерфейса | `Core/Services/SoundService.cs` |
| Система / HWID из железа | `Core/Services/SystemService.cs`, `ISystemService.cs` |
| Sentry, VPN, скины | `Core/Services/SentryInitializer.cs`, `VpnChecker.cs`, `SkinService.cs` |
| SignalR | `src/Gml.Client/.../Helpers/SignalRConnect.cs`, `GmlClientManager.cs` |
| Валидация/загрузка файлов | `src/Gml.Client/.../Helpers/Files/*`, `ApiProcedures.cs` |
| Кэш SHA-1, трассировка | `Helpers/Files/ValidationCache.cs`, `Gml.Client.Interfaces/Helpers/LaunchTrace.cs` |

## Решения и подводные камни (не ломать)

- **Самообновление удалено.** Лаунчер не дергает `/api/v1/launcher`, не скачивает и не
  перезапускает себя. Обновления — вручную с сайта. В `Gml.Client` удалён старый API
  (`SkipUpdate`, `GetActualVersion`, `UpdateCurrentLauncher`, `LauncherUpdater` и т.п.).
- **HWID обязателен для нового бэкенда.** Без заголовка `X-HWID` авторизация отдаёт 403
  («HWID отключена или недействительна»). Поэтому `SettingsService.GetHwidForAuthAsync`
  никогда не возвращает `null`: с включённым тумблером уходит реальный HWID
  (`CPU-плата-диски`), с выключенным — **новый случайный GUID на каждый запрос**
  (`SystemService.GetRandomHwid`). Не возвращать `null` обратно. `checkToken` тоже обязан
  слать `X-HWID`: `Auth(accessToken, hwid)` в интерфейсе → менеджере → `ApiProcedures`.
- **Звуки не должны играть от фоновых событий.** Бэкенд по SignalR присылает
  `RefreshProfiles`; перестройка списка профилей поднимает `SelectionChanged`, как от клика.
  Лечится `SoundService.Suppress()` вокруг перестройки коллекций + проверкой
  `TopLevel.GetTopLevel(control) is Window { IsActive: true }`. `ProfilesChanges`
  маршалится через `ObserveOn(RxApp.MainThreadScheduler)`.
- **UI-поток.** Подписки Rx, меняющие UI, переводить на `RxApp.MainThreadScheduler`;
  анимации/изменения из других потоков — через `Dispatcher.UIThread.CheckAccess()/Post`.
  Симптом нарушения: `InvalidOperationException: Call from invalid thread`.
- **Опциональные моды** хранятся с суффиксом `.disabled`; сервер отдаёт имя без суффикса.
  `HashValidationDecorator` считает мод опциональным по `-optional-mod.jar` в пути и
  принимает `<файл>.disabled` как актуальную копию, чтобы не качать его каждый запуск.
- **Кэш валидации файлов.** `ValidationCache` доверяет сохранённому SHA-1, только пока
  совпадают размер и `LastWriteTimeUtc` (тики). Хэширование — параллельное
  (`ProcessorCount * 2`, зажато в 4..16), `SystemHelper.CalculateFileHash` читает файл
  чанками по 1 МБ через `ArrayPool`. Битый кэш не роняет запуск; запись атомарная
  через `.tmp`. Формат файлов профиля меняется — кэш можно смело удалять.
- **Старое исключение для `assets` при валидации убрано.** Если сервер начнёт отдавать
  другие хэши для `assets`, файлы будут перекачиваться — это ожидаемо, не «чинить»
  возвратом исключения без проверки формата на сервере.
- **Трассировка запуска** — `Gml.Client.Interfaces/Helpers/LaunchTrace.cs` (namespace
  `Gml.Client.Helpers`), включается `GML_LAUNCH_TRACE=1`, пишет в
  `%LOCALAPPDATA%\Gml.Launcher\launch-trace.log`. По умолчанию выключена, на запуск не влияет.
- **Бейдж режима бэкенда.** `BackendChecker.UpdateBackendStatus` (сплэш, смена сервера,
  кнопка проверки) определяет доступность и режим; `MainWindowViewModel` опрашивает
  `RefreshBackendModeAsync` через 3 с после старта и каждые 20 с, применяет изменения в
  UI-потоке. `IsOffline` — это `GlobalVariables.BackendInactive` (глобальное состояние,
  не поле экземпляра). Ошибки сети не должны валить лаунчер: бейдж скрывается.
- **Управление режимом.** `MirrorControlService` шлёт заголовок `X-Mirror-Control-Key`:
  `GET /api/v1/mirror/mode` (таймаут 10 с) — статус и `data.canControl`;
  `PUT /api/v1/mirror/mode` с `{"mode":"local"|"proxy"}` — переключение.
  Ключ вводится вручную в настройках, сохраняется только если сервер его принял
  (`data.canControl == true`), лежит в `data.db` под `MirrorControlKey`; **в исходники
  его не класть**. Кнопки «Зеркало»/«Прокси» видны только при `IsBackendControlAvailable`.
  После смены режима сразу вызывается `MainWindowViewModel.RefreshBackendModeAsync`.
- **Маршрутизация через Tor.** В той же карточке есть переключатель
  «Отправлять запросы к основному бэкенду через Tor» (`PUT /api/v1/mirror/tor`,
  состояние читается из `GET /api/v1/mirror/mode`: `torEnabled`, `torAvailable`).
  Через Tor идут **только** запросы бэкенда к основному серверу (хосту из
  `MIRROR_BASE_URL`/`MIRROR_PROXY_BASE_URL`); маркет, скины,
  авторизация и сам лаунчер остаются прямыми. Состояние хранится на бэкенде
  (`tor.json`), поэтому переключатель общий для всех клиентов с сервисным ключом.
- **Самодиагностика.** `GET /api/v1/mirror/selftest` с тем же ключом, таймаут 45 с,
  только чтение (ничего не меняет). Ответ: `data.status/mode/upstream/durationMs` и
  `data.checks[]` (`key`, `name`, `status`, `message`, `durationMs`). Статусы строк:
  `ok` → «ОК» зелёный, `warn` → «ВНИМАНИЕ» оранжевый, иначе «ОШИБКА» красный.
  Без ключа сервер отвечает 403 — это не ошибка кода, а ожидаемое поведение.
- **Главное окно без системной рамки.** `MainWindow` объявлен с `SystemDecorations="None"` и
  **без** `ExtendClientAreaToDecorationsHint`: этот флаг заставлял DWM рисовать тень вокруг
  окна, которая на ярком фоне выглядела как матовая серая обводка. Кнопки
  свернуть/развернуть/закрыть свои (`Button.CaptionButton`), обработчики `MinimizeClick`/
  `MaximizeClick`/`CloseClick`. Ресайз за края и углы сделан вручную: `BeginResizeDrag` по
  8 зонам (`HitTestEdge`, хват 6 px), курсор меняется в `OnPointerMoved`. У корневого `Border`
  стоит `Margin="-5"`, чтобы контент перекрывал невидимую 5-пиксельную рамку окна — без этого
  страницы получают меньше высоты и низ наезжает на кнопки. Не возвращать
  `ExtendClientAreaToDecorationsHint` и не убирать отрицательный margin.
- **Брендинг окна загрузки.** Сплэш сделан типографикой: закруглённый тяжёлый шрифт Fredoka One
  (`Assets/Fonts/FredokaOne-Regular.ttf`, OFL рядом — статичные веса берутся из истории
  google/fonts, актуальный `Fredoka` там только вариативный, а Avalonia вес варианта не
  выбирает). Слева иконка `logo.svg` (56 px, по высоте строки вордмарка), `Vestalis` белым
  (кегль 58), `Femboy Edition` — по ширине примерно половина надписи (кегль 15), прижата к
  правому краю. Брендовый блок специально уже полоски загрузки (полоска идёт с полями 20,
  блок — с 22). У карточки **нет** `BorderBrush` (была матовая серая обводка), фон —
  `FrameBackgroundColor`. Переливание — `LinearGradientBrush` с `SpreadMethod="Repeat"`, который
  `DispatcherTimer` (32 мс, цикл 1.6 с) сдвигает через `StartPoint`/`EndPoint`; таймер
  останавливается по `Closed`. Пастельная палитра и медленный цикл — намеренно: яркий/быстрый
  градиент выглядел навязчиво.
- **Переход сплэш → лаунчер и запуск в фоне.** `App.OnFrameworkInitializationCompleted` создаёт
  главное окно с `PrepareIntro()` (контент `Opacity = 0`), показывает его и запускает морфинг:
  окно сплэша реально растягивается до границ окна лаунчера (`MorphBoundsAsync`, 520 мс, шаг
  16 мс через `DispatcherTimer`), затем карточка растворяется, а контент лаунчера проявляется
  (`Core/Helpers/WindowTransitions.cs`). Фаза `MainWindow` — `AvaloniaXamlLoader.Load(this)`,
  поэтому x:Name-полей нет, контент берётся через `Content`. Оба окна объявлены с
  `ShowActivated="False"` и поднимаются в z-order через `RaiseWithoutActivation`
  (`SetWindowPos` с `SWP_NOACTIVATE`), чтобы лаунчер было видно, но фокус оставался у
  приложения пользователя. Главное окно само скрывается примерно через 5 с после старта —
  так было и до перехода, это не регрессия.
- **Локализация карточки сервисного доступа.** Тексты карточки и сообщения
  `MirrorControlService`/`SettingsPageViewModel` захардкожены по-русски в axaml и коде,
  а не через `Resources.*` (в отличие от остального UI). При добавлении языков это
  первое место, где нужно вынести строки в ресурсы.
- **Смена сервера** (`MainWindowViewModel.SwitchServerAsync`) сохраняет хост, меняет
  хост клиента, **сбрасывает `UserInfo` и `SavedCredentials`** (авторизация привязана к
  бэкенду), обновляет режим и Sentry DSN. Это поведение по дизайну.
- **Git.** Сабмодуль коммитится отдельно, потом обновляется указатель в корне
  (`git add src/Gml.Client`). Локальные правки хостов в `ResourceKeysDictionary.cs`
  не коммитить без просьбы.

## Проверка UI (живой лаунчер)

Окно `Gml.Launcher`, координаты клика = смещение окна + координаты на скриншоте.
Готовые хелперы (вне репозитория): `%LOCALAPPDATA%\Temp\opencode\`
`shot.ps1` (скрин окна), `input.ps1 -X -Y` (клик), `focus.ps1` (вывести окно вперёд),
`read_db.py` (дамп `data.db`). Если окно перекрыто — сначала `focus.ps1`, иначе
`CopyFromScreen` снимет чужое окно.

Типовые сценарии:

- **Логин/HWID:** форма входа, тумблер «Отправлять HWID» и подсказка. Выключенный
  тумблер = случайный HWID на каждую попытку, включённый = реальный.
- **Бейдж режима:** правый верхний угол; пропал — бэкенд недоступен или эндпоинта нет.
- **Сервисный доступ:** Настройки → карточка «Сервисный доступ»; без принятого ключа
  блоки режима и самодиагностики скрыты.
- **Самодиагностика:** Настройки → «Проверить бэкенд» → строки со статусом и временем.

## Rust rewrite (`rust-launcher/`, ветка `feature/rust-launcher-rewrite`)

- Отдельный нативный лаунчер на Rust + Slint 1.18 (`ui/app.slint`), `main` и `src/`
  не трогаются. Ветка создана от `rust`.
- Сборка/запуск/тесты (MSVC Build Tools + VC workload обязательны для линковки):

```powershell
cd rust-launcher
cargo run
cargo test
cargo clippy --all-targets -- -D warnings
```

- Что уже работает: вход + 2FA + HWID, выбор сервера, профили/новости (обновление
  раз в 90 с), оффлайн-режим по `%APPDATA%\Vestalis\offline-mode`, SHA-1-валидация
  (импортирует старый кэш `%LOCALAPPDATA%\Gml.Launcher\validation-cache-v1.json`),
  параллельные загрузки, запуск `javaw.exe` с `-Xms/-Xmx` и подменой
  authlib-эндпоинта, страница модов (`.jar` ↔ `.jar.disabled`), звуки, безрамочное
  окно со своими кнопками, сплэш, настройки, сервисный доступ, Discord RPC.
- Выбор сервера — выпадающий список в правом верхнем углу. При смене бэкенда
  сохранённые логин/пароль остаются и выполняется автологин; сплэш висит до
  `Event::Ready` (жёсткий таймаут 20 с), поэтому форма входа не мелькает.
- Аватар игрока в сайдбаре — настоящая голова скина (базовый слой + слой шляпы),
  а не первая буква ника. Берётся из `GET /api/v1/users/info/{name}` →
  `textureSkinGuid`/`externalTextureSkinUrl`, режется 8×8 в (8,8), апскейлится
  nearest до 96×96 и кэшируется в `%LOCALAPPDATA%\Vestalis\avatars\<uuid>.png`.
  Буква остаётся только фолбэком, пока голова не загрузилась.
- Сервисный доступ (ключ, режимы `local/proxy`, Tor, selftest) — **только PL-1 и
  только под аккаунтом Kydasay** (`server-index == 3 && is-kydasay`): RU/EU
  отдают 404 на `/api/v1/mirror/*`, карточка скрыта, команды в воркере
  дополнительно проверяют хост.
- Официальные RU/EU требуют `X-Client-Signature`:
  `HMAC-SHA256(secret, "{unixSeconds}:{guidN}:{hwid}:Vestalis")`, заголовок
  `unixSeconds:guidN:hexDigest`. Ключ выдаёт проект; в билд он вшит обфусцированно
  (XOR в `resolve_client_secret` в `worker.rs`), а переопределение можно ввести в
  Настройки → «Ключ подписи клиента» (тогда лежит в `state.json`). **В исходники
  и git открытый ключ не класть.** Без ключа — `403`, тогда работает fallback на
  PL-1 (бейдж `ЗЕРКАЛО`), но зеркальная сессия не подходит официальным серверам.
- `OsArchitecture` в `/profiles/info` обязан быть `"64"` (как в C#:
  `ToString().ToLower().Replace("x","")`). С `"x64"` бэкенд отдаёт пустой
  профиль (`state=Error`, `files=[]`), и запуск падает с «Java не найдена».
- Полноэкранный запуск передаёт разрешение монитора (`GetSystemMetrics`), а не
  900×600 — иначе игра растягивается. Кнопка «развернуть» у безрамочного окна
  сделана через Win32 `SetWindowPos` по `SPI_GETWORKAREA`: стандартный
  `maximized` накрывал панель задач.
- Discord RPC — крейт `discord-rich-presence` в `src/discord.rs` (named pipe,
  READY, `SET_ACTIVITY`). Значения как в C# `ApiProcedures`: client id
  `1558556890605162577`, details «Продам гараж…», assets `logo2`/`logo`,
  state «Сидит в лаунчере» / `Играет в "Surv10"`.
- Иконка exe встраивается `winres` из `assets/logo.ico` в `build.rs`; окну
  дополнительно ставится `WM_SETICON` (32px/DPI-фрейм для обоих размеров), а
  процессу — тот же AppUserModelID, что в C#
  (`Vestalis.FemboyEdition.Launcher`). `assets/logo.ico`/`logo.png` генерируются
  `cargo run --example make_icon` из **нарисованной вручную** SVG-морды кота на
  скруглённом градиентном тайле (фиолетовый → розовый). Не возвращать трассировку
  `logo.svg` как источник иконки: у неё белый квадрат-подложка, который на панели
  задач выглядел тонкой рамкой, а мелкие детали пропадали в 16 px.
- В настройках есть «Скрывать лаунчер при запуске игры» (`hide_on_launch`):
  после spawn процесса окно сворачивается (`ShowWindow(SW_MINIMIZE)`).
- Настройки собраны в Slint-дереве постоянно (`visible: screen == 2`), поэтому
  открываются без холодного layout; переходы экранов/кнопки/строки профилей
  анимированы. Разворот окна — плавный `animate_to` по work area.
- Данные: `%APPDATA%\Vestalis\rust-launcher\state.json`,
  кэш `%LOCALAPPDATA%\Vestalis\validation-cache-rust.json`,
  головы скинов `%LOCALAPPDATA%\Vestalis\avatars\<uuid>.png`,
  логи `%LOCALAPPDATA%\Vestalis\logs\launcher.log.<date>`.
- Мелкие шрифты подняты (10→12, 11→13, 12/13→14) — не возвращать 10–11 px.
  На карточке отсчёта «НЕДРОЧАБРЬ» стоит `assets/nedrochabr.png` (картинка
  пользователя), скруглённая рамка 102×104, текст — свойство `countdown-text`.
- PL-1 (`https://201-58-180-2.sslip.io/`) — дефолт: RU/EU отдают 403 «неофициальный
  клиент». Остальные хосты остаются в селекторе.
- Подводные камни: в Rust `regex` **нет look-around** — не тащить сюда C#-паттерны
  с `(?<!…)`/`(?=…)`; `slint` facade не экспортирует feature `svg`, она включается
  через прямую зависимость `i-slint-core = { features = ["svg"] }`; UI-поток обязан
  остаться главным (`ui.run()`), Tokio — фоновый.
- Ещё не сделано: SignalR-подписки (вместо них опрос раз в 90 с), витрина/страница
  скинов (голова игрока уже рендерится), Sentry, установщик и подпись кода.
  Подробности — `rust-launcher/README.md` и `rust-launcher/docs/ARCHITECTURE.md`.

## Стиль работы

- Не читать большие каталоги целиком (там runtime Java и тысячи файлов) — только
  точечный `grep`/`read` по конкретным путям.
- Сборку/тесты выводить хвостом (`Select-Object -Last 4`), не заливать лог в контекст.
- Держать изменения в стиле окружающего кода, комментарии по-английски.
