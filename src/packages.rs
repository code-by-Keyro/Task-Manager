//! Graphical package transactions. No automatic answers, shells, or mutation in smoke tests.
use crate::apps::{self, App, Scope};
use eframe::egui::{self, RichText};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::fd::FromRawFd;
use std::os::unix::{fs::MetadataExt, process::CommandExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Pacman(String),
    Flatpak {
        installation: String,
        reference: String,
    },
    Starter(PathBuf),
}
#[derive(Clone, Debug)]
pub enum Operation {
    Remove(Box<App>),
    Upgrade { count: usize, drivers: Vec<String> },
}
#[derive(Clone, Debug)]
pub struct Plan {
    pub title: String,
    pub explanation: String,
    pub affected: Vec<String>,
    pub target: Option<Target>,
    pub app: Option<App>,
    pub identity: String,
    pub created: Instant,
}
fn token(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('-')
        && s.len() < 256
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._+-:".contains(&b))
}
fn protected(name: &str) -> bool {
    [
        "pacman",
        "glibc",
        "bash",
        "filesystem",
        "systemd",
        "systemd-sysvcompat",
        "polkit",
        "sudo",
        "linux-task-manager",
        "task-manager",
        "cachyos-settings",
        "cachyos-keyring",
        "archlinux-keyring",
        "plasma-workspace",
        "plasma-desktop",
        "kwin",
        "sddm",
        "gdm",
        "lightdm",
    ]
    .contains(&name)
        || name.starts_with("linux-")
        || name.starts_with("lib32-")
        || name.starts_with("nvidia")
        || name.starts_with("mesa")
}
fn same_app(a: &App, b: &App) -> bool {
    a.id == b.id
        && a.package == b.package
        && a.version == b.version
        && a.source == b.source
        && a.scope == b.scope
        && a.reference == b.reference
        && a.desktop == b.desktop
        && a.command == b.command
        && a.extra == b.extra
}
fn target(a: &App) -> Result<Target, String> {
    if a.extra {
        return Err(crate::tr!("Additional system/library packages cannot be removed here. Select an app with its own entry.").into());
    }
    if a.source == "Pacman" {
        let name = a
            .package
            .as_ref()
            .ok_or(crate::tr!("No unique package mapping"))?;
        if !token(name) || protected(name) {
            return Err(crate::tr!(
                "This essential system/driver package cannot be uninstalled here."
            )
            .into());
        }
        return Ok(Target::Pacman(name.clone()));
    }
    if let Some(rest) = a.id.strip_prefix("flatpak:") {
        let (installation, reference) = rest
            .split_once(':')
            .ok_or(crate::tr!("Ambiguous Flatpak installation"))?;
        let parts: Vec<_> = reference.split('/').collect();
        if !token(installation)
            || parts.len() != 3
            || !parts.iter().all(|s| token(s))
            || Some(parts[0]) != a.package.as_deref()
            || (installation == "user") != (a.scope == Scope::User)
        {
            return Err(crate::tr!("Ambiguous Flatpak identity").into());
        }
        return Ok(Target::Flatpak {
            installation: installation.into(),
            reference: format!("app/{reference}"),
        });
    }
    let path = a.desktop.as_ref().ok_or(crate::tr!(
        "This manual app has no supported uninstall method."
    ))?;
    if a.scope != Scope::User {
        return Err(crate::tr!("No package owner is known for this system launcher; safe uninstallation is unavailable.").into());
    }
    let base = std::env::var_os("XDG_DATA_HOME")
        .filter(|p| Path::new(p).is_absolute())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/share")))
        .ok_or(crate::tr!("Unknown user path"))?
        .join("applications");
    let parent = path
        .parent()
        .and_then(|p| p.canonicalize().ok())
        .ok_or(crate::tr!("Launcher path unreadable"))?;
    let base = base
        .canonicalize()
        .map_err(|_| crate::tr!("User launcher directory unreadable"))?;
    if !parent.starts_with(base) || path.extension().is_none_or(|s| s != "desktop") {
        return Err(crate::tr!(
            "This launcher is outside the supported user application directory."
        )
        .into());
    }
    // The task manager must retain its own click starters.
    if shell_words::split(&a.command)
        .ok()
        .and_then(|s| s.first().cloned())
        .and_then(|s| Path::new(&s).canonicalize().ok())
        == std::env::current_exe()
            .ok()
            .and_then(|p| p.canonicalize().ok())
    {
        return Err(crate::tr!("The Task-Manager's own launcher is preserved.").into());
    }
    Ok(Target::Starter(path.clone()))
}
pub fn action_label(a: &App) -> Result<&'static str, String> {
    target(a).map(|t| {
        if matches!(t, Target::Starter(_)) {
            crate::tr!("Remove launcher")
        } else {
            crate::tr!("Uninstall")
        }
    })
}
fn run(command: &str, args: &[&str]) -> Result<String, String> {
    crate::command::run(
        command,
        args,
        Duration::from_secs(8),
        crate::tr!("Package check"),
    )
}
fn fp_scope(installation: &str) -> String {
    if installation == "user" {
        "--user".into()
    } else if installation == "system" || installation == "default" {
        "--system".into()
    } else {
        format!("--installation={installation}")
    }
}
fn fingerprint(p: &Path) -> Result<String, String> {
    let m = fs::symlink_metadata(p).map_err(|e| e.to_string())?;
    // Trash the directory entry itself, never follow a desktop symlink to its target.
    if (!m.is_file() && !m.file_type().is_symlink()) || m.uid() != unsafe { libc::getuid() } {
        return Err(
            crate::tr!("Launcher is not owned by the current user or is not a file.").into(),
        );
    }
    Ok(format!(
        "{}:{}:{}:{}:{}",
        m.dev(),
        m.ino(),
        m.len(),
        m.mtime(),
        m.mtime_nsec()
    ))
}
fn prepare(op: Operation) -> Result<Plan, String> {
    match op {
        Operation::Upgrade { count, drivers } => Ok(Plan {
            title: crate::tr!("Update drivers and system").into(),
            explanation: crate::tf!(
                "CachyOS updates drivers together with the entire system. The last check reported {0} package updates. Pacman calculates the current full transaction and asks again before installation. Additional dependencies/replacements may be included. Administrator approval is graphical; kernel updates require a manual restart.",
                count
            ),
            affected: drivers,
            target: None,
            app: None,
            identity: String::new(),
            created: Instant::now(),
        }),
        Operation::Remove(a) => {
            let t = target(&a)?;
            let s = apps::sample();
            let current = s.apps.iter().find(|b| same_app(&a, b)).ok_or(crate::tr!(
                "The app has changed or is no longer installed. Refresh the list."
            ))?;
            let (title,explanation,identity,affected)=match &t {
                Target::Pacman(name)=>{
                    let q=run("/usr/bin/pacman", &["-Q","--",name])?;
                    let expected=format!("{} {}",name,a.version.as_deref().ok_or(crate::tr!("Missing package version"))?);
                    if q.trim()!=expected{return Err(crate::tr!("Package version changed; select again.").into());}
                    let preview=run("/usr/bin/pacman", &["-Rp","--print-format","%n %v","--",name])?;
                    if preview.trim()!=expected {return Err(crate::tr!("Uninstallation includes unexpected packages; request rejected.").into());}
                    let names=s.apps.iter().filter(|b|b.package==a.package && b.source=="Pacman" && !b.extra).map(|b|b.name.clone()).collect();
                    (crate::tr!("Uninstall app").into(),crate::tf!("Removes package {0}, including all its launchers listed below. Other packages are not removed automatically. Pacman checks dependencies; required packages are preserved. Personal app data is preserved. Administrator approval and final package confirmation follow." , name),expected,names)
                },
                Target::Flatpak{installation,reference}=>{
                    let scope=fp_scope(installation);let commit=run("/usr/bin/flatpak", &["info",&scope,"--show-commit",reference])?;
                    if commit.trim().is_empty(){return Err(crate::tr!("Ambiguous Flatpak version").into());}
                    (crate::tr!("Uninstall app").into(),crate::tf!("Removes exactly {0} from installation {1}. App data, shared runtimes and related extensions are preserved. A system installation may require graphical administrator approval." , reference, installation),commit.trim().into(),vec![current.name.clone()])
                },
                Target::Starter(path)=>(crate::tr!("Remove launcher only").into(),crate::tr!("This entry belongs to a manually installed app. Only the personal .desktop file is moved to the trash. Program files, AppImage, games and data remain; this does not uninstall the application.").into(),fingerprint(path)?,vec![current.name.clone()]),
            };
            Ok(Plan {
                title,
                explanation,
                affected,
                target: Some(t),
                app: Some(*a),
                identity,
                created: Instant::now(),
            })
        }
    }
}
#[derive(Debug, PartialEq, Eq)]
struct Spec {
    command: String,
    args: Vec<String>,
}
fn specification(plan: &Plan, exe: &Path) -> Result<Spec, String> {
    if plan.created.elapsed() > Duration::from_secs(300) {
        return Err(
            crate::tr!("Confirmation is older than five minutes. Please prepare again.").into(),
        );
    }
    let args = match &plan.target {
        Some(Target::Pacman(name)) => {
            if !token(name) || protected(name) || !plan.identity.starts_with(&format!("{name} ")) {
                return Err(crate::tr!("Invalid package request").into());
            }
            vec![
                "--disable-internal-agent".into(),
                exe.display().to_string(),
                "--package-action".into(),
                "remove".into(),
                name.clone(),
                plan.identity
                    .split_once(' ')
                    .ok_or(crate::tr!("Missing version"))?
                    .1
                    .into(),
            ]
        }
        None => vec![
            "--disable-internal-agent".into(),
            exe.display().to_string(),
            "--package-action".into(),
            "upgrade".into(),
        ],
        Some(Target::Flatpak {
            installation,
            reference,
        }) => {
            return Ok(Spec {
                command: "/usr/bin/flatpak".into(),
                args: vec![
                    "uninstall".into(),
                    fp_scope(installation),
                    "--no-related".into(),
                    "--app".into(),
                    reference.clone(),
                ],
            });
        }
        Some(Target::Starter(path)) => {
            return Ok(Spec {
                command: "/usr/bin/gio".into(),
                args: vec!["trash".into(), "--".into(), path.display().to_string()],
            });
        }
    };
    Ok(Spec {
        command: "/usr/bin/pkexec".into(),
        args,
    })
}
fn recheck(plan: &Plan) -> Result<(), String> {
    if let Some(a) = &plan.app {
        let s = apps::sample();
        if !s.apps.iter().any(|b| same_app(a, b)) {
            return Err(crate::tr!(
                "App or installation has changed since confirmation. Prepare again."
            )
            .into());
        }
        if Some(target(a)?) != plan.target {
            return Err(crate::tr!("Package mapping changed").into());
        }
    }
    match &plan.target {
        Some(Target::Pacman(n))
            if run("/usr/bin/pacman", &["-Q", "--", n])?.trim() != plan.identity =>
        {
            Err(crate::tr!("Package version has changed").into())
        }
        Some(Target::Flatpak {
            installation,
            reference,
        }) if run(
            "/usr/bin/flatpak",
            &["info", &fp_scope(installation), "--show-commit", reference],
        )?
        .trim()
            != plan.identity =>
        {
            Err(crate::tr!("Flatpak commit has changed").into())
        }
        Some(Target::Starter(p)) if fingerprint(p)? != plan.identity => {
            Err(crate::tr!("Launcher file has changed").into())
        }
        _ => Ok(()),
    }
}
// Invoked headlessly by pkexec. Only the CLI child receives administrator rights.
pub fn helper(args: &[String]) -> Result<(), String> {
    if unsafe { libc::geteuid() } != 0 {
        return Err(crate::tr!("Package action requires graphical administrator approval.").into());
    }
    let action = match args {
        [op] if op == "upgrade" => vec!["-Syu".to_owned(), "--color".into(), "never".into()],
        [op, name, version]
            if op == "remove" && token(name) && token(version) && !protected(name) =>
        {
            if run("/usr/bin/pacman", &["-Q", "--", name])?.trim() != format!("{name} {version}") {
                return Err(crate::tr!(
                    "Package version changed after administrator approval; request rejected."
                )
                .into());
            }
            vec![
                "-R".into(),
                "--color".into(),
                "never".into(),
                "--".into(),
                name.clone(),
            ]
        }
        _ => return Err(crate::tr!("Invalid package action").into()),
    };
    // Line buffering exposes the complete package summary before its interactive question.
    let error = Command::new("/usr/bin/stdbuf")
        .args(["-oL", "-eL", "/usr/bin/pacman"])
        .args(action)
        .env("LC_ALL", "C")
        .env("PATH", "/usr/bin:/bin")
        .exec();
    Err(error.to_string())
}
fn prompt(log: &str) -> bool {
    let line = log
        .lines()
        .last()
        .unwrap_or("")
        .trim()
        .trim_end_matches(':')
        .trim();
    line.to_ascii_lowercase().ends_with("[y/n]")
        || (line.starts_with("Enter ") && log.trim_end().ends_with(':'))
}
fn append(log: &mut String, s: &str) {
    log.push_str(&s.replace('\r', "\n"));
    if log.len() > 250_000 {
        let mut cut = log.len() - 200_000;
        while !log.is_char_boundary(cut) {
            cut += 1;
        }
        log.drain(..cut);
    }
}
enum Event {
    Plan(Box<Plan>),
    Log(String),
    Done(Result<String, String>),
}
fn emit(tx: &SyncSender<Event>, ctx: &egui::Context, event: Event) {
    if matches!(event, Event::Log(_)) {
        let _ = tx.try_send(event);
    } else {
        let _ = tx.send(event);
    }
    ctx.request_repaint();
}
fn transaction(
    spec: Spec,
    answers: Receiver<(String, String)>,
    tx: SyncSender<Event>,
    ctx: egui::Context,
) {
    let result = (|| {
        let mut fds = [-1; 2];
        // SAFETY: pipe2 initializes exactly two descriptors on success; File owns each once.
        if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        // SAFETY: descriptors were created above and ownership is transferred exactly once.
        let reader = unsafe { File::from_raw_fd(fds[0]) };
        // SAFETY: the second descriptor is independent of the read descriptor.
        let writer = unsafe { File::from_raw_fd(fds[1]) };
        let error_writer = writer.try_clone().map_err(|e| e.to_string())?;
        let mut child = Command::new(&spec.command)
            .args(&spec.args)
            .env("LC_ALL", "C")
            .env("PATH", "/usr/bin:/bin")
            .stdin(Stdio::piped())
            .stdout(Stdio::from(writer))
            .stderr(Stdio::from(error_writer))
            .spawn()
            .map_err(|e| crate::tf!("Unable to start package manager: {0}", e))?;
        let mut stdin = child.stdin.take().unwrap();
        let (chunks, input) = mpsc::sync_channel(64);
        let mut readers = vec![];
        {
            let pipe = reader;
            let chunks = chunks.clone();
            readers.push(thread::spawn(move || {
                let mut pipe = pipe;
                let mut bytes = [0; 4096];
                loop {
                    match pipe.read(&mut bytes) {
                        Ok(0) => break,
                        Ok(n) => {
                            if chunks.send(bytes[..n].to_vec()).is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
            }));
        }
        drop(chunks);
        let mut log = String::new();
        let mut ui_gone = false;
        let status = loop {
            while let Ok(bytes) = input.try_recv() {
                append(&mut log, &String::from_utf8_lossy(&bytes));
                emit(&tx, &ctx, Event::Log(log.clone()));
            }
            if prompt(&log) {
                match answers.try_recv() {
                    Ok((expected, answer))
                        if expected == log
                            && answer.len() <= 100
                            && !answer.contains(['\n', '\r']) =>
                    {
                        stdin
                            .write_all(format!("{answer}\n").as_bytes())
                            .map_err(|e| e.to_string())?;
                        append(&mut log, &format!("\n> {answer}\n"));
                        emit(&tx, &ctx, Event::Log(log.clone()));
                    }
                    Err(mpsc::TryRecvError::Disconnected) if !ui_gone => {
                        ui_gone = true;
                        stdin.write_all(b"n\n").map_err(|e| e.to_string())?;
                        append(&mut log, crate::tr!("\n> n (window closed)\n"));
                    }
                    _ => {}
                }
            }
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                break status;
            }
            thread::sleep(Duration::from_millis(40));
        };
        drop(stdin);
        // Never kill/timeout a package transaction, including its hooks. Drain until both pipes close.
        for bytes in input {
            append(&mut log, &String::from_utf8_lossy(&bytes));
        }
        for t in readers {
            let _ = t.join();
        }
        let _ = tx.send(Event::Log(log));
        match status.code(){Some(0)=>Ok(crate::tr!("Package manager completed successfully. Refreshing the list. Restart the computer when convenient after kernel updates.").into()),Some(126)=>Err(crate::tr!("Administrator approval cancelled; package action was not performed.").into()),Some(127)=>Err(crate::tr!("Administrator approval or launch failed. Is a Polkit agent active in your session?").into()),_=>Err(crate::tf!("Package manager exited with status {0}. See the output for details; after an error, a transaction may already have partly completed." , status))}
    })();
    emit(&tx, &ctx, Event::Done(result));
}

pub struct View {
    ctx: egui::Context,
    smoke: bool,
    receiver: Option<Receiver<Event>>,
    answers: Option<Sender<(String, String)>>,
    pub active: bool,
    open: bool,
    plan: Option<Plan>,
    preparing: bool,
    log: String,
    result: Option<Result<String, String>>,
    answer: String,
    changed: bool,
}
impl View {
    pub fn new(ctx: egui::Context, smoke: bool) -> Self {
        Self {
            ctx,
            smoke,
            receiver: None,
            answers: None,
            active: false,
            open: false,
            plan: None,
            preparing: false,
            log: String::new(),
            result: None,
            answer: String::new(),
            changed: false,
        }
    }
    pub fn busy(&self) -> bool {
        self.active || self.preparing
    }
    pub fn propose(&mut self, op: Operation) {
        if self.busy() {
            self.open = true;
            return;
        }
        self.open = true;
        self.plan = None;
        self.result = None;
        self.log.clear();
        self.preparing = true;
        let (tx, rx) = mpsc::sync_channel(8);
        self.receiver = Some(rx);
        let ctx = self.ctx.clone();
        thread::spawn(move || match prepare(op) {
            Ok(p) => emit(&tx, &ctx, Event::Plan(Box::new(p))),
            Err(e) => emit(&tx, &ctx, Event::Done(Err(e))),
        });
    }
    fn execute(&mut self) {
        if self.smoke || self.busy() {
            return;
        }
        let Some(plan) = self.plan.take() else {
            return;
        };
        self.active = true;
        let (tx, rx) = mpsc::sync_channel(8);
        self.receiver = Some(rx);
        let (answers, input) = mpsc::channel();
        self.answers = Some(answers);
        let ctx = self.ctx.clone();
        thread::spawn(move || {
            let result = recheck(&plan).and_then(|_| {
                specification(&plan, &std::env::current_exe().map_err(|e| e.to_string())?)
            });
            match result {
                Ok(spec) => transaction(spec, input, tx, ctx),
                Err(e) => emit(&tx, &ctx, Event::Done(Err(e))),
            }
        });
    }
    pub fn receive(&mut self) {
        if let Some(rx) = &self.receiver {
            while let Ok(e) = rx.try_recv() {
                match e {
                    Event::Plan(p) => {
                        self.preparing = false;
                        self.plan = Some(*p);
                    }
                    Event::Log(s) => self.log = s,
                    Event::Done(result) => {
                        self.active = false;
                        self.preparing = false;
                        self.answers = None;
                        self.changed = true;
                        self.result = Some(result);
                    }
                }
            }
        }
    }
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }
    fn answer(&mut self, value: &str) {
        if self.active
            && prompt(&self.log)
            && !self.smoke
            && let Some(tx) = &self.answers
            && tx.send((self.log.clone(), value.into())).is_ok()
        {
            append(&mut self.log, &format!("\n> {value}\n"));
        }
    }
    pub fn show(&mut self, ctx: &egui::Context) {
        if !self.open {
            return;
        }
        let mut open = true;
        let mut execute = false;
        let mut close = false;
        egui::Window::new(crate::tr!("Package manager")).open(&mut open).default_width(680.0).vscroll(true).hscroll(false).max_height((ctx.content_rect().height()-120.0).max(200.0)).show(ctx,|ui|{
            if self.smoke{ui.label(RichText::new(crate::tr!("Layout check · Actions locked")).strong());}
            if self.preparing{ui.spinner();ui.label(crate::tr!("Checking package action …"));}
            if let Some(p)=&self.plan{
                ui.heading(&p.title);ui.label(&p.explanation);ui.separator();
                for name in &p.affected{ui.label(name);}
                ui.separator();ui.horizontal(|ui|{if ui.button(crate::tr!("Cancel")).clicked(){close=true;}
                if ui.add_enabled(!self.smoke,egui::Button::new(if p.target.is_none(){crate::tr!("Start system upgrade")}else if matches!(p.target,Some(Target::Starter(_))){crate::tr!("Move launcher to trash")}else{crate::tr!("Start uninstall")})).clicked(){execute=true;}});
            }
            if self.active{ui.spinner();ui.label(crate::tr!("Package manager is running. Administrator approval appears separately when needed."));ui.label(crate::tr!("This window stays open until completion. Answer package questions below; installation is not interrupted automatically."));}
            if !self.log.is_empty(){egui::ScrollArea::vertical().id_salt("package-output").max_height(250.0).stick_to_bottom(true).show(ui,|ui|{ui.add(egui::Label::new(RichText::new(&self.log).monospace()).wrap());});}
            if self.active && prompt(&self.log){ui.separator();ui.label(crate::tr!("The package manager is waiting for an answer:"));ui.horizontal_wrapped(|ui|{if ui.button(crate::tr!("Yes")).clicked(){self.answer("y");}
                if ui.button(crate::tr!("No")).clicked(){self.answer("n");}ui.add(egui::TextEdit::singleline(&mut self.answer).hint_text(crate::tr!("Other answer, e.g. a number")).desired_width(230.0));if ui.button(crate::tr!("Send answer")).clicked(){let a=std::mem::take(&mut self.answer);self.answer(&a);}});}
            if let Some(result)=&self.result{ui.separator();ui.label(RichText::new(if result.is_ok(){crate::tr!("Complete")}else{crate::tr!("Did not complete successfully")}).strong());ui.label(result.as_ref().map(String::as_str).unwrap_or_else(|e|e.as_str()));if ui.button(crate::tr!("Close")).clicked(){close=true;}}
        });
        if execute {
            self.execute();
        }
        self.open = self.active || self.preparing || (open && !close);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn app() -> App {
        App {
            id: "desktop:test.desktop".into(),
            name: "Test".into(),
            version: Some("1:2.0-1".into()),
            publisher: None,
            source: "Pacman".into(),
            scope: Scope::System,
            command: "test".into(),
            description: String::new(),
            package: Some("test-app".into()),
            size: None,
            installed: None,
            desktop: None,
            extra: false,
            reference: None,
        }
    }
    fn plan(t: Option<Target>) -> Plan {
        Plan {
            title: String::new(),
            explanation: String::new(),
            affected: vec![],
            target: t,
            app: None,
            identity: "test-app 1:2.0-1".into(),
            created: Instant::now(),
        }
    }
    #[test]
    fn uninstall_targets_pin_package_and_full_flatpak_installation_branch_arch() {
        let mut a = app();
        assert_eq!(target(&a).unwrap(), Target::Pacman("test-app".into()));
        a.source = "Flatpak · origin".into();
        a.package = Some("org.example.Test".into());
        a.id = "flatpak:user:org.example.Test/aarch64/beta".into();
        a.scope = Scope::User;
        assert_eq!(
            target(&a).unwrap(),
            Target::Flatpak {
                installation: "user".into(),
                reference: "app/org.example.Test/aarch64/beta".into()
            }
        );
        a.id = "flatpak:secondary:org.example.Test/aarch64/beta".into();
        assert!(target(&a).is_err());
        a.scope = Scope::System;
        assert!(target(&a).is_ok());
        a.id = "flatpak:secondary:org.other.Test/aarch64/beta".into();
        assert!(target(&a).is_err());
    }
    #[test]
    fn extra_core_ambiguous_and_option_like_packages_are_rejected() {
        let mut a = app();
        a.extra = true;
        assert!(target(&a).is_err());
        a.extra = false;
        for n in [
            "pacman",
            "linux-cachyos",
            "glibc",
            "plasma-workspace",
            "-Rsc",
            "test;rm",
        ] {
            a.package = Some(n.into());
            assert!(target(&a).is_err(), "{n}");
        }
        a.package = None;
        assert!(target(&a).is_err());
        let mut b = app();
        let a = app();
        assert!(same_app(&a, &b));
        b.version = Some("2".into());
        assert!(!same_app(&a, &b));
        b = a.clone();
        b.scope = Scope::User;
        assert!(!same_app(&a, &b));
    }
    #[test]
    fn commands_never_force_remove_dependencies_delete_data_or_auto_answer() {
        let p = plan(Some(Target::Pacman("test-app".into())));
        let s = specification(&p, Path::new("/test/Task Manager")).unwrap();
        assert_eq!(s.command, "/usr/bin/pkexec");
        assert_eq!(
            s.args,
            [
                "--disable-internal-agent",
                "/test/Task Manager",
                "--package-action",
                "remove",
                "test-app",
                "1:2.0-1"
            ]
        );
        let p = plan(None);
        let s = specification(&p, Path::new("/test/app")).unwrap();
        assert_eq!(s.args.last().unwrap(), "upgrade");
        let p = plan(Some(Target::Flatpak {
            installation: "secondary".into(),
            reference: "app/org.test.App/x86_64/beta".into(),
        }));
        let s = specification(&p, Path::new("/test/app")).unwrap();
        assert_eq!(s.command, "/usr/bin/flatpak");
        assert!(s.args.contains(&"--installation=secondary".into()));
        assert!(s.args.contains(&"--no-related".into()));
        assert!(!s.args.iter().any(|a| {
            [
                "--force-remove",
                "--delete-data",
                "--all",
                "--unused",
                "--assumeyes",
                "--noconfirm",
            ]
            .contains(&a.as_str())
        }));
        let mut p = plan(None);
        p.created = Instant::now() - Duration::from_secs(301);
        assert!(specification(&p, Path::new("/test/app")).is_err());
    }
    #[test]
    fn log_prompt_detection_and_bounding_keep_unicode_valid() {
        assert!(prompt(
            "Packages (2) a b\n:: Proceed with installation? [Y/n] "
        ));
        assert!(prompt("Proceed with these changes? [Y/n]: "));
        assert!(prompt("Enter a number (default=1): "));
        assert!(!prompt("Installing packages...\n"));
        assert!(!prompt("Proceed? [Y/n]\n> y\n"));
        let mut log = "ü".repeat(150_000);
        append(&mut log, "\nProceed? [y/N]");
        assert!(log.len() <= 200_000);
        assert!(prompt(&log));
    }
    #[test]
    fn starter_identity_detects_replacement_and_keeps_symlink_target_untouched() {
        use std::os::unix::fs::symlink;
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("app.desktop");
        fs::write(&p, "A").unwrap();
        let first = fingerprint(&p).unwrap();
        fs::remove_file(&p).unwrap();
        fs::write(&p, "BBBB").unwrap();
        assert_ne!(first, fingerprint(&p).unwrap());
        let q = t.path().join("linked.desktop");
        symlink(&p, &q).unwrap();
        assert!(fingerprint(&q).is_ok());
        assert!(fingerprint(t.path()).is_err());
        assert_eq!(fs::read_to_string(&p).unwrap(), "BBBB");
    }
    #[test]
    fn smoke_mode_cannot_execute_even_a_constructed_upgrade_plan() {
        let mut view = View::new(egui::Context::default(), true);
        view.plan = Some(plan(None));
        view.execute();
        assert!(!view.active);
        assert!(view.receiver.is_none());
        assert!(view.plan.is_some());
    }
    #[test]
    fn interactive_transport_shows_prompt_accepts_one_answer_and_waits_for_child() {
        let ctx = egui::Context::default();
        let (tx, rx) = mpsc::sync_channel(8);
        let (input, answers) = mpsc::channel();
        let child = thread::spawn(move || {
            transaction(Spec{command:"/bin/sh".into(),args:vec!["-c".into(),"printf 'Packages: disposable fixture\n'; printf 'Proceed? [Y/n] ' >&2; read reply; printf '\nanswer=%s\n' \"$reply\"; printf 'Second? [y/N] '; read second; printf '\nsecond=%s\n' \"$second\"; test \"$reply/$second\" = n/n".into()]},answers,tx,ctx)
        });
        let mut seen = false;
        let mut final_log = String::new();
        let result = loop {
            match rx.recv_timeout(Duration::from_secs(3)).unwrap() {
                Event::Log(log) => {
                    if prompt(&log) && !seen {
                        seen = true;
                        input.send((log.clone(), "n".into())).unwrap();
                        input.send((log.clone(), "y".into())).unwrap();
                    }
                    if prompt(&log) && log.contains("Second?") {
                        input.send((log.clone(), "n".into())).unwrap();
                    }
                    final_log = log;
                }
                Event::Done(r) => break r,
                _ => {}
            }
        };
        child.join().unwrap();
        assert!(seen);
        assert!(result.is_ok());
        assert!(final_log.contains("answer=n"));
        assert!(final_log.contains("second=n"));
    }
}
