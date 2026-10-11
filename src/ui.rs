use crate::install::{self, InstallPaths};
use crate::monitor::{self, Control, Monitor, ProcessRow, Snapshot};
use crate::{docker, optimizer, processes};
use eframe::egui::{
    self, Align, Color32, FontId, Id, Layout, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Vec2,
};
use egui_extras::{Column, TableBuilder};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::File;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const BLUE: Color32 = Color32::from_rgb(0, 120, 215);
const COLORS: [Color32; 4] = [
    Color32::from_rgb(20, 133, 197),
    Color32::from_rgb(148, 96, 184),
    Color32::from_rgb(87, 155, 48),
    Color32::from_rgb(189, 120, 80),
];
#[derive(Clone, PartialEq, Eq, Hash)]
enum Resource {
    Cpu,
    Memory,
    Network,
    Disk(String),
    Gpu(String),
}

#[derive(Default)]
pub struct Startup {
    pub smoke: bool,
    pub screenshot: Option<PathBuf>,
    pub install: bool,
    pub uninstall: bool,
    pub performance: bool,
    pub dark: Option<bool>,
    pub view: Option<String>,
}

fn theme_path() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|p| std::path::Path::new(p).is_absolute())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(config.join("linux-task-manager/theme"))
}
fn load_theme(path: &std::path::Path) -> bool {
    std::fs::read_to_string(path).is_ok_and(|s| s.trim() == "dark")
}
fn save_theme(path: &std::path::Path, dark: bool) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, if dark { "dark\n" } else { "light\n" })
}
fn apply_theme(ctx: &egui::Context, dark: bool) {
    let mut visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    if !dark {
        visuals.panel_fill = Color32::WHITE;
        visuals.window_fill = Color32::WHITE;
    }
    visuals.override_text_color = Some(if dark {
        Color32::from_rgb(225, 230, 235)
    } else {
        Color32::from_rgb(32, 40, 50)
    });
    visuals.weak_text_color = Some(if dark {
        Color32::from_rgb(170, 185, 198)
    } else {
        Color32::from_rgb(96, 112, 128)
    });
    visuals.selection.bg_fill = if dark {
        Color32::from_rgb(35, 67, 93)
    } else {
        Color32::from_rgb(220, 238, 252)
    };
    visuals.selection.stroke = Stroke::new(
        1.0_f32,
        if dark {
            Color32::from_rgb(100, 185, 255)
        } else {
            BLUE
        },
    );
    ctx.set_visuals(visuals);
}
pub fn uptime(seconds: u64) -> String {
    crate::tf!(
        "{0} d {1}:{2}:{3}",
        seconds / 86400,
        format!("{:02}", seconds / 3600 % 24),
        format!("{:02}", seconds / 60 % 60),
        format!("{:02}", seconds % 60)
    )
}
fn temperature_text(values: &[crate::hardware::Temperature]) -> String {
    if values.is_empty() {
        return crate::tr!("Temperature: unavailable").into();
    }
    values
        .iter()
        .map(|t| format!("{}: {} °C", t.label, decimal(t.celsius, 1)))
        .collect::<Vec<_>>()
        .join(" · ")
}
fn optional_percent(value: Option<f64>) -> String {
    value
        .map(|v| format!("{} %", decimal(v, 1)))
        .unwrap_or_else(|| crate::tr!("unavailable").into())
}

pub fn decimal(value: f64, digits: usize) -> String {
    crate::i18n::decimal(value, digits)
}
pub fn size(mut value: f64) -> String {
    for suffix in ["B", "KiB", "MiB", "GiB", "TiB"] {
        if value.abs() < 1024.0 || suffix == "TiB" {
            return format!("{} {suffix}", decimal(value, 1));
        }
        value /= 1024.0;
    }
    "—".to_owned()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Processes,
    Performance,
    Details,
    System,
    Docker,
    Optimize,
    Installed,
    Drivers,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Sort {
    Name,
    Status,
    Cpu,
    Memory,
    Io,
    Pid,
    User,
    Threads,
    Command,
}

enum Dialog {
    End(ProcessRow, bool),
    EndGroup(Vec<ProcessRow>, bool),
    Docker(
        docker::Container,
        docker::Target,
        docker::Operation,
        Instant,
        String,
    ),
    Optimize(Vec<ProcessRow>),
    Install,
    Uninstall,
    NewTask,
    About,
    Message(String, String),
}

struct History {
    values: [f64; 6],
    cores: HashMap<usize, f64>,
    disks: HashMap<String, (f64, f64)>,
    gpus: HashMap<String, Option<f64>>,
}

pub struct TaskApp {
    packages: crate::packages::View,
    installed: crate::apps_ui::View,
    drivers: crate::drivers_ui::View,
    monitor: Monitor,
    docker_monitor: docker::Monitor,
    docker_snapshot: Option<docker::Snapshot>,
    docker_active: bool,
    docker_busy: bool,
    docker_selected: Option<String>,
    scan: Option<optimizer::Scan>,
    optimization: Option<optimizer::Run>,
    snapshot_received: Option<Instant>,
    snapshot: Option<Snapshot>,
    history: VecDeque<History>,
    visible: Vec<usize>,
    applications_open: bool,
    background_open: bool,
    process_group: Option<bool>,
    query: String,
    only_mine: bool,
    uid: u32,
    sort: Sort,
    descending: bool,
    dirty: bool,
    selected: Option<(u32, u64)>,
    selected_group: Option<processes::Key>,
    opened_processes: HashSet<processes::Key>,
    tab: Tab,
    resource: Resource,
    dark: bool,
    per_core: bool,
    paused: bool,
    interval: f64,
    top: bool,
    dialog: Option<Dialog>,
    new_command: String,
    error: Option<String>,
    cpu_name: String,
    distro: String,
    smoke: bool,
    screenshot: Option<PathBuf>,
    smoke_started: Instant,
    smoke_success: bool,
    smoke_layout_ready: bool,
    screenshot_requested: bool,
    screenshot_received: bool,
    samples: usize,
}

impl TaskApp {
    pub fn new(cc: &eframe::CreationContext<'_>, options: Startup) -> Self {
        let Startup {
            smoke,
            screenshot,
            install,
            uninstall,
            performance,
            dark,
            view,
        } = options;
        let dark = dark.unwrap_or_else(|| theme_path().is_some_and(|p| load_theme(&p)));
        let mut style = (*cc.egui_ctx.style()).clone();
        style.spacing.item_spacing = Vec2::new(9.0, 8.0);
        style.spacing.button_padding = Vec2::new(12.0, 7.0);
        style
            .text_styles
            .insert(egui::TextStyle::Body, FontId::proportional(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, FontId::proportional(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Small, FontId::proportional(12.0));
        cc.egui_ctx.set_style(style);
        apply_theme(&cc.egui_ctx, dark);
        // SAFETY: getuid has no arguments and no side effects.
        let uid = unsafe { libc::getuid() };
        Self {
            packages: crate::packages::View::new(cc.egui_ctx.clone(), smoke),
            installed: crate::apps_ui::View::new(cc.egui_ctx.clone(), smoke),
            drivers: crate::drivers_ui::View::new(cc.egui_ctx.clone(), smoke),
            monitor: Monitor::start(cc.egui_ctx.clone()),
            docker_monitor: docker::Monitor::start(cc.egui_ctx.clone()),
            docker_snapshot: None,
            docker_active: false,
            docker_busy: false,
            docker_selected: None,
            scan: None,
            optimization: None,
            snapshot_received: None,
            snapshot: None,
            history: VecDeque::new(),
            visible: Vec::new(),
            applications_open: true,
            background_open: true,
            process_group: match view.as_deref() {
                Some("applications") => Some(true),
                Some("background") => Some(false),
                _ => None,
            },
            query: String::new(),
            only_mine: false,
            uid,
            sort: Sort::Cpu,
            descending: true,
            dirty: true,
            selected: None,
            selected_group: None,
            opened_processes: HashSet::new(),
            tab: if matches!(
                view.as_deref(),
                Some("processes" | "applications" | "background")
            ) {
                Tab::Processes
            } else if view.as_deref() == Some("system") {
                Tab::System
            } else if view.as_deref() == Some("docker") {
                Tab::Docker
            } else if view.as_deref() == Some("drivers") {
                Tab::Drivers
            } else if view.as_deref() == Some("installed") {
                Tab::Installed
            } else if view.as_deref() == Some("optimize") {
                Tab::Optimize
            } else if performance || view.is_some() {
                Tab::Performance
            } else {
                Tab::Processes
            },
            resource: match view.as_deref() {
                Some("memory") => Resource::Memory,
                Some("gpu") => Resource::Gpu(String::new()),
                Some("disk") => Resource::Disk(if smoke {
                    std::env::var("TASK_MANAGER_SMOKE_DISK").unwrap_or_default()
                } else {
                    String::new()
                }),
                Some("network") => Resource::Network,
                _ => Resource::Cpu,
            },
            dark,
            per_core: true,
            paused: false,
            interval: 1.0,
            top: false,
            dialog: if let Some(notice) = crate::i18n::notice() {
                Some(Dialog::Message(
                    crate::tr!("Language configuration").into(),
                    notice,
                ))
            } else if install {
                Some(Dialog::Install)
            } else if uninstall {
                Some(Dialog::Uninstall)
            } else {
                None
            },
            new_command: String::new(),
            error: None,
            cpu_name: monitor::cpu_name(),
            distro: monitor::distribution(),
            smoke,
            screenshot,
            smoke_started: Instant::now(),
            smoke_success: false,
            smoke_layout_ready: false,
            screenshot_requested: false,
            screenshot_received: false,
            samples: 0,
        }
    }

    fn refresh(&mut self) {
        if self.tab == Tab::Installed {
            self.installed.refresh();
        }
        if self.tab == Tab::Drivers {
            self.drivers.refresh();
        }
        self.paused = false;
        let _ = self.monitor.control.send(Control::Refresh);
        if self.tab == Tab::Docker {
            let _ = self.docker_monitor.control.send(docker::Control::Refresh);
        }
    }

    fn receive(&mut self) {
        self.packages.receive();
        if self.packages.take_changed() {
            self.installed.refresh();
            self.drivers.refresh();
        }
        self.installed.receive();
        self.drivers.receive();
        while let Ok(snapshot) = self.docker_monitor.receiver.try_recv() {
            if self
                .docker_selected
                .as_ref()
                .is_some_and(|id| !snapshot.containers.iter().any(|c| &c.id == id))
            {
                self.docker_selected = None;
            }
            self.docker_snapshot = Some(snapshot);
        }
        while let Ok(result) = self.docker_monitor.action_receiver.try_recv() {
            self.docker_busy = false;
            self.dialog = Some(Dialog::Message(
                format!("{} · {}", result.operation.label(), result.name),
                match result.result {
                    Ok(message) => message,
                    Err(error) => error,
                },
            ));
        }
        while let Ok(result) = self.monitor.receiver.try_recv() {
            match result {
                Ok(snapshot) => {
                    self.samples += 1;
                    self.snapshot_received = Some(Instant::now());
                    self.error = None;
                    self.history.push_back(History {
                        values: [
                            snapshot.cpu,
                            snapshot.memory_percent(),
                            snapshot.disk_read,
                            snapshot.disk_write,
                            snapshot.network_recv,
                            snapshot.network_sent,
                        ],
                        cores: snapshot
                            .core_ids
                            .iter()
                            .copied()
                            .zip(snapshot.cores.iter().copied())
                            .collect(),
                        disks: snapshot
                            .disks
                            .iter()
                            .map(|d| (d.id.clone(), (d.read, d.write)))
                            .collect(),
                        gpus: snapshot
                            .gpus
                            .iter()
                            .map(|g| (g.id.clone(), g.usage))
                            .collect(),
                    });
                    if self.history.len() > 60 {
                        self.history.pop_front();
                    }
                    if self.selected.is_some_and(|identity| {
                        !snapshot
                            .processes
                            .iter()
                            .any(|p| (p.pid, p.started) == identity)
                    }) {
                        self.selected = None;
                    }
                    self.opened_processes
                        .retain(|key| snapshot.processes.iter().any(|p| &processes::key(p) == key));
                    if self.selected_group.as_ref().is_some_and(|key| {
                        !snapshot.processes.iter().any(|p| &processes::key(p) == key)
                    }) {
                        self.selected_group = None;
                    }
                    self.snapshot = Some(snapshot);
                    self.dirty = true;
                }
                Err(error) => self.error = Some(error),
            }
        }
    }

    fn filter(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        let query = self.query.trim().to_lowercase();
        self.visible = snapshot
            .processes
            .iter()
            .enumerate()
            .filter_map(|(index, p)| {
                if self.only_mine && p.uid != Some(self.uid) {
                    return None;
                }
                if !query.is_empty()
                    && !format!(
                        "{} {} {} {} {}",
                        p.name,
                        p.pid,
                        p.user,
                        p.command,
                        p.application.as_deref().unwrap_or("")
                    )
                    .to_lowercase()
                    .contains(&query)
                {
                    return None;
                }
                Some(index)
            })
            .collect();
        self.visible.sort_unstable_by(|a, b| {
            compare_processes(
                &snapshot.processes[*a],
                &snapshot.processes[*b],
                self.sort,
                self.tab == Tab::Details,
                self.descending,
            )
        });
    }

    fn menus(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menus")
            .frame(
                egui::Frame::new()
                    .fill(ctx.style().visuals.panel_fill)
                    .inner_margin(egui::Margin::symmetric(12, 3)),
            )
            .show(ctx, |ui| {
                egui::MenuBar::new().ui(ui, |ui| {
                    ui.menu_button(crate::tr!("File"), |ui| {
                        if ui.button(crate::tr!("Run new task …     Ctrl+N")).clicked() {
                            self.dialog = Some(Dialog::NewTask);
                            ui.close();
                        }
                        ui.separator();
                        if ui.button(crate::tr!("Install for my user …")).clicked() {
                            self.dialog = Some(Dialog::Install);
                            ui.close();
                        }
                        if ui.button(crate::tr!("Uninstall …")).clicked() {
                            self.dialog = Some(Dialog::Uninstall);
                            ui.close();
                        }
                        ui.separator();
                        if ui
                            .add_enabled(
                                !self.packages.active,
                                egui::Button::new(crate::tr!("Quit     Ctrl+Q")),
                            )
                            .clicked()
                        {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                    ui.menu_button(crate::tr!("Options"), |ui| {
                        if ui
                            .checkbox(&mut self.top, crate::tr!("Always on top"))
                            .changed()
                        {
                            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                                if self.top {
                                    egui::WindowLevel::AlwaysOnTop
                                } else {
                                    egui::WindowLevel::Normal
                                },
                            ));
                        }
                    });
                    ui.menu_button(crate::tr!("View"), |ui| {
                        if ui
                            .checkbox(&mut self.dark, crate::tr!("Dark mode"))
                            .changed()
                        {
                            apply_theme(ctx, self.dark);
                            let result = theme_path()
                                .ok_or_else(|| {
                                    crate::tr!("No configuration directory available").to_owned()
                                })
                                .and_then(|path| {
                                    save_theme(&path, self.dark).map_err(|e| e.to_string())
                                });
                            if let Err(error) = result {
                                self.dialog = Some(Dialog::Message(
                                    crate::tr!("Appearance").into(),
                                    crate::tf!(
                                        "The appearance was changed but could not be saved: {0}",
                                        error
                                    ),
                                ));
                            }
                        }
                        ui.separator();
                        if ui.button(crate::tr!("Refresh now     F5")).clicked() {
                            self.refresh();
                            ui.close();
                        }
                        if ui
                            .checkbox(&mut self.paused, crate::tr!("Pause updates"))
                            .changed()
                        {
                            let _ = self.monitor.control.send(Control::Pause(self.paused));
                        }
                        ui.separator();
                        ui.label(crate::tr!("Update speed"));
                        for (text, value) in [
                            (crate::tr!("High (0.5 s)"), 0.5),
                            (crate::tr!("Normal (1 s)"), 1.0),
                            (crate::tr!("Low (2 s)"), 2.0),
                        ] {
                            if ui.radio_value(&mut self.interval, value, text).changed() {
                                let _ = self
                                    .monitor
                                    .control
                                    .send(Control::Interval(Duration::from_secs_f64(value)));
                            }
                        }
                    });
                    ui.menu_button(crate::tr!("Help"), |ui| {
                        if ui.button(crate::tr!("About Task-Manager")).clicked() {
                            self.dialog = Some(Dialog::About);
                            ui.close();
                        }
                    });
                });
            });
        egui::TopBottomPanel::top("tabs")
            .frame(
                egui::Frame::new()
                    .fill(ctx.style().visuals.panel_fill)
                    .inner_margin(egui::Margin::symmetric(12, 6)),
            )
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (tab, text) in [
                        (Tab::Processes, crate::tr!("Processes")),
                        (Tab::Performance, crate::tr!("Performance")),
                        (Tab::Details, crate::tr!("Details")),
                        (Tab::System, crate::tr!("System")),
                        (Tab::Docker, "Docker"),
                        (Tab::Optimize, crate::tr!("Optimize")),
                        (Tab::Installed, crate::tr!("Installed apps")),
                        (Tab::Drivers, crate::tr!("Drivers")),
                    ] {
                        let active = self.tab == tab;
                        let response = ui.add(
                            egui::Button::new(RichText::new(text).color(if active {
                                BLUE
                            } else {
                                ui.visuals().weak_text_color()
                            }))
                            .frame(false)
                            .min_size(Vec2::new(95.0, 28.0)),
                        );
                        if response.clicked() {
                            self.tab = tab;
                            self.dirty = true;
                        }
                        if active {
                            let rect = response.rect;
                            ui.painter().line_segment(
                                [
                                    Pos2::new(rect.left(), rect.bottom() + 5.0),
                                    Pos2::new(rect.right(), rect.bottom() + 5.0),
                                ],
                                Stroke::new(3.0_f32, BLUE),
                            );
                        }
                    }
                });
            });
    }

    fn status_bar(&self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(if self.dark {
                        Color32::from_rgb(28, 33, 39)
                    } else {
                        Color32::from_rgb(249, 251, 252)
                    })
                    .inner_margin(egui::Margin::symmetric(14, 9)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if let Some(s) = &self.snapshot {
                        ui.label(
                            RichText::new(crate::tf!(
                                "{0} processes   |   CPU: {1} %   |   RAM: {2} %   |   Uptime: {3}",
                                s.processes.len(),
                                decimal(s.cpu, 0),
                                decimal(s.memory_percent(), 0),
                                uptime(s.uptime)
                            ))
                            .size(12.0),
                        )
                        .on_hover_text(crate::tf!(
                            "{0} processes unreadable due to missing permissions or process exit.",
                            s.skipped
                        ));
                    } else {
                        ui.label(crate::tr!("Reading system …"));
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let (text, color) = if self.error.is_some() {
                            (
                                crate::tr!("Measurement failed").to_owned(),
                                Color32::DARK_RED,
                            )
                        } else if self.paused {
                            (
                                crate::tr!("Paused").to_owned(),
                                Color32::from_rgb(153, 115, 44),
                            )
                        } else {
                            (
                                format!("Live · {} s", decimal(self.interval, 1)),
                                Color32::from_rgb(32, 121, 87),
                            )
                        };
                        ui.label(RichText::new(text).color(color).size(12.0));
                    });
                });
            });
    }

    fn process_page(&mut self, ui: &mut egui::Ui, details: bool) {
        if details {
            self.selected_group = None;
        }
        ui.style_mut().spacing.scroll.floating = false;
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let response = ui.add_sized(
                [430.0, 31.0],
                egui::TextEdit::singleline(&mut self.query)
                    .hint_text(crate::tr!("Search processes · Name, PID or user")),
            );
            if response.changed() {
                self.dirty = true;
            }
            if !self.query.is_empty() && ui.small_button("×").clicked() {
                self.query.clear();
                self.dirty = true;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .checkbox(&mut self.only_mine, crate::tr!("Only my processes"))
                    .changed()
                {
                    self.dirty = true;
                }
            });
        });
        ui.add_space(12.0);
        self.filter();
        let Some(snapshot) = &self.snapshot else {
            ui.label(crate::tr!("Reading processes …"));
            return;
        };
        if !details {
            let count = self
                .visible
                .iter()
                .filter(|i| snapshot.processes[**i].application.is_some())
                .count();
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.process_group,None,crate::tr!("All"));
                ui.selectable_value(&mut self.process_group,Some(true),crate::tf!("Applications ({0})" , count));
                ui.selectable_value(&mut self.process_group,Some(false),crate::tf!("Background processes ({0})" , self.visible.len()-count));
                if snapshot.application_source.contains(crate::tr!("estimated")) {ui.label(RichText::new(crate::tr!("Estimated classification")).small()).on_hover_text(crate::tr!("The desktop does not provide a readable window list. Classification uses desktop entries and may differ."));}
            });
        }
        let mut groups = processes::groups(&self.visible, &snapshot.processes);
        groups.sort_by(|a, b| {
            compare_processes(&a.summary, &b.summary, self.sort, false, self.descending)
        });
        let entries = if details {
            self.visible
                .iter()
                .map(|i| processes::Entry::Process(*i, false))
                .collect()
        } else {
            processes::entries(
                &groups,
                &self.opened_processes,
                self.applications_open,
                self.background_open,
                self.process_group,
            )
        };
        let columns = if details {
            vec![
                (Sort::Name, crate::tr!("Name"), 200.0),
                (Sort::Pid, "PID", 75.0),
                (Sort::User, crate::tr!("User"), 105.0),
                (Sort::Status, crate::tr!("Status"), 145.0),
                (Sort::Cpu, "CPU", 85.0),
                (Sort::Memory, crate::tr!("Memory"), 140.0),
                (Sort::Threads, crate::tr!("Threads"), 75.0),
                (Sort::Command, crate::tr!("Command line"), 300.0),
            ]
        } else {
            vec![
                (Sort::Name, crate::tr!("Name"), 270.0),
                (Sort::Status, crate::tr!("Status"), 160.0),
                (Sort::Cpu, "CPU", 90.0),
                (Sort::Memory, crate::tr!("Memory"), 155.0),
                (Sort::Io, crate::tr!("Disk"), 145.0),
                (Sort::Pid, "PID", 80.0),
            ]
        };
        let table_height = (ui.available_height() - 115.0).max(80.0);
        let mut selected = self.selected;
        let mut selected_group = self.selected_group.clone();
        let mut toggle_category = None;
        let mut toggle_process = None;
        let mut action = None;
        let mut go_details = None;
        let mut new_sort = None;
        egui::ScrollArea::horizontal().id_salt("table-horizontal").show(ui,|ui| {
            ui.set_min_width(if details {1170.0}else {970.0});
            let mut table=TableBuilder::new(ui).id_salt(if details {"details"}else {"processes"})
                .striped(false).resizable(true).cell_layout(Layout::left_to_right(Align::Center))
                .sense(Sense::click()).min_scrolled_height(0.0).max_scroll_height(table_height);
            for (_,_,width) in &columns {table=table.column(Column::initial(*width).at_least(65.0));}
            table.header(48.0,|mut header| {
                for (column,title,_) in &columns {
                    header.col(|ui| {
                        let arrow=if self.sort==*column {if self.descending {" v"}else {" ^"}}else {""};
                        let text=if !details && matches!(column,Sort::Cpu|Sort::Memory) {
                            let value=if *column==Sort::Cpu {snapshot.cpu}else {snapshot.memory_percent()};
                            format!("{} %\n{title}{arrow}",decimal(value,0))
                        }else {format!("{title}{arrow}")};
                        let response=ui.add_sized(ui.available_size(),egui::Button::new(RichText::new(text).size(13.0)).frame(false));
                        if response.clicked() {new_sort=Some(*column);}
                        if *column==Sort::Cpu {response.on_hover_text(crate::tr!("Share of total CPU capacity, normalized across all logical cores. Groups show the sum of their visible members."));}
                        else if *column==Sort::Memory {response.on_hover_text(crate::tr!("Resident memory (RSS). Groups sum members; shared pages may be counted more than once."));}
                    });
                }
            }).body(|body|body.rows(36.0,entries.len(),|mut row| {
                let entry=entries[row.index()];
                if let processes::Entry::Heading(applications,count)=entry {
                    let open=if applications {self.applications_open}else {self.background_open};
                    for (i,_) in columns.iter().enumerate() {
                        row.col(|ui| {if i==0 {
                            ui.add_space(4.0);
                            disclosure(ui,open,Sense::hover());
                            ui.label(RichText::new(format!("{} ({count})",if applications {crate::tr!("Applications")}else {crate::tr!("Background processes")})).strong().color(if self.dark {Color32::from_rgb(100,185,255)}else {BLUE}));
                        }});
                    }
                    if row.response().on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {toggle_category=Some(applications);}
                    return;
                }
                let (process,group,child)=match entry {
                    processes::Entry::Group(i)=>(&groups[i].summary,Some(&groups[i]),false),
                    processes::Entry::Process(i,child)=>(&snapshot.processes[i],None,child),
                    processes::Entry::Heading(..)=>unreachable!(),
                };
                let identity=(process.pid,process.started);
                let is_selected=if let Some(group)=group {selected_group.as_ref()==Some(&group.key)}else {selected==Some(identity)};
                row.set_selected(is_selected);
                for (column,_,_) in &columns {
                    row.col(|ui| {
                        let numeric=matches!(column,Sort::Cpu|Sort::Memory|Sort::Io|Sort::Pid|Sort::Threads);
                        let text=match column {
                            Sort::Name=> {
                                let name=if details || child {&process.name}else {process.application.as_deref().unwrap_or(&process.name)};
                                if let Some(group)=group {format!("{name} ({})",group.indices.len())}else {name.to_owned()}
                            },
                            Sort::Status=>crate::i18n::text(&process.status).to_owned(),
                            Sort::Cpu=>format!("{} %",decimal(process.cpu,1)),
                            Sort::Memory=>format!("{} MiB",decimal(process.memory as f64/1048576.0,1)),
                            Sort::Io=>process.io_rate.map(|v|format!("{} MiB/s",decimal(v/1048576.0,1))).unwrap_or_else(||"—".into()),
                            Sort::Pid=>group.map(|g|format!("{} PIDs",g.indices.len())).unwrap_or_else(||process.pid.to_string()),
                            Sort::User=>process.user.clone(),Sort::Threads=>process.threads.to_string(),Sort::Command=>process.command.clone(),
                        };
                        if matches!(column,Sort::Cpu|Sort::Memory|Sort::Io) && !is_selected {
                            let strength=match column {Sort::Cpu=>process.cpu/100.0,Sort::Memory=>process.memory as f64/1073741824.0,_=>process.io_rate.unwrap_or(0.0)/10485760.0}.clamp(0.0,1.0);
                            ui.painter().rect_filled(ui.max_rect().expand2(Vec2::new(4.0,0.0)),0.0,if self.dark {Color32::from_rgb(55+(50.0*strength) as u8,45+(20.0*strength) as u8,24)}else {Color32::from_rgb(255,247-(35.0*strength) as u8,214-(100.0*strength) as u8)});
                        }
                        ui.with_layout(if numeric {Layout::right_to_left(Align::Center)}else {Layout::left_to_right(Align::Center)},|ui| {
                            if !details && *column==Sort::Name {
                                ui.add_space(if child {48.0}else if group.is_some() {12.0}else {24.0});
                                if let Some(group)=group {
                                    let open=self.opened_processes.contains(&group.key);
                                    if disclosure(ui,open,Sense::click()).on_hover_text(crate::tr!("Expand/collapse individual processes")).clicked() {toggle_process=Some(group.key.clone());}
                                }
                            }
                            let tooltip=if let Some(group)=group {
                                crate::tf!("Sum of the {0} displayed processes. RSS may include shared pages more than once.\nPIDs: {1}" , group.indices.len(), group.indices.iter().map(|i|snapshot.processes[*i].pid.to_string()).collect::<Vec<_>>().join(", "))
                            }else {process.command.clone()};
                            ui.add(egui::Label::new(RichText::new(text).size(13.0)).truncate()).on_hover_text(tooltip);
                        });
                    });
                }
                let response=row.response();
                if response.clicked() {
                    if let Some(group)=group {selected_group=Some(group.key.clone());selected=None;}else {selected=Some(identity);selected_group=None;}
                }
                if response.double_clicked() && let Some(group)=group {toggle_process=Some(group.key.clone());}
                response.context_menu(|ui| {
                    for (label,force) in [(if group.is_some() {crate::tr!("End group …")}else {crate::tr!("End task …")},false),(crate::tr!("Force quit …"),true)] {
                        if ui.button(label).clicked() {
                            if let Some(group)=group {
                                action=Some(Dialog::EndGroup(group.indices.iter().map(|i|snapshot.processes[*i].clone()).collect(),force));
                                selected_group=Some(group.key.clone());selected=None;
                            }else {action=Some(Dialog::End(process.clone(),force));selected=Some(identity);selected_group=None;}
                            ui.close();
                        }
                    }
                    ui.separator();
                    if let Some(group)=group {
                        if ui.button(crate::tr!("Copy PID list")).clicked() {ui.ctx().copy_text(group.indices.iter().map(|i|snapshot.processes[*i].pid.to_string()).collect::<Vec<_>>().join(", "));ui.close();}
                    }else {
                        if ui.button(crate::tr!("Copy command line")).clicked() {ui.ctx().copy_text(process.command.clone());ui.close();}
                        if !details && ui.button(crate::tr!("Go to details")).clicked() {go_details=Some(identity);ui.close();}
                    }
                });
            }));
        });
        if let Some(key) = toggle_process
            && !self.opened_processes.remove(&key)
        {
            self.opened_processes.insert(key);
        }
        if let Some(applications) = toggle_category {
            if applications {
                self.applications_open = !self.applications_open;
            } else {
                self.background_open = !self.background_open;
            }
        }
        if let Some(column) = new_sort {
            if self.sort == column {
                self.descending = !self.descending;
            } else {
                self.sort = column;
                self.descending = !matches!(
                    column,
                    Sort::Name | Sort::User | Sort::Status | Sort::Command
                );
            }
            self.dirty = true;
        }
        let selected_members = selected_group
            .as_ref()
            .and_then(|key| groups.iter().find(|g| &g.key == key))
            .map(|g| {
                g.indices
                    .iter()
                    .map(|i| snapshot.processes[*i].clone())
                    .collect::<Vec<_>>()
            });
        let selected_row = selected
            .and_then(|identity| {
                snapshot
                    .processes
                    .iter()
                    .find(|p| (p.pid, p.started) == identity)
            })
            .cloned();
        self.selected = selected;
        self.selected_group = selected_group;
        if let Some(identity) = go_details {
            self.tab = Tab::Details;
            self.query = identity.0.to_string();
            self.only_mine = false;
            self.selected = Some(identity);
            self.selected_group = None;
            self.dirty = true;
        }
        if action.is_some() {
            self.dialog = action;
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(crate::tr!(
                    "CPU: system share · RAM: RSS · Groups: sum of visible members"
                ))
                .small(),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add_enabled(
                        selected_members.is_some() || selected_row.is_some(),
                        egui::Button::new(
                            RichText::new(if selected_members.is_some() {
                                crate::tr!("End group")
                            } else {
                                crate::tr!("End task")
                            })
                            .color(Color32::WHITE),
                        )
                        .fill(BLUE),
                    )
                    .clicked()
                {
                    if let Some(rows) = selected_members {
                        self.dialog = Some(Dialog::EndGroup(rows, false));
                    } else if let Some(row) = selected_row {
                        self.dialog = Some(Dialog::End(row, false));
                    }
                }
            });
        });
    }

    fn docker_page(&mut self, ui: &mut egui::Ui) {
        ui.style_mut().spacing.scroll.floating = false;
        ui.horizontal(|ui| {
            ui.heading(crate::tr!("Docker containers"));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button(crate::tr!("Refresh")).clicked() {
                    self.refresh();
                }
            });
        });
        ui.label(crate::tr!(
            "Containers and Compose projects · Metrics update about every 5 seconds"
        ));
        let Some(snapshot) = &self.docker_snapshot else {
            ui.add_space(30.0);
            ui.spinner();
            ui.label(crate::tr!("Querying Docker …"));
            return;
        };
        ui.label(
            RichText::new(crate::tf!(
                "Context: {0} · Last query {1} s ago{2}",
                snapshot.context,
                snapshot.updated.elapsed().as_secs(),
                if self.paused {
                    crate::tr!(" · paused")
                } else {
                    ""
                }
            ))
            .small(),
        );
        if let Some(error) = &snapshot.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
            if snapshot.containers.is_empty() {
                ui.label(crate::tr!("No container data available."));
                return;
            }
        }
        let cores = snapshot.cpus;
        let running = snapshot
            .containers
            .iter()
            .filter(|c| c.state == "running")
            .count();
        let cpu: Option<f64> = snapshot
            .containers
            .iter()
            .filter(|c| c.state == "running")
            .map(|c| c.cpu)
            .collect::<Option<Vec<_>>>()
            .map(|values| values.iter().sum());
        let memory: Option<u64> = snapshot
            .containers
            .iter()
            .filter(|c| c.state == "running")
            .map(|c| c.memory.map(|m| m.0))
            .collect::<Option<Vec<_>>>()
            .map(|v| v.iter().fold(0u64, |sum, n| sum.saturating_add(*n)));
        metric_cards(
            ui,
            &[
                (
                    crate::tr!("Containers").into(),
                    crate::tf!("{0} active / {1} total", running, snapshot.containers.len()),
                ),
                (
                    crate::tr!("CPU · Active containers").into(),
                    optional_percent(cpu.zip(cores).map(|(cpu, cores)| cpu / cores as f64)),
                ),
                (
                    crate::tr!("RAM · Active containers").into(),
                    memory
                        .map(|m| size(m as f64))
                        .unwrap_or_else(|| crate::tr!("unavailable").into()),
                ),
            ],
        );
        ui.add_space(8.0);
        ui.label(
            RichText::new(crate::tr!(
                "CPU: share of Docker host · RAM: Docker usage value · I/O: cumulative data"
            ))
            .small(),
        );
        ui.separator();
        if snapshot.containers.is_empty() {
            ui.label(if snapshot.error.is_some() {
                crate::tr!("No container data available.")
            } else {
                crate::tr!("No containers in the current Docker context.")
            });
            return;
        }
        let selected = self
            .docker_selected
            .as_ref()
            .and_then(|id| snapshot.containers.iter().find(|c| &c.id == id));
        let fresh = !self.paused
            && snapshot.error.is_none()
            && snapshot.updated.elapsed() < Duration::from_secs(30);
        ui.horizontal_wrapped(|ui| {
            ui.label(
                selected
                    .map(|c| crate::tf!("Selected: {0}", c.name))
                    .unwrap_or_else(|| crate::tr!("Select a container from the table").into()),
            );
            for operation in [docker::Operation::Stop, docker::Operation::Restart] {
                let enabled = fresh
                    && !self.docker_busy
                    && !self.smoke
                    && snapshot.target.is_some()
                    && selected.is_some_and(|c| operation.enabled(&c.state));
                if ui
                    .add_enabled(enabled, egui::Button::new(operation.label()))
                    .clicked()
                    && let (Some(c), Some(target)) = (selected, &snapshot.target)
                {
                    self.dialog = Some(Dialog::Docker(
                        c.clone(),
                        target.clone(),
                        operation,
                        snapshot.updated,
                        snapshot.context.clone(),
                    ));
                }
            }
            if self.docker_busy {
                ui.spinner();
                ui.label(crate::tr!("Docker action running …"));
            }
        });
        egui::ScrollArea::horizontal()
            .id_salt("docker-horizontal")
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
            .show(ui,|ui| {
            let pair_text = |value:Option<(u64,u64)>|value.map(|(a,b)|format!("{} / {}",size(a as f64),size(b as f64))).unwrap_or_else(||"—".into());
            let maximum = (ui.available_height() - 44.0).max(60.0);
            TableBuilder::new(ui).sense(Sense::click()).min_scrolled_height(0.0).max_scroll_height(maximum).striped(true).resizable(true)
                .column(Column::initial(225.0).at_least(180.0))
                .column(Column::initial(125.0).at_least(90.0))
                .column(Column::initial(70.0))
                .column(Column::initial(165.0))
                .column(Column::initial(145.0))
                .column(Column::initial(145.0))
                .column(Column::initial(55.0))
                .header(28.0,|mut row| {
                    for text in [crate::tr!("Name / project"),crate::tr!("Status"),"CPU",crate::tr!("RAM / limit"),crate::tr!("Network total"),crate::tr!("Disk total"),crate::tr!("Tasks")] {
                        row.col(|ui|{ui.strong(text);});
                    }
                }).body(|body|body.rows(65.0,snapshot.containers.len(),|mut row| {
                    let c=&snapshot.containers[row.index()];
                    row.set_selected(self.docker_selected.as_ref()==Some(&c.id));
                    row.col(|ui| {
                        ui.vertical(|ui| {
                            ui.strong(&c.name).on_hover_text(format!("Image: {}\nID: {}",c.image,c.id));
                            ui.label(RichText::new(if c.project.is_empty() {crate::tr!("Single container")} else {&c.project}).small());
                        });
                    });
                    row.col(|ui| {ui.label(match c.state.as_str() {"running"=>crate::tr!("Running (container)"),"exited"=>crate::tr!("Exited"),"paused"=>crate::tr!("Paused"),"created"=>crate::tr!("Created"),other=>other}).on_hover_text(&c.status);});
                    row.col(|ui| {ui.label(c.cpu.zip(cores).map(|(v,n)|format!("{} %",decimal(v/n as f64,1))).unwrap_or_else(||"—".into()))
                        .on_hover_text(crate::tr!("Docker reports 100 % per logical processor. Here it is normalized across all logical processors of the Docker host."));});
                    row.col(|ui| {ui.label(pair_text(c.memory)).on_hover_text(crate::tr!("Linux: Docker CLI subtracts inactive file cache. Without a fixed container limit, the limit may show all host RAM."));});
                    row.col(|ui| {ui.label(pair_text(c.network)).on_hover_text(crate::tr!("Received / sent, cumulative; not a transfer rate."));});
                    row.col(|ui| {ui.label(pair_text(c.disk)).on_hover_text(crate::tr!("Read / written, cumulative; not a transfer rate."));});
                    row.col(|ui| {ui.label(c.pids.map(|n|n.to_string()).unwrap_or_else(||"—".into())).on_hover_text(crate::tr!("Docker counts processes and threads together."));});
                    let response=row.response();
                    if response.clicked() {self.docker_selected=Some(c.id.clone());}
                    response.context_menu(|ui| {
                        ui.strong(&c.name);
                        for operation in [docker::Operation::Stop,docker::Operation::Restart] {
                            if ui.add_enabled(fresh && !self.docker_busy && !self.smoke && snapshot.target.is_some() && operation.enabled(&c.state),egui::Button::new(operation.label())).clicked() && let Some(target)=&snapshot.target {
                                self.docker_selected=Some(c.id.clone());
                                self.dialog=Some(Dialog::Docker(c.clone(),target.clone(),operation,snapshot.updated,snapshot.context.clone()));ui.close();
                            }
                        }
                    });
                }));
        });
    }

    fn start_scan(&mut self) {
        if let Some(snapshot) = &self.snapshot {
            let mut scan = optimizer::Scan::new(snapshot, self.uid, std::process::id());
            // Exercise selection/dialog layouts without enabling any signal action.
            if self.smoke
                && let Ok(mode) = std::env::var("TASK_MANAGER_SMOKE_OPTIMIZE")
                && matches!(mode.as_str(), "select" | "confirm" | "background")
            {
                if mode == "background" {
                    scan.category = Some(false);
                }
                scan.selecting = mode != "background";
                if mode != "background"
                    && let Some(candidate) = scan.candidates.first()
                {
                    scan.selected
                        .insert((candidate.process.pid, candidate.process.started));
                }
                if mode == "confirm" {
                    self.dialog = Some(Dialog::Optimize(scan.selected_rows()));
                }
            }
            self.scan = Some(scan);
        }
    }
    fn optimization_fresh(&self) -> bool {
        !self.paused
            && self.error.is_none()
            && self
                .snapshot_received
                .is_some_and(|t| t.elapsed() < Duration::from_secs(5))
            && self
                .scan
                .as_ref()
                .is_some_and(|s| s.created.elapsed() < Duration::from_secs(30))
    }

    fn optimize_page(&mut self, ui: &mut egui::Ui) {
        ui.style_mut().spacing.scroll.floating = false;
        ui.horizontal(|ui| {
            ui.heading(crate::tr!("Optimize system"));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let ready = !self.paused
                    && self
                        .snapshot_received
                        .is_some_and(|t| t.elapsed() < Duration::from_secs(5));
                if ui
                    .add_enabled(
                        ready,
                        egui::Button::new(
                            RichText::new(crate::tr!("Scan system")).color(Color32::WHITE),
                        )
                        .fill(BLUE)
                        .sense(Sense::click()),
                    )
                    .clicked()
                {
                    self.start_scan();
                }
            });
        });
        ui.label(crate::tr!("Linux cannot reliably tell which programs you still need. Check whether you need each candidate."));
        ui.label(RichText::new(crate::tr!("System services, containers and application helpers are excluded. A scan terminates nothing.")).small());
        if let Some(run) = &self.optimization {
            egui::CollapsingHeader::new(crate::tr!("Results of the last selection")).default_open(true).show(ui,|ui| {
                let latest=self.snapshot.as_ref();
                let gone=run.outcomes.iter().filter(|o|o.requested && latest.is_some_and(|s|!s.processes.iter().any(|p|p.pid==o.process.pid && p.started==o.process.started))).count();
                let requested=run.outcomes.iter().filter(|o|o.requested).count();
                ui.label(crate::tf!("{0} terminations requested · {1} processes no longer present" , requested, gone));
                if let Some(snapshot)=latest {
                    let change=snapshot.memory_available as i128-run.available_before as i128;
                    ui.label(crate::tf!("Available system RAM since action: {0}{1} (also affected by other activity)" , if change>=0 {"+"}else {"−"}, size(change.unsigned_abs() as f64)));
                }
                egui::ScrollArea::vertical().id_salt("optimization-results").max_height(95.0).show(ui,|ui| {
                    for o in &run.outcomes {
                        let gone=o.requested && latest.is_some_and(|s|!s.processes.iter().any(|p|p.pid==o.process.pid && p.started==o.process.started));
                        ui.label(format!("{} ({}): {}",o.process.name,o.process.pid,if gone {crate::tr!("no longer present")} else {&o.message}));
                    }
                });
            });
        }
        let fresh = self.optimization_fresh();
        let Some(scan) = &mut self.scan else {
            ui.add_space(25.0);
            ui.label(if self.paused {
                crate::tr!("Resume updates to scan.")
            } else {
                crate::tr!("Click “Scan system” to analyze possible savings.")
            });
            return;
        };
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui
                .radio_value(&mut scan.selecting, false, crate::tr!("Analysis only"))
                .changed()
            {
                scan.selected.clear();
            }
            ui.radio_value(&mut scan.selecting, true, crate::tr!("Select & end"));
            ui.label(
                RichText::new(crate::tf!(
                    "Scanned {0} s ago · {1} excluded",
                    scan.created.elapsed().as_secs(),
                    scan.excluded
                ))
                .small(),
            );
        });
        ui.horizontal_wrapped(|ui| {
            let applications = scan
                .candidates
                .iter()
                .filter(|c| c.process.application.is_some())
                .count();
            let old = scan.category;
            ui.selectable_value(
                &mut scan.category,
                None,
                crate::tf!("All ({0})", scan.candidates.len()),
            );
            ui.selectable_value(
                &mut scan.category,
                Some(true),
                crate::tf!("Applications ({0})", applications),
            );
            ui.selectable_value(
                &mut scan.category,
                Some(false),
                crate::tf!(
                    "Background processes ({0})",
                    scan.candidates.len() - applications
                ),
            );
            if scan.category != old {
                scan.selected.clear();
            }
        });
        let visible: Vec<_> = scan
            .candidates
            .iter()
            .enumerate()
            .filter_map(|(i, c)| {
                scan.category
                    .is_none_or(|kind| c.process.application.is_some() == kind)
                    .then_some(i)
            })
            .collect();
        let rows = scan.selected_rows();
        let estimate = if scan.selecting {
            optimizer::estimate(rows.iter())
        } else {
            optimizer::estimate(visible.iter().map(|i| &scan.candidates[*i].process))
        };
        metric_cards(
            ui,
            &[
                (
                    if scan.selecting {
                        crate::tr!("Selected")
                    } else {
                        crate::tr!("Candidates")
                    }
                    .into(),
                    if scan.selecting {
                        rows.len()
                    } else {
                        visible.len()
                    }
                    .to_string(),
                ),
                (
                    crate::tr!("CPU · Possible savings").into(),
                    format!("{} %", decimal(estimate.cpu, 1)),
                ),
                (
                    crate::tr!("RAM · RSS upper bound").into(),
                    size(estimate.memory as f64),
                ),
            ],
        );
        ui.label(RichText::new(crate::tr!("Snapshot, not guaranteed savings. Shared memory pages may be counted more than once.")).small());
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    scan.selecting && !rows.is_empty() && fresh && !self.smoke,
                    egui::Button::new(crate::tr!("End selection …")),
                )
                .clicked()
            {
                self.dialog = Some(Dialog::Optimize(rows));
            }
            if !fresh {
                ui.label(crate::tr!(
                    "Scan again: analysis is out of date or paused."
                ));
            } else if scan.selecting {
                ui.label(crate::tr!(
                    "Nothing is selected automatically. Confirmation is required before termination."
                ));
            }
        });
        ui.separator();
        if visible.is_empty() {
            ui.label(crate::tr!("No suitable candidates in this category. Services and detected helpers remain protected."));
            return;
        }
        egui::ScrollArea::horizontal()
            .id_salt("optimizer-horizontal")
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
            .show(ui, |ui| {
                let maximum = (ui.available_height() - 44.0).max(60.0);
                TableBuilder::new(ui)
                    .min_scrolled_height(0.0)
                    .max_scroll_height(maximum)
                    .striped(true)
                    .resizable(true)
                    .column(Column::exact(30.0))
                    .column(Column::initial(235.0).at_least(180.0))
                    .column(Column::initial(110.0))
                    .column(Column::initial(60.0))
                    .column(Column::initial(80.0))
                    .column(Column::initial(100.0))
                    .column(Column::remainder().at_least(275.0))
                    .header(28.0, |mut row| {
                        for text in [
                            "",
                            crate::tr!("Name"),
                            crate::tr!("Type"),
                            "PID",
                            "CPU",
                            "RAM (RSS)",
                            crate::tr!("Assessment"),
                        ] {
                            row.col(|ui| {
                                ui.strong(text);
                            });
                        }
                    })
                    .body(|body| {
                        body.rows(42.0, visible.len(), |mut row| {
                            let candidate = &scan.candidates[visible[row.index()]];
                            let process = &candidate.process;
                            let identity = (process.pid, process.started);
                            row.col(|ui| {
                                let mut selected = scan.selected.contains(&identity);
                                if ui
                                    .add_enabled(
                                        scan.selecting,
                                        egui::Checkbox::without_text(&mut selected),
                                    )
                                    .changed()
                                {
                                    if selected {
                                        scan.selected.insert(identity);
                                    } else {
                                        scan.selected.remove(&identity);
                                    }
                                }
                            });
                            row.col(|ui| {
                                ui.label(process.application.as_deref().unwrap_or(&process.name))
                                    .on_hover_text(&process.command);
                            });
                            row.col(|ui| {
                                ui.label(if process.application.is_some() {
                                    crate::tr!("Application")
                                } else {
                                    crate::tr!("Background")
                                });
                            });
                            row.col(|ui| {
                                ui.label(process.pid.to_string());
                            });
                            row.col(|ui| {
                                ui.label(format!("{} %", decimal(process.cpu, 1)));
                            });
                            row.col(|ui| {
                                ui.label(size(process.memory as f64));
                            });
                            row.col(|ui| {
                                ui.label(candidate.reason);
                            });
                        })
                    });
            });
    }

    fn series(&self, resource: &Resource) -> Vec<Vec<f64>> {
        let values = |index: usize| self.history.iter().map(|h| h.values[index]).collect();
        match resource {
            Resource::Cpu => vec![values(0)],
            Resource::Memory => vec![values(1)],
            Resource::Network => vec![values(4), values(5)],
            Resource::Disk(id) => (0..2)
                .map(|channel| {
                    self.history
                        .iter()
                        .map(|h| {
                            h.disks
                                .get(id)
                                .map(|r| if channel == 0 { r.0 } else { r.1 })
                                .unwrap_or(f64::NAN)
                        })
                        .collect()
                })
                .collect(),
            Resource::Gpu(id) => vec![
                self.history
                    .iter()
                    .map(|h| h.gpus.get(id).copied().flatten().unwrap_or(f64::NAN))
                    .collect(),
            ],
        }
    }

    fn performance(&mut self, ui: &mut egui::Ui) {
        let Some(snapshot) = self.snapshot.clone() else {
            ui.label(crate::tr!("Reading system …"));
            return;
        };
        match &self.resource {
            Resource::Disk(id) if !snapshot.disks.iter().any(|d| &d.id == id) => {
                self.resource = snapshot
                    .disks
                    .first()
                    .map(|d| Resource::Disk(d.id.clone()))
                    .unwrap_or(Resource::Cpu);
            }
            Resource::Gpu(id) if !snapshot.gpus.iter().any(|g| &g.id == id) => {
                self.resource = snapshot
                    .gpus
                    .first()
                    .map(|g| Resource::Gpu(g.id.clone()))
                    .unwrap_or(Resource::Cpu);
            }
            _ => {}
        }
        let mut resources = vec![
            (
                Resource::Cpu,
                "CPU".to_owned(),
                format!("{} %", decimal(snapshot.cpu, 0)),
                COLORS[0],
            ),
            (
                Resource::Memory,
                crate::tr!("Memory").into(),
                format!("{} %", decimal(snapshot.memory_percent(), 0)),
                COLORS[1],
            ),
        ];
        for (i, d) in snapshot.disks.iter().enumerate() {
            resources.push((
                Resource::Disk(d.id.clone()),
                crate::tf!("Disk {0} · {1}", i, d.id),
                format!("{}/s", size(d.read + d.write)),
                COLORS[2],
            ));
        }
        resources.push((
            Resource::Network,
            crate::tr!("Network").into(),
            format!("{}/s", size(snapshot.network_recv + snapshot.network_sent)),
            COLORS[3],
        ));
        for (i, g) in snapshot.gpus.iter().enumerate() {
            resources.push((
                Resource::Gpu(g.id.clone()),
                format!("GPU {i}"),
                optional_percent(g.usage),
                COLORS[0],
            ));
        }
        ui.add_space(10.0);
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(Vec2::new(228.0, ui.available_height()), Layout::top_down(Align::Min), |ui| {
                egui::ScrollArea::vertical().id_salt("hardware-list").show(ui, |ui| {
                    for (resource, title, metric, color) in &resources {
                        let series = self.series(resource);
                        let max = if matches!(resource,Resource::Cpu|Resource::Memory|Resource::Gpu(_)) {100.0}
                            else {series.iter().flatten().copied().filter(|v|v.is_finite()).fold(1048576.0,f64::max)};
                        let card = ResourceCard {title, metric, color:*color, maximum:max, series:&series};
                        if resource_card(ui, resource, &card, self.resource==*resource).clicked() {self.resource=resource.clone();}
                    }
                    if snapshot.gpus.is_empty() {ui.label(crate::tr!("No GPU detected"));}
                });
            });
            ui.separator();
            ui.vertical(|ui| {
                let detail_height=ui.available_height();
                egui::ScrollArea::vertical().id_salt("hardware-details").auto_shrink([false,false]).show(ui, |ui| {
                    let content_top=ui.cursor().top();
                    let resource = self.resource.clone();
                    let (title, color) = resources.iter().find(|r|r.0==resource).map(|r|(r.1.clone(),r.3)).unwrap_or(("CPU".into(),COLORS[0]));
                    ui.label(RichText::new(title).size(28.0));
                    let subtitle = match &resource {
                        Resource::Cpu => self.cpu_name.clone(),
                        Resource::Memory => crate::tr!("Physical memory").into(),
                        Resource::Network => crate::tr!("All network interfaces except loopback").into(),
                        Resource::Disk(id) => snapshot.disks.iter().find(|d|&d.id==id).map(|d|format!("{} · {}",d.model,d.kind)).unwrap_or_default(),
                        Resource::Gpu(id) => snapshot.gpus.iter().find(|g|&g.id==id).map(|g|g.name.clone()).unwrap_or_default(),
                    };
                    ui.label(RichText::new(subtitle).color(ui.visuals().weak_text_color()));
                    ui.add_space(10.0);
                    let series=self.series(&resource);
                    let percent=matches!(resource,Resource::Cpu|Resource::Memory|Resource::Gpu(_));
                    let maximum=if percent {100.0} else {series.iter().flatten().copied().filter(|v|v.is_finite()).fold(1048576.0,f64::max)};
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(if resource==Resource::Cpu {crate::tr!("Total usage")} else {crate::tr!("History")}).strong());
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(RichText::new(if percent {"100 %".into()} else {format!("{}/s",size(maximum))}).small().color(ui.visuals().weak_text_color()));
                        });
                    });
                    let height=if resource==Resource::Cpu && self.per_core {if detail_height<600.0 {85.0} else {120.0}} else {190.0};
                    let (rect,_)=ui.allocate_exact_size(Vec2::new(ui.available_width(),height),Sense::hover());
                    paint_chart(ui.painter(),rect,&series,color,maximum,true);
                    chart_legend(ui,&resource,color);
                    ui.label(RichText::new(crate::tr!("Grey lines: scale grid")).small().color(ui.visuals().weak_text_color()));
                    ui.label(RichText::new(crate::tr!("Last 60 samples · Now at the right")).small().color(ui.visuals().weak_text_color()));
                    ui.add_space(10.0);
                    let mut values=Vec::new();
                    let mut temperatures=Vec::new();
                    match &resource {
                        Resource::Cpu => {
                            values.extend([
                                (crate::tr!("Usage").into(),format!("{} %",decimal(snapshot.cpu,1))),
                                (crate::tr!("CPU clock (avg.)").into(),snapshot.cpu_frequency.as_ref().map(|f|format!("{} GHz",decimal(f.mhz/1000.0,2))).unwrap_or_else(||crate::tr!("unavailable").into())),
                                (crate::tr!("Processes").into(),snapshot.processes.len().to_string()),
                                (crate::tr!("Threads").into(),snapshot.thread_count().to_string()),
                                (crate::tr!("Physical cores").into(),snapshot.physical_cores.map(|n|n.to_string()).unwrap_or_else(||crate::tr!("unavailable").into())),
                                (crate::tr!("Logical processors").into(),snapshot.cores.len().to_string()),
                                (crate::tr!("System uptime").into(),uptime(snapshot.uptime)),
                            ]);
                            temperatures=snapshot.cpu_temperatures.clone();
                        }
                        Resource::Memory => {
                            values.extend([
                                (crate::tr!("In use").into(),size(snapshot.memory_used as f64)),
                                (crate::tr!("Available").into(),size(snapshot.memory_available as f64)),
                                (crate::tr!("Total").into(),size(snapshot.memory_total as f64)),
                                (crate::tr!("Cache").into(),snapshot.memory_cache.map(|n|size(n as f64)).unwrap_or_else(||crate::tr!("unavailable").into())),
                                (crate::tr!("Swap in use").into(),size(snapshot.swap_used as f64)),
                                (crate::tr!("Swap total").into(),size(snapshot.swap_total as f64)),
                            ]);
                            temperatures=snapshot.temperatures.iter().filter(|t|t.chip=="jc42" || t.label.to_lowercase().contains("dimm")).cloned().collect();
                        }
                        Resource::Network => {
                            values.extend([(crate::tr!("Receive").into(),format!("{}/s",size(snapshot.network_recv))), (crate::tr!("Send").into(),format!("{}/s",size(snapshot.network_sent)))]);
                        }
                        Resource::Disk(id) => if let Some(d)=snapshot.disks.iter().find(|d|&d.id==id) {
                            values.extend([(crate::tr!("Read").into(),format!("{}/s",size(d.read))), (crate::tr!("Write").into(),format!("{}/s",size(d.write))), (crate::tr!("Capacity (drive)").into(),size(d.capacity as f64))]);
                            let total=|f:fn(&crate::storage::Volume)->u64|if d.volumes.is_empty() {crate::tr!("unavailable").into()}else {size(d.volumes.iter().fold(0u64,|sum,v|sum.saturating_add(f(v))) as f64)};
                            values.extend([(crate::tr!("Used (mounted)").into(),total(|v|v.used)),(crate::tr!("Available (mounted)").into(),total(|v|v.available)),(crate::tr!("File system size").into(),total(|v|v.total))]);
                            temperatures=d.temperatures.clone();
                        },
                        Resource::Gpu(id) => if let Some(g)=snapshot.gpus.iter().find(|g|&g.id==id) {
                            values.extend([
                                (crate::tr!("GPU usage").into(),optional_percent(g.usage)),
                                (crate::tr!("GPU clock").into(),g.clock_mhz.map(|v|format!("{} MHz",decimal(v,0))).unwrap_or_else(||crate::tr!("unavailable").into())),
                                (crate::tr!("Power draw").into(),g.power_watts.map(|v|format!("{} W",decimal(v,1))).unwrap_or_else(||crate::tr!("unavailable").into())),
                                (crate::tr!("Graphics memory in use").into(),g.memory_used.map(|v|size(v as f64)).unwrap_or_else(||crate::tr!("unavailable").into())),
                                (crate::tr!("Graphics memory total").into(),g.memory_total.map(|v|size(v as f64)).unwrap_or_else(||crate::tr!("unavailable").into())),
                            ]);
                            temperatures=g.temperatures.clone();
                        },
                    }
                    let cards=metric_cards(ui, &values);
                    if resource==Resource::Cpu {
                        if let Some(frequency)=&snapshot.cpu_frequency {
                            cards[1].clone().on_hover_text(crate::tf!("Average across {0} of {1} active logical processors. Source: {2}. Core speeds may differ." , frequency.measured_cpus, snapshot.cores.len(), frequency.source));
                        }
                        cards[2].clone().on_hover_text(crate::tr!("All readable processes in this sample, regardless of search and user filters. Vanished or unreadable processes may be missing."));
                        cards[3].clone().on_hover_text(crate::tr!("Sum of software threads across all detected processes. Logical processors are the CPU's hardware execution units."));
                        cards[4].clone().on_hover_text(crate::tr!("Physical cores with at least one active logical processor. SMT threads of the same core are counted only once."));
                    } else if resource==Resource::Memory {
                        cards[3].clone().on_hover_text(crate::tr!("File/page cache including tmpfs and shared memory (Cached), plus reclaimable kernel cache (SReclaimable). Do not add to total RAM; partly already included in Available."));
                    } else if let Resource::Gpu(id)=&resource && let Some(gpu)=snapshot.gpus.iter().find(|g|&g.id==id) {
                        cards[1].clone().on_hover_text(crate::tr!("Current GPU graphics/shader clock in MHz, reported by the driver."));
                        cards[2].clone().on_hover_text(crate::tf!("Source: {0}. AMD reports GPU/SoC power; on APUs this may include the CPU. NVIDIA reports board power. Scope and averaging depend on the driver." , gpu.power_source));
                    }
                    if let Resource::Disk(id)=&resource && let Some(d)=snapshot.disks.iter().find(|d|&d.id==id) {
                        ui.add_space(10.0);
                        ui.label(RichText::new(crate::tr!("Space on mounted file systems")).strong());
                        if d.volumes.is_empty() {ui.label(crate::tr!("No readable mounted file system. Usage is unavailable."));}
                        for volume in &d.volumes {
                            let path=volume.mounts.first().map(String::as_str).unwrap_or("—");
                            ui.label(format!("{path} · {} · {}{}",volume.kind,volume.source,if volume.shared {crate::tr!(" · shared storage pool")}else {""}))
                                .on_hover_text(crate::tf!("Mounts: {0}\nEach file system is counted once per drive. For a shared pool, values apply to the entire pool." , volume.mounts.join(", ")));
                            ui.add(egui::ProgressBar::new(volume.used as f32/volume.total.max(1) as f32).fill(color)
                                .text(crate::tf!("{0} of {1} used · {2} available" , size(volume.used as f64), size(volume.total as f64), size(volume.available as f64))));
                        }
                        ui.label(RichText::new(crate::tr!("File system values; unmounted partitions and unallocated space are not assumed to be free.")).small());
                    }
                    if resource!=Resource::Network {
                        ui.add_space(12.0);
                        ui.label(RichText::new(crate::tr!("Temperatures")).strong());
                        temperature_cards(ui, &temperatures);
                    }
                    match &resource {
                        Resource::Cpu => {
                            ui.add_space(12.0);
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(crate::tr!("Individual logical processors")).strong());
                                ui.with_layout(Layout::right_to_left(Align::Center),|ui| {ui.checkbox(&mut self.per_core,crate::tr!("Show cores"));});
                            });
                            if self.per_core {
                                legend_items(ui,&[(crate::tr!("Usage of each logical processor"),color)]);
                                let height=(detail_height-(ui.cursor().top()-content_top)-8.0).clamp(125.0,400.0);
                                egui::ScrollArea::vertical().id_salt("cpu-cores").max_height(height).show(ui, |ui| {
                                    let available=(ui.available_width()-2.0).max(140.0);
                                    let columns=(available/185.0).floor().max(1.0) as usize;
                                    let width=((available-12.0*(columns-1) as f32)/columns as f32).max(140.0);
                                    egui::Grid::new("cpu-core-charts").spacing(Vec2::new(12.0,12.0)).show(ui, |ui| {
                                        for (i,value) in snapshot.cores.iter().enumerate() {
                                            ui.vertical(|ui| {
                                                ui.set_width(width);
                                                let id=snapshot.core_ids[i];
                                                ui.horizontal(|ui| {
                                                    ui.label(RichText::new(format!("CPU {id}")).strong());
                                                    ui.with_layout(Layout::right_to_left(Align::Center),|ui|{ui.label(format!("{} %",decimal(*value,1)));});
                                                });
                                                let points=self.history.iter().map(|h|h.cores.get(&id).copied().unwrap_or(f64::NAN)).collect();
                                                let (rect,_)=ui.allocate_exact_size(Vec2::new(width,62.0),Sense::hover());
                                                paint_chart(ui.painter(),rect,&[points],color,100.0,true);
                                                if let Some(t)=snapshot.core_temperatures[i] {ui.label(RichText::new(format!("{} °C",decimal(t,1))).small());}
                                            });
                                            if (i+1)%columns==0 {ui.end_row();}
                                        }
                                    });
                                });
                            }
                        }
                        Resource::Gpu(id) => if let Some(g)=snapshot.gpus.iter().find(|g|&g.id==id) {
                            ui.add_space(12.0);
                            ui.label(RichText::new(crate::tf!("Driver: {0} · PCI {1}" , g.driver, g.id)).small().color(ui.visuals().weak_text_color()));
                            if g.usage.is_none() {ui.label(crate::tr!("The driver does not provide readable GPU usage."));}
                        },
                        Resource::Disk(_) => {}
                        Resource::Network => {ui.add_space(10.0);ui.label(crate::tr!("VPNs and bridges may count traffic more than once. Further sensors are listed under System."));}
                        Resource::Memory => {}
                    }
                });
            });
        });
    }

    fn system_page(&mut self, ui: &mut egui::Ui) {
        ui.add_space(20.0);
        ui.label(RichText::new(crate::tr!("Your system")).size(30.0));
        ui.label(
            RichText::new(crate::tr!("Hardware and Linux at a glance"))
                .color(ui.visuals().weak_text_color()),
        );
        ui.add_space(25.0);
        let memory = self
            .snapshot
            .as_ref()
            .map(|s| size(s.memory_total as f64))
            .unwrap_or_else(|| "—".to_owned());
        let cores = self
            .snapshot
            .as_ref()
            .map(|s| s.cores.len().to_string())
            .unwrap_or_else(|| "—".to_owned());
        let physical_cores = self
            .snapshot
            .as_ref()
            .and_then(|s| s.physical_cores)
            .map(|n| n.to_string())
            .unwrap_or_else(|| crate::tr!("unavailable").into());
        egui::Grid::new("system-grid")
            .spacing(Vec2::new(45.0, 18.0))
            .show(ui, |ui| {
                for (key, value) in [
                    (crate::tr!("Operating system"), self.distro.clone()),
                    (
                        crate::tr!("Kernel"),
                        monitor::text_file("/proc/sys/kernel/osrelease"),
                    ),
                    (
                        crate::tr!("Architecture"),
                        std::env::consts::ARCH.to_owned(),
                    ),
                    (crate::tr!("Processor"), self.cpu_name.clone()),
                    (crate::tr!("Physical cores"), physical_cores),
                    (crate::tr!("Logical processors"), cores),
                    (crate::tr!("Memory"), memory),
                    (
                        crate::tr!("Computer name"),
                        monitor::text_file("/proc/sys/kernel/hostname"),
                    ),
                    (
                        crate::tr!("App version"),
                        format!("{} · Rust · egui / eframe", env!("CARGO_PKG_VERSION")),
                    ),
                ] {
                    ui.label(RichText::new(key).color(ui.visuals().weak_text_color()));
                    ui.label(value);
                    ui.end_row();
                }
            });
        ui.add_space(30.0);
        if let Some(s) = &self.snapshot {
            ui.label(crate::tf!(
                "System uptime: {0}    ·    {1} readable processes",
                uptime(s.uptime),
                s.processes.len()
            ));
        }
        if let Some(s) = &self.snapshot {
            ui.add_space(15.0);
            ui.heading(crate::tr!("Graphics devices and disks"));
            for g in &s.gpus {
                ui.label(format!(
                    "{} · {} · {}",
                    g.name,
                    g.driver,
                    temperature_text(&g.temperatures)
                ));
            }
            for d in &s.disks {
                ui.label(format!(
                    "{} · {} · {} · {}",
                    d.id,
                    d.model,
                    size(d.capacity as f64),
                    temperature_text(&d.temperatures)
                ));
            }
            ui.add_space(15.0);
            ui.heading(crate::tr!("Temperature sensors"));
            if s.temperatures.is_empty() {
                ui.label(crate::tr!("No readable temperature sensors available."));
            }
            for t in &s.temperatures {
                ui.label(format!(
                    "{} · {}: {} °C",
                    t.chip,
                    t.label,
                    decimal(t.celsius, 1)
                ));
            }
        }
        ui.add_space(15.0);
        if ui.button(crate::tr!("Install in app menu …")).clicked() {
            self.dialog = Some(Dialog::Install);
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.dialog.take() else {
            return;
        };
        let mut keep = true;
        let mut next = None;
        egui::Modal::new(Id::new("task-dialog")).show(ctx, |ui| {
            ui.set_width(440.0);
            match &dialog {
                Dialog::End(process, force) => {
                    ui.heading(if *force { crate::tr!("Force quit") } else { crate::tr!("End task") });
                    ui.add_space(10.0);
                    ui.label(crate::tf!("End “{0}” (PID {1})?" , process.name, process.pid));
                    ui.label(crate::tr!("Unsaved data may be lost."));
                    ui.label(RichText::new(if *force { crate::tr!("The process has no opportunity to save data (SIGKILL).") }
                        else { crate::tr!("The process is asked to exit (SIGTERM).") }).color(ui.visuals().weak_text_color()));
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.button(crate::tr!("Cancel")).clicked() { keep = false; }
                        if ui.add_enabled(!self.smoke,egui::Button::new(crate::tr!("Yes, end task"))).clicked() {
                            keep = false;
                            match monitor::end_process(process.pid, process.started, *force) {
                                Ok(()) => self.refresh(),
                                Err(e) => next = Some(Dialog::Message(crate::tr!("Unable to end task").to_owned(),
                                    if e.kind() == std::io::ErrorKind::PermissionDenied { crate::tr!("You are not allowed to end this process. The app runs with your user's permissions.").to_owned() }
                                    else { e.to_string() })),
                            }
                        }
                    });
                }
                Dialog::EndGroup(rows,force) => {
                    ui.heading(if *force {crate::tr!("Force quit process group?")}else {crate::tr!("End process group?")});
                    ui.label(crate::tf!("End only these {0} displayed processes?" , rows.len()));
                    ui.label(crate::tr!("Unsaved data may be lost."));
                    ui.label(if *force {crate::tr!("All listed processes receive SIGKILL with no opportunity to save.")}else {crate::tr!("All listed processes receive a termination request (SIGTERM).")});
                    egui::ScrollArea::vertical().max_height(170.0).show(ui,|ui| {for row in rows {ui.label(format!("{} (PID {})",row.name,row.pid));}});
                    ui.horizontal(|ui| {
                        if ui.button(crate::tr!("Cancel")).clicked() {keep=false;}
                        if ui.add_enabled(!self.smoke && !rows.is_empty(),egui::Button::new(crate::tr!("End group"))).clicked() {
                            keep=false;
                            let results=rows.iter().map(|p|format!("{} ({}): {}",p.name,p.pid,match monitor::end_process(p.pid,p.started,*force) {Ok(())=>crate::tr!("Termination requested").into(),Err(e)=>e.to_string()})).collect::<Vec<_>>();
                            next=Some(Dialog::Message(crate::tr!("Group action").into(),results.join("\n")));
                            self.refresh();
                        }
                    });
                }
                Dialog::Docker(container,target,operation,updated,context) => {
                    ui.heading(format!("Container {}?",operation.label().to_lowercase()));
                    ui.label(crate::tf!("{0} · Docker target: {1}" , container.name, context));
                    ui.label(RichText::new(format!("ID: {}",container.id)).small());
                    ui.label(crate::tr!("Docker asks the program to exit. After 10 seconds, Docker may force it to quit. Unsaved data may be lost."));
                    if *operation==docker::Operation::Restart {ui.label(crate::tr!("Docker then restarts the same container."));}
                    let fresh=!self.paused && updated.elapsed()<Duration::from_secs(30);
                    if !fresh {ui.label(crate::tr!("The selection is out of date or paused. Please cancel and refresh."));}
                    ui.horizontal(|ui| {
                        if ui.button(crate::tr!("Cancel")).clicked() {keep=false;}
                        if ui.add_enabled(fresh && !self.docker_busy && !self.smoke,egui::Button::new(crate::tf!("Yes, {0}" , operation.label().to_lowercase()))).clicked() {
                            keep=false;
                            let action=docker::Action {target:target.clone(),container:container.clone(),operation:*operation,queued:Instant::now()};
                            if self.docker_monitor.control.send(docker::Control::Action(Box::new(action))).is_ok() {self.docker_busy=true;}
                            else {next=Some(Dialog::Message(crate::tr!("Docker action failed").into(),crate::tr!("The Docker worker is unavailable.").into()));}
                        }
                    });
                }
                Dialog::Optimize(rows) => {
                    ui.heading(crate::tr!("End selected processes?"));
                    ui.label(crate::tr!("Unsaved data may be lost. The selected programs receive SIGTERM."));
                    let potential = optimizer::estimate(rows.iter());
                    ui.label(crate::tf!("{0} processes · CPU {1} % · RAM (RSS, upper bound) {2}" , rows.len(), decimal(potential.cpu,1), size(potential.memory as f64)));
                    egui::ScrollArea::vertical().max_height(180.0).show(ui,|ui| {
                        for row in rows { ui.label(format!("{} (PID {})",row.application.as_deref().unwrap_or(&row.name),row.pid)); }
                    });
                    let fresh = self.optimization_fresh();
                    if !fresh {ui.label(crate::tr!("The analysis is out of date or paused. Please scan again."));}
                    ui.horizontal(|ui| {
                        if ui.button(crate::tr!("Cancel")).clicked() { keep=false; }
                        if ui.add_enabled(fresh && !self.smoke,egui::Button::new(crate::tr!("End selection"))).clicked() {
                            keep=false;
                            if let Some(snapshot)=&self.snapshot {
                                self.optimization=Some(optimizer::execute_with(rows,snapshot,self.uid,std::process::id(),|row|
                                    optimizer::end_current(row,self.uid,std::process::id())));
                            }
                            if let Some(scan)=&mut self.scan {scan.selected.clear();scan.selecting=false;}
                            self.refresh();
                        }
                    });
                }
                Dialog::Install | Dialog::Uninstall => {
                    let installing = matches!(dialog, Dialog::Install);
                    ui.heading(if installing { crate::tr!("Install Task-Manager") } else { crate::tr!("Uninstall Task-Manager") });
                    ui.add_space(10.0);
                    match InstallPaths::for_user() {
                        Ok(paths) => {
                            if installing {
                                ui.label(crate::tr!("The app will be installed for your user and added to the app menu. No administrator permissions required."));
                                ui.label(crate::tf!("Location: {0}" , paths.directory().display()));
                                ui.label(crate::tr!("Terminal launcher: ~/.local/bin/task-manager"));
                            } else { ui.label(crate::tr!("Remove app menu entry, terminal launcher and installed program files?")); }
                            ui.add_space(12.0);
                            ui.horizontal(|ui| {
                                if ui.button(crate::tr!("Cancel")).clicked() { keep = false; }
                                if ui.button(if installing { crate::tr!("Install") } else { crate::tr!("Uninstall") }).clicked() {
                                    keep = false;
                                    let result = if installing { install::install(std::path::Path::new("/proc/self/exe"), &paths).map(|_| ()) }
                                        else { install::uninstall(&paths) };
                                    next = Some(match result {
                                        Ok(()) => Dialog::Message(if installing { crate::tr!("Installation complete") } else { crate::tr!("Uninstallation complete") }.to_owned(),
                                            if installing { crate::tr!("You can now start Task-Manager from your app menu.\n\nIn the terminal: ~/.local/bin/task-manager\nIf ~/.local/bin is in PATH: task-manager\n\nYou can now delete the extracted download package.") }
                                            else { crate::tr!("The installed app has been removed.") }.to_owned()),
                                        Err(error) => Dialog::Message(crate::tr!("Operation failed").to_owned(), error.to_string()),
                                    });
                                }
                            });
                        }
                        Err(error) => { ui.label(error.to_string()); if ui.button(crate::tr!("Close")).clicked() { keep = false; } }
                    }
                }
                Dialog::NewTask => {
                    ui.heading(crate::tr!("Run new task"));
                    ui.label(crate::tr!("Program and arguments:"));
                    ui.add(egui::TextEdit::singleline(&mut self.new_command).desired_width(f32::INFINITY));
                    ui.horizontal(|ui| {
                        if ui.button(crate::tr!("Cancel")).clicked() { keep = false; }
                        if ui.button(crate::tr!("Run")).clicked() {
                            match spawn_task(&self.new_command) {
                                Ok(()) => { keep = false; self.new_command.clear(); self.refresh(); }
                                Err(error) => { keep = false; next = Some(Dialog::Message(crate::tr!("Launch failed").to_owned(), error)); }
                            }
                        }
                    });
                }
                Dialog::About => {
                    ui.heading(crate::tf!("Task-Manager {0}" , env!("CARGO_PKG_VERSION")));
                    ui.label(crate::tr!("Native Linux Task-Manager in the style of Windows 10."));
                    ui.label(crate::tr!("Rust · egui / eframe · Linux /proc\nApp code license: MIT"));
                    ui.add_space(10.0);
                    ui.label(crate::tr!("Process CPU usage is normalized across all logical cores. RSS may count shared pages more than once. Unreadable I/O values appear as —."));
                    if ui.button(crate::tr!("Close")).clicked() { keep = false; }
                }
                Dialog::Message(title, text) => { ui.heading(title); ui.label(text); if ui.button("OK").clicked() { keep = false; } }
            }
        });
        self.dialog = next.or(if keep { Some(dialog) } else { None });
    }

    fn screenshot_events(&mut self, ctx: &egui::Context) {
        let images: Vec<_> = ctx.input(|input| {
            input
                .events
                .iter()
                .filter_map(|event| {
                    if let egui::Event::Screenshot { image, .. } = event {
                        Some(image.clone())
                    } else {
                        None
                    }
                })
                .collect()
        });
        for image in images {
            if let Some(path) = &self.screenshot {
                match write_screenshot(path, &image) {
                    Ok(()) => self.screenshot_received = true,
                    Err(error) => {
                        eprintln!("{}", crate::tf!("Screenshot failed: {0}", error));
                        self.error = Some(error);
                    }
                }
            }
        }
    }
}

impl eframe::App for TaskApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.receive();
        self.screenshot_events(ctx);
        if self.smoke && self.samples >= 2 && !self.smoke_layout_ready {
            if self.tab == Tab::Processes
                && let Ok(mode) = std::env::var("TASK_MANAGER_SMOKE_GROUPS")
                && let Some(snapshot) = &self.snapshot
            {
                let groups = processes::groups(
                    &(0..snapshot.processes.len()).collect::<Vec<_>>(),
                    &snapshot.processes,
                );
                for group in groups.iter().filter(|g| g.indices.len() > 1) {
                    self.opened_processes.insert(group.key.clone());
                }
                if mode == "confirm"
                    && let Some(group) = groups.iter().find(|g| g.indices.len() > 1)
                {
                    self.dialog = Some(Dialog::EndGroup(
                        group
                            .indices
                            .iter()
                            .map(|i| snapshot.processes[*i].clone())
                            .collect(),
                        false,
                    ));
                }
                self.smoke_layout_ready = true;
            } else if self.tab == Tab::Docker {
                if let Some(snapshot) = &self.docker_snapshot
                    && let Some(c) = snapshot.containers.first()
                {
                    self.docker_selected = Some(c.id.clone());
                    if std::env::var("TASK_MANAGER_SMOKE_DOCKER").is_ok_and(|s| s == "confirm")
                        && let Some(target) = &snapshot.target
                    {
                        self.dialog = Some(Dialog::Docker(
                            c.clone(),
                            target.clone(),
                            docker::Operation::Restart,
                            snapshot.updated,
                            snapshot.context.clone(),
                        ));
                    }
                    self.smoke_layout_ready = true;
                }
            } else {
                self.smoke_layout_ready = true;
            }
        }
        ctx.request_repaint_after(Duration::from_millis(250));
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F5)) {
            self.refresh();
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::N)) {
            self.dialog = Some(Dialog::NewTask);
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Q))
            && !self.packages.active
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.menus(ctx);
        let docker_active = self.tab == Tab::Docker && !self.paused;
        if docker_active != self.docker_active {
            let _ = self
                .docker_monitor
                .control
                .send(docker::Control::Pause(!docker_active));
            self.docker_active = docker_active;
        }
        if self.smoke && self.tab == Tab::Optimize && self.scan.is_none() && self.samples >= 2 {
            self.start_scan();
        }
        self.status_bar(ctx);
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(ctx.style().visuals.panel_fill)
                    .inner_margin(egui::Margin::symmetric(16, 8)),
            )
            .show(ctx, |ui| {
                if let Some(error) = &self.error {
                    ui.colored_label(
                        Color32::DARK_RED,
                        crate::tf!("Measurement failed: {0}", error),
                    );
                }
                match self.tab {
                    Tab::Processes => self.process_page(ui, false),
                    Tab::Details => self.process_page(ui, true),
                    Tab::Performance => self.performance(ui),
                    Tab::Docker => self.docker_page(ui),
                    Tab::Optimize => self.optimize_page(ui),
                    Tab::Installed => self.installed.show(ui, &mut self.packages),
                    Tab::Drivers => self.drivers.show(ui, &mut self.packages),
                    Tab::System => {
                        egui::ScrollArea::vertical().show(ui, |ui| self.system_page(ui));
                    }
                }
            });
        self.packages.show(ctx);
        if self.packages.active && ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        self.dialogs(ctx);
        if self.smoke && self.smoke_started.elapsed() > Duration::from_secs(3) {
            let valid = self.samples >= 2
                && self
                    .snapshot
                    .as_ref()
                    .is_some_and(|s| !s.processes.is_empty() && s.memory_total > 0);
            if !valid || self.smoke_started.elapsed() > Duration::from_secs(22) {
                eprintln!(
                    "{}",
                    crate::tr!("GUI test failed: no valid samples or screenshot timeout")
                );
                std::process::exit(1);
            }
            if self.packages.busy() {
                return;
            }
            if self.tab == Tab::Drivers && !self.drivers.ready() {
                return;
            }
            if self.tab == Tab::Installed && !self.installed.ready() {
                return;
            }
            if self.tab == Tab::Docker && self.docker_snapshot.is_none() {
                return;
            }
            if self.screenshot.is_some() && !self.screenshot_requested {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                self.screenshot_requested = true;
            } else if (self.screenshot.is_none() || self.screenshot_received) && !self.smoke_success
            {
                self.smoke_success = true;
                println!(
                    "{}",
                    crate::tf!(
                        "GUI OK: {0} samples, {1} real processes",
                        self.samples,
                        self.snapshot
                            .as_ref()
                            .map(|s| s.processes.len())
                            .unwrap_or(0)
                    )
                );
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}

struct ResourceCard<'a> {
    title: &'a str,
    metric: &'a str,
    color: Color32,
    maximum: f64,
    series: &'a [Vec<f64>],
}
fn resource_card(
    ui: &mut egui::Ui,
    resource: &Resource,
    card: &ResourceCard<'_>,
    selected: bool,
) -> egui::Response {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(218.0, 100.0), Sense::hover());
    let response = ui
        .interact(rect, Id::new(("resource-card", resource)), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let background = if selected {
        ui.visuals().selection.bg_fill
    } else if response.hovered() {
        ui.visuals().widgets.hovered.weak_bg_fill
    } else {
        ui.visuals().panel_fill
    };
    ui.painter().rect_filled(rect, 5.0, background);
    if selected {
        ui.painter().rect_filled(
            Rect::from_min_size(rect.min, Vec2::new(3.0, rect.height())),
            2.0,
            card.color,
        );
    }
    ui.painter().text(
        rect.min + Vec2::new(12.0, 10.0),
        egui::Align2::LEFT_TOP,
        card.title,
        FontId::proportional(14.0),
        ui.visuals().text_color(),
    );
    ui.painter().text(
        rect.min + Vec2::new(12.0, 32.0),
        egui::Align2::LEFT_TOP,
        card.metric,
        FontId::proportional(12.0),
        ui.visuals().weak_text_color(),
    );
    let chart = Rect::from_min_size(
        rect.min + Vec2::new(12.0, 53.0),
        Vec2::new(rect.width() - 24.0, 23.0),
    );
    paint_chart(
        ui.painter(),
        chart,
        card.series,
        card.color,
        card.maximum,
        false,
    );
    let labels = chart_labels(resource);
    let mut x = rect.left() + 12.0;
    for (i, label) in labels.iter().enumerate() {
        let tint = line_color(i, labels.len(), card.color);
        ui.painter().line_segment(
            [
                Pos2::new(x, rect.bottom() - 10.0),
                Pos2::new(x + 13.0, rect.bottom() - 10.0),
            ],
            Stroke::new(2.0_f32, tint),
        );
        ui.painter().text(
            Pos2::new(x + 18.0, rect.bottom() - 10.0),
            egui::Align2::LEFT_CENTER,
            *label,
            FontId::proportional(10.0),
            ui.visuals().text_color(),
        );
        x += 100.0;
    }
    response
}
pub(crate) fn metric_cards(ui: &mut egui::Ui, values: &[(String, String)]) -> Vec<egui::Response> {
    let mut responses = Vec::with_capacity(values.len());
    if values.is_empty() {
        return responses;
    }
    let max_columns = ((ui.available_width() + 10.0) / 190.0)
        .floor()
        .clamp(1.0, 4.0) as usize;
    let columns = values.len().div_ceil(values.len().div_ceil(max_columns));
    let width = ((ui.available_width() - 10.0 * (columns - 1) as f32) / columns as f32).max(100.0);
    // Explicit column widths prevent long sensor values from squeezing their neighbours.
    for row in values.chunks(columns) {
        ui.horizontal_top(|ui| {
            for (label, value) in row {
                let response = egui::Frame::new()
                    .fill(if ui.visuals().dark_mode {
                        Color32::from_rgb(34, 40, 47)
                    } else {
                        Color32::from_rgb(243, 247, 250)
                    })
                    .corner_radius(6.0)
                    .inner_margin(10.0)
                    .show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.set_width((width - 20.0).max(60.0));
                            ui.spacing_mut().item_spacing.y = 4.0;
                            ui.label(
                                RichText::new(label)
                                    .size(12.0)
                                    .color(ui.visuals().weak_text_color()),
                            );
                            ui.label(RichText::new(value).size(20.0));
                        });
                    })
                    .response;
                responses.push(response);
            }
        });
    }
    responses
}
fn temperature_cards(ui: &mut egui::Ui, values: &[crate::hardware::Temperature]) {
    if values.is_empty() {
        ui.label(
            RichText::new(crate::tr!("No readable temperature sensors available"))
                .small()
                .color(ui.visuals().weak_text_color()),
        );
        return;
    }
    let cards = values
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let label = match t.label.as_str() {
                "Tctl" => crate::tr!("CPU package (Tctl)").into(),
                "Tccd1" => "Chiplet 1 (Tccd1)".into(),
                "Tccd2" => "Chiplet 2 (Tccd2)".into(),
                "edge" => "GPU (Edge)".into(),
                "junction" => "Hotspot".into(),
                "mem" => crate::tr!("Graphics memory").into(),
                "Composite" => crate::tr!("Total (Composite)").into(),
                _ if t.chip == "jc42" => crate::tf!("RAM sensor {0}", i + 1),
                _ => t.label.clone(),
            };
            (label, format!("{} °C", decimal(t.celsius, 1)))
        })
        .collect::<Vec<_>>();
    metric_cards(ui, &cards);
}

fn compare_processes(
    a: &ProcessRow,
    b: &ProcessRow,
    sort: Sort,
    details: bool,
    descending: bool,
) -> std::cmp::Ordering {
    let result = match sort {
        Sort::Name => {
            let name = |p: &ProcessRow| {
                if details {
                    p.name.as_str()
                } else {
                    p.application.as_deref().unwrap_or(&p.name)
                }
                .to_lowercase()
            };
            name(a).cmp(&name(b))
        }
        Sort::Status => a.status.cmp(&b.status),
        Sort::Cpu => a.cpu.total_cmp(&b.cpu),
        Sort::Memory => a.memory.cmp(&b.memory),
        Sort::Io => a
            .io_rate
            .unwrap_or(-1.0)
            .total_cmp(&b.io_rate.unwrap_or(-1.0)),
        Sort::Pid => a.pid.cmp(&b.pid),
        Sort::User => a.user.cmp(&b.user),
        Sort::Threads => a.threads.cmp(&b.threads),
        Sort::Command => a.command.cmp(&b.command),
    };
    (if descending { result.reverse() } else { result }).then_with(|| a.pid.cmp(&b.pid))
}

fn line_color(index: usize, count: usize, primary: Color32) -> Color32 {
    if index == 0 {
        primary
    } else if count == 2 {
        COLORS[1]
    } else {
        Color32::from(egui::ecolor::Hsva::new(
            (index as f32 * 0.113 + 0.54) % 1.0,
            0.65,
            0.7,
            1.0,
        ))
    }
}
fn chart_labels(resource: &Resource) -> Vec<&'static str> {
    match resource {
        Resource::Cpu => vec![crate::tr!("CPU usage")],
        Resource::Memory => vec![crate::tr!("RAM usage")],
        Resource::Gpu(_) => vec![crate::tr!("GPU usage")],
        Resource::Disk(_) => vec![crate::tr!("Read"), crate::tr!("Write")],
        Resource::Network => vec![crate::tr!("Receive"), crate::tr!("Send")],
    }
}
fn legend_items(ui: &mut egui::Ui, items: &[(&str, Color32)]) {
    ui.horizontal_wrapped(|ui| {
        for (label, color) in items {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(22.0, 14.0), Sense::hover());
            ui.painter().line_segment(
                [rect.left_center(), rect.right_center()],
                Stroke::new(2.0_f32, *color),
            );
            ui.label(RichText::new(*label).small());
            ui.add_space(12.0);
        }
    });
}
fn chart_legend(ui: &mut egui::Ui, resource: &Resource, color: Color32) {
    let labels = chart_labels(resource);
    let items = labels
        .iter()
        .enumerate()
        .map(|(i, label)| (*label, line_color(i, labels.len(), color)))
        .collect::<Vec<_>>();
    legend_items(ui, &items);
}

fn paint_chart(
    painter: &egui::Painter,
    rect: Rect,
    series: &[Vec<f64>],
    color: Color32,
    maximum: f64,
    grid: bool,
) {
    let dark = painter.ctx().style().visuals.dark_mode;
    let grid_color = if dark {
        Color32::from_rgb(50, 66, 78)
    } else {
        Color32::from_rgb(217, 234, 243)
    };
    painter.rect_filled(
        rect,
        0.0,
        if dark {
            Color32::from_rgb(23, 28, 34)
        } else {
            Color32::from_rgb(252, 254, 255)
        },
    );
    if grid {
        for i in 1..12 {
            let x = rect.left() + rect.width() * i as f32 / 12.0;
            painter.line_segment(
                [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
                Stroke::new(1.0_f32, grid_color),
            );
        }
        for i in 1..8 {
            let y = rect.top() + rect.height() * i as f32 / 8.0;
            painter.line_segment(
                [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
                Stroke::new(1.0_f32, grid_color),
            );
        }
    }
    for (index, values) in series.iter().enumerate() {
        let points: Vec<Option<Pos2>> = values
            .iter()
            .enumerate()
            .map(|(i, value)| {
                value.is_finite().then(|| {
                    Pos2::new(
                        rect.right() - (values.len() - 1 - i) as f32 * rect.width() / 59.0,
                        rect.bottom()
                            - (value / maximum.max(1.0)).clamp(0.0, 1.0) as f32 * rect.height(),
                    )
                })
            })
            .collect();
        let color = line_color(index, series.len(), color);
        if series.len() == 1 {
            let mut mesh = egui::Mesh::default();
            let fill = color.gamma_multiply(0.14);
            for segment in points.windows(2) {
                let (Some(a), Some(b)) = (segment[0], segment[1]) else {
                    continue;
                };
                let segment = [a, b];
                let base = mesh.vertices.len() as u32;
                for point in [
                    segment[0],
                    segment[1],
                    Pos2::new(segment[1].x, rect.bottom()),
                    Pos2::new(segment[0].x, rect.bottom()),
                ] {
                    mesh.colored_vertex(point, fill);
                }
                mesh.add_triangle(base, base + 1, base + 2);
                mesh.add_triangle(base, base + 2, base + 3);
            }
            painter.add(egui::Shape::mesh(mesh));
        }
        for segment in points.windows(2) {
            if let (Some(a), Some(b)) = (segment[0], segment[1]) {
                painter.line_segment([a, b], Stroke::new(1.6_f32, color));
            }
        }
    }
    painter.rect_stroke(rect, 0.0, Stroke::new(1.0_f32, color), StrokeKind::Inside);
}

fn spawn_task(command: &str) -> Result<(), String> {
    let words = shell_words::split(command).map_err(|e| e.to_string())?;
    let program = words
        .first()
        .filter(|p| !p.is_empty())
        .ok_or(crate::tr!("Please enter a program name."))?;
    let mut child = Command::new(program)
        .args(&words[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|e| e.to_string())?;
    // Reap the child even when the task manager stays open for a long time.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

fn write_screenshot(path: &std::path::Path, image: &egui::ColorImage) -> Result<(), String> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let file = File::create(path).map_err(|e| e.to_string())?;
    let mut encoder = png::Encoder::new(file, image.width() as u32, image.height() as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    let bytes: Vec<u8> = image
        .pixels
        .iter()
        .flat_map(|pixel| pixel.to_array())
        .collect();
    writer.write_image_data(&bytes).map_err(|e| e.to_string())
}

fn disclosure(ui: &mut egui::Ui, open: bool, sense: Sense) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(20.0), sense);
    let c = rect.center();
    let points = if open {
        vec![
            c + Vec2::new(-4.0, -2.0),
            c + Vec2::new(4.0, -2.0),
            c + Vec2::new(0.0, 3.0),
        ]
    } else {
        vec![
            c + Vec2::new(-2.0, -4.0),
            c + Vec2::new(-2.0, 4.0),
            c + Vec2::new(3.0, 0.0),
        ]
    };
    ui.painter().add(egui::Shape::convex_polygon(
        points,
        ui.visuals().text_color(),
        Stroke::NONE,
    ));
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn theme_persists_and_both_modes_keep_legible_text() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config/theme");
        assert!(!load_theme(&path));
        save_theme(&path, true).unwrap();
        assert!(load_theme(&path));
        save_theme(&path, false).unwrap();
        assert!(!load_theme(&path));
        let ctx = egui::Context::default();
        for dark in [false, true] {
            apply_theme(&ctx, dark);
            assert_eq!(ctx.style().visuals.dark_mode, dark);
            assert_eq!(ctx.style().visuals.panel_fill.a(), 255);
        }
    }
    #[test]
    fn uptime_includes_days_hours_minutes_and_seconds() {
        assert_eq!(uptime(90061), "1 T. 01:01:01");
        assert_eq!(uptime(0), "0 T. 00:00:00");
    }
    #[test]
    fn resource_cards_click_on_graph_metric_and_empty_margin() {
        for offset in [
            Vec2::new(4.0, 78.0),
            Vec2::new(70.0, 36.0),
            Vec2::new(110.0, 65.0),
        ] {
            let ctx = egui::Context::default();
            let mut rect = Rect::NOTHING;
            let series = vec![vec![10.0, 20.0]];
            let card = ResourceCard {
                title: "CPU",
                metric: "20 %",
                color: BLUE,
                maximum: 100.0,
                series: &series,
            };
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    rect = resource_card(ui, &Resource::Cpu, &card, false).rect;
                });
            });
            let pos = rect.min + offset;
            let mut clicked = false;
            for pressed in [true, false] {
                let input = egui::RawInput {
                    events: vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                    ..Default::default()
                };
                let _ = ctx.run(input, |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        clicked |= resource_card(ui, &Resource::Cpu, &card, false).clicked();
                    });
                });
            }
            assert!(clicked, "Card area {offset:?} did not activate");
        }
    }
}
