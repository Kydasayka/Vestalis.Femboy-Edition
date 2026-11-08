# AGENTS.md — Vestalis Rust launcher

Короткая карта проекта для ветки `rust`. В этой ветке есть только нативный
лаунчер на Rust + Slint; C#/Avalonia лаунчер развивается отдельно и его файлов
здесь нет.

## Что это

- **Vestalis Rust launcher** — нативный лаунчер на Rust + Slint 1.18, Windows.
- Crate: `rust-launcher/Cargo.toml`; UI: `rust-launcher/ui/app.slint`.
- Точка входа и соединение UI/воркера: `rust-launcher/src/main.rs`.
- Асинхронная логика, авторизация, профили, моды и запуск:
  `rust-launcher/src/worker.rs`.
- Документация: корневой `README.md` — обзор и быстрый старт;
  `rust-launcher/README.md` — технический справочник;
  `rust-launcher/docs/ARCHITECTURE.md` — архитектура.

## Состояние репозитория

```powershell
git status --short
git log --oneline -6
git branch --show-current
```

- Рабочая ветка — `rust`. `main` содержит старый C#/Avalonia проект и не
  затрагивается правками Rust-лаунчера.
- `rust-launcher/Cargo.lock` коммитится, это бинарный crate.
- Сабмодулей, solution-файлов и .NET CI в ветке `rust` нет.
- `dist/` — локальный вывод сборки, в git не попадает.

## Сборка, тесты, запуск

```powershell
cd rust-launcher

cargo run
cargo run -- --offline      # кэшированные профили, без бэкенда
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo build --release
```

Для линковки нужны MSVC Build Tools с workload «Desktop development with C++».
Release-бинарь: `rust-launcher/target/release/vestalis-launcher.exe`. Портативная
копия (gitignored):

```powershell
Copy-Item target\release\vestalis-launcher.exe ..\dist\rust\vestalis-launcher.exe -Force
```

Без необходимости не завершать `javaw.exe`/Minecraft: для сборки, тестов и
clippy это не требуется.

## Данные

| Что | Где |
| --- | --- |
| Настройки/сессия | `%APPDATA%\Vestalis\rust-launcher\state.json` (+ `.bak`) |
| Геометрия окна | `%APPDATA%\Vestalis\rust-launcher\window.json` |
| Игровые файлы | `%APPDATA%\Vestalis` |
| Оффлайн-снимки | `%APPDATA%\Vestalis\offline-mode\...` |
| Темы | `%APPDATA%\Vestalis\rust-launcher\themes` |
| SHA-1 кэш | `%LOCALAPPDATA%\Vestalis\validation-cache-rust.json` |
| Головы скинов | `%LOCALAPPDATA%\Vestalis\avatars\<uuid>.png` |
| Иконки новостей | `%LOCALAPPDATA%\Vestalis\news-icons` |
| Фоны профилей | `%LOCALAPPDATA%\Vestalis\profile-backgrounds` |
| Кэш иконок темы | `%LOCALAPPDATA%\Vestalis\theme-icons` |
| Логи | `%LOCALAPPDATA%\Vestalis\logs\launcher.log.<date>` |

`state.json` пишется атомарно tmp→rename + `.bak`; битый JSON восстанавливается
по валидным полям (`recover_state`/`merge_settings`), не сбрасывать весь файл.
Первый запуск импортирует старый кэш
`%LOCALAPPDATA%\Gml.Launcher\validation-cache-v1.json`.

## Карта кода

| Что | Где |
| --- | --- |
| Точка входа, Slint callbacks, event pump | `src/main.rs` |
| Воркер: команды, auth, профили, моды, запуск | `src/worker.rs` |
| HTTP-клиент, auth, профили, новости | `src/api.rs` |
| Валидация манифеста и загрузки | `src/files.rs` |
| Запуск JVM и authlib | `src/launch.rs` |
| SignalR | `src/signalr.rs` |
| Темы, Catppuccin, Material 3, HSV | `src/theme.rs` |
| Layout-редактор | `src/layout.rs` |
| Локализация RU/EN/UA | `src/locale.rs` |
| Sentry и санитайзер | `src/telemetry.rs` |
| Диагностический ZIP | `src/diagnostics.rs` |
| Экранная пипетка (GDI) | `src/screen_picker.rs` |
| Win32-окно, DWM, буфер обмена | `src/window.rs` |
| Discord RPC | `src/discord.rs` |
| state.json и кэши | `src/storage.rs` |
| HWID, RAM, ОС | `src/hwid.rs` |
| Настройки, сессия, DTO, серверы | `src/model.rs` |
| UI | `ui/app.slint` |

## Решения и подводные камни

- **Авторизация.** Официальные RU/EU требуют `X-HWID` и `X-Client-Signature`:
  `HMAC-SHA256(secret, "{unixSeconds}:{guidN}:{hwid}:Vestalis")`, заголовок
  `unixSeconds:guidN:hexDigest`. Ключ вшит обфусцированно (XOR в
  `client_secret()` в `worker.rs`), поля для него нет. Открытый ключ в исходники
  и git не класть.
- **checkToken** выполняется только при старте. Фоновое обновление профилей раз
  в 90 с не должно разлогинивать: иначе повторный sign-in меняет AccessToken и
  уже запущенная игра получает «сессия недействительна».
- **`OsArchitecture`** в `/profiles/info` обязан быть `"64"`. С `"x64"` бэкенд
  отдаёт пустой профиль (`state=Error`, `files=[]`).
- **PL-1** (`https://201-58-180-2.sslip.io/`) выключен:
  `model::PL1_AVAILABLE = false`, пункт в селекторе показан как disabled
  `W.I.P`, `select_server(3)` игнорируется, сохранённый PL-1 мигрирует на RU-2.
  Сервисная карточка и `/api/v1/mirror/*` в Rust-лаунчере отсутствуют.
- **Сплэш** висит до `Event::Ready` (жёсткий таймаут 20 с), поэтому форма входа
  не мелькает. При смене сервера сохранённые логин/пароль остаются и выполняется
  автологин.
- **Аватар** — настоящая голова скина: `GET /api/v1/users/info/{name}` →
  `textureSkinGuid`/`externalTextureSkinUrl`, crop 8×8 в (8,8), nearest до
  96×96, кэш `avatars\<uuid>.png`. Буква — только фолбэк.
- **Темы.** `src/theme.rs` + `export global Theme` в `app.slint`. Встроенные
  палитры: `dark`, `light`, `femboy`, `ocean`, `Apple`, `Discord`, `Minecraft`,
  `Nord`, `Dracula`, `Gruvbox`, Catppuccin Mocha/Macchiato/Frappe/Latte,
  `Windows` (из акцента Windows). Пользовательские темы —
  `name.json` или `name/theme.json` + `icons/` в
  `%APPDATA%\Vestalis\rust-launcher\themes`. Битые значения логируются и
  заменяются на `dark`.
- **Режимы:** `base`, `material3`, `liquid-glass`, `acrylic`. M3-палитра
  генерируется из WinRT `UISettings.GetColorValue(Accent)` с registry-fallback.
  Серый/чёрный accent — валидная настройка Windows. Liquid Glass и Acrylic —
  имитации: в Slint 1.18 нет backdrop blur. Акцент M3 опрашивается раз в 20 с.
- **COM.** Не инициализировать COM как MTA до создания окна: winit делает
  `OleInitialize` (STA), иначе `RPC_E_CHANGED_MODE`. `system_accent()`
  вызывается только после `AppWindow::new()`.
- **Иконки темы:** семантические ID, только SVG/PNG до 512 КиБ, пути внутри
  папки темы, без `..`/абсолютных путей/симлинков. SVG `currentColor`
  тонируется и кэшируется в `theme-icons`. Тексты — allowlist `TEXT_KEYS`;
  ошибки и кнопки окна не переопределяются.
- **Layout.** `Settings.layout` отдельно от темы: шесть зон и блоки
  `user/profiles/countdown/sidebar-actions/news/main-actions`, селектор сервера
  закреплён в header. DnD + стрелки, ширины 240–420 px, `hidden` и
  `blockHeights`. Всё нормализуется и сохраняется сразу.
- **Скрытые блоки не занимают место.** Rust отдаёт для главного экрана списки
  `layout-*-visible` без скрытых элементов; редактор получает полные списки.
  Не возвращать `visible: !is-hidden(...)`.
- **`sidebar-actions`** прижат к низу сайдбара, остальные блоки — в `ScrollView`.
  Если все блоки правого сайдбара скрыты, панель исчезает.
- **Фон лаунчера:** `background-effect` (`none`/`fireflies`/`leaves`/`snow`),
  анимация на `animation-tick()` без Rust-таймера. В Slint нет `%`,
  `border-radius: 50%`; использовать `Math.mod`, `sin/cos` с `* 1deg` и длину.
  Частиц немного (10/14/22), чтобы не дёргать прокрутку.
- **Шрифт:** Nunito (`assets/fonts/Nunito-{Regular,SemiBold,Bold}.ttf` + OFL),
  `default-font-family: "Nunito"`, базовый вес 600. Fredoka One не использовать:
  в нём нет кириллицы. Мелкие размеры 10–11 px не возвращать.
- **Валидация файлов:** сначала `size` из манифеста, затем SHA-1. Кэш доверяет
  локальному хэшу только при совпадении размера, mtime и `server_hash` (пустой
  у старых записей — доверяем). Опциональные моды — `.jar.disabled`.
- **SignalR:** `wss://<host>/ws/launcher?access_token=…`, negotiate → handshake
  `{"protocol":"json","version":1}` → `RefreshProfiles`,
  `RequestLauncherHash`/`ConfirmLauncherHash`, ping `{"type":6}` каждые 15 с.
  Реконнект 1→30 с; задача отменяется при логауте, смене сервера и оффлайне.
  Опрос 90 с — только фолбэк.
- **Моды:** «Скачать» берёт хэш из манифеста и качает через
  `files::download_optional` в `.jar.disabled`; имена сопоставляются по
  alphanumerics (`normalize_name`). Список обновляется без запуска игры.
- **Темы и локализация:** приоритет theme override → `locale::get(lang, key)`.
  `Event::Settings` — `Box<Settings>` (clippy large_enum_variant).
- **Смена темы** анимируется в Rust (`Theme::mix`, 280 мс, `THEME_TIMER`):
  глобалы Slint не поддерживают `animate`.
- **DragArea/DropArea:** `data-transfer` нельзя создать из строки в биндинге.
  Payload готовится в Rust (`LayoutDrag.begin`/`LayoutDrag.payload`), дроп
  читается через `LayoutDrag.transfer-text(event.data)`. Стрелки — fallback.
- **Геометрия окна** — отдельный `window.json`, не `state.json` (воркер
  перезаписывает state при выходе). Клампится к work area и 1080×620;
  `maximized` + `NORMAL_GEOMETRY` не сохранять как normal.
- **Окно:** кастомный хром 42 px, кнопки 38×28, минимум 1080×620 через
  `WM_GETMINMAXINFO`, скругление DWM `DWMWA_WINDOW_CORNER_PREFERENCE`.
  Разворот — Win32 `SetWindowPos` по `SPI_GETWORKAREA`, не стандартный
  `maximized` (иначе накрывает панель задач).
- **Один экземпляр:** `window::ensure_single_instance` держит mutex
  `Local\\Vestalis.FemboyEdition.Launcher`.
- **Буфер обмена:** `window::copy_text` (Win32 `OpenClipboard` +
  `SetClipboardData` CF_UNICODETEXT). «Скопировать HWID» всегда копирует
  реальный HWID независимо от тумблера отправки.
- **Иконки новостей и фоны профилей:** `News.iconUrl`, `Profile.background` →
  `worker::cached_remote_image` в `news-icons`/`profile-backgrounds` (SHA-1 URL,
  png/jpg, tmp→rename, 30 дней, лимиты 300/200). `main::decode_cached_image`
  декодирует через thread-local `IMAGE_CACHE`. Фон профиля — `Image` + градиент
  `Theme.*` с `animate opacity` на hover/выборе.
- **Переходы:** `switch_screen` ставит `screen-opacity` в 0, меняет `screen` и
  через 20 мс возвращает 1; у экранов `animate opacity` 190 мс. Моды
  открываются сразу, список догружается.
- **Плавность:** `apply_profiles`/`emit_news` не пересобирают модель Slint при
  одинаковых данных (иначе сбрасываются scroll/hover каждые 90 с).
- **Автосохранение настроек:** кнопки «Сохранить» нет. `settings_from_ui`
  валидирует RAM/разрешение/путь/Java/accent, `autosave_timer` (150 мс опрос,
  500 мс стабильности) шлёт `SaveSettings`. Невалидный снапшот не пишется.
- **Телеметрия:** `telemetryEnabled` (по умолчанию false) + Sentry. DSN только
  из `VESTALIS_SENTRY_DSN`/`SENTRY_DSN`, в бинарь не вшивать. `before_send`
  удаляет token/password/secret/credential/hwid/authorization/savedLogin/user/
  email/login, чистит `C:\Users\<name>` и лог-хвост.
- **Онбординг:** `onboardingCompleted`; показывается после сплэша, старые state
  мигрируют в completed. «Пройти обучение заново» сбрасывает флаг.
- **Пикер цвета:** HSV-модал + `screen_picker.rs` (topmost GDI-лупа,
  `StretchBlt`, клик выбирает, Esc/ПКМ отменяет). `ChooseColorW` не возвращать.
- **Завершение игры:** `launch::spawn` возвращает `Child`; поток ждёт
  `child.wait()`, шлёт `Event::GameExited`, main сбрасывает Discord/статус и
  восстанавливает свёрнутое окно.
- **Отмена запуска:** `Worker.launch_cancel` (`Arc<AtomicBool>`),
  `Command::CancelLaunch`; `sync_profile` проверяет флаг между файлами и внутри
  загрузки, удаляет `.tmp`; `play_inner` не спавнит игру после отмены. Флаг
  сбрасывать в начале `play`.
- **Кэши:** аватары/иконки/новости/фоны чистить `storage::cleanup_cache_dir`
  (30 дней, лимиты), писать только tmp→rename, битые файлы игнорировать.
- **Диагностика:** ZIP на рабочий стол (логи, версия, темы, санитизированный
  state). Секреты в исходники не класть.
- **Rust/Slint:** в `regex` нет look-around — не тащить .NET-паттерны с
  `(?<!…)`/`(?=…)`. Feature `svg` включается через прямую зависимость
  `i-slint-core`; UI-поток обязан остаться главным (`ui.run()`), Tokio — фон.

## Ещё не сделано

- Витрина/страница скинов (голова игрока уже рендерится).
- Установщик и подпись кода.
- Точный morph сплэш → окно как в старой версии (сейчас анимированный стартовый
  оверлей).
- Ошибки/статусы, которые приходят с бэкенда, остаются на языке бэкенда;
  статические строки локализованы в `src/locale.rs`.
- macOS/Linux не поддерживаются: окно, HWID и DWM — Windows-only.

## Стиль работы

- Не читать большие каталоги целиком — только точечный `grep`/`read` по
  конкретным путям.
- Сборку/тесты выводить хвостом (`Select-Object -Last 4`), не заливать лог в
  контекст.
- Держать изменения в стиле окружающего кода, комментарии по-английски.
