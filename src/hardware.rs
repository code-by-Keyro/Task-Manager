//! Read-only hardware discovery. Missing driver metrics remain explicitly unavailable.
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct Temperature {
    pub chip: String,
    pub label: String,
    pub celsius: f64,
    pub device: PathBuf,
}
#[derive(Clone, Debug)]
pub struct Disk {
    pub volumes: Vec<crate::storage::Volume>,
    pub id: String,
    pub model: String,
    pub capacity: u64,
    pub kind: String,
    pub read: f64,
    pub write: f64,
    pub temperatures: Vec<Temperature>,
}
#[derive(Clone, Debug)]
pub struct Gpu {
    pub id: String,
    pub name: String,
    pub driver: String,
    pub usage: Option<f64>,
    pub memory_used: Option<u64>,
    pub memory_total: Option<u64>,
    pub clock_mhz: Option<f64>,
    pub power_watts: Option<f64>,
    pub power_source: &'static str,
    pub temperatures: Vec<Temperature>,
}

fn text(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path)
        .unwrap_or_default()
        .trim()
        .to_owned()
}
fn integer(path: impl AsRef<Path>) -> Option<u64> {
    text(path).parse().ok()
}
fn entries(path: impl AsRef<Path>) -> Vec<PathBuf> {
    let mut paths: Vec<_> = fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .collect();
    paths.sort();
    paths
}
fn temperature(value: &str) -> Option<f64> {
    let value = value.trim().parse::<f64>().ok()? / 1000.0;
    (value.is_finite() && (-273.15..=300.0).contains(&value)).then_some(value)
}

pub fn temperatures(sys: &Path) -> Vec<Temperature> {
    let mut result = Vec::new();
    for hwmon in entries(sys.join("class/hwmon")) {
        let chip = text(hwmon.join("name"));
        let device = fs::canonicalize(hwmon.join("device"))
            .or_else(|_| fs::canonicalize(&hwmon))
            .unwrap_or_default();
        for input in entries(&hwmon) {
            let Some(name) = input.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let Some(channel) = name
                .strip_prefix("temp")
                .and_then(|n| n.strip_suffix("_input"))
            else {
                continue;
            };
            if channel.parse::<u32>().is_err()
                || integer(hwmon.join(format!("temp{channel}_fault"))).is_some_and(|n| n != 0)
            {
                continue;
            }
            let Some(celsius) = temperature(&text(&input)) else {
                continue;
            };
            let label = text(hwmon.join(format!("temp{channel}_label")));
            result.push(Temperature {
                chip: chip.clone(),
                label: if label.is_empty() {
                    format!("Sensor {channel}")
                } else {
                    label
                },
                celsius,
                device: device.clone(),
            });
        }
    }
    // Zones complement hwmon on machines whose firmware only exposes thermal zones.
    for zone in entries(sys.join("class/thermal")) {
        if !zone
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with("thermal_zone"))
        {
            continue;
        }
        let chip = text(zone.join("type"));
        let Some(celsius) = temperature(&text(zone.join("temp"))) else {
            continue;
        };
        if !result
            .iter()
            .any(|t| t.chip == chip && (t.celsius - celsius).abs() < 0.1)
        {
            result.push(Temperature {
                chip,
                label: crate::tr!("Thermal zone").into(),
                celsius,
                device: fs::canonicalize(&zone).unwrap_or_default(),
            });
        }
    }
    result
}

pub fn cpu_temperatures(all: &[Temperature]) -> Vec<Temperature> {
    all.iter()
        .filter(|t| {
            matches!(
                t.chip.as_str(),
                "coretemp"
                    | "k10temp"
                    | "zenpower"
                    | "cpu_thermal"
                    | "cpu-thermal"
                    | "peci_cputemp"
            ) || t.chip.starts_with("cpu")
        })
        .cloned()
        .collect()
}
pub fn core_temperature(sys: &Path, cpu_id: usize, all: &[Temperature]) -> Option<f64> {
    let topology = sys.join(format!("devices/system/cpu/cpu{cpu_id}/topology"));
    let core = integer(topology.join("core_id"))?;
    let package = integer(topology.join("physical_package_id"))?;
    all.iter()
        .find(|t| {
            t.chip == "coretemp"
                && t.label == format!("Core {core}")
                && t.device
                    .file_name()
                    .is_some_and(|n| n == format!("coretemp.{package}").as_str())
        })
        .map(|t| t.celsius)
}

fn device_temperatures(all: &[Temperature], device: &Path) -> Vec<Temperature> {
    let Ok(device) = fs::canonicalize(device) else {
        return Vec::new();
    };
    all.iter()
        .filter(|t| !t.device.as_os_str().is_empty() && t.device.starts_with(&device))
        .cloned()
        .collect()
}

pub type DiskCounters = HashMap<String, (u64, u64)>;
pub fn disk_counters(stats: &str) -> DiskCounters {
    stats
        .lines()
        .filter_map(|line| {
            let v: Vec<_> = line.split_whitespace().collect();
            if v.len() < 10 {
                return None;
            }
            Some((
                v[2].to_owned(),
                (
                    v[5].parse::<u64>().ok()?.saturating_mul(512),
                    v[9].parse::<u64>().ok()?.saturating_mul(512),
                ),
            ))
        })
        .collect()
}
pub fn disks(
    sys: &Path,
    current: &DiskCounters,
    previous: &DiskCounters,
    seconds: f64,
    sensors: &[Temperature],
) -> Vec<Disk> {
    entries(sys.join("block"))
        .into_iter()
        .filter_map(|path| {
            // /sys/block contains whole devices, not their partitions. Omit RAM/loop/stacked virtual devices.
            if !path.join("device").exists() {
                return None;
            }
            let id = path.file_name()?.to_str()?.to_owned();
            let model = text(path.join("device/model"));
            let capacity = integer(path.join("size")).unwrap_or(0).saturating_mul(512);
            let now = current.get(&id).copied();
            let rates = now
                .zip(previous.get(&id).copied())
                .map(|(n, p)| {
                    (
                        n.0.saturating_sub(p.0) as f64 / seconds.max(0.001),
                        n.1.saturating_sub(p.1) as f64 / seconds.max(0.001),
                    )
                })
                .unwrap_or_default();
            let kind = if id.starts_with("nvme") {
                "NVMe SSD"
            } else if integer(path.join("queue/rotational")) == Some(1) {
                "HDD"
            } else {
                crate::tr!("SSD / non-rotating")
            };
            Some(Disk {
                volumes: Vec::new(),
                id,
                model,
                capacity,
                kind: kind.into(),
                read: rates.0,
                write: rates.1,
                temperatures: device_temperatures(sensors, &path.join("device")),
            })
        })
        .collect()
}

fn pci_name(ids: &str, vendor: &str, device: &str) -> Option<String> {
    let mut in_vendor = false;
    let mut vendor_name = String::new();
    for line in ids.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        if !line.starts_with('\t') {
            if in_vendor {
                break;
            }
            if let Some(name) = line.strip_prefix(vendor).and_then(|s| s.strip_prefix("  ")) {
                in_vendor = true;
                vendor_name = name.to_owned();
            }
        } else if in_vendor
            && let Some(name) = line
                .strip_prefix('\t')
                .and_then(|s| s.strip_prefix(device))
                .and_then(|s| s.strip_prefix("  "))
        {
            return Some(format!("{vendor_name} {name}"));
        }
    }
    None
}

pub struct Hardware {
    pci_ids: String,
    previous_disks: DiskCounters,
    nvidia: Option<PathBuf>,
    disk_usage: HashMap<String, Vec<crate::storage::Volume>>,
    usage_at: Option<Instant>,
}
impl Hardware {
    pub fn new() -> Self {
        let pci_ids = ["/usr/share/hwdata/pci.ids", "/usr/share/misc/pci.ids"]
            .into_iter()
            .find_map(|p| fs::read_to_string(p).ok())
            .unwrap_or_default();
        let nvidia = ["/usr/bin/nvidia-smi", "/bin/nvidia-smi"]
            .into_iter()
            .map(PathBuf::from)
            .find(|p| p.is_file());
        Self {
            pci_ids,
            previous_disks: disk_counters(&text("/proc/diskstats")),
            nvidia,
            disk_usage: HashMap::new(),
            usage_at: None,
        }
    }
    pub fn sample(&mut self, seconds: f64) -> (Vec<Disk>, Vec<Gpu>, Vec<Temperature>) {
        let sys = Path::new("/sys");
        let sensors = temperatures(sys);
        let counters = disk_counters(&text("/proc/diskstats"));
        let mut disks = disks(sys, &counters, &self.previous_disks, seconds, &sensors);
        if self
            .usage_at
            .is_none_or(|time| time.elapsed() >= Duration::from_secs(5))
            || disks.iter().any(|d| !self.disk_usage.contains_key(&d.id))
        {
            let ids = disks.iter().map(|d| d.id.clone()).collect::<Vec<_>>();
            self.disk_usage = crate::storage::collect(
                sys,
                &text("/proc/self/mountinfo"),
                &ids,
                crate::storage::read_usage,
            );
            self.usage_at = Some(Instant::now());
        }
        for disk in &mut disks {
            disk.volumes = self.disk_usage.get(&disk.id).cloned().unwrap_or_default();
        }
        self.previous_disks = counters;
        let mut gpus = discover_gpus(sys, &self.pci_ids, &sensors);
        if gpus.iter().any(|g| g.driver == "nvidia")
            && let Some(command) = &self.nvidia
            && let Some(output) = nvidia_query(command)
        {
            apply_nvidia(&mut gpus, &output);
        }
        (disks, gpus, sensors)
    }
}

fn active_gpu_clock(table: &str) -> Option<f64> {
    table.lines().find_map(|line| {
        if !line.split_whitespace().any(|part| part == "*") {
            return None;
        }
        let (_, value) = line.split_once(':')?;
        let frequency = value.split_whitespace().next()?.to_ascii_lowercase();
        non_negative(frequency.strip_suffix("mhz")?)
    })
}

fn non_negative(value: &str) -> Option<f64> {
    value
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|n| n.is_finite() && *n >= 0.0)
}

fn gpu_metrics(device: &Path, driver: &str) -> (Option<f64>, Option<f64>, &'static str) {
    if !matches!(driver, "amdgpu" | "radeon") {
        return (None, None, crate::tr!("unavailable"));
    }
    // Only sensors belonging to this GPU; never substitute a power cap or maximum clock.
    let monitors = entries(device.join("hwmon"));
    let clock = monitors
        .iter()
        .find_map(|hwmon| integer(hwmon.join("freq1_input")).map(|hz| hz as f64 / 1_000_000.0))
        .or_else(|| active_gpu_clock(&text(device.join("pp_dpm_sclk"))));
    for (file, source) in [
        ("power1_input", crate::tr!("GPU/SoC instantaneous value")),
        ("power1_average", crate::tr!("GPU/SoC average value")),
    ] {
        if let Some(power) = monitors.iter().find_map(|hwmon| integer(hwmon.join(file))) {
            return (clock, Some(power as f64 / 1_000_000.0), source);
        }
    }
    (clock, None, crate::tr!("unavailable"))
}

pub fn discover_gpus(sys: &Path, pci_ids: &str, sensors: &[Temperature]) -> Vec<Gpu> {
    // PCI discovery also finds GPUs without a DRM card (e.g. proprietary NVIDIA).
    let mut devices: Vec<_> = entries(sys.join("bus/pci/devices"))
        .into_iter()
        .filter(|p| text(p.join("class")).starts_with("0x03"))
        .collect();
    for card in entries(sys.join("class/drm")) {
        let Some(name) = card.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name
            .strip_prefix("card")
            .is_some_and(|n| n.parse::<u32>().is_ok())
            && let Ok(device) = fs::canonicalize(card.join("device"))
            && !devices
                .iter()
                .any(|p| fs::canonicalize(p).ok().as_ref() == Some(&device))
        {
            devices.push(device);
        }
    }
    let mut gpus: Vec<_> = devices
        .into_iter()
        .filter_map(|device| {
            let canonical = fs::canonicalize(&device).ok()?;
            let id = canonical.file_name()?.to_str()?.to_owned();
            let vendor = text(device.join("vendor"))
                .trim_start_matches("0x")
                .to_owned();
            let product = text(device.join("device"))
                .trim_start_matches("0x")
                .to_owned();
            let fallback = match vendor.as_str() {
                "1002" => "AMD",
                "10de" => "NVIDIA",
                "8086" => "Intel",
                _ => crate::tr!("Graphics device"),
            };
            let name = pci_name(pci_ids, &vendor, &product)
                .unwrap_or_else(|| format!("{fallback} · PCI {vendor}:{product}"));
            let driver = fs::read_link(device.join("driver"))
                .ok()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                .unwrap_or_else(|| crate::tr!("no driver").into());
            let usage = integer(device.join("gpu_busy_percent"))
                .filter(|n| *n <= 100)
                .map(|n| n as f64);
            let (clock_mhz, power_watts, power_source) = gpu_metrics(&device, &driver);
            Some(Gpu {
                id,
                name,
                driver,
                usage,
                memory_used: integer(device.join("mem_info_vram_used")),
                memory_total: integer(device.join("mem_info_vram_total")),
                clock_mhz,
                power_watts,
                power_source,
                temperatures: device_temperatures(sensors, &device),
            })
        })
        .collect();
    gpus.sort_by(|a, b| a.id.cmp(&b.id));
    gpus
}

fn nvidia_query(command: &Path) -> Option<String> {
    let mut child = Command::new(command)
        .args([
            "--query-gpu=pci.bus_id,name,utilization.gpu,memory.used,memory.total,temperature.gpu,clocks.current.graphics,power.draw",
            "--format=csv,noheader,nounits",
        ])
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() < Duration::from_millis(700) => {
                std::thread::sleep(Duration::from_millis(10))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let output = child.wait_with_output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}
fn pci_key(id: &str) -> Option<(u32, u32, u32, u32)> {
    let (domain, rest) = id.split_once(':')?;
    let (bus, rest) = rest.split_once(':')?;
    let (slot, function) = rest.split_once('.')?;
    Some((
        u32::from_str_radix(domain, 16).ok()?,
        u32::from_str_radix(bus, 16).ok()?,
        u32::from_str_radix(slot, 16).ok()?,
        u32::from_str_radix(function, 16).ok()?,
    ))
}
fn apply_nvidia(gpus: &mut [Gpu], output: &str) {
    for line in output.lines() {
        let v: Vec<_> = line.split(',').map(str::trim).collect();
        if !matches!(v.len(), 6 | 8) {
            continue;
        }
        let Some(key) = pci_key(v[0]) else { continue };
        let Some(gpu) = gpus.iter_mut().find(|g| pci_key(&g.id) == Some(key)) else {
            continue;
        };
        gpu.name = v[1].into();
        gpu.usage = v[2]
            .parse::<f64>()
            .ok()
            .filter(|n| n.is_finite() && (0.0..=100.0).contains(n));
        gpu.memory_used = v[3].parse::<u64>().ok().map(|n| n.saturating_mul(1048576));
        gpu.memory_total = v[4].parse::<u64>().ok().map(|n| n.saturating_mul(1048576));
        gpu.clock_mhz = v.get(6).and_then(|value| non_negative(value));
        gpu.power_watts = v.get(7).and_then(|value| non_negative(value));
        gpu.power_source = if gpu.power_watts.is_some() {
            crate::tr!("NVIDIA driver value (average or instantaneous, depending on GPU)")
        } else {
            crate::tr!("unavailable")
        };
        if let Ok(value) = v[5].parse::<f64>()
            && value.is_finite()
            && (-273.15..=300.0).contains(&value)
        {
            gpu.temperatures.push(Temperature {
                chip: "nvidia".into(),
                label: "GPU".into(),
                celsius: value,
                device: PathBuf::new(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    fn write(root: &Path, path: &str, value: &str) {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, value).unwrap();
    }
    #[test]
    fn multiple_disks_partition_filter_rates_hotplug_and_nvme_sensor() {
        let dir = tempfile::tempdir().unwrap();
        let sys = dir.path();
        for name in ["nvme0n1", "sda"] {
            write(sys, &format!("block/{name}/size"), "2048");
            write(sys, &format!("block/{name}/device/model"), name);
        }
        write(sys, "block/zram0/size", "2048");
        write(sys, "block/sda/queue/rotational", "1");
        let sensors = vec![Temperature {
            chip: "nvme".into(),
            label: "Composite".into(),
            celsius: 45.0,
            device: sys.join("block/nvme0n1/device"),
        }];
        let current = disk_counters(
            "259 0 nvme0n1 1 0 20 0 1 0 40 0\n8 0 sda 1 0 8 0 1 0 2 0\n8 1 sda1 1 0 8 0 1 0 2 0",
        );
        let old = disk_counters("259 0 nvme0n1 1 0 10 0 1 0 20 0");
        let found = disks(sys, &current, &old, 2.0, &sensors);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].read, 2560.0);
        assert_eq!(found[0].write, 5120.0);
        assert_eq!(found[0].temperatures[0].celsius, 45.0);
        assert_eq!(found[1].read, 0.0);
        assert_eq!(found[1].capacity, 1048576);
        let reset = disks(sys, &old, &current, 1.0, &sensors);
        assert_eq!(reset[0].read, 0.0);
    }
    #[test]
    fn multiple_gpus_without_duplicate_drm_connector_and_missing_metrics() {
        let dir = tempfile::tempdir().unwrap();
        let sys = dir.path();
        for (id, vendor, product) in [
            ("0000:01:00.0", "1002", "1234"),
            ("0000:02:00.0", "8086", "5678"),
        ] {
            write(sys, &format!("bus/pci/devices/{id}/class"), "0x030000");
            write(
                sys,
                &format!("bus/pci/devices/{id}/vendor"),
                &format!("0x{vendor}"),
            );
            write(
                sys,
                &format!("bus/pci/devices/{id}/device"),
                &format!("0x{product}"),
            );
        }
        write(sys, "bus/pci/devices/0000:01:00.0/gpu_busy_percent", "42");
        fs::create_dir_all(sys.join("class/drm/card1")).unwrap();
        symlink(
            sys.join("bus/pci/devices/0000:01:00.0"),
            sys.join("class/drm/card1/device"),
        )
        .unwrap();
        let gpus = discover_gpus(sys, "1002  AMD\n\t1234  Test GPU\n", &[]);
        assert_eq!(gpus.len(), 2);
        assert_eq!(gpus[0].name, "AMD Test GPU");
        assert_eq!(gpus[0].usage, Some(42.0));
        assert_eq!(gpus[1].usage, None);
    }
    #[test]
    fn sensor_units_faults_labels_and_missing_values() {
        let dir = tempfile::tempdir().unwrap();
        let sys = dir.path();
        write(sys, "class/hwmon/hwmon0/name", "coretemp");
        write(sys, "class/hwmon/hwmon0/temp1_input", "55500");
        write(sys, "class/hwmon/hwmon0/temp1_label", "Package id 0");
        write(sys, "class/hwmon/hwmon0/temp2_input", "123000");
        write(sys, "class/hwmon/hwmon0/temp2_fault", "1");
        write(sys, "class/hwmon/hwmon0/temp3_input", "bad");
        let values = temperatures(sys);
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].celsius, 55.5);
        assert_eq!(cpu_temperatures(&values).len(), 1);
    }
    #[test]
    fn core_sensor_matching_respects_package_and_smt_topology() {
        let dir = tempfile::tempdir().unwrap();
        let sys = dir.path();
        write(sys, "devices/system/cpu/cpu8/topology/core_id", "2");
        write(
            sys,
            "devices/system/cpu/cpu8/topology/physical_package_id",
            "1",
        );
        let wrong = Temperature {
            chip: "coretemp".into(),
            label: "Core 2".into(),
            celsius: 33.0,
            device: PathBuf::from("/sys/devices/platform/coretemp.0"),
        };
        let correct = Temperature {
            celsius: 55.0,
            device: PathBuf::from("/sys/devices/platform/coretemp.1"),
            ..wrong.clone()
        };
        assert_eq!(core_temperature(sys, 8, &[wrong, correct]), Some(55.0));
        assert_eq!(core_temperature(sys, 9, &[]), None);
    }
    #[test]
    fn gpu_query_timeout_reaps_its_own_subprocess() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let command = temp.path().join("slow-query");
        fs::write(&command, "#!/bin/sh\nexec sleep 5\n").unwrap();
        fs::set_permissions(&command, fs::Permissions::from_mode(0o755)).unwrap();
        let start = Instant::now();
        assert!(nvidia_query(&command).is_none());
        assert!(start.elapsed() < Duration::from_secs(3));
    }
    #[test]
    fn gpu_clock_power_units_current_state_and_missing_metrics() {
        let dir = tempfile::tempdir().unwrap();
        let device = dir.path();
        write(device, "hwmon/hwmon7/freq1_input", "853000000");
        write(device, "hwmon/hwmon7/freq2_input", "1250000000");
        write(device, "hwmon/hwmon7/power1_average", "35750000");
        write(device, "hwmon/hwmon7/power1_cap", "300000000");
        write(
            device,
            "pp_dpm_sclk",
            "0: 500Mhz\n1: 853Mhz *\n2: 2460Mhz\n",
        );
        assert_eq!(
            gpu_metrics(device, "amdgpu"),
            (Some(853.0), Some(35.75), "Mittelwert GPU/SoC")
        );
        write(device, "hwmon/hwmon7/power1_input", "42500000");
        assert_eq!(gpu_metrics(device, "amdgpu").1, Some(42.5));
        write(device, "hwmon/hwmon7/freq1_input", "invalid");
        write(device, "pp_dpm_sclk", "S: 19Mhz *\n0: 615Mhz\n1: 1000Mhz\n");
        assert_eq!(gpu_metrics(device, "amdgpu").0, Some(19.0));
        write(device, "pp_dpm_sclk", "0: 615Mhz\n1: 1000Mhz\n");
        write(device, "hwmon/hwmon7/power1_input", "-1");
        write(device, "hwmon/hwmon7/power1_average", "invalid");
        assert_eq!(
            gpu_metrics(device, "amdgpu"),
            (None, None, "nicht verfügbar")
        );
        write(device, "hwmon/hwmon7/freq1_input", "0");
        write(device, "hwmon/hwmon7/power1_input", "0");
        assert_eq!(
            gpu_metrics(device, "amdgpu"),
            (Some(0.0), Some(0.0), "Momentanwert GPU/SoC")
        );
        assert_eq!(gpu_metrics(device, "i915"), (None, None, "nicht verfügbar"));
    }

    #[test]
    fn nvidia_pci_matching_and_unavailable_fields() {
        let mut gpus = vec![Gpu {
            id: "0000:01:00.0".into(),
            name: "NVIDIA".into(),
            driver: "nvidia".into(),
            usage: None,
            memory_used: None,
            memory_total: None,
            clock_mhz: None,
            power_watts: None,
            power_source: "nicht verfügbar",
            temperatures: vec![],
        }];
        apply_nvidia(
            &mut gpus,
            "00000000:01:00.0, RTX Test, 25, 100, 8000, 55, 1875, 112.50\n",
        );
        assert_eq!(gpus[0].usage, Some(25.0));
        assert_eq!(gpus[0].memory_used, Some(104857600));
        assert_eq!(gpus[0].temperatures[0].celsius, 55.0);
        assert_eq!(gpus[0].clock_mhz, Some(1875.0));
        assert_eq!(gpus[0].power_watts, Some(112.5));
        apply_nvidia(
            &mut gpus,
            "00000000:01:00.0, RTX Test, [N/A], [N/A], [N/A], [N/A], NaN, -1",
        );
        assert_eq!(gpus[0].usage, None);
        assert_eq!(gpus[0].memory_total, None);
        assert_eq!(gpus[0].clock_mhz, None);
        assert_eq!(gpus[0].power_watts, None);
        apply_nvidia(
            &mut gpus,
            "00000000:01:00.0, RTX Test, 0, 100, 8000, 55, 0, 0",
        );
        assert_eq!(gpus[0].clock_mhz, Some(0.0));
        assert_eq!(gpus[0].power_watts, Some(0.0));
        apply_nvidia(&mut gpus, "00000000:01:00.0, RTX Test, 10, 100, 8000, 55");
        assert_eq!(gpus[0].usage, Some(10.0));
        assert_eq!(gpus[0].clock_mhz, None);
    }
}
