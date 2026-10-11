//! Linux /proc sampling. Process actions use pidfds, never shell commands.
use crate::cpu::{self, Frequency};
use crate::hardware::{self, Disk, Gpu, Hardware, Temperature};
use crate::windows::WindowTracker;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct ProcessRow {
    pub pid: u32,
    pub started: u64,
    pub parent_pid: u32,
    pub cgroup: Option<String>,
    pub name: String,
    pub status: String,
    pub cpu: f64,
    pub memory: u64,
    pub io_rate: Option<f64>,
    pub uid: Option<u32>,
    pub user: String,
    pub threads: u64,
    pub command: String,
    pub executable: Option<String>,
    pub application: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub cpu: f64,
    pub cores: Vec<f64>,
    pub physical_cores: Option<usize>,
    pub cpu_frequency: Option<Frequency>,
    pub core_ids: Vec<usize>,
    pub core_temperatures: Vec<Option<f64>>,
    pub memory_used: u64,
    pub memory_total: u64,
    pub memory_available: u64,
    pub memory_cache: Option<u64>,
    pub swap_used: u64,
    pub swap_total: u64,
    pub disks: Vec<Disk>,
    pub gpus: Vec<Gpu>,
    pub temperatures: Vec<Temperature>,
    pub cpu_temperatures: Vec<Temperature>,
    pub disk_read: f64,
    pub disk_write: f64,
    pub network_recv: f64,
    pub network_sent: f64,
    pub uptime: u64,
    pub processes: Vec<ProcessRow>,
    pub skipped: usize,
    pub application_source: String,
}

impl Snapshot {
    pub fn thread_count(&self) -> u64 {
        self.processes
            .iter()
            .fold(0, |sum, p| sum.saturating_add(p.threads))
    }

    pub fn memory_percent(&self) -> f64 {
        self.memory_used as f64 / self.memory_total.max(1) as f64 * 100.0
    }
}

#[derive(Clone, Copy, Default)]
struct CpuCounters {
    id: usize,
    total: u64,
    idle: u64,
}

impl CpuCounters {
    fn usage(self, previous: Self) -> f64 {
        let total = self.total.saturating_sub(previous.total);
        if total == 0 {
            return 0.0;
        }
        (total.saturating_sub(self.idle.saturating_sub(previous.idle)) as f64 / total as f64
            * 100.0)
            .clamp(0.0, 100.0)
    }
}

#[derive(Debug)]
struct ProcessStat {
    parent_pid: u32,
    name: String,
    state: char,
    cpu_ticks: u64,
    started: u64,
    threads: u64,
    resident_pages: u64,
}

fn number(text: &str) -> u64 {
    text.parse().unwrap_or(0)
}

fn parse_process_stat(text: &str) -> io::Result<ProcessStat> {
    let begin = text
        .find('(')
        .ok_or_else(|| io::Error::other(crate::tr!("Missing process name")))?;
    let end = text
        .rfind(')')
        .ok_or_else(|| io::Error::other(crate::tr!("Incomplete process name")))?;
    if end <= begin {
        return Err(io::Error::other(crate::tr!("Invalid process name")));
    }
    let values: Vec<_> = text[end + 1..].split_whitespace().collect();
    if values.len() < 22 {
        return Err(io::Error::other(crate::tr!("Incomplete process data")));
    }
    Ok(ProcessStat {
        parent_pid: values[1]
            .parse()
            .map_err(|_| io::Error::other(crate::tr!("Missing parent process")))?,
        name: text[begin + 1..end].to_owned(),
        state: values[0].chars().next().unwrap_or('?'),
        cpu_ticks: number(values[11]).saturating_add(number(values[12])),
        started: number(values[19]),
        threads: number(values[17]),
        resident_pages: values[21].parse::<i64>().unwrap_or(0).max(0) as u64,
    })
}

pub fn process_identity(pid: u32) -> io::Result<u64> {
    Ok(parse_process_stat(&fs::read_to_string(format!("/proc/{pid}/stat"))?)?.started)
}

/// Refresh security metadata immediately before an optimizer action.
pub fn revalidate_process(row: &ProcessRow) -> io::Result<ProcessRow> {
    let directory = std::path::PathBuf::from(format!("/proc/{}", row.pid));
    let stat = parse_process_stat(&fs::read_to_string(directory.join("stat"))?)?;
    if stat.started != row.started {
        return Err(io::Error::other(crate::tr!(
            "Process identity changed; please scan again."
        )));
    }
    let mut current = row.clone();
    current.parent_pid = stat.parent_pid;
    current.cgroup = Some(fs::read_to_string(directory.join("cgroup"))?);
    current.uid = fs::read_to_string(directory.join("status"))?
        .lines()
        .find_map(|line| {
            let ids = line
                .strip_prefix("Uid:")?
                .split_whitespace()
                .map(str::parse::<u32>)
                .collect::<Result<Vec<_>, _>>()
                .ok()?;
            // Reject changed/elevated effective, saved or filesystem credentials.
            (ids.len() == 4 && ids.iter().all(|id| *id == ids[0])).then_some(ids[0])
        });
    current.command = String::from_utf8_lossy(&fs::read(directory.join("cmdline"))?)
        .replace('\0', " ")
        .trim()
        .into();
    current.name = stat.name;
    current.status = state_name(stat.state).into();
    Ok(current)
}

fn cpu_counters() -> io::Result<Vec<CpuCounters>> {
    let text = fs::read_to_string("/proc/stat")?;
    let mut result = Vec::new();
    for line in text.lines().filter(|line| line.starts_with("cpu")) {
        let values: Vec<u64> = line
            .split_whitespace()
            .skip(1)
            .take(8)
            .map(number)
            .collect();
        if values.len() >= 5 {
            result.push(CpuCounters {
                id: line
                    .split_whitespace()
                    .next()
                    .and_then(|n| n.strip_prefix("cpu"))
                    .and_then(|n| n.parse().ok())
                    .unwrap_or(usize::MAX),
                total: values.iter().sum(),
                idle: values[3] + values[4],
            });
        }
    }
    if result.is_empty() {
        return Err(io::Error::other(crate::tr!("CPU counters unavailable")));
    }
    Ok(result)
}

fn memory_counters() -> io::Result<HashMap<String, u64>> {
    let text = fs::read_to_string("/proc/meminfo")?;
    Ok(parse_memory_counters(&text))
}

fn parse_memory_counters(text: &str) -> HashMap<String, u64> {
    text.lines()
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            Some((
                key.to_owned(),
                value
                    .split_whitespace()
                    .next()?
                    .parse::<u64>()
                    .ok()?
                    .checked_mul(1024)?,
            ))
        })
        .collect()
}

fn cached_memory(memory: &HashMap<String, u64>) -> Option<u64> {
    Some(
        memory
            .get("Cached")?
            .saturating_add(*memory.get("SReclaimable").unwrap_or(&0)),
    )
}

fn network_counters() -> io::Result<(u64, u64)> {
    let text = fs::read_to_string("/proc/net/dev")?;
    let mut recv = 0u64;
    let mut sent = 0u64;
    for line in text.lines() {
        if let Some((name, data)) = line.split_once(':') {
            let values: Vec<_> = data.split_whitespace().collect();
            if name.trim() != "lo" && values.len() >= 9 {
                recv = recv.saturating_add(number(values[0]));
                sent = sent.saturating_add(number(values[8]));
            }
        }
    }
    Ok((recv, sent))
}

fn io_bytes(pid: u32) -> Option<u64> {
    let text = fs::read_to_string(format!("/proc/{pid}/io")).ok()?;
    let mut total = 0u64;
    for line in text.lines() {
        if let Some((key, value)) = line.split_once(':')
            && matches!(key, "read_bytes" | "write_bytes")
        {
            total = total.saturating_add(number(value.trim()));
        }
    }
    Some(total)
}

fn state_name(state: char) -> &'static str {
    match state {
        'R' => "Wird ausgeführt",
        'S' => "Bereit",
        'D' => "Wartet auf E/A",
        'T' | 't' => "Angehalten",
        'Z' => "Zombie",
        'X' | 'x' => "Beendet",
        'I' => "Inaktiv",
        'P' => "Geparkt",
        _ => "Unbekannt",
    }
}

pub struct Sampler {
    previous_cpu: Vec<CpuCounters>,
    previous_process: HashMap<(u32, u64), (u64, Option<u64>)>,
    hardware: Hardware,
    windows: WindowTracker,
    previous_network: (u64, u64),
    previous_time: Instant,
    page_size: u64,
    users: HashMap<u32, String>,
}

impl Sampler {
    pub fn new() -> io::Result<Self> {
        let users = fs::read_to_string("/etc/passwd")
            .unwrap_or_default()
            .lines()
            .filter_map(|line| {
                let values: Vec<_> = line.split(':').collect();
                Some((values.get(2)?.parse().ok()?, values.first()?.to_string()))
            })
            .collect();
        // SAFETY: sysconf takes a constant key and has no pointer arguments.
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) }.max(1) as u64;
        Ok(Self {
            previous_cpu: cpu_counters()?,
            previous_process: HashMap::new(),
            hardware: Hardware::new(),
            windows: WindowTracker::new(),
            previous_network: network_counters()?,
            previous_time: Instant::now(),
            page_size,
            users,
        })
    }

    pub fn sample(&mut self) -> io::Result<Snapshot> {
        let now = Instant::now();
        let seconds = now
            .duration_since(self.previous_time)
            .as_secs_f64()
            .max(0.001);
        let cpu = cpu_counters()?;
        let delta_cpu = cpu[0].total.saturating_sub(self.previous_cpu[0].total);
        let memory = memory_counters()?;
        let (disks, gpus, temperatures) = self.hardware.sample(seconds);
        let network = network_counters()?;
        let applications = self.windows.sample();
        // SAFETY: getuid has no arguments or side effects.
        let current_uid = unsafe { libc::getuid() };
        let mut processes = Vec::new();
        let mut next_process = HashMap::new();
        let mut skipped = 0;
        for entry in fs::read_dir("/proc")? {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };
            let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
                continue;
            };
            let Ok(stat) =
                fs::read_to_string(entry.path().join("stat")).and_then(|s| parse_process_stat(&s))
            else {
                skipped += 1;
                continue;
            };
            let key = (pid, stat.started);
            let io = io_bytes(pid);
            let previous = self.previous_process.get(&key);
            let usage = previous
                .map(|p| {
                    stat.cpu_ticks.saturating_sub(p.0) as f64 / delta_cpu.max(1) as f64 * 100.0
                })
                .unwrap_or(0.0)
                .clamp(0.0, 100.0);
            let io_rate = io
                .zip(previous.and_then(|p| p.1))
                .map(|(current, old)| current.saturating_sub(old) as f64 / seconds);
            next_process.insert(key, (stat.cpu_ticks, io));
            let status = fs::read_to_string(entry.path().join("status")).unwrap_or_default();
            let uid = status.lines().find_map(|line| {
                line.strip_prefix("Uid:")
                    .and_then(|values| values.split_whitespace().next()?.parse::<u32>().ok())
            });
            let user = uid
                .map(|id| {
                    self.users
                        .get(&id)
                        .cloned()
                        .unwrap_or_else(|| id.to_string())
                })
                .unwrap_or_else(|| "—".to_owned());
            let command = fs::read(entry.path().join("cmdline"))
                .map(|bytes| {
                    String::from_utf8_lossy(&bytes)
                        .replace('\0', " ")
                        .trim()
                        .to_owned()
                })
                .unwrap_or_default();
            // Linux's comm field is limited to 15 bytes. Extend it using the
            // actual executable when the full basename agrees with that prefix.
            let executable = fs::read_link(entry.path().join("exe")).ok();
            let name = executable
                .as_ref()
                .and_then(|path| {
                    path.file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                })
                .filter(|name| stat.name.len() >= 15 && name.starts_with(&stat.name))
                .unwrap_or(stat.name);
            // Keep signals tied to this sampled identity; application membership is display metadata only.
            let application = if applications.exact {
                applications.names.get(&pid).map(|app| {
                    if app.is_empty() {
                        name.clone()
                    } else {
                        app.clone()
                    }
                })
            } else {
                self.windows
                    .fallback_name(&name, &command, uid == Some(current_uid))
            };
            processes.push(ProcessRow {
                pid,
                started: stat.started,
                parent_pid: stat.parent_pid,
                cgroup: fs::read_to_string(entry.path().join("cgroup")).ok(),
                name,
                status: state_name(stat.state).to_owned(),
                cpu: usage,
                memory: stat.resident_pages.saturating_mul(self.page_size),
                io_rate,
                uid,
                user,
                threads: stat.threads,
                command,
                executable: executable.map(|p| p.to_string_lossy().into_owned()),
                application,
            });
        }
        let total_memory = *memory.get("MemTotal").unwrap_or(&0);
        let available = *memory.get("MemAvailable").unwrap_or(&0);
        let total_swap = *memory.get("SwapTotal").unwrap_or(&0);
        let online: Vec<_> = cpu.iter().skip(1).map(|c| c.id).collect();
        let cpu_details = cpu::sample(
            std::path::Path::new("/sys"),
            &fs::read_to_string("/proc/cpuinfo").unwrap_or_default(),
            &online,
        );
        let snapshot = Snapshot {
            cpu: cpu[0].usage(self.previous_cpu[0]),
            physical_cores: cpu_details.physical_cores,
            cpu_frequency: cpu_details.frequency,
            cores: cpu
                .iter()
                .skip(1)
                .map(|counter| {
                    counter.usage(
                        *self
                            .previous_cpu
                            .iter()
                            .find(|p| p.id == counter.id)
                            .unwrap_or(counter),
                    )
                })
                .collect(),
            core_ids: cpu.iter().skip(1).map(|c| c.id).collect(),
            core_temperatures: cpu
                .iter()
                .skip(1)
                .map(|c| {
                    hardware::core_temperature(std::path::Path::new("/sys"), c.id, &temperatures)
                })
                .collect(),
            memory_used: total_memory.saturating_sub(available),
            memory_total: total_memory,
            memory_available: available,
            memory_cache: cached_memory(&memory),
            swap_total: total_swap,
            swap_used: total_swap.saturating_sub(*memory.get("SwapFree").unwrap_or(&0)),
            disk_read: disks.iter().map(|d| d.read).sum(),
            disk_write: disks.iter().map(|d| d.write).sum(),
            disks,
            gpus,
            cpu_temperatures: hardware::cpu_temperatures(&temperatures),
            temperatures,
            network_recv: network.0.saturating_sub(self.previous_network.0) as f64 / seconds,
            network_sent: network.1.saturating_sub(self.previous_network.1) as f64 / seconds,
            uptime: fs::read_to_string("/proc/uptime")
                .ok()
                .and_then(|s| s.split_whitespace().next()?.parse::<f64>().ok())
                .unwrap_or(0.0) as u64,
            processes,
            skipped,
            application_source: applications.source.into(),
        };
        self.previous_cpu = cpu;
        self.previous_process = next_process;
        self.previous_network = network;
        self.previous_time = now;
        Ok(snapshot)
    }
}

pub fn end_process(pid: u32, started: u64, force: bool) -> io::Result<()> {
    if pid <= 1 || pid == std::process::id() {
        return Err(io::Error::other(crate::tr!("This process is protected.")));
    }
    // SAFETY: pidfd_open returns an owned file descriptor or -1. No pointers.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid as libc::pid_t, 0u32) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the successful syscall above returned a new descriptor owned by us.
    let descriptor = unsafe { OwnedFd::from_raw_fd(fd as i32) };
    if process_identity(pid)? != started {
        return Err(io::Error::other(crate::tr!(
            "The original process no longer exists. Please refresh the list."
        )));
    }
    // SAFETY: descriptor is live; a null siginfo pointer requests the default signal.
    // Using a pidfd prevents signaling a different process if the PID gets reused.
    let result = unsafe {
        libc::syscall(
            libc::SYS_pidfd_send_signal,
            descriptor.as_raw_fd(),
            if force { libc::SIGKILL } else { libc::SIGTERM },
            std::ptr::null::<libc::siginfo_t>(),
            0u32,
        )
    };
    if result < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub enum Control {
    Pause(bool),
    Interval(Duration),
    Refresh,
    Stop,
}

pub struct Monitor {
    pub control: Sender<Control>,
    pub receiver: Receiver<Result<Snapshot, String>>,
    thread: Option<JoinHandle<()>>,
}

impl Monitor {
    pub fn start(ctx: eframe::egui::Context) -> Self {
        let (control, commands) = mpsc::channel();
        let (sender, receiver) = mpsc::sync_channel(1);
        let thread = thread::spawn(move || {
            let mut sampler = match Sampler::new() {
                Ok(sampler) => sampler,
                Err(error) => {
                    let _ = sender.send(Err(error.to_string()));
                    ctx.request_repaint();
                    return;
                }
            };
            let mut interval = Duration::from_secs(1);
            let mut paused = false;
            loop {
                if !paused {
                    let result = sampler.sample().map_err(|e| e.to_string());
                    match sender.try_send(result) {
                        Ok(()) | Err(mpsc::TrySendError::Full(_)) => ctx.request_repaint(),
                        Err(mpsc::TrySendError::Disconnected(_)) => break,
                    }
                }
                match commands.recv_timeout(interval) {
                    Ok(Control::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Ok(Control::Pause(value)) => paused = value,
                    Ok(Control::Interval(value)) => interval = value,
                    Ok(Control::Refresh) => paused = false,
                    Err(mpsc::RecvTimeoutError::Timeout) => (),
                }
            }
        });
        Self {
            control,
            receiver,
            thread: Some(thread),
        }
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        let _ = self.control.send(Control::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub fn cpu_name() -> String {
    fs::read_to_string("/proc/cpuinfo")
        .unwrap_or_default()
        .lines()
        .find_map(|line| {
            line.strip_prefix("model name")
                .and_then(|s| s.split_once(':'))
                .map(|(_, s)| s.trim().to_owned())
        })
        .unwrap_or_else(|| std::env::consts::ARCH.to_owned())
}

pub fn distribution() -> String {
    fs::read_to_string("/etc/os-release")
        .unwrap_or_default()
        .lines()
        .find_map(|line| {
            line.strip_prefix("PRETTY_NAME=")
                .map(|s| s.trim_matches('"').to_owned())
        })
        .unwrap_or_else(|| "Linux".to_owned())
}

pub fn text_file(path: &str) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|_| "—".to_owned())
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Child, Command};

    struct ChildGuard(Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[test]
    fn stat_handles_spaces_and_closing_parentheses() {
        let mut fields = vec!["0"; 22];
        fields[0] = "S";
        fields[11] = "20";
        fields[12] = "10";
        fields[17] = "7";
        fields[19] = "123456789";
        fields[21] = "42";
        let stat =
            parse_process_stat(&format!("9 (name with ) brackets) {}", fields.join(" "))).unwrap();
        assert_eq!(stat.name, "name with ) brackets");
        assert_eq!(
            (
                stat.cpu_ticks,
                stat.started,
                stat.threads,
                stat.resident_pages
            ),
            (30, 123456789, 7, 42)
        );
        assert!(parse_process_stat("9 broken").is_err());
    }

    #[test]
    fn memory_cache_units_missing_values_and_reclaimable_slab() {
        let memory = parse_memory_counters(
            "MemTotal: 10000 kB\nCached: 2000 kB\nSReclaimable: 300 kB\nBuffers: 50 kB\nShmem: 400 kB\nSwapCached: 100 kB\n",
        );
        assert_eq!(cached_memory(&memory), Some(2300 * 1024));
        // Shared memory is already inside Cached; buffers and SwapCached are separate.
        assert_eq!(memory["Cached"], 2000 * 1024);
        assert_eq!(
            cached_memory(&parse_memory_counters("Cached: 42 kB\n")),
            Some(42 * 1024)
        );
        assert_eq!(
            cached_memory(&parse_memory_counters("Cached: 0 kB\n")),
            Some(0)
        );
        assert_eq!(
            cached_memory(&parse_memory_counters(
                "Cached: unknown kB\nSReclaimable: 300 kB\n"
            )),
            None
        );
        assert_eq!(
            cached_memory(&parse_memory_counters("Cached: 18446744073709551615 kB\n")),
            None
        );
    }

    #[test]
    fn real_snapshot_contains_our_process() {
        let mut sampler = Sampler::new().unwrap();
        sampler.sample().unwrap();
        thread::sleep(Duration::from_millis(100));
        let sample = sampler.sample().unwrap();
        assert!(sample.memory_total > 0 && !sample.cores.is_empty());
        assert!((0.0..=100.0).contains(&sample.cpu));
        assert!(sample.processes.iter().any(|p| p.pid == std::process::id()));
        assert!(sample.thread_count() >= sample.processes.len() as u64);
        if let Some(physical) = sample.physical_cores {
            assert!(physical > 0 && physical <= sample.cores.len());
        }
        if let Some(frequency) = sample.cpu_frequency {
            assert!(frequency.mhz.is_finite() && frequency.mhz > 0.0);
        }
    }

    #[test]
    fn pidfd_rejects_stale_identity_then_signals_only_our_child() {
        for force in [false, true] {
            let mut child = ChildGuard(Command::new("sleep").arg("30").spawn().unwrap());
            let pid = child.0.id();
            let started = process_identity(pid).unwrap();
            assert!(end_process(pid, started + 1, force).is_err());
            assert!(child.0.try_wait().unwrap().is_none());
            end_process(pid, started, force).unwrap();
            assert!(!child.0.wait().unwrap().success());
        }
    }

    #[test]
    fn protects_self_and_init() {
        for pid in [0, 1, std::process::id()] {
            assert!(end_process(pid, 0, true).is_err());
        }
    }
}
