//! Conservative, explicit process selection. Idle is never treated as proof of redundancy.
use crate::monitor::{self, ProcessRow, Snapshot};
use std::collections::{HashMap, HashSet};
use std::time::Instant;

pub type Identity = (u32, u64);

#[derive(Clone)]
pub struct Candidate {
    pub process: ProcessRow,
    pub reason: &'static str,
}
pub struct Scan {
    pub candidates: Vec<Candidate>,
    pub excluded: usize,
    pub selected: HashSet<Identity>,
    pub selecting: bool,
    pub category: Option<bool>,
    pub created: Instant,
}
#[derive(Default)]
pub struct Estimate {
    pub cpu: f64,
    pub memory: u64,
}

pub fn estimate<'a>(rows: impl Iterator<Item = &'a ProcessRow>) -> Estimate {
    let mut seen = HashSet::new();
    let mut result = Estimate::default();
    for row in rows {
        if seen.insert((row.pid, row.started)) {
            result.cpu += row.cpu;
            result.memory = result.memory.saturating_add(row.memory);
        }
    }
    result
}

fn protected(row: &ProcessRow, uid: u32, own_pid: u32) -> bool {
    if uid == 0
        || row.uid != Some(uid)
        || row.pid <= 1
        || row.pid == own_pid
        || row.parent_pid <= 1
        || row.command.is_empty()
        || matches!(row.status.as_str(), "Zombie" | "Beendet")
    {
        return true;
    }
    let Some(cgroup) = &row.cgroup else {
        return true;
    };
    if cgroup.trim().is_empty() {
        return true;
    }
    let group = cgroup.to_ascii_lowercase();
    // user@UID.service is the ancestor of every systemd user session, not a
    // background service candidate itself. Desktop app-* units may own windows.
    let service = group
        .lines()
        .filter_map(|line| line.splitn(3, ':').nth(2))
        .flat_map(|path| path.split('/'))
        .any(|unit| {
            let unit = unit.trim();
            unit.ends_with(".service") && !unit.starts_with("user@") && !unit.starts_with("app-")
        });
    if service {
        return true;
    }
    if [
        "/system.slice/",
        "docker",
        "containerd",
        "libpod",
        "kubepods",
        "lxc",
    ]
    .iter()
    .any(|part| group.contains(part))
    {
        return true;
    }
    let name = row.name.to_ascii_lowercase();
    [
        "systemd",
        "init",
        "kwin_wayland",
        "kwin_x11",
        "plasmashell",
        "plasma-",
        "rustdesk",
        "anydesk",
        "kioworker",
        "klauncher",
        "xdg-",
        "at-spi",
        "ksmserver",
        "kded6",
        "kded5",
        "kglobalacceld",
        "kscreenlocker_greet",
        "xdg-desktop-portal",
        "dbus",
        "pipewire",
        "wireplumber",
        "pulseaudio",
        "xorg",
        "xwayland",
        "sshd",
        "ssh-agent",
        "gpg-agent",
        "polkit",
        "dockerd",
        "containerd",
        "task-manager",
        "codex",
        "cachyos",
        "sway",
        "hyprland",
        "gnome-shell",
    ]
    .iter()
    .any(|part| name.starts_with(part))
        || ["--type=", "-contentproc", "crashpad", "chrome-sandbox"]
            .iter()
            .any(|part| row.command.contains(part))
}

fn ancestors(pid: u32, rows: &HashMap<u32, &ProcessRow>) -> HashSet<u32> {
    let mut chain = HashSet::new();
    let mut current = pid;
    while chain.insert(current) {
        let Some(row) = rows.get(&current) else { break };
        if row.parent_pid == 0 {
            break;
        }
        current = row.parent_pid;
    }
    chain
}

impl Scan {
    pub fn new(snapshot: &Snapshot, uid: u32, own_pid: u32) -> Self {
        let rows: HashMap<_, _> = snapshot.processes.iter().map(|p| (p.pid, p)).collect();
        let own_chain = ancestors(own_pid, &rows);
        let window_groups: HashSet<_> = snapshot
            .processes
            .iter()
            .filter(|p| p.application.is_some())
            .filter_map(|p| p.cgroup.as_deref().map(str::trim))
            .collect();
        let mut candidates = Vec::new();
        for row in &snapshot.processes {
            let chain = ancestors(row.parent_pid, &rows);
            let helper = row.application.is_none()
                && (row
                    .cgroup
                    .as_deref()
                    .is_some_and(|g| window_groups.contains(g.trim()))
                    || chain
                        .iter()
                        .any(|pid| rows.get(pid).is_some_and(|p| p.application.is_some())));
            // Descendants of this task manager and of protected session/automation tools
            // must not become candidates just because they have no window.
            let child_of_tool = chain.iter().any(|pid| {
                *pid == own_pid
                    || rows
                        .get(pid)
                        .is_some_and(|p| p.name.to_ascii_lowercase().starts_with("codex"))
            });
            if protected(row, uid, own_pid)
                || own_chain.contains(&row.pid)
                || helper
                || child_of_tool
                || (row.memory < 8 * 1024 * 1024 && row.cpu < 0.1)
            {
                continue;
            }
            candidates.push(Candidate {
                process: row.clone(),
                reason: if row.application.is_some() {
                    crate::tr!("Application – close only if no longer needed")
                } else {
                    crate::tr!("Independent background process – check whether you need it")
                },
            });
        }
        candidates.sort_by(|a, b| {
            b.process
                .memory
                .cmp(&a.process.memory)
                .then_with(|| a.process.pid.cmp(&b.process.pid))
        });
        Self {
            excluded: snapshot.processes.len() - candidates.len(),
            candidates,
            selected: HashSet::new(),
            selecting: false,
            category: None,
            created: Instant::now(),
        }
    }
    pub fn selected_rows(&self) -> Vec<ProcessRow> {
        self.candidates
            .iter()
            .filter(|c| self.selected.contains(&(c.process.pid, c.process.started)))
            .map(|c| c.process.clone())
            .collect()
    }
}

pub struct Outcome {
    pub process: ProcessRow,
    pub requested: bool,
    pub message: String,
}
pub struct Run {
    pub outcomes: Vec<Outcome>,
    pub available_before: u64,
}

/// Recheck the current policy and start time. The injected action permits testing
/// every rejection path without ever sending signals to user processes.
pub fn execute_with(
    selected: &[ProcessRow],
    latest: &Snapshot,
    uid: u32,
    own_pid: u32,
    mut action: impl FnMut(&ProcessRow) -> Result<(), String>,
) -> Run {
    let allowed = Scan::new(latest, uid, own_pid);
    let eligible: HashMap<_, _> = allowed
        .candidates
        .iter()
        .map(|c| ((c.process.pid, c.process.started), &c.process))
        .collect();
    let mut seen = HashSet::new();
    let outcomes = selected
        .iter()
        .filter(|p| seen.insert((p.pid, p.started)))
        .map(|process| {
            let result = match eligible.get(&(process.pid, process.started)) {
                Some(current) => action(current),
                None => Err(crate::tr!("No longer present or now protected; skipped.").into()),
            };
            Outcome {
                process: process.clone(),
                requested: result.is_ok(),
                message: result
                    .err()
                    .unwrap_or_else(|| crate::tr!("SIGTERM requested").into()),
            }
        })
        .collect();
    Run {
        outcomes,
        available_before: latest.memory_available,
    }
}

pub fn end_current(row: &ProcessRow, uid: u32, own_pid: u32) -> Result<(), String> {
    let current = monitor::revalidate_process(row).map_err(|e| e.to_string())?;
    if protected(&current, uid, own_pid) {
        return Err(crate::tr!("Current process data is protected or unreadable; skipped.").into());
    }
    monitor::end_process(current.pid, current.started, false).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn row(pid: u32, parent_pid: u32) -> ProcessRow {
        ProcessRow {
            pid,
            parent_pid,
            started: 100,
            name: "editor".into(),
            status: "Bereit".into(),
            cgroup: Some(format!("0::/user.slice/user-1000.slice/app-{pid}.scope\n")),
            uid: Some(1000),
            cpu: 1.0,
            memory: 16 * 1024 * 1024,
            io_rate: None,
            user: "test".into(),
            threads: 1,
            command: "/usr/bin/editor".into(),
            executable: Some("/usr/bin/editor".into()),
            application: None,
        }
    }
    #[test]
    fn policy_excludes_services_containers_unknown_owners_and_app_helpers() {
        let mut rows = vec![
            row(20, 10),
            row(21, 20),
            row(22, 10),
            row(23, 10),
            row(24, 10),
            row(25, 10),
            row(26, 10),
            row(27, 10),
            row(28, 10),
            row(29, 30),
            row(30, 10),
        ];
        rows[0].application = Some("Editor".into());
        rows[2].cgroup = Some("0::/system.slice/example.service".into());
        rows[3].cgroup = Some("0::/user.slice/docker-deadbeef.scope".into());
        rows[4].cgroup = None;
        rows[5].uid = Some(0);
        rows[6].name = "plasmashell".into();
        rows[7].command = "browser --type=renderer".into();
        rows[10].name = "codex".into();
        let snapshot = Snapshot {
            processes: rows,
            ..Default::default()
        };
        let scan = Scan::new(&snapshot, 1000, 99);
        assert_eq!(
            scan.candidates
                .iter()
                .map(|c| c.process.pid)
                .collect::<Vec<_>>(),
            vec![20, 28]
        );
        assert!(scan.selected.is_empty());
        assert!(Scan::new(&snapshot, 0, 99).candidates.is_empty());
    }
    #[test]
    fn systemd_user_ancestor_does_not_exclude_desktop_applications() {
        let mut app = row(20, 10);
        app.application = Some("Editor".into());
        app.cgroup = Some("1:net_cls:/\n0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-editor@123.service\n".into());
        let mut helper = app.clone();
        helper.pid = 21;
        helper.parent_pid = 20;
        helper.application = None;
        let mut background = row(22, 10);
        background.cgroup = Some(
            "0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-worker.scope\n".into(),
        );
        let mut daemon = row(23, 10);
        daemon.cgroup = Some(
            "0::/user.slice/user-1000.slice/user@1000.service/background.slice/sync.service\n"
                .into(),
        );
        let mut same_unit_helper = helper.clone();
        same_unit_helper.pid = 24;
        same_unit_helper.parent_pid = 10;
        let mut standalone_service = row(25, 10);
        standalone_service.cgroup =
            Some("0::/user.slice/user@1000.service/app.slice/app-worker@456.service\n".into());
        let snapshot = Snapshot {
            processes: vec![
                app,
                helper,
                background,
                daemon,
                same_unit_helper,
                standalone_service,
            ],
            ..Default::default()
        };
        assert_eq!(
            Scan::new(&snapshot, 1000, 99)
                .candidates
                .iter()
                .map(|c| c.process.pid)
                .collect::<Vec<_>>(),
            vec![20, 22, 25]
        );
    }
    #[test]
    fn own_ancestors_descendants_and_small_idle_processes_are_excluded() {
        let mut small = row(43, 10);
        small.memory = 100;
        small.cpu = 0.0;
        let snapshot = Snapshot {
            processes: vec![row(40, 10), row(41, 40), row(42, 41), small, row(44, 10)],
            ..Default::default()
        };
        assert_eq!(
            Scan::new(&snapshot, 1000, 41)
                .candidates
                .iter()
                .map(|c| c.process.pid)
                .collect::<Vec<_>>(),
            vec![44]
        );
    }
    #[test]
    fn selection_rechecks_identity_policy_and_deduplicates_without_real_signals() {
        let selected = vec![row(20, 10), row(21, 10), row(22, 10), row(20, 10)];
        let mut reused = row(21, 10);
        reused.started = 200;
        let mut service = row(22, 10);
        service.cgroup = Some("0::/a.service".into());
        let latest = Snapshot {
            processes: vec![row(20, 10), reused, service],
            ..Default::default()
        };
        let mut calls = Vec::new();
        let run = execute_with(&selected, &latest, 1000, 99, |p| {
            calls.push(p.pid);
            Ok(())
        });
        assert_eq!(calls, vec![20]);
        assert_eq!(run.outcomes.len(), 3);
        assert_eq!(run.outcomes.iter().filter(|o| o.requested).count(), 1);
        assert!(
            execute_with(&[], &latest, 1000, 99, |_| panic!("empty selection"))
                .outcomes
                .is_empty()
        );
        assert_eq!(estimate(selected.iter()).memory, 48 * 1024 * 1024);
    }
}
