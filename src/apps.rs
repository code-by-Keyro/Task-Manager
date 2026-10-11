//! Installed desktop apps and package metadata. Never executes a desktop entry.
use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    User,
    System,
}
impl Scope {
    pub fn label(self) -> &'static str {
        match self {
            Self::User => crate::tr!("User"),
            Self::System => crate::tr!("System"),
        }
    }
}
#[derive(Clone, Debug)]
pub struct App {
    pub id: String,
    pub name: String,
    pub version: Option<String>,
    pub publisher: Option<String>,
    pub source: String,
    pub scope: Scope,
    pub command: String,
    pub description: String,
    pub package: Option<String>,
    pub size: Option<u64>,
    pub installed: Option<u64>,
    pub desktop: Option<PathBuf>,
    pub extra: bool,
    pub reference: Option<String>,
}
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub apps: Vec<App>,
    pub warnings: Vec<String>,
    pub updated: Instant,
}
#[derive(Clone)]
struct Desktop {
    id: String,
    path: PathBuf,
    scope: Scope,
    fields: HashMap<String, String>,
}
#[derive(Clone)]
struct Package {
    name: String,
    version: Option<String>,
    publisher: Option<String>,
    size: Option<u64>,
    installed: Option<u64>,
    description: String,
}
struct Packages {
    rows: Vec<Package>,
    owners: HashMap<PathBuf, usize>,
}

fn text(path: &Path) -> Result<String, String> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
        .map_err(|e| e.to_string())?;
    // Do not block on FIFOs/devices supplied as an application entry.
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err(crate::tr!("Not a regular file entry").into());
    }
    let mut data = String::new();
    file.take(4_000_001)
        .read_to_string(&mut data)
        .map_err(|e| e.to_string())?;
    if data.len() > 4_000_000 {
        Err(crate::tr!("File too large").into())
    } else {
        Ok(data)
    }
}
fn unescape(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('s') => out.push(' '),
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some(c) => {
                    out.push('\\');
                    out.push(c);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}
fn fields(text: &str) -> HashMap<String, String> {
    let mut result = HashMap::new();
    let mut entry = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            entry = line == "[Desktop Entry]";
        } else if entry
            && !line.starts_with('#')
            && let Some((key, value)) = line.split_once('=')
        {
            result.insert(key.trim().into(), unescape(value));
        }
    }
    result
}
fn localized(fields: &HashMap<String, String>, key: &str) -> String {
    crate::i18n::desktop_keys(key)
        .iter()
        .find_map(|key| fields.get(key))
        .cloned()
        .unwrap_or_default()
}
fn dirs() -> Vec<(PathBuf, Scope)> {
    let mut result = Vec::new();
    let user = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
    if let Some(user) = &user {
        result.push((user.join("applications"), Scope::User));
    }
    let system = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    result.extend(system.split(':').filter(|s| s.starts_with('/')).map(|s| {
        let p = PathBuf::from(s);
        let scope = if std::env::var_os("HOME").is_some_and(|h| p.starts_with(PathBuf::from(h))) {
            Scope::User
        } else {
            Scope::System
        };
        (p.join("applications"), scope)
    }));
    // Flatpak exports may be absent from XDG_DATA_DIRS in a desktop launch environment.
    if let Some(home) = std::env::var_os("HOME") {
        result.push((
            PathBuf::from(home).join(".local/share/flatpak/exports/share/applications"),
            Scope::User,
        ));
    }
    result.push((
        PathBuf::from("/var/lib/flatpak/exports/share/applications"),
        Scope::System,
    ));
    let mut seen = HashSet::new();
    result.retain(|(p, _)| seen.insert(p.clone()));
    result
}
fn desktops(dirs: &[(PathBuf, Scope)], warnings: &mut Vec<String>) -> Vec<Desktop> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    let mut unreadable = 0;
    for (root, scope) in dirs {
        let mut pending = vec![(root.clone(), 0)];
        while let Some((dir, depth)) = pending.pop() {
            let entries = match fs::read_dir(&dir) {
                Ok(e) => e,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => {
                    unreadable += 1;
                    continue;
                }
            };
            let mut paths = entries.flatten().map(|e| e.path()).collect::<Vec<_>>();
            paths.sort();
            for path in paths {
                if path.symlink_metadata().is_ok_and(|m| m.is_dir()) {
                    if depth < 12 {
                        pending.push((path, depth + 1));
                    }
                    continue;
                }
                if path.extension().is_none_or(|e| e != "desktop") {
                    continue;
                }
                let Ok(relative) = path.strip_prefix(root) else {
                    continue;
                };
                let id = relative.to_string_lossy().replace('/', "-");
                if !seen.insert(id.clone()) {
                    continue;
                }
                match text(&path) {
                    Ok(t) => {
                        let fields = fields(&t);
                        if fields.get("Type").is_some_and(|t| t == "Application")
                            && fields.get("Hidden").is_none_or(|v| v != "true")
                            && fields.get("NoDisplay").is_none_or(|v| v != "true")
                            && !localized(&fields, "Name").is_empty()
                        {
                            result.push(Desktop {
                                id,
                                path,
                                scope: *scope,
                                fields,
                            });
                        }
                    }
                    Err(_) => unreadable += 1,
                }
            }
        }
    }
    if unreadable > 0 {
        warnings.push(crate::tf!(
            "Unable to read {0} desktop files/directories.",
            unreadable
        ));
    }
    result
}
fn executable(command: &str) -> Option<PathBuf> {
    let words = shell_words::split(command).ok()?;
    let mut words = words.iter();
    let mut first = words.next()?;
    if Path::new(first).file_name().is_some_and(|p| p == "env") {
        first = words.find(|s| !s.contains('=') && !s.starts_with('-'))?;
    }
    let name = Path::new(first).file_name()?.to_str()?;
    // Wrappers own their package, not the app/game addressed by their arguments.
    if [
        "sh", "bash", "zsh", "wine", "wine64", "steam", "flatpak", "gio", "xdg-open", "python",
        "python3",
    ]
    .contains(&name)
    {
        return None;
    }
    let path = PathBuf::from(first);
    if path.is_absolute() {
        return Some(path);
    }
    if path.components().count() != 1 {
        return None;
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(first))
            .find(|p| p.is_file())
    })
}
fn sections(text: &str) -> HashMap<String, Vec<String>> {
    let mut result: HashMap<String, Vec<String>> = HashMap::new();
    let mut key = None;
    for line in text.lines() {
        if line.starts_with('%') && line.ends_with('%') {
            key = Some(line.trim_matches('%').to_owned());
        } else if !line.is_empty()
            && let Some(key) = &key
        {
            result.entry(key.clone()).or_default().push(line.to_owned());
        }
    }
    result
}
fn pacman(root: &Path, wanted: &HashSet<PathBuf>, warnings: &mut Vec<String>) -> Packages {
    let mut result = Packages {
        rows: vec![],
        owners: HashMap::new(),
    };
    let entries = match fs::read_dir(root) {
        Ok(e) => e,
        Err(e) => {
            warnings.push(crate::tf!("Pacman package data unavailable: {0}", e));
            return result;
        }
    };
    if root.parent().is_some_and(|p| p.join("db.lck").exists()) {
        warnings.push(
            crate::tr!("Pacman is busy; the list may be incomplete. Refresh afterwards.").into(),
        );
    }
    let mut paths = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect::<Vec<_>>();
    paths.sort();
    let mut incomplete = 0;
    for path in paths {
        let desc = match text(&path.join("desc")) {
            Ok(t) => sections(&t),
            Err(_) => {
                incomplete += 1;
                continue;
            }
        };
        let first = |key: &str| desc.get(key).and_then(|v| v.first()).cloned();
        let Some(name) = first("NAME").filter(|s| !s.is_empty()) else {
            incomplete += 1;
            continue;
        };
        let index = result.rows.len();
        result.rows.push(Package {
            name,
            version: first("VERSION"),
            publisher: first("PACKAGER"),
            size: first("SIZE").and_then(|v| v.parse().ok()),
            installed: first("INSTALLDATE").and_then(|v| v.parse().ok()),
            description: first("DESC").unwrap_or_default(),
        });
        match text(&path.join("files")) {
            Ok(t) => {
                for file in sections(&t).remove("FILES").unwrap_or_default() {
                    let file = Path::new("/").join(file);
                    if file.extension().is_some_and(|e| e == "desktop") || wanted.contains(&file) {
                        result.owners.insert(file, index);
                    }
                }
            }
            Err(_) => incomplete += 1,
        }
    }
    if incomplete > 0 {
        warnings.push(crate::tf!(
            "{0} Pacman package records could not be read completely.",
            incomplete
        ));
    }
    result
}
fn flatpak_size(text: &str) -> Option<u64> {
    let mut words = text.split_whitespace();
    let number = words.next()?.parse::<f64>().ok()?;
    let scale = match words.next()? {
        "B" => 1.0,
        "kB" | "KB" => 1000.0,
        "MB" => 1e6,
        "GB" => 1e9,
        "TB" => 1e12,
        "KiB" => 1024.0,
        "MiB" => 1048576.0,
        "GiB" => 1073741824.0,
        _ => return None,
    };
    let size = number * scale;
    (size.is_finite() && size >= 0.0 && size < u64::MAX as f64).then(|| size.round() as u64)
}
fn flatpaks(text: &str) -> Result<Vec<App>, String> {
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let f = line.split('\t').collect::<Vec<_>>();
        if f.len() != 8 || f[0].is_empty() || f[3].is_empty() || f[4].is_empty() || f[6].is_empty()
        {
            return Err(crate::tr!("Unable to read the complete Flatpak list.").into());
        }
        let id = format!("flatpak:{}:{}/{}/{}", f[6], f[0], f[4], f[3]);
        if !seen.insert(id.clone()) {
            continue;
        }
        result.push(App {
            id,
            name: if f[1].is_empty() { f[0] } else { f[1] }.into(),
            version: (!f[2].is_empty()).then(|| f[2].into()),
            publisher: None,
            source: format!("Flatpak · {}", f[5]),
            scope: if f[6] == "user" {
                Scope::User
            } else {
                Scope::System
            },
            command: String::new(),
            description: String::new(),
            package: Some(f[0].into()),
            size: flatpak_size(f[7]),
            installed: None,
            desktop: None,
            extra: false,
            reference: Some(format!("{} · {} · {}", f[3], f[4], f[6])),
        });
    }
    Ok(result)
}
fn combine(desktops: Vec<Desktop>, packages: Packages, mut apps: Vec<App>) -> Vec<App> {
    let mut used = HashSet::new();
    for d in desktops {
        let command = d.fields.get("Exec").cloned().unwrap_or_default();
        if let Some(id) = d.fields.get("X-Flatpak") {
            let words = shell_words::split(&command).unwrap_or_default();
            let branch = words.iter().find_map(|s| s.strip_prefix("--branch="));
            let arch = words.iter().find_map(|s| s.strip_prefix("--arch="));
            let candidates = apps
                .iter()
                .enumerate()
                .filter(|(_, a)| {
                    a.package.as_ref() == Some(id)
                        && a.source.starts_with("Flatpak")
                        && branch.is_none_or(|b| a.id.ends_with(&format!("/{b}")))
                        && arch.is_none_or(|arch| a.id.contains(&format!("/{arch}/")))
                })
                .map(|(i, a)| (i, a.scope == d.scope))
                .collect::<Vec<_>>();
            let index = candidates
                .iter()
                .find(|(_, same)| *same)
                .or_else(|| candidates.first())
                .map(|(i, _)| *i);
            if let Some(i) = index {
                let a = &mut apps[i];
                if a.desktop.is_none() {
                    a.command = command;
                    a.desktop = Some(d.path);
                    a.name = localized(&d.fields, "Name");
                    a.description = localized(&d.fields, "Comment");
                }
                continue;
            }
        }
        let owned = packages
            .owners
            .get(&d.path)
            .or_else(|| {
                fs::canonicalize(&d.path)
                    .ok()
                    .as_ref()
                    .and_then(|p| packages.owners.get(p))
            })
            .copied()
            .or_else(|| {
                executable(&command).and_then(|p| {
                    packages
                        .owners
                        .get(&p)
                        .or_else(|| {
                            fs::canonicalize(&p)
                                .ok()
                                .as_ref()
                                .and_then(|p| packages.owners.get(p))
                        })
                        .copied()
                })
            });
        let mut app = App {
            id: format!("desktop:{}", d.id),
            name: localized(&d.fields, "Name"),
            version: None,
            publisher: None,
            source: if command.contains(".AppImage") {
                crate::tr!("AppImage / desktop entry")
            } else {
                crate::tr!("Desktop entry")
            }
            .into(),
            scope: d.scope,
            command,
            description: localized(&d.fields, "Comment"),
            package: None,
            size: None,
            installed: None,
            desktop: Some(d.path),
            extra: false,
            reference: None,
        };
        if let Some(i) = owned {
            used.insert(i);
            let p = &packages.rows[i];
            app.version = p.version.clone();
            app.publisher = p.publisher.clone();
            app.package = Some(p.name.clone());
            app.size = p.size;
            app.installed = p.installed;
            app.source = "Pacman".into();
            app.scope = Scope::System;
            if app.description.is_empty() {
                app.description = p.description.clone();
            }
        }
        apps.push(app);
    }
    for (i, p) in packages.rows.into_iter().enumerate() {
        if !used.contains(&i) {
            apps.push(App {
                id: format!("pacman:{}", p.name),
                name: p.name.clone(),
                version: p.version,
                publisher: p.publisher,
                source: "Pacman".into(),
                scope: Scope::System,
                command: String::new(),
                description: p.description,
                package: Some(p.name),
                size: p.size,
                installed: p.installed,
                desktop: None,
                extra: true,
                reference: None,
            });
        }
    }
    apps.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.id.cmp(&b.id))
    });
    apps
}
pub fn sample() -> Snapshot {
    let mut warnings = Vec::new();
    let desktops = desktops(&dirs(), &mut warnings);
    let mut wanted = HashSet::new();
    for d in &desktops {
        if let Some(p) = d.fields.get("Exec").and_then(|e| executable(e)) {
            wanted.insert(p.clone());
            if let Ok(p) = fs::canonicalize(p) {
                wanted.insert(p);
            }
        }
    }
    let db = if Path::new("/usr/bin/pacman-conf").is_file() {
        match crate::command::run(
            "/usr/bin/pacman-conf",
            &["DBPath"],
            Duration::from_secs(2),
            crate::tr!("Pacman configuration"),
        ) {
            Ok(path) if Path::new(path.trim()).is_absolute() => {
                Some(PathBuf::from(path.trim()).join("local"))
            }
            Ok(_) => {
                warnings.push(
                    crate::tr!("Pacman configuration returned no absolute database path.").into(),
                );
                None
            }
            Err(e) => {
                warnings.push(e);
                None
            }
        }
    } else {
        Some(PathBuf::from("/var/lib/pacman/local"))
    };
    let packages = match db {
        Some(db) => pacman(&db, &wanted, &mut warnings),
        None => Packages {
            rows: vec![],
            owners: HashMap::new(),
        },
    };
    let flatpaks = if Path::new("/usr/bin/flatpak").is_file() {
        match crate::command::run("/usr/bin/flatpak",&["list","--app","--columns=application:full,name:full,version:full,branch:full,arch:full,origin:full,installation:full,size:full"],Duration::from_secs(6),"Flatpak").and_then(|t|flatpaks(&t)) {Ok(apps)=>apps,Err(e)=>{warnings.push(e);vec![]}}
    } else {
        vec![]
    };
    Snapshot {
        apps: combine(desktops, packages, flatpaks),
        warnings,
        updated: Instant::now(),
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    Name,
    Version,
    Publisher,
    Source,
    Scope,
    Size,
}
pub fn visible(
    snapshot: &Snapshot,
    query: &str,
    extras: bool,
    scope: Option<Scope>,
    sort: Sort,
    descending: bool,
) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    let mut indices = snapshot
        .apps
        .iter()
        .enumerate()
        .filter_map(|(i, a)| {
            ((extras || !a.extra)
                && scope.is_none_or(|scope| scope == a.scope)
                && (query.is_empty()
                    || [
                        &a.name,
                        &a.source,
                        &a.command,
                        &a.description,
                        a.package.as_deref().unwrap_or(""),
                        a.publisher.as_deref().unwrap_or(""),
                        a.version.as_deref().unwrap_or(""),
                    ]
                    .iter()
                    .any(|s| s.to_lowercase().contains(&query))))
            .then_some(i)
        })
        .collect::<Vec<_>>();
    indices.sort_by(|i, j| {
        let (a, b) = (&snapshot.apps[*i], &snapshot.apps[*j]);
        let order = match sort {
            Sort::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            Sort::Version => a.version.cmp(&b.version),
            Sort::Publisher => a.publisher.cmp(&b.publisher),
            Sort::Source => a.source.cmp(&b.source),
            Sort::Scope => a.scope.label().cmp(b.scope.label()),
            Sort::Size => a.size.cmp(&b.size),
        }
        .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        .then(a.id.cmp(&b.id));
        if descending { order.reverse() } else { order }
    });
    indices
}
pub fn date(seconds: u64) -> Option<String> {
    let time = libc::time_t::try_from(seconds).ok()?;
    let mut out = std::mem::MaybeUninit::<libc::tm>::uninit();
    // SAFETY: valid pointers and localtime_r writes into caller-owned storage.
    if unsafe { libc::localtime_r(&time, out.as_mut_ptr()) }.is_null() {
        return None;
    }
    // SAFETY: successful localtime_r initialized the output.
    let out = unsafe { out.assume_init() };
    Some(format!(
        "{:02}.{:02}.{} {:02}:{:02}",
        out.tm_mday,
        out.tm_mon + 1,
        out.tm_year + 1900,
        out.tm_hour,
        out.tm_min
    ))
}
enum Control {
    Refresh,
    Stop,
}
pub struct Monitor {
    commands: SyncSender<Control>,
    pub receiver: Receiver<Snapshot>,
    thread: Option<JoinHandle<()>>,
}
impl Monitor {
    pub fn start(ctx: eframe::egui::Context) -> Self {
        let (commands, input) = mpsc::sync_channel(1);
        let (output, receiver) = mpsc::sync_channel(1);
        let thread = thread::spawn(move || {
            while let Ok(control) = input.recv() {
                if matches!(control, Control::Stop) {
                    break;
                }
                let snapshot = sample();
                if matches!(
                    output.try_send(snapshot),
                    Err(mpsc::TrySendError::Disconnected(_))
                ) {
                    break;
                }
                ctx.request_repaint();
            }
        });
        Self {
            commands,
            receiver,
            thread: Some(thread),
        }
    }
    pub fn refresh(&self) -> bool {
        self.commands.try_send(Control::Refresh).is_ok()
    }
}
impl Drop for Monitor {
    fn drop(&mut self) {
        let _ = self.commands.send(Control::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::os::unix::fs::symlink;
    fn desktop(id: &str, path: &str, scope: Scope, extra: &str) -> Desktop {
        Desktop {
            id: id.into(),
            path: path.into(),
            scope,
            fields: fields(&format!(
                "[Desktop Entry]\nType=Application\nName=Editor\nExec=/opt/acme/editor\n{extra}"
            )),
        }
    }
    fn package(root: &Path, name: &str, files: &str) -> PathBuf {
        let p = root.join(format!("{name}-2.0-1"));
        fs::create_dir_all(&p).unwrap();
        fs::write(p.join("desc"),format!("%NAME%\n{name}\n\n%VERSION%\n2.0-1\n\n%PACKAGER%\nCachyOS Test\n\n%SIZE%\n12000\n\n%INSTALLDATE%\n1600000000\n\n%DESC%\nTest package\n")).unwrap();
        fs::write(p.join("files"), format!("%FILES%\n{files}\n")).unwrap();
        p
    }
    #[test]
    fn desktop_precedence_nested_ids_localization_and_hidden_tombstones() {
        let dir = tempfile::tempdir().unwrap();
        let user = dir.path().join("user");
        let system = dir.path().join("system");
        fs::create_dir_all(user.join("wine/Programs")).unwrap();
        fs::create_dir_all(&system).unwrap();
        fs::write(
            system.join("hidden.desktop"),
            "[Desktop Entry]\nType=Application\nName=System app\n",
        )
        .unwrap();
        fs::write(
            user.join("hidden.desktop"),
            "[Desktop Entry]\nHidden=true\n",
        )
        .unwrap();
        fs::write(user.join("wine/Programs/editor.desktop"),"[Desktop Entry]\nType=Application\nName=English\nName[de]=Deutsch\nComment=Line\\nnext\nVersion=1.0\nTerminal=true\nExec=sh -c 'do-not-execute-this'\n[Desktop Action Other]\nName=Wrong\n").unwrap();
        fs::write(
            system.join("invisible.desktop"),
            "[Desktop Entry]\nType=Application\nName=Internal\nNoDisplay=true\n",
        )
        .unwrap();
        fs::write(
            system.join("directory.desktop"),
            "[Desktop Entry]\nType=Directory\nName=Wrong\n",
        )
        .unwrap();
        symlink(&user, user.join("loop")).unwrap();
        let mut warnings = vec![];
        let rows = desktops(
            &[(user, Scope::User), (system, Scope::System)],
            &mut warnings,
        );
        assert!(warnings.is_empty());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "wine-Programs-editor.desktop");
        assert_eq!(localized(&rows[0].fields, "Name"), "Deutsch");
        assert_eq!(rows[0].fields["Comment"], "Line\nnext");
        assert_eq!(rows[0].scope, Scope::User);
        let apps = combine(
            rows,
            Packages {
                rows: vec![],
                owners: HashMap::new(),
            },
            vec![],
        );
        assert_eq!(
            apps[0].version, None,
            "Desktop Version is the specification version, not the app version"
        );
    }
    #[test]
    fn package_ownership_metadata_shared_packages_and_wrappers_are_not_confused() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("local");
        fs::create_dir(&db).unwrap();
        package(
            &db,
            "acme",
            "usr/share/applications/one.desktop\nusr/share/applications/two.desktop\nopt/acme/editor",
        );
        package(&db, "library", "usr/lib/libexample.so");
        let mut warnings = vec![];
        let packages = pacman(
            &db,
            &HashSet::from([PathBuf::from("/opt/acme/editor")]),
            &mut warnings,
        );
        assert!(warnings.is_empty());
        let apps = combine(
            vec![
                desktop(
                    "one.desktop",
                    "/usr/share/applications/one.desktop",
                    Scope::System,
                    "",
                ),
                desktop(
                    "two.desktop",
                    "/usr/share/applications/two.desktop",
                    Scope::System,
                    "",
                ),
                desktop(
                    "personal.desktop",
                    "/home/user/personal.desktop",
                    Scope::User,
                    "",
                ),
                desktop(
                    "game.desktop",
                    "/home/user/game.desktop",
                    Scope::User,
                    "Exec=steam steam://rungameid/123\n",
                ),
            ],
            packages,
            vec![],
        );
        let owned = apps
            .iter()
            .filter(|a| a.package.as_deref() == Some("acme"))
            .collect::<Vec<_>>();
        assert_eq!(owned.len(), 3);
        assert!(owned.iter().all(|a| a.version.as_deref() == Some("2.0-1")
            && a.size == Some(12000)
            && a.installed == Some(1600000000)
            && a.scope == Scope::System));
        let game = apps
            .iter()
            .find(|a| a.id == "desktop:game.desktop")
            .unwrap();
        assert_eq!(game.package, None);
        assert_eq!(game.scope, Scope::User);
        assert!(
            apps.iter()
                .any(|a| a.extra && a.package.as_deref() == Some("library"))
        );
        assert!(
            !apps
                .iter()
                .any(|a| a.extra && a.package.as_deref() == Some("acme"))
        );
        assert_eq!(
            executable("env EDITOR=true /opt/acme/editor --flag"),
            Some(PathBuf::from("/opt/acme/editor"))
        );
        assert!(executable("/usr/bin/wine game.exe").is_none());
        assert!(executable("sh -c '/opt/acme/editor'").is_none());
    }
    #[test]
    fn flatpak_refs_scopes_branches_units_and_export_deduplication() {
        let text = "org.test.Editor\tEditor\t3.1\tstable\tx86_64\tflathub\tsystem\t20.5 MB\norg.test.Editor\tEditor\t3.2\tbeta\tx86_64\tflathub\tuser\t1.5 GiB\norg.test.Editor\tEditor\t3.3\tstable\tx86_64\tflathub\tuser\t0 B\n";
        let apps = flatpaks(&format!("{text}{text}")).unwrap();
        assert_eq!(apps.len(), 3);
        assert_eq!(apps[0].size, Some(20500000));
        assert_eq!(apps[1].size, Some(1610612736));
        assert_eq!(apps[2].size, Some(0));
        let mut d = desktop(
            "org.test.Editor.desktop",
            "/exports/org.test.Editor.desktop",
            Scope::User,
            "X-Flatpak=org.test.Editor\nExec=/usr/bin/flatpak run --branch=beta --arch=x86_64 org.test.Editor\n",
        );
        d.fields.insert("Name[de]".into(), "Test-Editor".into());
        let result = combine(
            vec![d],
            Packages {
                rows: vec![],
                owners: HashMap::new(),
            },
            apps,
        );
        assert_eq!(result.len(), 3);
        let matched = result.iter().find(|a| a.desktop.is_some()).unwrap();
        assert_eq!(matched.version.as_deref(), Some("3.2"));
        assert_eq!(matched.name, "Test-Editor");
        assert_eq!(matched.scope, Scope::User);
        assert!(matched.command.contains("--branch=beta"));
        assert!(
            result
                .iter()
                .all(|a| a.installed.is_none() && a.publisher.is_none())
        );
        assert!(flatpaks("bad\trow").is_err());
        assert!(flatpak_size("NaN MB").is_none());
        assert!(flatpak_size("-1 MB").is_none());
        assert!(flatpak_size("unknown").is_none());
    }
    #[test]
    fn search_scope_extra_packages_numeric_size_sorting_and_stable_ids() {
        let mut apps=flatpaks("org.example.User\tAlpha\t2.0\tstable\tx86_64\tflathub\tuser\t10 MB\norg.example.System\tBeta\t1.0\tstable\tx86_64\tflathub\tsystem\t2 MB\n").unwrap();
        let mut package = apps[1].clone();
        package.id = "pacman:test".into();
        package.name = "Library".into();
        package.extra = true;
        package.source = "Pacman".into();
        package.size = None;
        apps.push(package);
        let snapshot = Snapshot {
            apps,
            warnings: vec![],
            updated: Instant::now(),
        };
        assert_eq!(
            visible(&snapshot, "", false, None, Sort::Size, false),
            vec![1, 0]
        );
        assert_eq!(
            visible(&snapshot, "", true, None, Sort::Size, true),
            vec![0, 1, 2]
        );
        assert_eq!(
            visible(&snapshot, "", true, Some(Scope::User), Sort::Name, false),
            vec![0]
        );
        assert_eq!(
            visible(&snapshot, "flathub", false, None, Sort::Name, false),
            vec![0, 1]
        );
        assert_eq!(
            visible(&snapshot, "2.0", false, None, Sort::Name, false),
            vec![0]
        );
        assert!(visible(&snapshot, "missing-app", true, None, Sort::Name, false).is_empty());
    }
    #[test]
    fn partial_databases_nonregular_files_size_limits_and_missing_sources_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("local");
        fs::create_dir(&db).unwrap();
        let p = package(&db, "acme", "");
        fs::remove_file(p.join("files")).unwrap();
        let mut warnings = vec![];
        let rows = pacman(&db, &HashSet::new(), &mut warnings);
        assert_eq!(rows.rows.len(), 1);
        assert_eq!(warnings.len(), 1);
        let mut warnings = vec![];
        assert!(
            pacman(&dir.path().join("missing"), &HashSet::new(), &mut warnings)
                .rows
                .is_empty()
        );
        assert_eq!(warnings.len(), 1);
        let large = dir.path().join("large.desktop");
        File::create(&large).unwrap().set_len(4_000_001).unwrap();
        assert!(text(&large).is_err());
        let fifo = dir.path().join("fifo.desktop");
        let c = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
        // SAFETY: the path is inside our disposable fixture; no user file is touched.
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        let start = Instant::now();
        assert!(text(&fifo).is_err());
        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(date(u64::MAX).is_none());
        assert!(date(1600000000).unwrap().contains("2020"));
    }
}
