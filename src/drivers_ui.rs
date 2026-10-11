//! Driver page styled like the existing application inventory.
use crate::drivers::{self, Details, Filter, Sort, State};
use eframe::egui::{self, Align, Layout, RichText, Sense, Vec2};
use egui_extras::{Column, TableBuilder};
use std::collections::HashMap;
use std::sync::atomic::Ordering;

pub struct View {
    monitor: drivers::Monitor,
    snapshot: Option<drivers::Snapshot>,
    loading: bool,
    started: bool,
    kernel: Option<String>,
    query: String,
    filter: Filter,
    sort: Sort,
    descending: bool,
    selected: Option<String>,
    detail_open: bool,
    details: HashMap<String, Details>,
    pending: Option<String>,
    updates: Option<drivers::Updates>,
    checking: bool,
    updates_open: bool,
    smoke: bool,
    smoke_mode: String,
    package_smoke_done: bool,
}
impl View {
    pub fn new(ctx: egui::Context, smoke: bool) -> Self {
        Self {
            monitor: drivers::Monitor::start(ctx),
            snapshot: None,
            loading: false,
            started: false,
            kernel: None,
            query: String::new(),
            filter: Filter::All,
            sort: Sort::Name,
            descending: false,
            selected: None,
            detail_open: false,
            details: HashMap::new(),
            pending: None,
            updates: None,
            checking: false,
            updates_open: false,
            smoke,
            package_smoke_done: false,
            smoke_mode: if smoke {
                std::env::var("TASK_MANAGER_SMOKE_DRIVERS").unwrap_or_default()
            } else {
                String::new()
            },
        }
    }
    pub fn refresh(&mut self) {
        if !self.loading && self.monitor.refresh(self.kernel.clone()) {
            self.loading = true;
            self.started = true;
        }
    }
    pub fn ready(&self) -> bool {
        self.snapshot.is_some() && !self.loading && !self.checking && self.pending.is_none()
    }
    pub fn receive(&mut self) {
        while let Ok(event) = self.monitor.receiver.try_recv() {
            match event {
                drivers::Event::Inventory(s) => {
                    self.loading = false;
                    self.pending = None;
                    self.details.clear();
                    self.selected = None;
                    if self.smoke && self.snapshot.is_none() {
                        match self.smoke_mode.as_str() {
                            "loaded" => self.filter = Filter::Loaded,
                            "builtin" => self.filter = Filter::Builtin,
                            "empty" => self.query = "nothing-matches-driver-76192".into(),
                            "installed-amdgpu" => self.query = "amdgpu".into(),
                            "online" => {
                                self.checking = self.monitor.check();
                                self.updates_open = true;
                            }
                            "details" => {
                                self.query = "amdgpu".into();
                                self.detail_open = true;
                            }
                            _ => {}
                        }
                        if self.smoke_mode.starts_with("installed")
                            && let Some(k) = s
                                .kernels
                                .iter()
                                .find(|k| *k != &s.running && !k.contains("lts"))
                        {
                            self.kernel = Some(k.clone());
                            self.snapshot = Some(s);
                            self.refresh();
                            continue;
                        }
                        if self.smoke_mode == "updates" || self.smoke_mode == "update-error" {
                            self.updates_open = true;
                            self.updates = Some(drivers::Updates {
                                list: if self.smoke_mode == "updates" {
                                    vec![
                                        drivers::Update {
                                            package: "linux-cachyos".into(),
                                            old: crate::tr!("Example old").into(),
                                            new: crate::tr!("Example new").into(),
                                            reason: crate::tr!("Kernel / kernel modules").into(),
                                        },
                                        drivers::Update {
                                            package: "mesa".into(),
                                            old: crate::tr!("Example old").into(),
                                            new: crate::tr!("Example new").into(),
                                            reason: crate::tr!(
                                                "Graphics driver / graphics library"
                                            )
                                            .into(),
                                        },
                                    ]
                                } else {
                                    vec![]
                                },
                                all: 2,
                                error: if self.smoke_mode == "update-error" {
                                    Some(crate::tr!("Example: package sources unreachable.").into())
                                } else {
                                    None
                                },
                                checked: std::time::Instant::now(),
                            });
                        }
                    }
                    self.snapshot = Some(s);
                }
                drivers::Event::Details(kernel, name, detail) => {
                    if self.pending.as_ref() == Some(&name) {
                        self.pending = None;
                    }
                    if let Some(s) = &mut self.snapshot
                        && s.kernel == kernel
                    {
                        if let Some(m) = s.modules.iter_mut().find(|m| m.name == name) {
                            if let Some(file) =
                                detail.fields.get("filename").and_then(|f| f.first())
                                && std::path::Path::new(file).is_absolute()
                            {
                                m.path = Some(file.into());
                            }
                            if m.version.is_none() {
                                m.version = detail
                                    .fields
                                    .get("version")
                                    .and_then(|f| f.first())
                                    .cloned();
                            }
                            // Current taint is authoritative for a loaded module; disk metadata is only a fallback.
                            if m.external.is_none() && detail.error.is_none() {
                                m.external = Some(
                                    !detail
                                        .fields
                                        .get("intree")
                                        .is_some_and(|f| f.iter().any(|v| v == "Y")),
                                );
                            }
                        }
                        self.details.insert(name, detail);
                    }
                }
            }
        }
        while let Ok(updates) = self.monitor.updates.try_recv() {
            self.checking = false;
            self.updates = Some(updates);
        }
    }
    pub fn show(&mut self, ui: &mut egui::Ui, manager: &mut crate::packages::View) {
        ui.style_mut().spacing.scroll.floating = false;
        if !self.started {
            self.refresh();
        }
        ui.horizontal_wrapped(|ui|{
            ui.heading(crate::tr!("Drivers"));
            if ui.add_enabled(!self.loading,egui::Button::new(crate::tr!("Refresh"))).clicked(){self.refresh();}
            if ui.add_enabled(!self.checking && !self.smoke,egui::Button::new(crate::tr!("Check for updates"))).on_hover_text(crate::tr!("Checks kernel, driver and firmware packages in the configured package sources. Installs nothing.")).clicked() && self.monitor.check(){
                self.checking=true;self.updates_open=true;
            }
            if self.loading || self.checking {ui.spinner();}
            if self.checking && ui.button(crate::tr!("Cancel search")).clicked(){self.monitor.update_cancel.store(true,Ordering::Relaxed);}
            if self.updates.is_some() && ui.button(crate::tr!("Update results")).clicked(){self.updates_open=true;}
        });
        let can_update = self
            .updates
            .as_ref()
            .is_some_and(|u| u.error.is_none() && u.all > 0)
            && !self.checking
            && !manager.busy();
        if ui.add_enabled(can_update,egui::Button::new(crate::tr!("Install updates …"))).on_hover_text(crate::tr!("Full CachyOS system upgrade with graphical administrator approval and package confirmation.")).clicked() { self.updates_open = false; propose_upgrade(manager,self.updates.as_ref().unwrap()); }
        if self.smoke
            && !self.package_smoke_done
            && std::env::var("TASK_MANAGER_SMOKE_PACKAGE").is_ok_and(|m| m == "upgrade")
            && can_update
        {
            self.updates_open = false;
            propose_upgrade(manager, self.updates.as_ref().unwrap());
            self.package_smoke_done = true;
        }
        ui.label(crate::tr!(
            "Kernel modules and drivers · Updates from your system's package sources"
        ));
        let Some(s) = &self.snapshot else {
            ui.spinner();
            ui.label(crate::tr!("Reading drivers …"));
            return;
        };
        let mut chosen = s.kernel.clone();
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .desired_width(265.0)
                    .hint_text(crate::tr!("Search drivers · Name or path")),
            );
            if !self.query.is_empty() && ui.button(crate::tr!("Reset")).clicked() {
                self.query.clear();
            }
            ui.label(crate::tr!("Kernel:"));
            egui::ComboBox::from_id_salt("driver-kernel")
                .selected_text(&chosen)
                .show_ui(ui, |ui| {
                    for k in &s.kernels {
                        ui.selectable_value(
                            &mut chosen,
                            k.clone(),
                            format!(
                                "{k}{}",
                                if k == &s.running {
                                    crate::tr!(" · running")
                                } else {
                                    crate::tr!(" · installed")
                                }
                            ),
                        );
                    }
                });
        });
        if chosen != s.kernel && !self.loading {
            self.kernel = Some(chosen);
            self.refresh();
            return;
        }
        let s = self.snapshot.as_ref().unwrap();
        crate::ui::metric_cards(
            ui,
            &[
                (crate::tr!("Modules").into(), s.modules.len().to_string()),
                (
                    crate::tr!("Loaded").into(),
                    if s.kernel == s.running {
                        s.modules
                            .iter()
                            .filter(|m| m.state == State::Loaded)
                            .count()
                            .to_string()
                    } else {
                        "—".into()
                    },
                ),
                (
                    crate::tr!("Confirmed external").into(),
                    s.modules
                        .iter()
                        .filter(|m| m.external == Some(true))
                        .count()
                        .to_string(),
                ),
                (
                    crate::tr!("Module memory¹").into(),
                    if s.kernel == s.running {
                        crate::ui::size(
                            s.modules.iter().filter_map(|m| m.memory).sum::<u64>() as f64
                        )
                    } else {
                        "—".into()
                    },
                ),
            ],
        );
        ui.horizontal_wrapped(|ui| {
            for (f, label) in [
                (Filter::All, crate::tr!("All")),
                (Filter::Loaded, crate::tr!("Loaded")),
                (Filter::Available, crate::tr!("Not loaded")),
                (Filter::Builtin, crate::tr!("Built-in")),
                (Filter::External, crate::tr!("External")),
            ] {
                ui.selectable_value(&mut self.filter, f, label);
            }
        });
        ui.label(RichText::new(if s.kernel==s.running {crate::tf!("Running kernel: {0} · ¹ Sizes from /proc/modules; excluding built-in drivers" , s.running)}else{crate::tf!("Installed modules for {0} · Running kernel: {1} · Runtime values available only there" , s.kernel, s.running)}).small());
        if !s.warnings.is_empty() {
            egui::CollapsingHeader::new(crate::tf!("Collection notes ({0})", s.warnings.len()))
                .default_open(true)
                .show(ui, |ui| {
                    for w in &s.warnings {
                        ui.label(w);
                    }
                });
        }
        ui.label(RichText::new(crate::tr!("Unloaded modules are available drivers, not running processes. Module version/source are added when selected.")).small());
        let visible = drivers::visible(s, &self.query, self.filter, self.sort, self.descending);
        if self
            .selected
            .as_ref()
            .is_none_or(|name| !visible.iter().any(|i| &s.modules[*i].name == name))
        {
            self.selected = visible.first().map(|i| s.modules[*i].name.clone());
        }
        if !self.loading
            && self.pending.is_none()
            && let Some(name) = &self.selected
            && !self.details.contains_key(name)
            && self.monitor.details(s.kernel.clone(), name.clone())
        {
            self.pending = Some(name.clone());
        }
        let wide = ui.available_width() >= 1020.0;
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(crate::tf!(
                    "{0} shown · Updated {1} s ago",
                    visible.len(),
                    s.updated.elapsed().as_secs()
                ))
                .small(),
            );
            if !wide
                && ui
                    .add_enabled(
                        self.selected.is_some(),
                        egui::Button::new(crate::tr!("Module details")),
                    )
                    .clicked()
            {
                self.detail_open = true;
            }
            if self.pending.is_some() {
                ui.label(RichText::new(crate::tr!("Loading details …")).small());
            }
        });
        ui.separator();
        let height = ui.available_height().max(80.0);
        let width = ui.available_width()
            - if wide {
                310.0 + ui.spacing().item_spacing.x
            } else {
                0.0
            };
        if wide {
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    Vec2::new(width, height),
                    Layout::top_down(Align::Min),
                    |ui| {
                        table(
                            ui,
                            s,
                            &visible,
                            Vec2::new(width, height),
                            TableState {
                                selected: &mut self.selected,
                                sort: &mut self.sort,
                                descending: &mut self.descending,
                                detail_open: &mut self.detail_open,
                            },
                        )
                    },
                );
                egui::Frame::new()
                    .fill(if ui.visuals().dark_mode {
                        egui::Color32::from_rgb(34, 40, 47)
                    } else {
                        egui::Color32::from_rgb(243, 247, 250)
                    })
                    .corner_radius(6.0)
                    .inner_margin(12.0)
                    .show(ui, |ui| {
                        ui.with_layout(Layout::top_down(Align::Min), |ui| {
                            ui.set_width(286.0);
                            egui::ScrollArea::vertical()
                                .id_salt("driver-details")
                                .max_height(height - 24.0)
                                .show(ui, |ui| {
                                    details(ui, s, self.selected.as_deref(), &self.details);
                                });
                        });
                    });
            });
        } else {
            table(
                ui,
                s,
                &visible,
                Vec2::new(width, height),
                TableState {
                    selected: &mut self.selected,
                    sort: &mut self.sort,
                    descending: &mut self.descending,
                    detail_open: &mut self.detail_open,
                },
            );
        }
        if self.detail_open && !wide {
            egui::Window::new(crate::tr!("Module details"))
                .open(&mut self.detail_open)
                .default_width(450.0)
                .vscroll(true)
                .hscroll(false)
                .max_height((ui.ctx().content_rect().height() - 120.0).max(150.0))
                .show(ui.ctx(), |ui| {
                    details(ui, s, self.selected.as_deref(), &self.details);
                });
        }
        if self.updates_open {
            egui::Window::new(crate::tr!("Driver updates")).open(&mut self.updates_open).default_width(680.0).vscroll(true).hscroll(false).max_height((ui.ctx().content_rect().height()-120.0).max(180.0)).show(ui.ctx(),|ui|{
            if self.smoke && ["updates","update-error"].contains(&self.smoke_mode.as_str()){ui.label(RichText::new(crate::tr!("Layout check · Sample data")).strong());}
            ui.label(crate::tr!("Kernel, drivers, graphics libraries, firmware and CPU microcode from the configured Pacman package sources."));
            if self.checking{ui.spinner();ui.label(crate::tr!("Checking package sources … (up to 60 s)"));}
            else if let Some(updates)=&self.updates{
                if let Some(error)=&updates.error {ui.label(RichText::new(crate::tr!("Check incomplete")).strong());ui.label(error);}
                else{
                    ui.label(RichText::new(crate::tf!("{0} matching updates · {1} package updates in total" , updates.list.len(), updates.all)).strong());
                    if updates.list.is_empty(){ui.label(crate::tr!("No updates found for the detected driver packages."));}
                    for u in &updates.list{ui.separator();ui.label(RichText::new(&u.package).strong());ui.label(format!("{} -> {}",u.old,u.new));ui.label(&u.reason);}
                    ui.label(RichText::new(crate::tf!("Checked online {0} s ago" , updates.checked.elapsed().as_secs())).small());
                }
            }
            ui.separator();ui.label(crate::tr!("On CachyOS, updates are part of a full system upgrade. The search does not install packages or load/unload drivers."));
            ui.label(RichText::new(crate::tr!("AUR, manufacturer websites, BIOS and alternative drivers that are not installed are not checked here.")).small());
        });
        }
    }
}
struct TableState<'a> {
    selected: &'a mut Option<String>,
    sort: &'a mut Sort,
    descending: &'a mut bool,
    detail_open: &'a mut bool,
}
fn table(
    ui: &mut egui::Ui,
    s: &drivers::Snapshot,
    visible: &[usize],
    dimensions: Vec2,
    state: TableState<'_>,
) {
    let TableState {
        selected,
        sort,
        descending,
        detail_open,
    } = state;
    ui.set_width(dimensions.x);
    if visible.is_empty() {
        ui.label(crate::tr!("No matching drivers found."));
        return;
    }
    egui::ScrollArea::horizontal()
        .id_salt("drivers-horizontal")
        .show(ui, |ui| {
            let columns = [
                (Sort::Name, crate::tr!("Modules"), 210.0),
                (Sort::State, crate::tr!("Status"), 115.0),
                (Sort::Memory, crate::tr!("Module memory"), 110.0),
                (Sort::References, crate::tr!("References"), 90.0),
                (Sort::Version, crate::tr!("Version"), 115.0),
                (Sort::Source, crate::tr!("Source"), 90.0),
            ];
            let mut table = TableBuilder::new(ui)
                .id_salt("drivers-table")
                .sense(Sense::click())
                .striped(true)
                .resizable(true)
                .cell_layout(Layout::left_to_right(Align::Center))
                .min_scrolled_height(0.0)
                .max_scroll_height((dimensions.y - 45.0).max(50.0));
            for (_, _, width) in columns {
                table = table.column(Column::initial(width).at_least(65.0));
            }
            table
                .header(32.0, |mut row| {
                    for (col, title, _) in columns {
                        row.col(|ui| {
                            let arrow = if *sort == col {
                                if *descending { " v" } else { " ^" }
                            } else {
                                ""
                            };
                            if ui
                                .add_sized(
                                    ui.available_size(),
                                    egui::Button::new(format!("{title}{arrow}")).frame(false),
                                )
                                .clicked()
                            {
                                if *sort == col {
                                    *descending = !*descending;
                                } else {
                                    *sort = col;
                                    *descending = false;
                                }
                            }
                        });
                    }
                })
                .body(|body| {
                    body.rows(35.0, visible.len(), |mut row| {
                        let m = &s.modules[visible[row.index()]];
                        row.set_selected(selected.as_ref() == Some(&m.name));
                        for value in [
                            m.name.clone(),
                            m.state.label().into(),
                            m.memory
                                .map(|v| crate::ui::size(v as f64))
                                .unwrap_or_else(|| "—".into()),
                            m.references
                                .map(|v| v.to_string())
                                .unwrap_or_else(|| "—".into()),
                            m.version.clone().unwrap_or_else(|| "—".into()),
                            m.source().into(),
                        ] {
                            row.col(|ui| {
                                ui.add(egui::Label::new(&value).truncate())
                                    .on_hover_text(value);
                            });
                        }
                        let response = row.response();
                        if response.clicked() {
                            *selected = Some(m.name.clone());
                        }
                        if response.double_clicked() {
                            *selected = Some(m.name.clone());
                            *detail_open = true;
                        }
                        response.context_menu(|ui| {
                            if ui.button(crate::tr!("Copy module name")).clicked() {
                                ui.ctx().copy_text(m.name.clone());
                                ui.close();
                            }
                        });
                    })
                });
        });
}
fn details(
    ui: &mut egui::Ui,
    s: &drivers::Snapshot,
    selected: Option<&str>,
    cache: &HashMap<String, Details>,
) {
    ui.heading(crate::tr!("Module details"));
    let Some(m) = selected.and_then(|name| s.modules.iter().find(|m| m.name == name)) else {
        ui.label(crate::tr!("Select a driver from the list."));
        return;
    };
    let d = cache.get(&m.name);
    let field = |key: &str| {
        d.and_then(|d| d.fields.get(key))
            .map(|v| v.join(", "))
            .unwrap_or_else(|| "—".into())
    };
    let rows = [
        (crate::tr!("Name"), m.name.clone()),
        (crate::tr!("Status"), m.state.label().into()),
        (crate::tr!("Kernel"), s.kernel.clone()),
        (crate::tr!("Source"), m.source().into()),
        (
            crate::tr!("Module version"),
            m.version.clone().unwrap_or_else(|| field("version")),
        ),
        (crate::tr!("Source version"), field("srcversion")),
        (
            crate::tr!("Module memory"),
            m.memory
                .map(|v| crate::ui::size(v as f64))
                .unwrap_or_else(|| "—".into()),
        ),
        (
            crate::tr!("References"),
            m.references
                .map(|v| v.to_string())
                .unwrap_or_else(|| "—".into()),
        ),
        (
            crate::tr!("Used by"),
            if m.holders.is_empty() {
                "—".into()
            } else {
                m.holders.join(", ")
            },
        ),
        (
            crate::tr!("Dependencies"),
            if m.depends.is_empty() {
                field("depends")
            } else {
                m.depends.join(", ")
            },
        ),
        (crate::tr!("Description"), field("description")),
        (crate::tr!("Author"), field("author")),
        (crate::tr!("License"), field("license")),
        (crate::tr!("Signed by"), field("signer")),
        (
            crate::tr!("File path"),
            m.path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| {
                    if m.state == State::Builtin {
                        crate::tr!("Built into the kernel").into()
                    } else {
                        field("filename")
                    }
                }),
        ),
    ];
    for (label, value) in rows {
        ui.add_space(5.0);
        ui.label(RichText::new(label).strong());
        ui.add(egui::Label::new(value).wrap());
    }
    if let Some(e) = d.and_then(|d| d.error.as_ref()) {
        ui.separator();
        ui.label(crate::tr!("Further details unavailable:"));
        ui.add(egui::Label::new(e).wrap());
    }
    ui.separator();
    ui.label(
        RichText::new(crate::tr!(
            "Module version is not the package or kernel version. Missing values remain unknown."
        ))
        .small(),
    );
    if ui.button(crate::tr!("Copy module name")).clicked() {
        ui.ctx().copy_text(m.name.clone());
    }
}

fn propose_upgrade(manager: &mut crate::packages::View, u: &drivers::Updates) {
    manager.propose(crate::packages::Operation::Upgrade {
        count: u.all,
        drivers: u
            .list
            .iter()
            .map(|p| format!("{}: {} -> {} ({})", p.package, p.old, p.new, p.reason))
            .collect(),
    });
}
