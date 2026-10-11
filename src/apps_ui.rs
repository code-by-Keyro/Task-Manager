//! App inventory UI reuses the task manager's theme, metric cards and tables.
use crate::apps::{self, App, Scope, Sort};
use eframe::egui::{self, Align, Layout, RichText, Sense, Vec2};
use egui_extras::{Column, TableBuilder};

pub struct View {
    monitor: apps::Monitor,
    snapshot: Option<apps::Snapshot>,
    loading: bool,
    started: bool,
    query: String,
    extras: bool,
    scope: Option<Scope>,
    sort: Sort,
    descending: bool,
    selected: Option<String>,
    detail_open: bool,
    smoke: bool,
    package_smoke_done: bool,
}
impl View {
    pub fn new(ctx: egui::Context, smoke: bool) -> Self {
        Self {
            monitor: apps::Monitor::start(ctx),
            snapshot: None,
            loading: false,
            started: false,
            query: String::new(),
            extras: false,
            scope: None,
            sort: Sort::Name,
            descending: false,
            selected: None,
            detail_open: false,
            smoke,
            package_smoke_done: false,
        }
    }
    pub fn refresh(&mut self) {
        if !self.loading && self.monitor.refresh() {
            self.loading = true;
            self.started = true;
        }
    }
    pub fn ready(&self) -> bool {
        self.snapshot.is_some() && !self.loading
    }
    pub fn receive(&mut self) {
        while let Ok(snapshot) = self.monitor.receiver.try_recv() {
            self.loading = false;
            if self.smoke && self.snapshot.is_none() {
                match std::env::var("TASK_MANAGER_SMOKE_APPS")
                    .unwrap_or_default()
                    .as_str()
                {
                    "packages" => self.extras = true,
                    "flatpak" => self.query = "Flatpak".into(),
                    "user" => self.scope = Some(Scope::User),
                    "empty" => self.query = "no-app-matches-this-test-76391".into(),
                    "details" => {
                        self.detail_open = true;
                        self.query = "Konsole".into();
                    }
                    "pacman" => self.query = "Konsole".into(),
                    _ => {}
                }
            }
            if self
                .selected
                .as_ref()
                .is_some_and(|id| !snapshot.apps.iter().any(|a| &a.id == id))
            {
                self.selected = None;
            }
            self.snapshot = Some(snapshot);
        }
    }
    pub fn show(&mut self, ui: &mut egui::Ui, manager: &mut crate::packages::View) {
        ui.style_mut().spacing.scroll.floating = false;
        if !self.started {
            self.refresh();
        }
        ui.horizontal(|ui| {
            ui.heading(crate::tr!("Installed apps"));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add_enabled(!self.loading, egui::Button::new(crate::tr!("Refresh")))
                    .clicked()
                {
                    self.refresh();
                }
                if self.loading {
                    ui.spinner();
                }
            });
        });
        ui.label(crate::tr!(
            "Apps and personal launchers · Versions and package data from Pacman and Flatpak"
        ));
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .desired_width(300.0)
                    .hint_text(crate::tr!("Search apps · Name, package or publisher")),
            );
            if !self.query.is_empty() && ui.button(crate::tr!("Reset")).clicked() {
                self.query.clear();
            }
            ui.selectable_value(&mut self.scope, None, crate::tr!("All"));
            ui.selectable_value(&mut self.scope, Some(Scope::User), crate::tr!("User"));
            ui.selectable_value(&mut self.scope, Some(Scope::System), crate::tr!("System"));
        });
        let Some(snapshot) = &self.snapshot else {
            ui.add_space(30.0);
            ui.spinner();
            ui.label(crate::tr!("Reading installed apps …"));
            return;
        };
        let base = snapshot
            .apps
            .iter()
            .filter(|a| !a.extra)
            .collect::<Vec<_>>();
        crate::ui::metric_cards(
            ui,
            &[
                (
                    crate::tr!("Apps / launchers").into(),
                    base.len().to_string(),
                ),
                (
                    crate::tr!("User").into(),
                    base.iter()
                        .filter(|a| a.scope == Scope::User)
                        .count()
                        .to_string(),
                ),
                (
                    crate::tr!("System").into(),
                    base.iter()
                        .filter(|a| a.scope == Scope::System)
                        .count()
                        .to_string(),
                ),
            ],
        );
        ui.checkbox(&mut self.extras,crate::tr!("Show packages without an app entry")).on_hover_text(crate::tr!("Also show Pacman libraries, system packages and packages without a visible desktop launcher. These are not all standalone apps."));
        if !snapshot.warnings.is_empty() {
            egui::CollapsingHeader::new(crate::tf!(
                "Collection notes ({0})",
                snapshot.warnings.len()
            ))
            .show(ui, |ui| {
                for warning in &snapshot.warnings {
                    ui.label(warning);
                }
            });
        }
        let visible = apps::visible(
            snapshot,
            &self.query,
            self.extras,
            self.scope,
            self.sort,
            self.descending,
        );
        // A filtered-away selection must never leave unrelated stale details visible.
        if self
            .selected
            .as_ref()
            .is_none_or(|id| !visible.iter().any(|i| &snapshot.apps[*i].id == id))
        {
            self.selected = visible.first().map(|i| snapshot.apps[*i].id.clone());
        }
        if let Some(a) = self
            .selected
            .as_ref()
            .and_then(|id| snapshot.apps.iter().find(|a| &a.id == id))
        {
            let action = crate::packages::action_label(a);
            ui.horizontal_wrapped(|ui| {
                let label = action.as_ref().copied().unwrap_or(crate::tr!("Uninstall"));
                let response = ui.add_enabled(
                    action.is_ok() && !self.loading && !manager.busy(),
                    egui::Button::new(label),
                );
                if let Err(reason) = &action {
                    response.clone().on_hover_text(reason);
                }
                if response.clicked() {
                    manager.propose(crate::packages::Operation::Remove(Box::new(a.clone())));
                }
            });
            if self.smoke
                && !self.package_smoke_done
                && std::env::var("TASK_MANAGER_SMOKE_PACKAGE").is_ok_and(|m| m == "remove")
                && !manager.busy()
            {
                manager.propose(crate::packages::Operation::Remove(Box::new(a.clone())));
                // Prepare the read-only preview once; never execute in smoke mode.
                self.package_smoke_done = true;
            }
        }
        let wide = ui.available_width() >= 1020.0;
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(crate::tf!(
                    "{0} shown · Updated {1} s ago",
                    visible.len(),
                    snapshot.updated.elapsed().as_secs()
                ))
                .small(),
            );
            if !wide
                && ui
                    .add_enabled(
                        self.selected.is_some(),
                        egui::Button::new(crate::tr!("App details")),
                    )
                    .clicked()
            {
                self.detail_open = true;
            }
            if self.loading {
                ui.label(RichText::new(crate::tr!("Refreshing list …")).small());
            }
        });
        ui.separator();
        let height = ui.available_height().max(80.0);
        let table_width = if wide {
            ui.available_width() - 310.0 - ui.spacing().item_spacing.x
        } else {
            ui.available_width()
        };
        if wide {
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    Vec2::new(table_width, height),
                    Layout::top_down(Align::Min),
                    |ui| {
                        table(
                            ui,
                            snapshot,
                            &visible,
                            Vec2::new(table_width, height),
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
                                .id_salt("installed-apps-details")
                                .max_height(height - 24.0)
                                .show(ui, |ui| {
                                    details(
                                        ui,
                                        self.selected.as_ref().and_then(|id| {
                                            snapshot.apps.iter().find(|a| &a.id == id)
                                        }),
                                    );
                                });
                        });
                    });
            });
        } else {
            table(
                ui,
                snapshot,
                &visible,
                Vec2::new(table_width, height),
                TableState {
                    selected: &mut self.selected,
                    sort: &mut self.sort,
                    descending: &mut self.descending,
                    detail_open: &mut self.detail_open,
                },
            );
        }
        if self.detail_open && !wide {
            egui::Window::new(crate::tr!("App details"))
                .id(egui::Id::new("installed-app-details-window"))
                .open(&mut self.detail_open)
                .default_width(440.0)
                .resizable(true)
                .vscroll(true)
                .hscroll(false)
                .max_height((ui.ctx().content_rect().height() - 120.0).max(150.0))
                .show(ui.ctx(), |ui| {
                    details(
                        ui,
                        self.selected
                            .as_ref()
                            .and_then(|id| snapshot.apps.iter().find(|a| &a.id == id)),
                    );
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
    snapshot: &apps::Snapshot,
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
    let (table_width, height) = (dimensions.x, dimensions.y);

    ui.set_width(table_width);
    if visible.is_empty() {
        ui.label(crate::tr!("No matching apps or packages found."));
        return;
    }
    egui::ScrollArea::horizontal()
        .id_salt("installed-apps-horizontal")
        .show(ui, |ui| {
            let columns = [
                (Sort::Name, crate::tr!("App / package"), 200.0),
                (Sort::Version, crate::tr!("Version"), 115.0),
                (Sort::Publisher, crate::tr!("Publisher / maintainer"), 155.0),
                (Sort::Source, crate::tr!("Source"), 140.0),
                (Sort::Scope, crate::tr!("Scope"), 80.0),
                (Sort::Size, crate::tr!("Size"), 95.0),
            ];
            let mut table = TableBuilder::new(ui)
                .id_salt("installed-apps-table")
                .sense(Sense::click())
                .striped(true)
                .resizable(true)
                .cell_layout(Layout::left_to_right(Align::Center))
                .min_scrolled_height(0.0)
                .max_scroll_height((height - 45.0).max(50.0));
            for (_, _, width) in columns {
                table = table.column(Column::initial(width).at_least(65.0));
            }
            table
                .header(32.0, |mut row| {
                    for (column, title, _) in columns {
                        row.col(|ui| {
                            let arrow = if *sort == column {
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
                                if *sort == column {
                                    *descending = !*descending;
                                } else {
                                    *sort = column;
                                    *descending = false;
                                }
                            }
                        });
                    }
                })
                .body(|body| {
                    body.rows(35.0, visible.len(), |mut row| {
                        let a = &snapshot.apps[visible[row.index()]];
                        row.set_selected(selected.as_ref() == Some(&a.id));
                        let values = [
                            a.name.clone(),
                            a.version.clone().unwrap_or_else(|| "—".into()),
                            a.publisher.clone().unwrap_or_else(|| "—".into()),
                            a.source.clone(),
                            a.scope.label().into(),
                            a.size
                                .map(|n| crate::ui::size(n as f64))
                                .unwrap_or_else(|| "—".into()),
                        ];
                        for (i, value) in values.iter().enumerate() {
                            row.col(|ui| {
                                ui.add(egui::Label::new(value).truncate()).on_hover_text(
                                    if i == 5 {
                                        size_note(a).to_owned()
                                    } else {
                                        value.clone()
                                    },
                                );
                            });
                        }
                        let response = row.response();
                        if response.clicked() {
                            *selected = Some(a.id.clone());
                        }
                        if response.double_clicked() {
                            *selected = Some(a.id.clone());
                            *detail_open = true;
                        }
                        response.context_menu(|ui| {
                            if ui.button(crate::tr!("Copy name")).clicked() {
                                ui.ctx().copy_text(a.name.clone());
                                ui.close();
                            }
                            if !a.command.is_empty()
                                && ui.button(crate::tr!("Copy launch command")).clicked()
                            {
                                ui.ctx().copy_text(a.command.clone());
                                ui.close();
                            }
                        });
                    })
                });
        });
}
fn size_note(a: &App) -> &'static str {
    if a.source == "Pacman" {
        crate::tr!(
            "Installed size of the associated package, excluding dependencies. A package may contain multiple apps; the size is not limited to this launcher."
        )
    } else if a.source.starts_with("Flatpak") {
        crate::tr!(
            "App size reported by Flatpak, excluding shared runtimes. Not the total occupied space."
        )
    } else {
        crate::tr!("No reliable size information is available for this desktop entry.")
    }
}
fn details(ui: &mut egui::Ui, app: Option<&App>) {
    ui.heading(crate::tr!("App details"));
    ui.add_space(8.0);
    let Some(a) = app else {
        ui.label(crate::tr!("Select an app from the table."));
        return;
    };
    ui.label(RichText::new(&a.name).size(20.0));
    if !a.description.is_empty() {
        ui.label(&a.description);
    }
    ui.separator();
    for (label, value) in [
        (
            crate::tr!("Version"),
            a.version
                .clone()
                .unwrap_or_else(|| crate::tr!("unavailable").into()),
        ),
        (
            if a.source == "Pacman" {
                crate::tr!("Package maintainer")
            } else {
                crate::tr!("Publisher")
            },
            a.publisher
                .clone()
                .unwrap_or_else(|| crate::tr!("unavailable").into()),
        ),
        (crate::tr!("Source"), a.source.clone()),
        (crate::tr!("Scope"), a.scope.label().into()),
        (
            crate::tr!("Package / app ID"),
            a.package
                .clone()
                .unwrap_or_else(|| crate::tr!("unavailable").into()),
        ),
        (
            if a.source == "Pacman" {
                crate::tr!("Installed package size")
            } else {
                crate::tr!("App size")
            },
            a.size
                .map(|n| crate::ui::size(n as f64))
                .unwrap_or_else(|| crate::tr!("unavailable").into()),
        ),
        (
            crate::tr!("Installed / last updated"),
            a.installed
                .and_then(apps::date)
                .unwrap_or_else(|| crate::tr!("unavailable").into()),
        ),
        (
            crate::tr!("Launch command"),
            if a.command.is_empty() {
                crate::tr!("unavailable").into()
            } else {
                a.command.clone()
            },
        ),
    ] {
        ui.label(
            RichText::new(label)
                .small()
                .color(ui.visuals().weak_text_color()),
        );
        ui.add(egui::Label::new(value).wrap());
        ui.add_space(3.0);
    }
    if let Some(reference) = &a.reference {
        ui.label(RichText::new(crate::tr!("Branch · Architecture · Installation")).small());
        ui.label(reference);
    }
    if let Some(path) = &a.desktop {
        ui.label(RichText::new(crate::tr!("Desktop file")).small());
        ui.add(egui::Label::new(path.display().to_string()).wrap());
    }
    ui.add_space(8.0);
    ui.label(RichText::new(size_note(a)).small());
    if !a.command.is_empty() && ui.button(crate::tr!("Copy launch command")).clicked() {
        ui.ctx().copy_text(a.command.clone());
    }
}
