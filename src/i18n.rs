//! Configurable UI language, independent of machine-readable subprocess output.
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const DEFAULTS: &str = include_str!("../config/languages.json");
static RUNTIME: OnceLock<Runtime> = OnceLock::new();
static LEGACY_KEYS: OnceLock<BTreeMap<String, String>> = OnceLock::new();

// Keep 0.12.0 user translations and internal process-state identifiers readable.
fn canonical_key(source: &str) -> &str {
    LEGACY_KEYS
        .get_or_init(|| {
            serde_json::from_str(include_str!("../config/legacy-language-keys.json"))
                .expect("embedded legacy language keys")
        })
        .get(source)
        .map(String::as_str)
        .unwrap_or(source)
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Language {
    #[serde(default)]
    decimal_separator: Option<String>,
    #[serde(default)]
    messages: BTreeMap<String, String>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    #[serde(default = "automatic")]
    language: String,
    #[serde(default)]
    languages: BTreeMap<String, Language>,
}
fn automatic() -> String {
    "auto".into()
}
impl Config {
    fn defaults() -> Self {
        serde_json::from_str(DEFAULTS).expect("embedded language configuration")
    }
    fn merge(&mut self, custom: Self) -> Vec<String> {
        self.language = custom.language;
        let mut warnings = Vec::new();
        for (code, custom_language) in custom.languages {
            if code.is_empty()
                || code != normalize(&code)
                || !code.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                warnings.push(format!("Invalid language code: {code}"));
                continue;
            }
            let mut accepted = BTreeMap::new();
            let explicit_keys: BTreeSet<_> = custom_language.messages.keys().cloned().collect();
            for (key, value) in custom_language.messages {
                let canonical = canonical_key(&key);
                // An explicit English key wins over an old alias in mixed files.
                if canonical != key && explicit_keys.contains(canonical) {
                    continue;
                }
                let Some(expected) = self.languages["en"].messages.get(canonical) else {
                    warnings.push(format!("Unknown translation key: {key}"));
                    continue;
                };
                if !value.trim().is_empty()
                    && placeholders(&value).is_some()
                    && placeholders(&value) == placeholders(expected)
                {
                    accepted.insert(canonical.to_owned(), value);
                } else {
                    warnings.push(format!(
                        "Invalid translation placeholders or empty text: {key}"
                    ));
                }
            }
            let language = self.languages.entry(code).or_insert_with(|| Language {
                decimal_separator: None,
                messages: BTreeMap::new(),
            });
            language.messages.extend(accepted);
            if let Some(separator) = custom_language.decimal_separator {
                if separator == "." || separator == "," {
                    language.decimal_separator = Some(separator);
                } else {
                    warnings.push("decimal_separator must be '.' or ','".into());
                }
            }
        }
        warnings
    }
    fn supported(&self, locale: &str) -> Option<String> {
        let mut code = normalize(locale);
        loop {
            if self.languages.contains_key(&code) {
                return Some(code);
            }
            match code.rsplit_once('_') {
                Some((base, _)) => code = base.to_owned(),
                None => return None,
            }
        }
    }
    fn select(&self, env: &impl Fn(&str) -> Option<String>) -> String {
        if self.language != "auto" {
            return self
                .supported(&self.language)
                .unwrap_or_else(|| "en".into());
        }
        let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .find_map(|name| env(name).filter(|v| !v.trim().is_empty()))
            .unwrap_or_else(|| "C".into());
        if matches!(normalize(&locale).as_str(), "c" | "posix") {
            return "en".into();
        }
        // GNU message language preferences apply only outside the C/POSIX locale.
        if let Some(preferences) = env("LANGUAGE").filter(|v| !v.trim().is_empty()) {
            return preferences
                .split(':')
                .find_map(|code| self.supported(code))
                .unwrap_or_else(|| "en".into());
        }
        self.supported(&locale).unwrap_or_else(|| "en".into())
    }
}
fn normalize(locale: &str) -> String {
    locale
        .trim()
        .split(['.', '@'])
        .next()
        .unwrap_or_default()
        .replace('-', "_")
        .to_ascii_lowercase()
}

struct Runtime {
    config: Config,
    language: String,
    // Preserve the region for localized desktop entries, while UI text may use a base code.
    desktop_locale: String,
    path: Option<PathBuf>,
    warnings: Vec<String>,
}
impl Runtime {
    fn text<'a>(&'a self, source: &'a str) -> &'a str {
        let source = canonical_key(source);
        self.config.languages[&self.language]
            .messages
            .get(source)
            .or_else(|| self.config.languages["en"].messages.get(source))
            .map(String::as_str)
            .unwrap_or(source)
    }
    fn decimal_separator(&self) -> &str {
        self.config.languages[&self.language]
            .decimal_separator
            .as_deref()
            .unwrap_or(".")
    }
}
pub fn config_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("TASK_MANAGER_LANGUAGE_CONFIG") {
        return Some(PathBuf::from(path));
    }
    user_config_directory(
        std::env::var_os("XDG_CONFIG_HOME"),
        std::env::var_os("HOME"),
    )
    .map(|directory| directory.join("linux-task-manager/languages.json"))
}
fn user_config_directory(
    xdg: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
) -> Option<PathBuf> {
    xdg.map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            home.map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .map(|p| p.join(".config"))
        })
}
fn read_config(path: &Path) -> Result<Config, String> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
        .map_err(|e| e.to_string())?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("Language configuration must be a regular file".into());
    }
    let mut data = String::new();
    file.take(4_000_001)
        .read_to_string(&mut data)
        .map_err(|e| e.to_string())?;
    if data.len() > 4_000_000 {
        return Err("Language configuration exceeds 4 MB".into());
    }
    serde_json::from_str(&data).map_err(|e| e.to_string())
}
fn create_config(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    // Never overwrite an existing user translation file.
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(DEFAULTS.as_bytes())
}
fn load(path: Option<PathBuf>, create: bool, env: &impl Fn(&str) -> Option<String>) -> Runtime {
    let mut config = Config::defaults();
    let mut warnings = Vec::new();
    if let Some(path) = &path {
        if !path.try_exists().unwrap_or(true) && create {
            match create_config(path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => warnings.push(format!("{}: {e}", path.display())),
            }
        }
        if path.try_exists().unwrap_or(true) {
            match read_config(path) {
                Ok(custom) => warnings.extend(config.merge(custom)),
                Err(e) => warnings.push(format!("{}: {e}", path.display())),
            }
        }
    } else if create {
        warnings.push("No configuration directory available".into());
    }
    if config.language != "auto" && config.supported(&config.language).is_none() {
        warnings.push(format!(
            "Unsupported configured language: {}",
            config.language
        ));
    }
    let language = config.select(env);
    let requested = if config.language != "auto" {
        config.language.clone()
    } else {
        env("LANGUAGE")
            .and_then(|s| {
                s.split(':')
                    .find(|s| config.supported(s).as_ref() == Some(&language))
                    .map(str::to_owned)
            })
            .or_else(|| {
                ["LC_ALL", "LC_MESSAGES", "LANG"]
                    .iter()
                    .find_map(|name| env(name).filter(|v| !v.trim().is_empty()))
            })
            .unwrap_or_default()
    };
    let desktop_locale = if config.supported(&requested).as_ref() == Some(&language) {
        normalize(&requested)
    } else {
        language.clone()
    };
    Runtime {
        config,
        language,
        desktop_locale,
        path,
        warnings,
    }
}
pub fn initialize(create: bool) {
    RUNTIME.get_or_init(|| load(config_path(), create, &|name| std::env::var(name).ok()));
}
fn current() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        // Existing model fixtures remain language independent during parallel unit tests.
        #[cfg(test)]
        return load(None, false, &|name| {
            (name == "LANG").then(|| "de_DE.UTF-8".into())
        });
        #[cfg(not(test))]
        load(config_path(), false, &|name| std::env::var(name).ok())
    })
}
pub fn text(source: &str) -> &str {
    current().text(source)
}
pub fn language() -> &'static str {
    &current().language
}
pub fn decimal(value: f64, digits: usize) -> String {
    format!("{value:.digits$}").replace('.', current().decimal_separator())
}
pub fn notice() -> Option<String> {
    let warnings = &current().warnings;
    if warnings.is_empty() {
        None
    } else {
        Some(format!(
            "{}\n\n{}",
            text(
                "Language configuration could not be fully used. Built-in translations remain available."
            ),
            warnings.join("\n")
        ))
    }
}
pub fn check() {
    let runtime = current();
    println!(
        "Language: {}\nConfiguration: {}\nDecimal separator: {}",
        runtime.language,
        runtime
            .path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(embedded defaults)".into()),
        runtime.decimal_separator()
    );
    if let Some(warning) = notice() {
        eprintln!("{warning}");
    }
}
pub fn desktop_keys(key: &str) -> Vec<String> {
    desktop_keys_for(key, &current().desktop_locale, language())
}
fn desktop_keys_for(key: &str, locale: &str, language: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut code = locale.to_owned();
    loop {
        // Desktop entry region identifiers are uppercase (de_DE, en_GB).
        let desktop_code = code
            .split_once('_')
            .map(|(base, region)| format!("{base}_{}", region.to_ascii_uppercase()))
            .unwrap_or_else(|| code.clone());
        result.push(format!("{key}[{desktop_code}]"));
        match code.rsplit_once('_') {
            Some((base, _)) => code = base.into(),
            None => break,
        }
    }
    if language != "en" {
        result.push(format!("{key}[en]"));
    }
    result.push(key.into());
    result
}

pub fn desktop_entry_text(field: &str, key: &str) -> String {
    let config = &current().config;
    let english = config.languages["en"]
        .messages
        .get(key)
        .map(String::as_str)
        .unwrap_or(key);
    let escape = |value: &str| {
        value
            .replace('\\', "\\\\")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t")
    };
    let mut result = format!("{field}={}\n", escape(english));
    for (code, language) in &config.languages {
        let code = code
            .split_once('_')
            .map(|(base, region)| format!("{base}_{}", region.to_ascii_uppercase()))
            .unwrap_or_else(|| code.clone());
        let value = language
            .messages
            .get(key)
            .map(String::as_str)
            .unwrap_or(english);
        result.push_str(&format!("{field}[{code}]={}\n", escape(value)));
    }
    result
}

fn placeholders(template: &str) -> Option<BTreeSet<usize>> {
    let mut found = BTreeSet::new();
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
            }
            '{' => {
                let mut token = String::new();
                loop {
                    match chars.next()? {
                        '}' => break,
                        c if c.is_ascii_digit() => token.push(c),
                        _ => return None,
                    }
                }
                found.insert(token.parse().ok()?);
            }
            '}' => return None,
            _ => {}
        }
    }
    Some(found)
}
pub fn render(template: &str, args: &[String]) -> String {
    let mut result = String::new();
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                result.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                result.push('}');
            }
            '{' => {
                let token: String = chars.by_ref().take_while(|c| *c != '}').collect();
                if let Some(value) = token.parse::<usize>().ok().and_then(|i| args.get(i)) {
                    result.push_str(value);
                }
            }
            _ => result.push(c),
        }
    }
    result
}
#[macro_export]
macro_rules! tr {
    ($source:literal) => {
        $crate::i18n::text($source)
    };
}
#[macro_export]
macro_rules! tf {
    ($source:literal $(, $value:expr)* $(,)?) => {
        $crate::i18n::render($crate::i18n::text($source), &[$(format!("{}", $value)),*])
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    fn environment<'a>(pairs: &'a [(&str, &str)]) -> impl Fn(&str) -> Option<String> + 'a {
        |name| {
            pairs
                .iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| v.to_string())
        }
    }
    #[test]
    fn system_locale_precedence_and_english_fallback() {
        let config = Config::defaults();
        for (pairs, expected) in [
            (vec![("LANG", "de_DE.UTF-8")], "de"),
            (vec![("LANG", "de-AT")], "de"),
            (vec![("LANG", "en_GB.UTF-8")], "en"),
            (vec![("LANG", "fr_FR.UTF-8")], "en"),
            (vec![("LANG", "de_DE"), ("LC_MESSAGES", "en_US")], "en"),
            (
                vec![
                    ("LANG", "en_US"),
                    ("LC_MESSAGES", "fr_FR"),
                    ("LC_ALL", "de_CH.UTF-8"),
                ],
                "de",
            ),
            (vec![("LANG", "en_US"), ("LANGUAGE", "fr:de:en")], "de"),
            (vec![("LANG", "de_DE"), ("LANGUAGE", "fr")], "en"),
            (
                vec![("LANG", "de_DE"), ("LC_ALL", "C.UTF-8"), ("LANGUAGE", "de")],
                "en",
            ),
            (vec![("LANG", "POSIX"), ("LANGUAGE", "de")], "en"),
            (vec![("LANG", "de_DE"), ("LC_ALL", "")], "de"),
            (vec![], "en"),
        ] {
            assert_eq!(config.select(&environment(&pairs)), expected, "{pairs:?}");
        }
    }
    #[test]
    fn configurable_languages_partial_translations_and_placeholders() {
        let mut config = Config::defaults();
        let custom = serde_json::from_str(r#"{"language":"auto","languages":{"fr":{"decimal_separator":",","messages":{"File":"Fichier","Applications ({0})":"Applications ({0})","Drivers":"","All ({0})":"Tous ({8})"}}}}"#).unwrap();
        assert_eq!(config.merge(custom).len(), 2);
        let locale = environment(&[("LANG", "fr_FR.UTF-8")]);
        let runtime = Runtime {
            language: config.select(&locale),
            desktop_locale: "fr_fr".into(),
            config,
            path: None,
            warnings: vec![],
        };
        assert_eq!(runtime.language, "fr");
        assert_eq!(runtime.text("File"), "Fichier");
        assert_eq!(runtime.text("Drivers"), "Drivers");
        assert_eq!(runtime.text("All ({0})"), "All ({0})");
        assert_eq!(
            render(runtime.text("Applications ({0})"), &["42".into()]),
            "Applications (42)"
        );
        assert_eq!(runtime.decimal_separator(), ",");
        let mut forced = runtime.config;
        forced.language = "de".into();
        assert_eq!(forced.select(&locale), "de");
        forced.language = "unknown".into();
        assert_eq!(forced.select(&locale), "en");
    }
    #[test]
    fn legacy_keys_preserve_translations_and_explicit_english_keys_win() {
        let mut config = Config::defaults();
        let custom = serde_json::from_str(
            r#"{"languages":{"fr":{"messages":{"Datei":"Ancien fichier","File":"Fichier","Anwendungen ({0})":"Applications ({0})","Wird ausgeführt":"En cours","Läuft":"Conteneur actif"}}}}"#,
        ).unwrap();
        assert!(config.merge(custom).is_empty());
        let runtime = Runtime {
            language: "fr".into(),
            desktop_locale: "fr".into(),
            config,
            path: None,
            warnings: vec![],
        };
        assert_eq!(runtime.text("File"), "Fichier");
        assert_eq!(runtime.text("Datei"), "Fichier");
        assert_eq!(runtime.text("Applications ({0})"), "Applications ({0})");
        assert_eq!(runtime.text("Wird ausgeführt"), "En cours");
        assert_eq!(runtime.text("Running (container)"), "Conteneur actif");
        assert_eq!(runtime.text("Bereit"), "Ready");
        assert_eq!(
            runtime.text("unknown external data"),
            "unknown external data"
        );
        let mut german = runtime;
        german.language = "de".into();
        assert_eq!(german.text("Running"), "Wird ausgeführt");
        assert_eq!(german.text("Running (container)"), "Läuft");
    }
    #[test]
    fn templates_allow_reordering_repetition_unicode_and_literal_braces() {
        assert_eq!(
            placeholders("{1} / {0} / {1} {{ok}}"),
            Some(BTreeSet::from([0, 1]))
        );
        for bad in ["{name}", "{}", "{0", "0}", "{999999999999999999999999999}"] {
            assert!(placeholders(bad).is_none());
        }
        assert_eq!(
            render("{1} / {0} / {1} {{ok}}", &["{1}".into(), "ä中文".into()]),
            "ä中文 / {1} / ä中文 {ok}"
        );
    }
    #[test]
    fn configuration_is_created_once_and_bad_files_are_preserved() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("settings/languages.json");
        let env = environment(&[("LANG", "en_US")]);
        let readonly = load(Some(path.clone()), false, &env);
        assert_eq!(readonly.language, "en");
        assert!(!path.exists());
        let created = load(Some(path.clone()), true, &env);
        assert!(created.warnings.is_empty());
        assert!(path.is_file());
        fs::write(&path, r#"{"language":"de"}"#).unwrap();
        assert_eq!(load(Some(path.clone()), true, &env).language, "de");
        fs::write(&path, "invalid JSON").unwrap();
        let broken = load(Some(path.clone()), true, &env);
        assert_eq!(broken.language, "en");
        assert_eq!(broken.warnings.len(), 1);
        assert_eq!(fs::read_to_string(&path).unwrap(), "invalid JSON");
        assert!(read_config(temp.path()).is_err());
        fs::write(&path, "x".repeat(4_000_001)).unwrap();
        assert!(read_config(&path).unwrap_err().contains("4 MB"));
    }
    #[test]
    fn configuration_paths_and_desktop_language_candidates() {
        assert_eq!(
            user_config_directory(Some("/tmp/custom".into()), Some("/tmp/user".into())),
            Some("/tmp/custom".into())
        );
        assert_eq!(
            user_config_directory(Some("relative".into()), Some("/tmp/user".into())),
            Some("/tmp/user/.config".into())
        );
        assert_eq!(user_config_directory(None, Some("relative".into())), None);
        assert_eq!(
            desktop_keys_for("Name", "de_at", "de"),
            ["Name[de_AT]", "Name[de]", "Name[en]", "Name"]
        );
        assert_eq!(
            desktop_keys_for("Comment", "en", "en"),
            ["Comment[en]", "Comment"]
        );
    }
    #[test]
    fn bundled_catalogues_are_complete_and_templates_match() {
        let config = Config::defaults();
        let de = &config.languages["de"].messages;
        let en = &config.languages["en"].messages;
        assert_eq!(de.len(), en.len());
        for (key, value) in en {
            assert!(!value.trim().is_empty(), "{key}");
            assert!(de.contains_key(key), "{key}");
            assert!(placeholders(key).is_some(), "{key}");
            assert_eq!(placeholders(key), placeholders(value), "{key}");
            assert!(
                key == value || matches!(key.as_str(), "Running (container)" | "Kernel module"),
                "{key}"
            );
        }
        let aliases: BTreeMap<String, String> =
            serde_json::from_str(include_str!("../config/legacy-language-keys.json")).unwrap();
        for (old, key) in aliases {
            assert!(en.contains_key(&key), "{old} -> {key}");
            assert!(!en.contains_key(&old), "ambiguous legacy key: {old}");
            assert_eq!(placeholders(&old), placeholders(&key), "{old}");
        }
    }

    #[test]
    fn every_application_translation_key_is_in_the_catalogue() {
        let config = Config::defaults();
        for source in [
            include_str!("main.rs"),
            include_str!("ui.rs"),
            include_str!("apps_ui.rs"),
            include_str!("drivers_ui.rs"),
            include_str!("apps.rs"),
            include_str!("drivers.rs"),
            include_str!("packages.rs"),
            include_str!("monitor.rs"),
            include_str!("windows.rs"),
            include_str!("hardware.rs"),
            include_str!("cpu.rs"),
            include_str!("command.rs"),
            include_str!("docker.rs"),
            include_str!("install.rs"),
            include_str!("optimizer.rs"),
        ] {
            for marker in ["crate::tr!(", "crate::tf!("] {
                for tail in source.split(marker).skip(1) {
                    let tail = tail.trim_start();
                    assert!(tail.starts_with('"'));
                    let mut escaped = false;
                    let end = tail
                        .char_indices()
                        .skip(1)
                        .find_map(|(i, c)| {
                            if escaped {
                                escaped = false;
                                None
                            } else if c == '\\' {
                                escaped = true;
                                None
                            } else {
                                (c == '"').then_some(i)
                            }
                        })
                        .unwrap();
                    let key: String = serde_json::from_str(&tail[..=end]).unwrap();
                    for code in ["de", "en"] {
                        assert!(
                            config.languages[code].messages.contains_key(&key),
                            "{code}: {key}"
                        );
                    }
                }
            }
        }
    }
}
