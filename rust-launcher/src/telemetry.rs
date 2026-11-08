//! Optional Sentry integration. Disabled by default and controlled by
//! `settings.telemetryEnabled`. The DSN is never embedded in the binary; it is
//! read from `VESTALIS_SENTRY_DSN` (or `SENTRY_DSN`).
//!
//! Telemetry is deliberately limited to a sanitized panic/error event, the
//! launcher version/OS and a short log tail. Tokens, credentials, HWID, user
//! names and local user paths are removed before sending.

use std::cell::RefCell;
use std::sync::Arc;

use regex::Regex;
use sentry::protocol::Event;
use serde_json::Value;

thread_local! {
    static CLIENT: RefCell<Option<sentry::ClientInitGuard>> = const { RefCell::new(None) };
}

/// Enables or disables Sentry for the current process. Safe to call repeatedly;
/// initialization or transport failures never propagate to the launcher.
pub fn configure(enabled: bool) {
    CLIENT.with(|client| {
        if !enabled {
            let _ = client.borrow_mut().take();
            return;
        }
        if client.borrow().is_some() {
            return;
        }
        if let Some(guard) = client_from_env() {
            *client.borrow_mut() = Some(guard);
        }
    });
}

fn client_from_env() -> Option<sentry::ClientInitGuard> {
    let dsn = std::env::var("VESTALIS_SENTRY_DSN")
        .or_else(|_| std::env::var("SENTRY_DSN"))
        .ok()?;
    let dsn: sentry::types::Dsn = dsn.parse().ok()?;

    let options = sentry::ClientOptions {
        dsn: Some(dsn),
        release: Some(format!("vestalis-launcher@{}", env!("CARGO_PKG_VERSION")).into()),
        environment: Some(
            std::env::var("VESTALIS_ENV")
                .unwrap_or_else(|_| "production".to_string())
                .into(),
        ),
        attach_stacktrace: true,
        send_default_pii: false,
        before_send: Some(Arc::new(|mut event| {
            sanitize_event(&mut event);
            Some(event)
        })),
        ..Default::default()
    };

    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sentry::init(options))).ok()
}

fn sanitize_event(event: &mut Event<'static>) {
    event.user = None;
    event.request = None;

    if let Some(tail) = recent_log_tail(40) {
        event
            .extra
            .insert("log_tail".to_string(), Value::String(tail));
    }

    // Serialize/deserialize the whole event so messages, exceptions, extra,
    // breadcrumbs and contexts all go through the same scrubber.
    if let Ok(value) = serde_json::to_value(&*event) {
        let mut value = value;
        scrub_json(&mut value);
        if let Ok(cleaned) = serde_json::from_value::<Event<'static>>(value) {
            *event = cleaned;
        }
    }
}

fn recent_log_tail(lines: usize) -> Option<String> {
    let dir = crate::model::local_app_dir().join("logs");
    let mut newest: Option<(std::time::SystemTime, std::path::PathBuf)> = None;
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Ok(modified) = entry.metadata().and_then(|meta| meta.modified()) else {
            continue;
        };
        if newest
            .as_ref()
            .map(|(current, _)| modified > *current)
            .unwrap_or(true)
        {
            newest = Some((modified, path));
        }
    }

    let text = std::fs::read_to_string(newest?.1).ok()?;
    let tail: Vec<&str> = text.lines().rev().take(lines).collect();
    let mut result = tail.into_iter().rev().collect::<Vec<_>>().join("\n");
    result.truncate(8 * 1024);
    Some(scrub_text(&result))
}

fn sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    [
        "token",
        "password",
        "secret",
        "credential",
        "hwid",
        "authorization",
        "savedlogin",
        "user",
        "email",
        "login",
    ]
    .iter()
    .any(|needle| key.contains(needle))
}

pub fn scrub_json(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.retain(|key, _| !sensitive_key(key));
            for value in map.values_mut() {
                scrub_json(value);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(scrub_json),
        Value::String(text) => *text = scrub_text(text),
        _ => {}
    }
}

pub fn scrub_text(text: &str) -> String {
    let user_path = Regex::new(r#"(?i)[A-Z]:\\Users\\[^\\\s"]+"#).expect("valid regex");
    let slash_path = Regex::new(r#"(?i)/home/[^/\s"]+"#).expect("valid regex");
    let bearer = Regex::new(r#"(?i)bearer\s+[A-Za-z0-9._~+/-]+=*"#).expect("valid regex");
    let token_pair =
        Regex::new(r#"(?i)(access[_-]?token|password|secret|hwid|authorization)=([^\s&]+)"#)
            .expect("valid regex");

    let text = user_path.replace_all(text, r"C:\Users\<user>");
    let text = slash_path.replace_all(&text, "/home/<user>");
    let text = bearer.replace_all(&text, "Bearer <redacted>");
    let mut text = token_pair.replace_all(&text, "$1=<redacted>").into_owned();

    // Redact the current Windows account name when it appears outside a path
    // (for example in a custom log message).
    if let Ok(user) = std::env::var("USERNAME") {
        let user = user.trim();
        if user.len() >= 3 {
            if let Ok(pattern) = Regex::new(&format!("(?i){}", regex::escape(user))) {
                text = pattern.replace_all(&text, "<user>").into_owned();
            }
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn scrubber_removes_credentials_and_user_paths() {
        let user = std::env::var("USERNAME").unwrap_or_else(|_| "testuser".into());
        let user = if user.len() >= 3 {
            user
        } else {
            "testuser".to_string()
        };
        let mut value = json!({
            "accessToken": "abc",
            "password": "secret",
            "hwid": "CPU-123",
            "user": { "name": user },
            "message": format!("login failed for {user} at C:\\Users\\{user}\\AppData"),
            "nested": { "Authorization": "Bearer abc.def" }
        });

        scrub_json(&mut value);
        let text = value.to_string();

        assert!(!text.contains("abc"));
        assert!(!text.contains("secret"));
        assert!(!text.contains("CPU-123"));
        assert!(!text.contains(&user));
        assert!(text.contains("<user>"));
        assert!(text.contains("login failed"));
    }

    #[test]
    fn scrubber_redacts_token_pairs_and_bearer() {
        let text = scrub_text(
            "GET https://x/?access_token=TOKEN&x=1 authorization=Basic%20abc Bearer xyz.123",
        );
        assert!(!text.contains("TOKEN"));
        assert!(!text.contains("abc"));
        assert!(!text.contains("xyz.123"));
        assert!(text.contains("<redacted>"));
    }
}
