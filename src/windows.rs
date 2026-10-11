//! Read-only window ownership for process categories. No window-management requests.
use std::collections::HashMap;
use std::fs;
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle, backend::ObjectId, protocol::wl_registry,
};
use wayland_protocols_plasma::plasma_window_management::client::{
    org_kde_plasma_window as window, org_kde_plasma_window_management as management,
};
use x11rb::{connection::Connection as _, protocol::xproto::ConnectionExt as _};

#[derive(Clone, Default)]
struct WindowInfo {
    pid: u32,
    app_id: String,
    skip_taskbar: bool,
}
#[derive(Default)]
struct PlasmaState {
    manager: Option<management::OrgKdePlasmaWindowManagement>,
    windows: HashMap<ObjectId, WindowInfo>,
}
impl Dispatch<wl_registry::WlRegistry, ()> for PlasmaState {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
            && interface == "org_kde_plasma_window_management"
            && version >= 13
            && state.manager.is_none()
        {
            state.manager = Some(registry.bind(name, version.min(18), qh, ()));
        }
    }
}
impl Dispatch<management::OrgKdePlasmaWindowManagement, ()> for PlasmaState {
    fn event(
        state: &mut Self,
        manager: &management::OrgKdePlasmaWindowManagement,
        event: management::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let management::Event::WindowWithUuid { uuid, .. } = event {
            let window = manager.get_window_by_uuid(uuid, qh, ());
            state.windows.insert(window.id(), WindowInfo::default());
        }
    }
}
impl Dispatch<window::OrgKdePlasmaWindow, ()> for PlasmaState {
    fn event(
        state: &mut Self,
        window: &window::OrgKdePlasmaWindow,
        event: window::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if matches!(event, window::Event::Unmapped) {
            state.windows.remove(&window.id());
            window.destroy();
            return;
        }
        let info = state.windows.entry(window.id()).or_default();
        match event {
            window::Event::PidChanged { pid } => info.pid = pid,
            window::Event::AppIdChanged { app_id } => info.app_id = app_id,
            window::Event::StateChanged { flags } => {
                info.skip_taskbar = flags & management::State::Skiptaskbar as u32 != 0
            }
            _ => {}
        }
    }
}
struct Plasma {
    queue: EventQueue<PlasmaState>,
    state: PlasmaState,
    first: bool,
}
impl Plasma {
    fn new() -> Option<Self> {
        let connection = Connection::connect_to_env().ok()?;
        let queue = connection.new_event_queue();
        connection.display().get_registry(&queue.handle(), ());
        Some(Self {
            queue,
            state: PlasmaState::default(),
            first: true,
        })
    }
    fn sample(&mut self) -> Option<HashMap<u32, String>> {
        let budget = if self.first {
            Duration::from_millis(150)
        } else {
            Duration::from_millis(8)
        };
        self.first = false;
        let start = Instant::now();
        while start.elapsed() < budget {
            self.queue.dispatch_pending(&mut self.state).ok()?;
            self.queue.flush().ok()?;
            let Some(guard) = self.queue.prepare_read() else {
                continue;
            };
            let mut fd = libc::pollfd {
                fd: guard.connection_fd().as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: fd points to one valid pollfd and the timeout is bounded.
            let ready = unsafe { libc::poll(&mut fd, 1, 3) };
            if ready < 0 {
                return None;
            }
            if ready == 0 {
                drop(guard);
                if self.state.manager.is_some() {
                    break;
                }
                continue;
            }
            guard.read().ok()?;
        }
        self.queue.dispatch_pending(&mut self.state).ok()?;
        self.state.manager.as_ref()?;
        Some(
            self.state
                .windows
                .values()
                .filter(|w| w.pid > 0 && !w.skip_taskbar)
                .map(|w| (w.pid, w.app_id.clone()))
                .collect(),
        )
    }
}

#[derive(Default)]
struct Catalog {
    names: HashMap<String, String>,
    executables: HashMap<String, String>,
}
impl Catalog {
    fn new() -> Self {
        let mut result = Self::default();
        let data_dirs =
            std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
        let mut dirs: Vec<_> = data_dirs
            .split(':')
            .filter(|s| s.starts_with('/'))
            .map(|s| PathBuf::from(s).join("applications"))
            .collect();
        let user = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/share")));
        if let Some(user) = user {
            dirs.push(user.join("applications"));
        }
        for dir in dirs {
            for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.extension().is_none_or(|e| e != "desktop") {
                    continue;
                }
                if let Ok(text) = fs::read_to_string(&path) {
                    result.add(path.file_stem().unwrap().to_string_lossy().as_ref(), &text);
                }
            }
        }
        result
    }
    fn add(&mut self, id: &str, text: &str) {
        let mut entry = false;
        let mut fields = HashMap::new();
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                entry = line == "[Desktop Entry]";
                continue;
            }
            if entry && let Some((key, value)) = line.split_once('=') {
                fields.insert(key, value);
            }
        }
        if fields.get("Hidden") == Some(&"true") || fields.get("Type") != Some(&"Application") {
            return;
        }
        let Some(name) = crate::i18n::desktop_keys("Name")
            .iter()
            .find_map(|key| fields.get(key.as_str()).copied())
        else {
            return;
        };
        self.names.insert(id.into(), name.into());
        if fields.get("NoDisplay") == Some(&"true") || fields.get("Terminal") == Some(&"true") {
            return;
        }
        if let Some(exec) = fields.get("Exec")
            && let Ok(words) = shell_words::split(exec)
            && let Some(program) = words
                .first()
                .and_then(|p| std::path::Path::new(p).file_name())
                .and_then(|p| p.to_str())
            && !["env", "sh", "bash", "python", "python3"].contains(&program)
            && !words.iter().skip(1).any(|w| w.contains("://"))
        {
            self.executables.insert(program.into(), name.into());
        }
    }
    fn name(&self, id: &str) -> Option<String> {
        self.names.get(id.trim_end_matches(".desktop")).cloned()
    }
}

pub struct WindowTracker {
    plasma: Option<Plasma>,
    catalog: Catalog,
}
pub struct Applications {
    pub names: HashMap<u32, String>,
    pub source: &'static str,
    pub exact: bool,
}
impl WindowTracker {
    pub fn new() -> Self {
        Self {
            plasma: Plasma::new(),
            catalog: Catalog::new(),
        }
    }
    pub fn sample(&mut self) -> Applications {
        if let Some(plasma) = &mut self.plasma
            && let Some(windows) = plasma.sample()
        {
            return Applications {
                names: windows
                    .into_iter()
                    .map(|(pid, id)| {
                        (
                            pid,
                            self.catalog
                                .name(&id)
                                .unwrap_or_else(|| id.rsplit('.').next().unwrap_or(&id).to_owned()),
                        )
                    })
                    .collect(),
                source: crate::tr!("KDE windows"),
                exact: true,
            };
        }
        if let Some(windows) = x11_windows() {
            return Applications {
                names: windows,
                source: crate::tr!("X11 windows"),
                exact: true,
            };
        }
        Applications {
            names: HashMap::new(),
            source: crate::tr!("Desktop entries (estimated)"),
            exact: false,
        }
    }
    pub fn fallback_name(
        &self,
        executable: &str,
        command: &str,
        same_user: bool,
    ) -> Option<String> {
        if !same_user || is_helper(command) {
            return None;
        }
        self.catalog.executables.get(executable).cloned()
    }
}
fn is_helper(command: &str) -> bool {
    [
        "--type=",
        "-contentproc",
        "--utility-sub-type",
        "--background",
        "--daemon",
    ]
    .iter()
    .any(|flag| command.contains(flag))
}
fn x11_windows() -> Option<HashMap<u32, String>> {
    // Only native X11: an Xwayland-only list cannot classify native Wayland applications.
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return None;
    }
    let (connection, screen) = x11rb::connect(None).ok()?;
    let root = connection.setup().roots.get(screen)?.root;
    let atom = |name: &[u8]| {
        connection
            .intern_atom(true, name)
            .ok()?
            .reply()
            .ok()
            .map(|r| r.atom)
    };
    let clients = atom(b"_NET_CLIENT_LIST")?;
    let pid_atom = atom(b"_NET_WM_PID")?;
    if clients == 0 || pid_atom == 0 {
        return None;
    }
    let windows = connection
        .get_property(
            false,
            root,
            clients,
            x11rb::protocol::xproto::AtomEnum::WINDOW,
            0,
            4096,
        )
        .ok()?
        .reply()
        .ok()?;
    let mut result = HashMap::new();
    for window in windows.value32()? {
        if let Ok(cookie) = connection.get_property(
            false,
            window,
            pid_atom,
            x11rb::protocol::xproto::AtomEnum::CARDINAL,
            0,
            1,
        ) && let Ok(value) = cookie.reply()
            && let Some(pid) = value.value32().and_then(|mut v| v.next())
            && pid > 0
        {
            result.insert(pid, String::new());
        }
    }
    Some(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn desktop_catalog_excludes_helpers_and_terminal_tools() {
        let mut catalog = Catalog::default();
        catalog.add("org.test.App","[Desktop Entry]\nType=Application\nName=Test\nName[de]=Test-App\nExec=\"/opt/Test App/app\" %U\n[Desktop Action Other]\nName=Wrong\nExec=other\n");
        assert_eq!(
            catalog.name("org.test.App.desktop"),
            Some("Test-App".into())
        );
        assert_eq!(catalog.executables.get("app"), Some(&"Test-App".into()));
        catalog.add(
            "hidden",
            "[Desktop Entry]\nType=Application\nHidden=true\nName=Hidden\nExec=hidden",
        );
        catalog.add(
            "term",
            "[Desktop Entry]\nType=Application\nTerminal=true\nName=Terminal Tool\nExec=tool",
        );
        assert!(!catalog.executables.contains_key("hidden"));
        assert!(!catalog.executables.contains_key("tool"));
        assert!(is_helper("browser --type=renderer"));
        assert!(!is_helper("browser https://example.invalid"));
    }
}
