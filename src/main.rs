#[cfg(not(target_os = "linux"))]
compile_error!("Dieser Task-Manager ist für Linux vorgesehen.");

mod apps;
mod apps_ui;
mod command;
mod cpu;
mod docker;
mod drivers;
mod drivers_ui;
mod hardware;
mod i18n;
mod install;
mod monitor;
mod optimizer;
mod packages;
mod processes;
mod storage;
mod ui;
mod windows;

use std::path::PathBuf;
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let helper_args: Vec<String> = std::env::args().skip(1).collect();
    if helper_args.first().is_some_and(|a| a == "--package-action") {
        return packages::helper(&helper_args[1..]).map_err(Into::into);
    }
    let read_only = helper_args.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "--version"
                | "-V"
                | "--help"
                | "-h"
                | "--check"
                | "--apps-check"
                | "--drivers-check"
                | "--driver-updates-check"
                | "--docker-check"
                | "--optimize-check"
                | "--language-check"
                | "--smoke-test"
        )
    });
    i18n::initialize(!read_only);
    let mut smoke = false;
    let mut screenshot = None;
    let mut install = false;
    let mut uninstall = false;
    let mut performance = false;
    let mut dark = None;
    let mut view = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--language-check" => {
                i18n::check();
                return Ok(());
            }
            "--version" | "-V" => {
                println!(
                    "{}",
                    crate::tf!("Task-Manager {0} (Rust)", env!("CARGO_PKG_VERSION"))
                );
                return Ok(());
            }
            "--help" | "-h" => {
                println!(
                    "{}",
                    crate::tr!(
                        "Task-Manager for Linux (Rust)\n\nStart: task-manager\n--install       Graphical user installation\n--uninstall     Graphical uninstallation\n--check         Check system metrics without a window\n--docker-check  Check Docker without changes\n--apps-check    Check installed apps without changes\n--drivers-check Check driver inventory without changes\n--driver-updates-check  Check driver package updates online\n--optimize-check  Analyze optimization only\n--smoke-test    Check GUI and live metrics, then exit\n--screenshot PATH  Save screenshot during GUI test\n--performance   Open performance view\n--dark / --light  Dark / light appearance\n--view VIEW  cpu, memory, gpu, disk, network, system, processes, applications, background, docker, optimize, installed or drivers\n--version       Show version\n--language-check Show language and configuration path"
                    )
                );
                return Ok(());
            }
            "--install" => install = true,
            "--uninstall" => uninstall = true,
            "--smoke-test" => smoke = true,
            "--performance" => performance = true,
            "--dark" => dark = Some(true),
            "--light" => dark = Some(false),
            "--view" => {
                let value = args.next().ok_or(crate::tr!("Missing view after --view"))?;
                if ![
                    "cpu",
                    "memory",
                    "gpu",
                    "disk",
                    "network",
                    "system",
                    "processes",
                    "applications",
                    "background",
                    "docker",
                    "optimize",
                    "installed",
                    "drivers",
                ]
                .contains(&value.as_str())
                {
                    return Err(crate::tr!("View must be cpu, memory, gpu, disk, network, system, processes, applications, background, docker, optimize, installed or drivers").into());
                }
                view = Some(value);
            }
            "--screenshot" => {
                screenshot = Some(PathBuf::from(
                    args.next()
                        .ok_or(crate::tr!("Missing path after --screenshot"))?,
                ))
            }
            "--drivers-check" => {
                let s = drivers::sample(None);
                println!(
                    "{}",
                    crate::tf!(
                        "Drivers: {0} modules · Running kernel {1} · Installed kernels {2}",
                        s.modules.len(),
                        s.running,
                        s.kernels.join(", ")
                    )
                );
                for m in s.modules {
                    println!(
                        "{} · {} · {} · {}",
                        m.name,
                        m.state.label(),
                        m.source(),
                        m.memory
                            .map(|v| ui::size(v as f64))
                            .unwrap_or_else(|| "—".into())
                    );
                }
                for w in s.warnings {
                    eprintln!("{}", crate::tf!("Note: {0}", w));
                }
                return Ok(());
            }
            "--driver-updates-check" => {
                let result = drivers::check_updates(&std::sync::atomic::AtomicBool::new(false));
                if let Some(error) = result.error {
                    return Err(error.into());
                }
                println!(
                    "{}",
                    crate::tf!(
                        "Driver package updates: {0} · All package updates {1}",
                        result.list.len(),
                        result.all
                    )
                );
                for u in result.list {
                    println!("{} {} -> {} · {}", u.package, u.old, u.new, u.reason);
                }
                return Ok(());
            }
            "--apps-check" => {
                let snapshot = apps::sample();
                let list = snapshot
                    .apps
                    .iter()
                    .filter(|a| !a.extra)
                    .collect::<Vec<_>>();
                println!(
                    "{}",
                    crate::tf!(
                        "Installed apps / launchers: {0} · User {1} · System {2} · Additional packages {3}",
                        list.len(),
                        list.iter().filter(|a| a.scope == apps::Scope::User).count(),
                        list.iter()
                            .filter(|a| a.scope == apps::Scope::System)
                            .count(),
                        snapshot.apps.iter().filter(|a| a.extra).count()
                    )
                );
                for a in list {
                    println!(
                        "{} · {} · {} · {} · {}",
                        a.name,
                        a.version.as_deref().unwrap_or("—"),
                        a.source,
                        a.scope.label(),
                        a.size
                            .map(|n| ui::size(n as f64))
                            .unwrap_or_else(|| "—".into())
                    );
                }
                for warning in snapshot.warnings {
                    eprintln!("{}", crate::tf!("Note: {0}", warning));
                }
                return Ok(());
            }
            "--docker-check" => {
                let snapshot = docker::sample();
                if let Some(error) = snapshot.error {
                    return Err(error.into());
                }
                println!(
                    "{}",
                    crate::tf!(
                        "Docker: {0} containers · Context {1}",
                        snapshot.containers.len(),
                        snapshot.context
                    )
                );
                for c in snapshot.containers {
                    println!(
                        "{}",
                        crate::tf!(
                            "{0} · {1} · CPU (one core): {2} % · RAM: {3} · Tasks: {4}",
                            c.name,
                            c.state,
                            format!("{:?}", c.cpu),
                            format!("{:?}", c.memory),
                            format!("{:?}", c.pids)
                        )
                    );
                }
                return Ok(());
            }
            "--optimize-check" => {
                let mut sampler = monitor::Sampler::new()?;
                sampler.sample()?;
                std::thread::sleep(Duration::from_millis(250));
                let snapshot = sampler.sample()?;
                // SAFETY: getuid is a read-only call with no arguments.
                let scan =
                    optimizer::Scan::new(&snapshot, unsafe { libc::getuid() }, std::process::id());
                let total = optimizer::estimate(scan.candidates.iter().map(|c| &c.process));
                println!(
                    "{}",
                    crate::tf!(
                        "Analysis: {0} processes · {1} candidates · {2} protected/excluded",
                        snapshot.processes.len(),
                        scan.candidates.len(),
                        scan.excluded
                    )
                );
                println!(
                    "{}",
                    crate::tf!(
                        "Potential (snapshot): CPU {0} % · RSS upper bound {1}",
                        ui::decimal(total.cpu, 1),
                        ui::size(total.memory as f64)
                    )
                );
                for c in scan.candidates {
                    println!("{} ({}) · {}", c.process.name, c.process.pid, c.reason);
                }
                return Ok(());
            }
            "--check" => {
                let mut sampler = monitor::Sampler::new()?;
                sampler.sample()?;
                std::thread::sleep(Duration::from_millis(250));
                let snapshot = sampler.sample()?;
                if snapshot.processes.is_empty() || snapshot.memory_total == 0 {
                    return Err(crate::tr!("No system metrics available").into());
                }
                println!(
                    "{}",
                    crate::tf!(
                        "System metrics OK: {0} processes, {1} logical cores, CPU {2} %, RAM {3} / {4}",
                        snapshot.processes.len(),
                        snapshot.cores.len(),
                        ui::decimal(snapshot.cpu, 1),
                        ui::size(snapshot.memory_used as f64),
                        ui::size(snapshot.memory_total as f64)
                    )
                );
                println!(
                    "{}",
                    crate::tf!(
                        "Uptime: {0} · {1} disks · {2} GPUs · {3} temperature sensors",
                        ui::uptime(snapshot.uptime),
                        snapshot.disks.len(),
                        snapshot.gpus.len(),
                        snapshot.temperatures.len()
                    )
                );
                println!(
                    "{}",
                    crate::tf!(
                        "CPU: {0} physical cores · {1} logical processors · {2} processes · {3} threads · Clock (avg.): {4}",
                        snapshot
                            .physical_cores
                            .map(|n| n.to_string())
                            .unwrap_or_else(|| crate::tr!("unavailable").into()),
                        snapshot.cores.len(),
                        snapshot.processes.len(),
                        snapshot.thread_count(),
                        snapshot
                            .cpu_frequency
                            .as_ref()
                            .map(|f| crate::tf!(
                                "{0} GHz · {1} of {2} CPUs · {3}",
                                ui::decimal(f.mhz / 1000.0, 2),
                                f.measured_cpus,
                                snapshot.cores.len(),
                                f.source
                            ))
                            .unwrap_or_else(|| crate::tr!("unavailable").into())
                    )
                );
                println!(
                    "{}",
                    crate::tf!(
                        "RAM cache: {0} (Cached + SReclaimable)",
                        snapshot
                            .memory_cache
                            .map(|v| ui::size(v as f64))
                            .unwrap_or_else(|| crate::tr!("unavailable").into())
                    )
                );
                for d in &snapshot.disks {
                    println!(
                        "{}",
                        crate::tf!(
                            "Disk {0}: {1} · {2}",
                            d.id,
                            d.model,
                            ui::size(d.capacity as f64)
                        )
                    );
                    for volume in &d.volumes {
                        println!(
                            "{}",
                            crate::tf!(
                                "  {0}: {1} · Used {2} · Available {3} · FS total {4}{5}",
                                volume.source,
                                volume.mounts.join(", "),
                                ui::size(volume.used as f64),
                                ui::size(volume.available as f64),
                                ui::size(volume.total as f64),
                                if volume.shared {
                                    crate::tr!(" · shared")
                                } else {
                                    ""
                                }
                            )
                        );
                    }
                }
                for g in &snapshot.gpus {
                    println!("GPU {}: {} · {}", g.id, g.name, g.driver);
                    println!(
                        "{}",
                        crate::tf!(
                            "  Clock: {0} · Power draw: {1} · {2}",
                            g.clock_mhz
                                .map(|v| format!("{} MHz", ui::decimal(v, 0)))
                                .unwrap_or_else(|| crate::tr!("unavailable").into()),
                            g.power_watts
                                .map(|v| format!("{} W", ui::decimal(v, 1)))
                                .unwrap_or_else(|| crate::tr!("unavailable").into()),
                            g.power_source
                        )
                    );
                }
                let apps = snapshot
                    .processes
                    .iter()
                    .filter(|p| p.application.is_some())
                    .count();
                println!(
                    "{}",
                    crate::tf!(
                        "Process groups: {0} applications · {1} background processes · {2}",
                        apps,
                        snapshot.processes.len() - apps,
                        snapshot.application_source
                    )
                );
                for app in snapshot
                    .processes
                    .iter()
                    .filter(|p| p.application.is_some())
                    .take(20)
                {
                    println!(
                        "{}",
                        crate::tf!(
                            "Application {0}: {1} ({2})",
                            app.pid,
                            app.application.as_deref().unwrap_or(&app.name),
                            app.name
                        )
                    );
                }
                return Ok(());
            }
            _ => return Err(crate::tf!("Unknown option: {0}. See --help.", arg).into()),
        }
    }
    if screenshot.is_some() && !smoke {
        return Err(crate::tr!("--screenshot requires --smoke-test").into());
    }
    if smoke && (install || uninstall) {
        return Err(
            crate::tr!("Installation and uninstallation are disabled in GUI tests.").into(),
        );
    }
    let window_size = if smoke {
        std::env::var("TASK_MANAGER_SMOKE_SIZE")
            .ok()
            .and_then(|s| {
                let (w, h) = s.split_once('x')?;
                Some([
                    w.parse::<f32>().ok()?.clamp(850.0, 2000.0),
                    h.parse::<f32>().ok()?.clamp(590.0, 1200.0),
                ])
            })
            .unwrap_or([1180.0, 820.0])
    } else {
        [1180.0, 820.0]
    };
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Task-Manager")
            .with_app_id(install::APP_ID)
            .with_inner_size(window_size)
            .with_min_inner_size([850.0, 590.0])
            .with_icon(std::sync::Arc::new(app_icon())),
        renderer: eframe::Renderer::Glow,
        persist_window: false,
        ..Default::default()
    };
    eframe::run_native(
        install::APP_ID,
        options,
        Box::new(move |cc| {
            Ok(Box::new(ui::TaskApp::new(
                cc,
                ui::Startup {
                    smoke,
                    screenshot,
                    install,
                    uninstall,
                    performance,
                    dark,
                    view,
                },
            )))
        }),
    )?;
    Ok(())
}

fn app_icon() -> eframe::egui::IconData {
    let width = 64;
    let mut rgba = vec![0; width * width * 4];
    for y in 5..50 {
        for x in 4..60 {
            let inner = (9..55).contains(&x) && (10..45).contains(&y);
            let color = if inner {
                [234, 246, 255, 255]
            } else {
                [8, 121, 207, 255]
            };
            rgba[(y * width + x) * 4..(y * width + x) * 4 + 4].copy_from_slice(&color);
        }
    }
    for y in 50..60 {
        for x in 23..41 {
            rgba[(y * width + x) * 4..(y * width + x) * 4 + 4].copy_from_slice(&[8, 121, 207, 255]);
        }
    }
    for x in 12..53 {
        let y = match x {
            12..=22 => 30,
            23..=29 => 30 - (x - 23) * 2,
            30..=39 => 16 + (x - 30) * 2,
            40..=47 => 34 - (x - 40) * 2,
            _ => 20 + (x - 48) * 2,
        };
        for dy in 0..3 {
            rgba[((y + dy) * width + x) * 4..((y + dy) * width + x) * 4 + 4]
                .copy_from_slice(&[8, 121, 207, 255]);
        }
    }
    eframe::egui::IconData {
        rgba,
        width: width as u32,
        height: width as u32,
    }
}
