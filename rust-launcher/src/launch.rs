use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{anyhow, Context, Result};
use regex::Regex;

use crate::model::{ProfileInfo, Settings};

pub struct LaunchSpec {
    pub java: PathBuf,
    pub arguments: String,
    pub working_dir: PathBuf,
}

pub fn build_spec(
    info: &ProfileInfo,
    install_dir: &Path,
    settings: &Settings,
    online: bool,
    base: &str,
    xms: u32,
    xmx: u32,
) -> Result<LaunchSpec> {
    let root = install_dir.to_string_lossy().to_string();

    let manifest_java = info.java_path.replace("{localPath}", &root);
    let custom_java = settings.custom_java.trim();
    let java = if custom_java.is_empty() {
        PathBuf::from(manifest_java.replace('/', "\\"))
    } else {
        PathBuf::from(custom_java)
    };

    if !java.is_file() {
        return Err(anyhow!(
            "Java не найдена: {}. Проверьте папку установки или переустановите профиль.",
            java.display()
        ));
    }

    let mut arguments = info.arguments.replace("{localPath}", &root);

    if online {
        arguments = apply_auth_endpoint(&arguments, base);
    } else {
        arguments = remove_authlib_agent(&arguments);
    }

    arguments = configure_heap(&arguments, &settings.custom_jvm, xms, xmx);

    let working_dir = install_dir.join(&info.relative_path);

    Ok(LaunchSpec {
        java,
        arguments,
        working_dir,
    })
}

pub fn spawn(spec: &LaunchSpec) -> Result<u32> {
    if !spec.working_dir.is_dir() {
        std::fs::create_dir_all(&spec.working_dir)
            .with_context(|| format!("создание {}", spec.working_dir.display()))?;
    }

    let mut command = Command::new(&spec.java);
    command
        .current_dir(&spec.working_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.raw_arg(&spec.arguments);
        // CREATE_NO_WINDOW: do not flash a console window for javaw.exe.
        command.creation_flags(0x0800_0000);
    }

    #[cfg(not(windows))]
    {
        for argument in split_arguments(&spec.arguments) {
            command.arg(argument);
        }
    }

    let mut child = command
        .spawn()
        .with_context(|| format!("запуск {}", spec.java.display()))?;
    let pid = child.id();

    std::thread::spawn(move || {
        let _ = child.wait();
    });

    Ok(pid)
}

fn apply_auth_endpoint(arguments: &str, base: &str) -> String {
    let endpoint = format!(
        "{}api/v1/integrations/authlib/minecraft",
        if base.ends_with('/') {
            base.to_string()
        } else {
            format!("{base}/")
        }
    );

    let with_placeholder = arguments.replace("{authEndpoint}", &endpoint);

    let pattern = Regex::new(r#"(?i)https?://[^\s"']+/api/v1/integrations/authlib/minecraft"#)
        .expect("valid regex");
    pattern
        .replace_all(&with_placeholder, endpoint.as_str())
        .into_owned()
}

fn remove_authlib_agent(arguments: &str) -> String {
    let pattern = Regex::new(
        r#"(?i)(?:^|\s)(?:"[^"]*authlib-injector[^"]*"|-javaagent:[^\s"]*authlib-injector[^\s"]*)"#,
    )
    .expect("valid regex");

    pattern.replace_all(arguments, " ").trim().to_string()
}

fn configure_heap(arguments: &str, custom_jvm: &str, xms: u32, xmx: u32) -> String {
    // Some manifests write `-Xmx 8G` with a space; consume the value too,
    // otherwise the leftover `8G` becomes a program argument.
    let heap = Regex::new(r"(?i)-Xm[sx](?:[0-9]+(?:[kKmMgGtT])?|\s+[0-9]+(?:[kKmMgGtT])?)")
        .expect("valid regex");

    let arguments = heap.replace_all(arguments, " ").trim().to_string();
    let custom = heap.replace_all(custom_jvm, " ").trim().to_string();

    let xmx = xmx.max(1024);
    let xms = xms.clamp(512, xmx);
    let memory = format!("-Xms{xms}M -Xmx{xmx}M");
    let injected = if custom.is_empty() {
        memory
    } else {
        format!("{memory} {custom}")
    };

    let anchor = Regex::new(r"(?i)(?:^|\s)(?:-cp|-classpath|--class-path|-jar|-m|--module)\s")
        .expect("valid regex");

    let mut result = match anchor.find(&arguments) {
        Some(found) => {
            let start = arguments[found.start()..]
                .find(|character: char| !character.is_whitespace())
                .map(|offset| found.start() + offset)
                .unwrap_or(found.start());

            let mut result = String::with_capacity(arguments.len() + injected.len() + 1);
            result.push_str(&arguments[..start]);
            result.push_str(&injected);
            result.push(' ');
            result.push_str(&arguments[start..]);
            result
        }
        None => {
            if arguments.is_empty() {
                injected
            } else {
                format!("{injected} {arguments}")
            }
        }
    };

    // Belt and braces: a malformed manifest must never leave the game with
    // -Xms but without an explicit maximum heap.
    let lower = result.to_lowercase();
    if !lower.contains("-xmx") {
        result = format!("-Xms{xms}M -Xmx{xmx}M {result}");
    }

    result
}

#[cfg(not(windows))]
fn split_arguments(arguments: &str) -> Vec<String> {
    arguments
        .split_whitespace()
        .map(|value| value.trim_matches('"').to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heap_flags_replace_existing_values() {
        let result = configure_heap("-Xmx2G -jar client.jar", "-XX:+UseZGC", 2048, 8192);
        assert!(result.contains("-Xms2048M -Xmx8192M -XX:+UseZGC -jar client.jar"));
        assert!(!result.contains("-Xmx2G"));
    }

    #[test]
    fn auth_endpoint_replaces_cached_host() {
        let result = apply_auth_endpoint(
            "-javaagent:authlib-injector.jar=https://old.example/api/v1/integrations/authlib/minecraft",
            "https://new.example/",
        );
        assert!(result.contains("https://new.example/api/v1/integrations/authlib/minecraft"));
        assert!(!result.contains("old.example"));
    }

    #[test]
    fn spaced_heap_flags_are_removed_entirely() {
        let result = configure_heap("-Xmx 4G -jar client.jar", "", 2048, 8192);
        assert!(result.contains("-Xms2048M -Xmx8192M -jar client.jar"));
        assert!(!result.contains(" 4G"));
    }

    #[test]
    fn custom_java_overrides_manifest_path() {
        let root = std::env::temp_dir().join(format!("vestalis-java-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let java = root.join("javaw.exe");
        std::fs::write(&java, b"stub").unwrap();

        let info = ProfileInfo {
            java_path: "{localPath}/missing/javaw.exe".into(),
            relative_path: String::new(),
            arguments: "-jar client.jar".into(),
            files: Vec::new(),
            white_list_files: Vec::new(),
        };
        let settings = Settings {
            custom_java: java.to_string_lossy().into_owned(),
            ..Settings::default()
        };

        let spec = build_spec(&info, &root, &settings, false, "", 1024, 2048).unwrap();
        assert_eq!(spec.java, java);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn offline_removes_agent() {
        let result =
            remove_authlib_agent("-javaagent:C:\\a\\authlib-injector.jar=http://x -jar a.jar");
        assert!(!result.to_lowercase().contains("authlib"));
        assert!(result.contains("-jar a.jar"));
    }
}
