//! Read-only online CPU topology and frequency, with explicit unavailable values.
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::Path;

#[derive(Clone, Debug)]
pub struct Frequency {
    pub mhz: f64,
    pub measured_cpus: usize,
    pub source: &'static str,
}

#[derive(Default, Clone, Debug)]
pub struct CpuDetails {
    pub physical_cores: Option<usize>,
    pub frequency: Option<Frequency>,
}

pub fn sample(sys: &Path, cpuinfo: &str, online: &[usize]) -> CpuDetails {
    let info: HashMap<usize, HashMap<&str, &str>> = cpuinfo
        .split("\n\n")
        .filter_map(|section| {
            let fields: HashMap<_, _> = section
                .lines()
                .filter_map(|line| line.split_once(':').map(|(k, v)| (k.trim(), v.trim())))
                .collect();
            Some((fields.get("processor")?.parse().ok()?, fields))
        })
        .collect();
    CpuDetails {
        physical_cores: physical_cores(sys, &info, online),
        frequency: frequency(sys, &info, online),
    }
}

fn cpu_list(text: &str) -> Option<BTreeSet<usize>> {
    let mut ids = BTreeSet::new();
    for part in text.trim().split(',') {
        if let Some((first, last)) = part.split_once('-') {
            let first = first.parse::<usize>().ok()?;
            let last = last.parse::<usize>().ok()?;
            if last < first || last.saturating_sub(first) > 1_000_000 {
                return None;
            }
            ids.extend(first..=last);
        } else {
            ids.insert(part.parse().ok()?);
        }
    }
    (!ids.is_empty()).then_some(ids)
}

fn physical_cores(
    sys: &Path,
    info: &HashMap<usize, HashMap<&str, &str>>,
    online: &[usize],
) -> Option<usize> {
    if online.is_empty() {
        return None;
    }
    // Sibling sets distinguish SMT threads without assuming two threads per core.
    for file in ["core_cpus_list", "thread_siblings_list"] {
        let groups: Option<HashSet<_>> = online
            .iter()
            .map(|id| {
                let text = fs::read_to_string(
                    sys.join(format!("devices/system/cpu/cpu{id}/topology/{file}")),
                )
                .ok()?;
                let group = cpu_list(&text)?;
                group.contains(id).then_some(group)
            })
            .collect();
        if let Some(groups) = groups {
            return Some(groups.len());
        }
    }
    let topology: Option<HashSet<_>> = online
        .iter()
        .map(|id| {
            let root = sys.join(format!("devices/system/cpu/cpu{id}/topology"));
            let package = fs::read_to_string(root.join("physical_package_id"))
                .ok()?
                .trim()
                .parse::<u32>()
                .ok()?;
            let core = fs::read_to_string(root.join("core_id"))
                .ok()?
                .trim()
                .parse::<u32>()
                .ok()?;
            let die = fs::read_to_string(root.join("die_id"))
                .ok()
                .and_then(|v| v.trim().parse::<u32>().ok());
            Some((package, die, core))
        })
        .collect();
    if let Some(topology) = topology {
        return Some(topology.len());
    }
    let topology: Option<HashSet<_>> = online
        .iter()
        .map(|id| {
            let fields = info.get(id)?;
            Some((
                fields.get("physical id")?.parse::<u32>().ok()?,
                fields.get("core id")?.parse::<u32>().ok()?,
            ))
        })
        .collect();
    topology.map(|groups| groups.len())
}

fn positive(text: &str) -> Option<f64> {
    let value: f64 = text.trim().parse().ok()?;
    (value.is_finite() && value > 0.0).then_some(value)
}

fn average(values: Vec<f64>, source: &'static str) -> Option<Frequency> {
    (!values.is_empty()).then(|| Frequency {
        mhz: values.iter().sum::<f64>() / values.len() as f64,
        measured_cpus: values.len(),
        source,
    })
}

fn frequency(
    sys: &Path,
    info: &HashMap<usize, HashMap<&str, &str>>,
    online: &[usize],
) -> Option<Frequency> {
    // Prefer hardware feedback; do not mix requested scaling values with it.
    for (file, source) in [
        ("cpuinfo_avg_freq", crate::tr!("Hardware average (CPUFreq)")),
        ("cpuinfo_cur_freq", crate::tr!("Hardware clock (CPUFreq)")),
    ] {
        if let Some(result) = average(
            online
                .iter()
                .filter_map(|id| {
                    positive(
                        &fs::read_to_string(
                            sys.join(format!("devices/system/cpu/cpu{id}/cpufreq/{file}")),
                        )
                        .ok()?,
                    )
                    .map(|khz| khz / 1000.0)
                })
                .collect(),
            source,
        ) {
            return Some(result);
        }
    }
    average(
        online
            .iter()
            .filter_map(|id| positive(info.get(id)?.get("cpu MHz")?))
            .collect(),
        "/proc/cpuinfo",
    )
    .or_else(|| {
        average(
            online
                .iter()
                .filter_map(|id| {
                    positive(
                        &fs::read_to_string(sys.join(format!(
                            "devices/system/cpu/cpu{id}/cpufreq/scaling_cur_freq"
                        )))
                        .ok()?,
                    )
                    .map(|khz| khz / 1000.0)
                })
                .collect(),
            crate::tr!("Driver clock (may differ from the actual clock)"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn write(sys: &Path, id: usize, file: &str, value: &str) {
        let path = sys.join(format!("devices/system/cpu/cpu{id}/{file}"));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, value).unwrap();
    }

    #[test]
    fn topology_handles_smt_packages_gaps_and_missing_data() {
        let temp = tempfile::tempdir().unwrap();
        let sys = temp.path();
        // Two packages, repeated core IDs, non-contiguous logical CPUs and SMT.
        for (id, group) in [(0, "0,8"), (8, "0,8"), (2, "2"), (19, "19")] {
            write(sys, id, "topology/core_cpus_list", group);
        }
        assert_eq!(sample(sys, "", &[0, 8, 2, 19]).physical_cores, Some(3));
        assert_eq!(sample(sys, "", &[8, 2, 19]).physical_cores, Some(3));
        assert_eq!(sample(sys, "", &[0, 8, 2, 19, 31]).physical_cores, None);
        for (id, package, core) in [(0, 0, 0), (8, 0, 0), (2, 0, 1), (19, 1, 0)] {
            write(sys, id, "topology/core_cpus_list", "invalid");
            write(
                sys,
                id,
                "topology/physical_package_id",
                &package.to_string(),
            );
            write(sys, id, "topology/core_id", &core.to_string());
        }
        assert_eq!(sample(sys, "", &[0, 8, 2, 19]).physical_cores, Some(3));
        write(sys, 19, "topology/physical_package_id", "-1");
        assert_eq!(sample(sys, "", &[0, 8, 2, 19]).physical_cores, None);
        let info = "processor: 0\nphysical id: 0\ncore id: 0\n\nprocessor: 8\nphysical id: 0\ncore id: 0\n\nprocessor: 19\nphysical id: 1\ncore id: 0\n";
        assert_eq!(sample(sys, info, &[0, 8, 19]).physical_cores, Some(2));
        assert_eq!(cpu_list("2-4,3,8").unwrap().len(), 4);
        assert!(cpu_list("4-2").is_none());
    }

    #[test]
    fn frequency_units_precedence_partial_readings_and_fallbacks() {
        let temp = tempfile::tempdir().unwrap();
        let sys = temp.path();
        let info = "processor: 0\ncpu MHz: 2000\n\nprocessor: 8\ncpu MHz: 4000\n\nprocessor: 19\ncpu MHz: NaN\n";
        write(sys, 0, "cpufreq/cpuinfo_avg_freq", "3000000");
        write(sys, 8, "cpufreq/cpuinfo_avg_freq", "5000000");
        write(sys, 19, "cpufreq/cpuinfo_avg_freq", "0");
        write(sys, 0, "cpufreq/cpuinfo_cur_freq", "1000000");
        write(sys, 0, "cpufreq/scaling_cur_freq", "9000000");
        let frequency = sample(sys, info, &[0, 8, 19]).frequency.unwrap();
        assert_eq!((frequency.mhz, frequency.measured_cpus), (4000.0, 2));
        assert_eq!(frequency.source, "Hardware-Mittelwert (CPUFreq)");
        write(sys, 0, "cpufreq/cpuinfo_avg_freq", "NaN");
        write(sys, 8, "cpufreq/cpuinfo_avg_freq", "-1");
        assert_eq!(sample(sys, info, &[0, 8]).frequency.unwrap().mhz, 1000.0);
        write(sys, 0, "cpufreq/cpuinfo_cur_freq", "inf");
        let frequency = sample(sys, info, &[0, 8, 19]).frequency.unwrap();
        assert_eq!((frequency.mhz, frequency.measured_cpus), (3000.0, 2));
        assert_eq!(frequency.source, "/proc/cpuinfo");
        let frequency = sample(sys, "", &[0, 8]).frequency.unwrap();
        assert_eq!(frequency.mhz, 9000.0);
        assert!(frequency.source.contains("abweichen"));
        assert!(sample(sys, "", &[8, 19]).frequency.is_none());
    }
}
