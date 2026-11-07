//! Built-in interface translations. Theme text overrides are applied on top of
//! these values, so a theme can still restyle user-facing labels.

#[allow(dead_code)]
pub const LANGUAGES: [(&str, &str); 3] =
    [("ru", "Русский"), ("en", "English"), ("ua", "Українська")];

pub fn normalize(code: &str) -> &'static str {
    match code.trim().to_ascii_lowercase().as_str() {
        "en" => "en",
        "ua" | "uk" => "ua",
        _ => "ru",
    }
}

fn pick<'a>(lang: &str, ru: &'a str, en: &'a str, ua: &'a str) -> &'a str {
    match normalize(lang) {
        "en" => en,
        "ua" => ua,
        _ => ru,
    }
}

/// Returns the built-in label for `key`. Unknown keys fall back to the key
/// itself so a missing translation is visible instead of empty.
pub fn get(lang: &str, key: &str) -> &'static str {
    match key {
        "app.title" => pick(lang, "Vestalis Launcher", "Vestalis Launcher", "Vestalis Launcher"),
        "app.subtitle" => pick(lang, "Femboy", "Femboy", "Femboy"),
        "login.title" => pick(lang, "Вход в аккаунт", "Sign in", "Вхід в акаунт"),
        "login.subtitle" => pick(
            lang,
            "Войдите, чтобы играть на серверах Vestalis",
            "Sign in to play on Vestalis servers",
            "Увійдіть, щоб грати на серверах Vestalis",
        ),
        "login.submit" => pick(lang, "Войти", "Sign in", "Увійти"),
        "login.offline" => pick(lang, "Оффлайн-режим", "Offline mode", "Офлайн-режим"),
        "login.username" => pick(lang, "Логин", "Username", "Логін"),
        "login.password" => pick(lang, "Пароль", "Password", "Пароль"),
        "login.hwid" => pick(lang, "Отправлять HWID", "Send HWID", "Надсилати HWID"),
        "login.2fa" => pick(
            lang,
            "Код двухфакторной аутентификации",
            "Two-factor authentication code",
            "Код двофакторної аутентифікації",
        ),
        "login.2fa_submit" => pick(lang, "Подтвердить код", "Confirm code", "Підтвердити код"),
        "login.busy" => pick(lang, "Подключение…", "Connecting…", "Підключення…"),
        "login.checking" => pick(lang, "Проверка…", "Checking…", "Перевірка…"),
        "login.profile_select" => pick(lang, "Выберите профиль", "Select a profile", "Виберіть профіль"),

        "buttons.play" => pick(lang, "Играть", "Play", "Грати"),
        "buttons.settings" => pick(lang, "Настройки", "Settings", "Налаштування"),
        "buttons.logout" => pick(lang, "Выйти", "Log out", "Вийти"),
        "buttons.refresh" => pick(lang, "Обновить", "Refresh", "Оновити"),
        "buttons.mods" => pick(lang, "Моды", "Mods", "Моди"),
        "buttons.save" => pick(lang, "Сохранить", "Save", "Зберегти"),
        "buttons.apply" => pick(lang, "Применить", "Apply", "Застосувати"),
        "buttons.pick" => pick(lang, "Пипетка", "Pick", "Піпетка"),
        "buttons.load" => pick(lang, "Загрузить", "Load", "Завантажити"),
        "buttons.delete" => pick(lang, "Удалить", "Delete", "Видалити"),
        "buttons.rename" => pick(lang, "Переименовать", "Rename", "Перейменувати"),
        "buttons.download" => pick(lang, "Скачать", "Download", "Завантажити"),
        "buttons.downloading" => pick(lang, "Скачивание…", "Downloading…", "Завантаження…"),

        "sections.profiles" => pick(lang, "ПРОФИЛИ", "PROFILES", "ПРОФІЛІ"),
        "profiles.search" => pick(lang, "Поиск профилей", "Search profiles", "Пошук профілів"),
        "profiles.not_found" => pick(
            lang,
            "Ничего не найдено",
            "Nothing found",
            "Нічого не знайдено",
        ),
        "profiles.empty" => pick(
            lang,
            "Профили не загружены",
            "Profiles not loaded",
            "Профілі не завантажені",
        ),
        "settings.interface.test_sound" => pick(
            lang,
            "Проверить звук",
            "Test sound",
            "Перевірити звук",
        ),
        "sections.news" => pick(lang, "НОВОСТИ", "NEWS", "НОВИНИ"),

        "settings.title" => pick(lang, "Настройки", "Settings", "Налаштування"),
        "settings.back" => pick(lang, "← Назад", "← Back", "← Назад"),
        "settings.memory.title" => pick(lang, "Память", "Memory", "Пам'ять"),
        "settings.memory.desc" => pick(
            lang,
            "Выделите оперативную память под Minecraft. Оставьте запас системе: ",
            "Memory allocated to Minecraft. Keep headroom for the system: ",
            "Виділіть оперативну пам'ять під Minecraft. Залиште запас системі: ",
        ),
        "settings.memory.xms" => pick(lang, "Минимум (Xms)", "Minimum (Xms)", "Мінімум (Xms)"),
        "settings.memory.xmx" => pick(lang, "Максимум (Xmx)", "Maximum (Xmx)", "Максимум (Xmx)"),
        "settings.jvm.title" => pick(
            lang,
            "Дополнительные параметры JVM",
            "Extra JVM arguments",
            "Додаткові параметри JVM",
        ),
        "settings.jvm.desc" => pick(
            lang,
            "Без параметров памяти: -Xms/-Xmx задаются выше.",
            "Do not add memory flags: -Xms/-Xmx are set above.",
            "Без параметрів пам'яті: -Xms/-Xmx задаються вище.",
        ),
        "settings.game.title" => pick(lang, "Игра", "Game", "Гра"),
        "settings.game.fullscreen" => pick(
            lang,
            "Полноэкранный режим",
            "Fullscreen",
            "Повноекранний режим",
        ),
        "settings.game.resolution" => pick(lang, "Разрешение игры", "Game resolution", "Роздільність гри"),
        "settings.game.java" => pick(
            lang,
            "Java (пусто — из манифеста профиля)",
            "Java (empty — from the profile manifest)",
            "Java (порожньо — з маніфесту профілю)",
        ),
        "settings.interface.auto_login" => pick(
            lang,
            "Автовход с сохранёнными данными",
            "Sign in automatically with saved credentials",
            "Автовхід із збереженими даними",
        ),
        "settings.interface.title" => pick(lang, "Интерфейс", "Interface", "Інтерфейс"),
        "settings.interface.sounds" => pick(
            lang,
            "Звуки интерфейса",
            "Interface sounds",
            "Звуки інтерфейсу",
        ),
        "settings.interface.hwid" => pick(
            lang,
            "Отправлять реальный HWID",
            "Send real HWID",
            "Надсилати реальний HWID",
        ),
        "settings.interface.hide_on_launch" => pick(
            lang,
            "Скрывать лаунчер при запуске игры",
            "Hide launcher when the game starts",
            "Ховати лаунчер під час запуску гри",
        ),
        "settings.interface.background" => pick(
            lang,
            "Фоновый эффект",
            "Background effect",
            "Фоновий ефект",
        ),
        "settings.interface.effects" => pick(
            lang,
            "Фон: выкл / светлячки / листья / снег",
            "Background: off / fireflies / leaves / snow",
            "Фон: викл / світлячки / листя / сніг",
        ),
        "settings.appearance.title" => pick(lang, "Оформление", "Appearance", "Оформлення"),
        "settings.appearance.desc" => pick(
            lang,
            "Тема применяется сразу. Пользовательские темы берутся из папки themes рядом с state.json.",
            "Themes apply instantly. Custom themes live in the themes folder next to state.json.",
            "Тема застосовується одразу. Власні теми беруться з папки themes поруч зі state.json.",
        ),
        "settings.appearance.mode" => pick(
            lang,
            "Режим оформления",
            "Appearance mode",
            "Режим оформлення",
        ),
        "settings.appearance.accent_hint" => pick(
            lang,
            "Material 3 использует цвет Windows; при недоступности — accent темы.",
            "Material 3 uses the Windows accent; falls back to the theme accent.",
            "Material 3 використовує колір Windows; за недоступності — accent теми.",
        ),
        "settings.appearance.windows" => pick(lang, "Тема Windows", "Windows theme", "Тема Windows"),
        "settings.appearance.manual" => pick(
            lang,
            "Ручной accent для Material 3",
            "Manual accent for Material 3",
            "Ручний accent для Material 3",
        ),
        "settings.appearance.follow" => pick(
            lang,
            "Следовать светлой/тёмной теме Windows",
            "Follow Windows light/dark mode",
            "Слідувати світлій/темній темі Windows",
        ),
        "settings.appearance.light" => pick(lang, "Светлая тема", "Light theme", "Світла тема"),
        "settings.appearance.dark" => pick(lang, "Тёмная тема", "Dark theme", "Темна тема"),
        "settings.appearance.builtin" => pick(lang, "встроенная", "built-in", "вбудована"),
        "settings.appearance.custom" => pick(lang, "своя", "custom", "власна"),
        "settings.appearance.refresh" => pick(
            lang,
            "Обновить темы",
            "Refresh themes",
            "Оновити теми",
        ),
        "settings.appearance.open" => pick(
            lang,
            "Открыть папку тем",
            "Open themes folder",
            "Відкрити папку тем",
        ),
        "settings.appearance.reset" => pick(lang, "Сбросить тему", "Reset theme", "Скинути тему"),
        "settings.language.title" => pick(lang, "Язык", "Language", "Мова"),
        "settings.language.hint" => pick(
            lang,
            "Тексты темы имеют приоритет над переводом.",
            "Theme text overrides take priority over the translation.",
            "Тексти теми мають пріоритет над перекладом.",
        ),

        "layout.server" => pick(lang, "Выбор сервера", "Server selector", "Вибір сервера"),
        "layout.user" => pick(lang, "Карточка пользователя", "User card", "Картка користувача"),
        "layout.profile_item" => pick(lang, "Список профилей", "Profile list", "Список профілів"),
        "layout.countdown" => pick(lang, "Отсчёт «Недрочабрь»", "Countdown", "Відлік"),
        "layout.sidebar_actions" => pick(
            lang,
            "Настройки / Выйти",
            "Settings / Log out",
            "Налаштування / Вийти",
        ),
        "layout.news" => pick(lang, "Новости", "News", "Новини"),
        "layout.main_actions" => pick(
            lang,
            "Играть / Обновить / Моды",
            "Play / Refresh / Mods",
            "Грати / Оновити / Моди",
        ),
        "layout.pinned" => pick(lang, "закреплён", "pinned", "закріплено"),
        "layout.hidden" => pick(lang, "скрыт", "hidden", "приховано"),
        "layout.visible" => pick(lang, "виден", "visible", "видимий"),
        "layout.empty" => pick(lang, "пусто", "empty", "порожньо"),
        "layout.title" => pick(lang, "Layout", "Layout", "Layout"),
        "layout.desc" => pick(
            lang,
            "Перетаскивайте блоки между зонами и на нужную строку; стрелки и переключатель видимости остаются как запасной вариант. Выбор сервера закреплён в шапке.",
            "Drag blocks between zones or onto a row; arrows and the visibility switch remain as a fallback. The server selector is pinned to the header.",
            "Перетягуйте блоки між зонами або на потрібний рядок; стрілки й перемикач видимості лишаються як запасний варіант. Вибір сервера закріплено в шапці.",
        ),
        "layout.zone_left" => pick(lang, "Левый сайдбар", "Left sidebar", "Лівий сайдбар"),
        "layout.zone_right" => pick(lang, "Правый сайдбар", "Right sidebar", "Правий сайдбар"),
        "layout.zone_header" => pick(lang, "Шапка", "Header", "Шапка"),
        "layout.zone_content_top" => pick(lang, "Верх контента", "Content top", "Верх контенту"),
        "layout.zone_content_bottom" => pick(lang, "Низ контента", "Content bottom", "Низ контенту"),
        "layout.zone_footer" => pick(lang, "Подвал", "Footer", "Підвал"),
        "layout.widths" => pick(lang, "Ширина сайдбаров", "Sidebar width", "Ширина сайдбарів"),
        "layout.width_left" => pick(lang, "Слева", "Left", "Зліва"),
        "layout.width_right" => pick(lang, "Справа", "Right", "Справа"),
        "layout.profiles" => pick(lang, "Профили layout", "Layout profiles", "Профілі layout"),
        "layout.active" => pick(lang, "Активный профиль", "Active profile", "Активний профіль"),
        "layout.name_placeholder" => pick(
            lang,
            "Название профиля",
            "Profile name",
            "Назва профілю",
        ),
        "layout.save_profile" => pick(
            lang,
            "Сохранить профиль",
            "Save profile",
            "Зберегти профіль",
        ),
        "layout.reset" => pick(lang, "Сбросить layout", "Reset layout", "Скинути layout"),

        "mods.title" => pick(lang, "Моды", "Mods", "Моди"),
        "mods.count" => pick(lang, "включено", "enabled", "увімкнено"),
        "mods.refresh" => pick(lang, "Обновить", "Refresh", "Оновити"),
        "mods.desc" => pick(
            lang,
            "Опциональные моды. Выключенные файлы хранятся с суффиксом .disabled.",
            "Optional mods. Disabled files are stored with the .disabled suffix.",
            "Опціональні моди. Вимкнені файли зберігаються з суфіксом .disabled.",
        ),
        "mods.empty" => pick(
            lang,
            "Для этого профиля нет опциональных модов.",
            "This profile has no optional mods.",
            "Для цього профілю немає опціональних модів.",
        ),
        "mods.search" => pick(lang, "Поиск модов", "Search mods", "Пошук модів"),
        "mods.not_found" => pick(lang, "Ничего не найдено", "Nothing found", "Нічого не знайдено"),
        "mods.folder" => pick(lang, "Папка модов", "Mods folder", "Папка модів"),
        "mods.download_missing" => pick(
            lang,
            "Скачать отсутствующие",
            "Download missing",
            "Завантажити відсутні",
        ),
        "mods.enable_all" => pick(lang, "Включить все", "Enable all", "Увімкнути всі"),
        "mods.disable_all" => pick(lang, "Выключить все", "Disable all", "Вимкнути всі"),
        "buttons.cancel" => pick(lang, "Отмена", "Cancel", "Скасувати"),
        "settings.interface.discord" => pick(
            lang,
            "Discord Rich Presence",
            "Discord Rich Presence",
            "Discord Rich Presence",
        ),
        "settings.utility.install" => pick(
            lang,
            "Папка установки",
            "Install folder",
            "Папка встановлення",
        ),
        "settings.utility.logs" => pick(lang, "Логи", "Logs", "Логи"),
        "settings.utility.reset_window" => pick(
            lang,
            "Сбросить окно",
            "Reset window",
            "Скинути вікно",
        ),
        "settings.utility.check_files" => pick(
            lang,
            "Проверить файлы",
            "Verify files",
            "Перевірити файли",
        ),
        "settings.utility.clear_cache" => pick(
            lang,
            "Очистить кэш",
            "Clear cache",
            "Очистити кеш",
        ),
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_codes_are_normalized() {
        assert_eq!(normalize("EN"), "en");
        assert_eq!(normalize("uk"), "ua");
        assert_eq!(normalize("de"), "ru");
    }

    #[test]
    fn every_language_has_a_translation() {
        for (code, _) in LANGUAGES {
            assert_ne!(get(code, "buttons.play"), "buttons.play");
        }
    }
}
