# AGENTS.md — рабочие заметки по GML Launcher (Vestalis fork)

Короткая карта проекта, чтобы не перечитывать весь код. Держать этот файл в актуальном
состоянии: сюда дописываются решения, подводные камни и проверенные команды.

## Что это

Форк GML Launcher (Avalonia 11.3.3, .NET 8) для проекта Vestalis. Клиентская логика
(загрузка файлов, авторизация, SignalR) лежит в git-сабмодуле `src/Gml.Client`.

- Решение: `Gml.Launcher.sln`
- Лаунчер: `src/Gml.Launcher`
- Клиент (сабмодуль): `src/Gml.Client/src/Gml.Client`, интерфейсы: `src/Gml.Client/src/Gml.Client.Interfaces`
- Тесты клиента: `src/Gml.Client/Tests/Gml.Client.Tests` (8 тестов)

## Сборка и запуск

```powershell
# Обязательно: остановить запущенный лаунчер, иначе MSB3027/MSB3021 (файл занят)
Stop-Process -Name Gml.Launcher -Force -ErrorAction SilentlyContinue

dotnet build Gml.Launcher.sln -v q 2>&1 | Select-Object -Last 4
dotnet test Gml.Launcher.sln --no-build -v q 2>&1 | Select-Object -Last 2

# Запуск (старт ~35–50 с: сплэш, загрузка данных)
Start-Process 'src\Gml.Launcher\bin\Debug\net8.0\Gml.Launcher.exe'
```

Локальные данные: `%APPDATA%\Vestalis\data.db` (SQLite, таблица `StorageItems`, колонки
`Key`, `Value` — JSON). Ключи: `Settings`, `ServerHost`, `LastSelectedProfile`,
`SavedCredentials`, `UserInfo`. Пока лаунчер запущен — читать копию файла.

Быстро посмотреть содержимое (Python 3 есть в системе):
`python %LOCALAPPDATA%\Temp\opencode\read_db.py`

## Серверы

Хосты заданы в `src/Gml.Launcher/Assets/Resources/ResourceKeysDictionary.cs`:

- `ServerRu1Host` — `https://gmlfrr.vestalis.xyz/`
- `ServerRu2Host` — `https://msk1.gmlfr.vestalis.xyz/`
- `ServerEu1Host` — `https://gmlfr.vestalis.xyz/`
- `ServerPl1Host` / `Host` — `https://201-58-180-2.sslip.io/`

## Карта кода (главное)

| Что | Где |
| --- | --- |
| Точка входа, DI | `Program.cs`, `App.axaml.cs`, `Core/Extensions/ServiceLocator.cs` |
| Логин, 2FA, HWID | `ViewModels/Pages/LoginPageViewModel.cs`, `Core/Services/AutoLoginService.cs` |
| Настройки | `ViewModels/Pages/SettingsPageViewModel.cs`, `Core/Services/SettingsService.cs` |
| Главная, профили, новости | `ViewModels/Pages/OverviewPageViewModel.cs` |
| Список профилей | `ViewModels/Components/ListViewModel.cs`, `Views/Components/ServerListComponent.axaml` |
| Выбор сервера | `ViewModels/MainWindowViewModel.cs`, `Views/Components/ServerSelectorComponent.axaml` |
| Звуки интерфейса | `Core/Services/SoundService.cs` |
| Система / HWID из железа | `Core/Services/SystemService.cs` |
| SignalR | `src/Gml.Client/.../Helpers/SignalRConnect.cs`, `GmlClientManager.cs` |
| Загрузка/валидация файлов | `src/Gml.Client/.../Helpers/Files/*`, `ApiProcedures.cs` |

## Решения и подводные камни (не ломать)

- **Самообновление удалено.** Лаунчер не дергает `/api/v1/launcher`, не скачивает и не
  перезапускает себя. Обновления — вручную с сайта. В `Gml.Client` удалён старый API
  (`SkipUpdate`, `GetActualVersion`, `UpdateCurrentLauncher`, `LauncherUpdater` и т.п.).
- **HWID обязателен для нового бэкенда.** Без заголовка `X-HWID` авторизация отдаёт 403
  («HWID отключена или недействительна»). Поэтому `SettingsService.GetHwidForAuthAsync`
  никогда не возвращает `null`: при включённом тумблере уходит реальный HWID
  (`CPU-плата-диски`), при выключенном — **новый случайный GUID на каждый запрос**
  (`SystemService.GetRandomHwid`), чтобы не светить серийники и чтобы бан, привязанный
  к конкретному идентификатору, не залипал. Не возвращать `null` обратно.
- **Звуки не должны играть от фоновых событий.** Бэкенд по SignalR присылает
  `RefreshProfiles`; перестройка списка профилей поднимает `SelectionChanged`, как от клика.
  Лечится `SoundService.Suppress()` вокруг перестройки коллекций + проверкой
  `TopLevel.GetTopLevel(control) is Window { IsActive: true }`. `ProfilesChanges`
  маршалится через `ObserveOn(RxApp.MainThreadScheduler)`.
- **UI-поток.** Подписки Rx, меняющие UI, переводить на `RxApp.MainThreadScheduler`;
  анимации/изменения из других потоков — через `Dispatcher.UIThread.CheckAccess()/Post`.
  Симптом нарушения: `InvalidOperationException: Call from invalid thread`.
- **Опциональные моды** хранятся с суффиксом `.disabled`; при выключенном моде сервер
  всё равно отдаёт имя без суффикса. Логика — в `HashValidationDecorator` /
  `ApiProcedures.DownloadFiles`.
- **Кэш валидации файлов** — `Helpers/Files/ValidationCache.cs`
  (`%LOCALAPPDATA%\Gml.Launcher\validation-cache-v1.json`): доверяет SHA-1, пока совпадают
  размер и `LastWriteTimeUtc`. Формат файлов профиля меняется — кэш можно смело удалять.
- **Трассировка запуска** — `Gml.Client.Interfaces/Helpers/LaunchTrace.cs`, включается
  переменной окружения `GML_LAUNCH_TRACE=1`, пишет в
  `%LOCALAPPDATA%\Gml.Launcher\launch-trace.log`. По умолчанию выключена.
- **Индикатор режима бэкенда.** `IBackendChecker.GetBackendModeAsync` раз в 20 с дергает
  `GET /api/v1/mirror/mode` (у основного бэкенда такого эндпоинта нет — тогда режим
  `direct`). `MainWindowViewModel` красит бейдж в правом верхнем углу: `ЗЕРКАЛО`
  (local, зелёный), `ПРОКСИ` (proxy, оранжевый), `ОСНОВНОЙ` (direct, синий). Ошибки
  сети не должны валить лаунчер: бейдж просто скрывается. Значение хранится в
  `GlobalVariables.BackendMode`, в `data.db` не пишется.
- **Git.** Сабмодуль коммитится отдельно, потом обновляется указатель в корне
  (`git add src/Gml.Client`). Локальные правки хостов в `ResourceKeysDictionary.cs`
  не коммитить без просьбы.

## Проверка UI (живой лаунчер)

Окно `Gml.Launcher`, координаты клика = смещение окна + координаты на скриншоте.
Готовые хелперы (вне репозитория): `%LOCALAPPDATA%\Temp\opencode\`
`shot.ps1` (скрин окна), `input.ps1 -X -Y` (клик), `focus.ps1` (вывести окно вперёд),
`read_db.py` (дамп `data.db`). Если окно перекрыто — сначала `focus.ps1`, иначе
`CopyFromScreen` снимет чужое окно.

## Стиль работы

- Не читать большие каталоги целиком (там runtime Java и тысячи файлов) — только
  точечный `grep`/`read` по конкретным путям.
- Сборку/тесты выводить хвостом (`Select-Object -Last 4`), не заливать лог в контекст.
- Держать изменения в стиле окружающего кода, комментарии по-английски.
