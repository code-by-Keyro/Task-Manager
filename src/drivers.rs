//! Kernel/module inventory and explicit repository update check. Never loads modules.
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::{self, OpenOptions};
use std::io::Read;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, SyncSender},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum State {
    Loaded,
    Builtin,
    Available,
}
impl State {
    pub fn label(self) -> &'static str {
        match self {
            Self::Loaded => crate::tr!("Loaded"),
            Self::Builtin => crate::tr!("Built-in"),
            Self::Available => crate::tr!("Not loaded"),
        }
    }
}
#[derive(Clone, Debug)]
pub struct Module {
    pub name: String,
    pub state: State,
    pub memory: Option<u64>,
    pub references: Option<u64>,
    pub holders: Vec<String>,
    pub depends: Vec<String>,
    pub path: Option<PathBuf>,
    pub version: Option<String>,
    pub external: Option<bool>,
}
impl Module {
    pub fn source(&self) -> &'static str {
        match self.external {
            Some(true) => crate::tr!("External"),
            Some(false) => crate::tr!("Kernel"),
            None => crate::tr!("Unknown"),
        }
    }
}
pub struct Snapshot {
    pub running: String,
    pub kernel: String,
    pub kernels: Vec<String>,
    pub modules: Vec<Module>,
    pub warnings: Vec<String>,
    pub updated: Instant,
}
#[derive(Clone, Debug, Default)]
pub struct Details {
    pub fields: BTreeMap<String, Vec<String>>,
    pub error: Option<String>,
}
pub struct Update {
    pub package: String,
    pub old: String,
    pub new: String,
    pub reason: String,
}
pub struct Updates {
    pub list: Vec<Update>,
    pub all: usize,
    pub error: Option<String>,
    pub checked: Instant,
}

fn text(path: &Path) -> Result<String, String> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
        .map_err(|e| e.to_string())?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err(crate::tr!("Not a regular file").into());
    }
    let mut s = String::new();
    file.take(16_000_001)
        .read_to_string(&mut s)
        .map_err(|e| e.to_string())?;
    if s.len() > 16_000_000 {
        Err(crate::tr!("File too large").into())
    } else {
        Ok(s)
    }
}
fn normalized(name: &str) -> String {
    name.replace('-', "_")
}
fn module_name(path: &str) -> Option<String> {
    let name = Path::new(path).file_name()?.to_str()?;
    let name = name
        .strip_suffix(".zst")
        .or_else(|| name.strip_suffix(".xz"))
        .or_else(|| name.strip_suffix(".gz"))
        .unwrap_or(name);
    let name = name.strip_suffix(".ko")?;
    valid_name(name).then(|| normalized(name))
}
fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('-')
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
fn relative_path(s: &str) -> Option<PathBuf> {
    let p = Path::new(s);
    (!p.as_os_str().is_empty() && p.components().all(|c| matches!(c, Component::Normal(_))))
        .then(|| p.to_owned())
}
fn blank(name: String) -> Module {
    Module {
        name,
        state: State::Available,
        memory: None,
        references: None,
        holders: vec![],
        depends: vec![],
        path: None,
        version: None,
        external: None,
    }
}
fn loaded(s: &str) -> Vec<Module> {
    s.lines()
        .filter_map(|line| {
            let cols: Vec<_> = line.split_whitespace().collect();
            if cols.len() < 5 || !valid_name(cols[0]) {
                return None;
            }
            let mut m = blank(normalized(cols[0]));
            m.state = State::Loaded;
            m.memory = cols[1].parse().ok();
            m.references = cols[2].parse().ok();
            m.holders = cols[3]
                .split(',')
                .filter(|s| valid_name(s))
                .map(normalized)
                .collect();
            Some(m)
        })
        .collect()
}
fn files(
    root: &Path,
    depth: usize,
    modules: &mut BTreeMap<String, Module>,
    warnings: &mut Vec<String>,
) {
    if depth > 20 {
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            files(&p, depth + 1, modules, warnings);
        } else if kind.is_file() || kind.is_symlink() {
            let Some(name) = p.to_str().and_then(module_name) else {
                continue;
            };
            if let Some(existing) = modules.get(&name) {
                // depmod's lookup remains authoritative; warn rather than silently misidentify duplicates.
                if existing.path.as_ref() != Some(&p) {
                    warnings.push(crate::tf!(
                        "Multiple files for module {0}; details use the kmod mapping.",
                        name
                    ));
                }
            } else {
                let mut m = blank(name.clone());
                m.path = Some(p.clone());
                // A kernel/ path alone is not evidence of MODULE_INFO(intree).
                modules.insert(name, m);
            }
        }
    }
}
fn inventory(root: &Path, proc: &Path, sys: &Path, running: String, kernel: String) -> Snapshot {
    let mut warnings = vec![];
    let mut kernels: Vec<_> = fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let s = e.file_name().to_string_lossy().into_owned();
            (e.path().is_dir() && !s.starts_with('.') && relative_path(&s).is_some()).then_some(s)
        })
        .collect();
    if !kernels.contains(&running) {
        kernels.push(running.clone());
    }
    kernels.sort();
    kernels.dedup();
    let base = root.join(&kernel);
    let mut map = BTreeMap::new();
    if base.is_dir() {
        files(&base, 0, &mut map, &mut warnings);
        match text(&base.join("modules.builtin")) {
            Ok(s) => {
                for line in s.lines() {
                    if relative_path(line).is_some()
                        && let Some(name) = module_name(line)
                    {
                        let m = map.entry(name.clone()).or_insert_with(|| blank(name));
                        m.state = State::Builtin;
                        m.path = None; // Built-in object paths are not separate installed .ko files.
                        m.external = Some(false);
                    }
                }
            }
            Err(e) => warnings.push(crate::tf!("Unable to read all built-in modules: {0}", e)),
        }
        // Dependency text is an optional display aid, never used to load/resolve a module.
        if let Ok(s) = text(&base.join("modules.dep")) {
            for line in s.lines() {
                if let Some((path, deps)) = line.split_once(':')
                    && let Some(name) = module_name(path)
                    && let Some(m) = map.get_mut(&name)
                {
                    m.depends = deps.split_whitespace().filter_map(module_name).collect();
                }
            }
        }
    } else {
        warnings.push(crate::tf!("Module files for kernel {0} are missing. After an update, the running kernel may be older than the installed files." , kernel));
    }
    if kernel == running {
        match text(proc) {
            Ok(s) => {
                for mut m in loaded(&s) {
                    if let Some(old) = map.remove(&m.name) {
                        m.path = old.path;
                        m.depends = old.depends;
                    }
                    let dir = sys.join(&m.name);
                    m.version = text(&dir.join("version"))
                        .ok()
                        .map(|s| s.trim().to_owned())
                        .filter(|s| !s.is_empty());
                    m.external = text(&dir.join("taint")).ok().map(|s| s.contains('O'));
                    map.insert(m.name.clone(), m);
                }
            }
            Err(e) => warnings.push(crate::tf!("Unable to read loaded modules: {0}", e)),
        }
        // sysfs includes built-ins that do not occur in /proc/modules.
        if let Ok(entries) = fs::read_dir(sys) {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if valid_name(&name) && !map.contains_key(&name) {
                    let mut m = blank(name.clone());
                    m.state = State::Builtin;
                    m.external = Some(false);
                    m.version = text(&e.path().join("version"))
                        .ok()
                        .map(|s| s.trim().to_owned());
                    map.insert(name, m);
                }
            }
        }
    }
    Snapshot {
        running,
        kernel,
        kernels,
        modules: map.into_values().collect(),
        warnings,
        updated: Instant::now(),
    }
}
pub fn sample(kernel: Option<&str>) -> Snapshot {
    let running = text(Path::new("/proc/sys/kernel/osrelease"))
        .unwrap_or_default()
        .trim()
        .to_owned();
    let kernel = kernel
        .filter(|s| relative_path(s).is_some() && !s.contains('/'))
        .unwrap_or(&running)
        .to_owned();
    inventory(
        Path::new("/usr/lib/modules"),
        Path::new("/proc/modules"),
        Path::new("/sys/module"),
        running,
        kernel,
    )
}
fn parse_details(s: &str) -> Details {
    let mut d = Details::default();
    for field in s.split('\0') {
        if let Some(index) = field.find([':', '=']) {
            let (key, value) = field.split_at(index);
            let value = &value[1..];
            let value = value.trim();
            if !value.is_empty() {
                d.fields
                    .entry(key.trim().into())
                    .or_default()
                    .push(value.into());
            }
        }
    }
    d
}
fn details(kernel: &str, name: &str, cancel: &AtomicBool) -> Details {
    if !valid_name(name) || relative_path(kernel).is_none() || kernel.contains('/') {
        return Details {
            error: Some(crate::tr!("Invalid module identity").into()),
            ..Default::default()
        };
    }
    match crate::command::run_config(
        "/usr/bin/modinfo",
        &["-0", "-k", kernel, name],
        Duration::from_secs(5),
        crate::tr!("Module details"),
        &[],
        &[0],
        Some(cancel),
    ) {
        Ok(s) => parse_details(&s),
        Err(e) => Details {
            error: Some(e),
            ..Default::default()
        },
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    All,
    Loaded,
    Available,
    Builtin,
    External,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    Name,
    State,
    Memory,
    References,
    Version,
    Source,
}
pub fn visible(
    s: &Snapshot,
    query: &str,
    filter: Filter,
    sort: Sort,
    descending: bool,
) -> Vec<usize> {
    let q = query.to_lowercase();
    let mut v: Vec<_> = s
        .modules
        .iter()
        .enumerate()
        .filter(|(_, m)| {
            (match filter {
                Filter::All => true,
                Filter::Loaded => m.state == State::Loaded,
                Filter::Available => m.state == State::Available,
                Filter::Builtin => m.state == State::Builtin,
                Filter::External => m.external == Some(true),
            }) && [
                m.name.clone(),
                m.version.clone().unwrap_or_default(),
                m.source().into(),
                m.path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
            ]
            .join(" ")
            .to_lowercase()
            .contains(&q)
        })
        .map(|(i, _)| i)
        .collect();
    v.sort_by(|a, b| {
        let (a, b) = (&s.modules[*a], &s.modules[*b]);
        let o = match sort {
            Sort::Name => a.name.cmp(&b.name),
            Sort::State => a.state.cmp(&b.state),
            Sort::Memory => a.memory.cmp(&b.memory),
            Sort::References => a.references.cmp(&b.references),
            Sort::Version => a.version.cmp(&b.version),
            Sort::Source => a.external.cmp(&b.external),
        }
        .then_with(|| a.name.cmp(&b.name));
        if descending { o.reverse() } else { o }
    });
    v
}
fn sections(s: &str) -> HashMap<&str, Vec<&str>> {
    let mut result = HashMap::new();
    let mut key = "";
    for line in s.lines() {
        if line.starts_with('%') && line.ends_with('%') {
            key = line;
        } else if !line.is_empty() {
            result.entry(key).or_insert_with(Vec::new).push(line);
        }
    }
    result
}
fn relevant(name: &str, files: &[&str]) -> Option<String> {
    if files.iter().any(|p| {
        p.starts_with("usr/lib/modules/")
            && (module_name(p).is_some() || p.ends_with("/vmlinuz") || p.ends_with("/pkgbase"))
    }) {
        return Some(crate::tr!("Kernel / kernel modules").into());
    }
    if files
        .iter()
        .any(|p| p.starts_with("usr/src/") && p.ends_with("/dkms.conf"))
        || name.ends_with("-dkms")
    {
        return Some(crate::tr!("DKMS driver").into());
    }
    if name == "linux-firmware"
        || name.starts_with("linux-firmware-")
        || files
            .iter()
            .any(|p| p.starts_with("usr/lib/firmware/") && !p.ends_with('/'))
    {
        return Some(crate::tr!("Firmware").into());
    }
    if ["amd-ucode", "intel-ucode"].contains(&name) {
        return Some(crate::tr!("CPU microcode").into());
    }
    if [
        "mesa",
        "lib32-mesa",
        "vulkan-radeon",
        "lib32-vulkan-radeon",
        "vulkan-intel",
        "lib32-vulkan-intel",
        "nvidia-utils",
        "lib32-nvidia-utils",
        "nvidia-open",
        "nvidia",
        "xf86-video-amdgpu",
        "xf86-video-intel",
        "xf86-video-nouveau",
        "xf86-video-ati",
        "xf86-video-vmware",
        "xf86-video-vesa",
    ]
    .contains(&name)
        || name.starts_with("nvidia-")
    {
        return Some(crate::tr!("Graphics driver / graphics library").into());
    }
    None
}
fn packages(root: &Path) -> Result<HashMap<String, String>, String> {
    let mut result = HashMap::new();
    let entries =
        fs::read_dir(root).map_err(|e| crate::tf!("Unable to read Pacman package data: {0}", e))?;
    for e in entries {
        let e = e.map_err(|e| e.to_string())?;
        if !e.path().is_dir() {
            continue;
        } // ALPM_DB_VERSION is metadata, not a package.
        let desc = text(&e.path().join("desc"))?;
        let fields = sections(&desc);
        let name = fields
            .get("%NAME%")
            .and_then(|s| s.first())
            .ok_or(crate::tr!("Package has no name"))?;
        let list = text(&e.path().join("files"))?;
        let fields = sections(&list);
        if let Some(reason) = relevant(
            name,
            fields.get("%FILES%").map(Vec::as_slice).unwrap_or(&[]),
        ) {
            result.insert((*name).into(), reason);
        }
    }
    Ok(result)
}
fn parse_updates(
    s: &str,
    relevant: &HashMap<String, String>,
) -> Result<(Vec<Update>, usize), String> {
    let mut updates = vec![];
    let mut seen = BTreeSet::new();
    let mut all = 0;
    for line in s.lines().filter(|s| !s.trim().is_empty()) {
        let f: Vec<_> = line.split_whitespace().collect();
        if f.len() != 4 || f[2] != "->" || !seen.insert(f[0]) {
            return Err(crate::tr!("Unexpected update result format; check incomplete.").into());
        }
        all += 1;
        if let Some(reason) = relevant.get(f[0]) {
            updates.push(Update {
                package: f[0].into(),
                old: f[1].into(),
                new: f[3].into(),
                reason: reason.clone(),
            });
        }
    }
    updates.sort_by(|a, b| a.package.cmp(&b.package));
    Ok((updates, all))
}
struct TempDb(PathBuf);
impl TempDb {
    fn new() -> Result<Self, String> {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        for i in 0..100 {
            let p = std::env::temp_dir().join(format!(
                "task-manager-updates-{}-{seed}-{i}",
                std::process::id()
            ));
            match fs::DirBuilder::new().mode(0o700).create(&p) {
                Ok(()) => return Ok(Self(p)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.to_string()),
            }
        }
        Err(crate::tr!("Unable to create the temporary update database.").into())
    }
}
impl Drop for TempDb {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
pub fn check_updates(cancel: &AtomicBool) -> Updates {
    let result = (|| {
        if !Path::new("/usr/bin/checkupdates").is_file() {
            return Err(crate::tr!("Update search requires checkupdates from pacman-contrib. No packages are installed automatically.").into());
        }
        let db = crate::command::run_config(
            "/usr/bin/pacman-conf",
            &["DBPath"],
            Duration::from_secs(3),
            crate::tr!("Pacman configuration"),
            &[],
            &[0],
            Some(cancel),
        )?;
        let relevant = packages(&Path::new(db.trim()).join("local"))?;
        let tmp = TempDb::new()?;
        let path = tmp
            .0
            .to_str()
            .ok_or(crate::tr!("Temporary path is not UTF-8"))?;
        // No -d/-c/-n: fresh repository metadata in our private DB, never install or change the real sync DB.
        let s = crate::command::run_config(
            "/usr/bin/checkupdates",
            &["--nocolor"],
            Duration::from_secs(60),
            crate::tr!("Update search"),
            &[
                ("CHECKUPDATES_DB", path),
                ("PATH", "/usr/bin:/bin"),
                ("LIBRARY", "/usr/share/makepkg"),
            ],
            &[0, 2],
            Some(cancel),
        )?;
        parse_updates(&s, &relevant)
    })();
    match result {
        Ok((list, all)) => Updates {
            list,
            all,
            error: None,
            checked: Instant::now(),
        },
        Err(e) => Updates {
            list: vec![],
            all: 0,
            error: Some(e),
            checked: Instant::now(),
        },
    }
}

enum Control {
    Inventory(Option<String>),
    Details(String, String),
    Stop,
}
pub enum Event {
    Inventory(Snapshot),
    Details(String, String, Details),
}
pub struct Monitor {
    commands: SyncSender<Control>,
    pub receiver: Receiver<Event>,
    cancel: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    update_commands: SyncSender<bool>,
    pub updates: Receiver<Updates>,
    pub update_cancel: Arc<AtomicBool>,
    update_thread: Option<JoinHandle<()>>,
}
impl Monitor {
    pub fn start(ctx: eframe::egui::Context) -> Self {
        let (commands, input) = mpsc::sync_channel(1);
        let (output, receiver) = mpsc::sync_channel(2);
        let cancel = Arc::new(AtomicBool::new(false));
        let token = cancel.clone();
        let repaint = ctx.clone();
        let thread = thread::spawn(move || {
            while let Ok(c) = input.recv() {
                let event = match c {
                    Control::Inventory(kernel) => Event::Inventory(sample(kernel.as_deref())),
                    Control::Details(k, n) => {
                        let d = details(&k, &n, &token);
                        Event::Details(k, n, d)
                    }
                    Control::Stop => break,
                };
                if output.send(event).is_err() {
                    break;
                }
                repaint.request_repaint();
            }
        });
        let (update_commands, update_input) = mpsc::sync_channel::<bool>(1);
        let (result, updates) = mpsc::sync_channel(1);
        let update_cancel = Arc::new(AtomicBool::new(false));
        let token = update_cancel.clone();
        let update_thread = thread::spawn(move || {
            while let Ok(start) = update_input.recv() {
                if !start {
                    break;
                }
                let u = check_updates(&token);
                if result.send(u).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        });
        Self {
            commands,
            receiver,
            cancel,
            thread: Some(thread),
            update_commands,
            updates,
            update_cancel,
            update_thread: Some(update_thread),
        }
    }
    pub fn refresh(&self, kernel: Option<String>) -> bool {
        self.commands.try_send(Control::Inventory(kernel)).is_ok()
    }
    pub fn details(&self, kernel: String, name: String) -> bool {
        self.commands
            .try_send(Control::Details(kernel, name))
            .is_ok()
    }
    pub fn check(&self) -> bool {
        self.update_cancel.store(false, Ordering::Relaxed);
        self.update_commands.try_send(true).is_ok()
    }
}
impl Drop for Monitor {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.update_cancel.store(true, Ordering::Relaxed);
        // Drop result receivers before joining so a queued response cannot hold a worker open.
        let (_, empty) = mpsc::channel();
        let old = std::mem::replace(&mut self.receiver, empty);
        drop(old);
        let (_, empty) = mpsc::channel();
        let old = std::mem::replace(&mut self.updates, empty);
        drop(old);
        let _ = self.commands.send(Control::Stop);
        let _ = self.update_commands.send(false);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        if let Some(t) = self.update_thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn write(p: &Path, s: &str) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, s).unwrap();
    }
    #[test]
    fn kernel_inventory_keeps_runtime_and_other_installed_kernel_separate() {
        let t = tempfile::tempdir().unwrap();
        let r = t.path().join("modules");
        let sys = t.path().join("sys");
        let proc = t.path().join("proc");
        write(&r.join("test/kernel/net/foo-bar.ko.zst"), "");
        write(&r.join("test/kernel/test.ko"), "");
        write(
            &r.join("test/modules.builtin"),
            "kernel/base/base.ko\n../../evil.ko\n",
        );
        write(
            &r.join("test/modules.dep"),
            "kernel/net/foo-bar.ko.zst: kernel/test.ko\n",
        );
        write(&proc, "foo_bar 4096 2 helper, Live 0\ninvalid 123\n");
        write(&sys.join("foo_bar/taint"), "O");
        write(&sys.join("foo_bar/version"), "v1\n");
        write(&sys.join("built_in/version"), "2");
        let s = inventory(&r, &proc, &sys, "test".into(), "test".into());
        assert!(s.warnings.is_empty());
        assert_eq!(s.modules.len(), 4);
        let m = s.modules.iter().find(|m| m.name == "foo_bar").unwrap();
        assert_eq!(m.state, State::Loaded);
        assert_eq!(m.memory, Some(4096));
        assert_eq!(m.references, Some(2));
        assert_eq!(m.holders, ["helper"]);
        assert_eq!(m.depends, ["test"]);
        assert_eq!(m.external, Some(true));
        assert_eq!(m.version.as_deref(), Some("v1"));
        write(&r.join("other/kernel/new.ko.xz"), "");
        write(&r.join("other/modules.builtin"), "kernel/base/base.ko\n");
        let s = inventory(&r, &proc, &sys, "test".into(), "other".into());
        assert_eq!(s.modules.len(), 2);
        assert!(
            s.modules
                .iter()
                .all(|m| m.memory.is_none() && m.state != State::Loaded)
        );
        assert!(s.modules.iter().all(|m| m.name != "foo_bar"));
    }
    #[test]
    fn absent_running_files_preserve_live_modules_and_report_incomplete_inventory() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("proc");
        let sys = t.path().join("sys");
        write(&p, "live 8192 0 - Live 0\n");
        write(&sys.join("live/taint"), "");
        let s = inventory(
            &t.path().join("missing"),
            &p,
            &sys,
            "old".into(),
            "old".into(),
        );
        assert_eq!(s.modules.len(), 1);
        assert_eq!(s.modules[0].state, State::Loaded);
        assert_eq!(s.modules[0].external, Some(false));
        assert_eq!(s.kernels, ["old"]);
        assert_eq!(s.warnings.len(), 1);
        let m = loaded("foo bad bad - Live 0\n");
        assert!(m[0].memory.is_none());
        assert!(m[0].references.is_none());
    }
    #[test]
    fn compressed_names_modinfo_fields_filters_and_numeric_sort() {
        assert_eq!(
            module_name("kernel/foo-bar.ko.gz").as_deref(),
            Some("foo_bar")
        );
        assert!(module_name("x.ko.exe").is_none());
        assert!(relative_path("../x").is_none());
        assert!(!valid_name("x;rm"));
        let d = parse_details(
            "filename: /a.ko\0version=1\0alias=pci:a\0alias=pci:b\0description=two\nlines\0intree=Y\0",
        );
        assert_eq!(d.fields["alias"], ["pci:a", "pci:b"]);
        assert_eq!(d.fields["description"], ["two\nlines"]);
        let mut a = blank("small".into());
        a.memory = Some(8);
        a.state = State::Loaded;
        a.external = Some(true);
        let mut b = blank("large".into());
        b.memory = Some(4096);
        b.state = State::Loaded;
        b.external = Some(false);
        let mut s = Snapshot {
            running: "k".into(),
            kernel: "k".into(),
            kernels: vec![],
            modules: vec![a, b],
            warnings: vec![],
            updated: Instant::now(),
        };
        assert_eq!(visible(&s, "", Filter::All, Sort::Memory, true), [1, 0]);
        assert_eq!(visible(&s, "", Filter::External, Sort::Name, false), [0]);
        assert!(visible(&s, "none", Filter::All, Sort::Name, false).is_empty());
        s.modules[0].state = State::Builtin;
        assert_eq!(
            visible(&s, "SMALL", Filter::Builtin, Sort::Name, false),
            [0]
        );
    }
    #[test]
    fn installed_package_ownership_classifies_kernel_dkms_firmware_and_graphics() {
        let t = tempfile::tempdir().unwrap();
        for (n, f) in [
            ("custom-kernel", "usr/lib/modules/k/kernel/test.ko.zst"),
            ("custom-network", "usr/src/custom-network-1/dkms.conf"),
            ("custom-fw", "usr/lib/firmware/vendor/a.bin"),
            ("mesa", "usr/lib/libGL.so"),
            ("linux-cachyos-headers", "usr/lib/modules/k/build/Makefile"),
            ("editor", "usr/bin/editor"),
        ] {
            write(
                &t.path().join(n).join("desc"),
                &format!("%NAME%\n{n}\n\n%VERSION%\n1\n"),
            );
            write(&t.path().join(n).join("files"), &format!("%FILES%\n{f}\n"));
        }
        write(&t.path().join("ALPM_DB_VERSION"), "9");
        let p = packages(t.path()).unwrap();
        assert_eq!(p.len(), 4);
        assert!(p.contains_key("custom-kernel"));
        assert!(p.contains_key("custom-fw"));
        assert!(!p.contains_key("linux-cachyos-headers"));
        assert!(relevant("amd-ucode", &[]).is_some());
        write(&t.path().join("broken/desc"), "%NAME%\nbroken\n");
        assert!(packages(t.path()).is_err());
    }
    #[test]
    fn update_parser_keeps_versions_and_excludes_unrelated_packages_without_false_success() {
        let p = HashMap::from([
            ("mesa".into(), "Grafik".into()),
            ("kernel-custom".into(), "Kernel".into()),
        ]);
        let (u, total) = parse_updates(
            "editor 1 -> 2\nmesa 1:1.0-2 -> 2:2.0-1\nkernel-custom 1 -> 2\n",
            &p,
        )
        .unwrap();
        assert_eq!(total, 3);
        assert_eq!(u.len(), 2);
        assert_eq!(u[1].package, "mesa");
        assert_eq!(u[1].old, "1:1.0-2");
        assert_eq!(u[1].new, "2:2.0-1");
        assert!(parse_updates("failure", &p).is_err());
        assert!(parse_updates("mesa 1 -> 2\nmesa 1 -> 2\n", &p).is_err());
        assert_eq!(parse_updates("", &p).unwrap().1, 0);
    }
    #[test]
    fn private_update_database_has_private_mode_and_does_not_follow_local_symlink_on_cleanup() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let outside = tempfile::tempdir().unwrap();
        write(&outside.path().join("keep"), "untouched");
        let path;
        {
            let tmp = TempDb::new().unwrap();
            path = tmp.0.clone();
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o700
            );
            symlink(outside.path(), path.join("local")).unwrap();
        }
        assert!(!path.exists());
        assert_eq!(text(&outside.path().join("keep")).unwrap(), "untouched");
    }
}
