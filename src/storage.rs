//! Mounted filesystem occupancy; physical capacity is deliberately kept separate.
use std::collections::{BTreeSet, HashMap, HashSet};
use std::ffi::CString;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

#[derive(Clone, Debug)]
pub struct Volume {
    pub source: String,
    pub kind: String,
    pub mounts: Vec<String>,
    pub total: u64,
    pub used: u64,
    pub available: u64,
    pub shared: bool,
}
#[derive(Clone, Debug)]
struct Mount {
    id: u64,
    device: String,
    path: String,
    source: String,
    kind: String,
}
#[derive(Clone, Copy)]
pub struct Usage {
    pub mount_id: Option<u64>,
    pub fsid: u64,
    pub dev: u64,
    pub total: u64,
    pub used: u64,
    pub available: u64,
}

fn decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut result = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\'
            && i + 3 < bytes.len()
            && bytes[i + 1..i + 4]
                .iter()
                .all(|b| (b'0'..=b'7').contains(b))
        {
            let number = (bytes[i + 1] - b'0') as u16 * 64
                + (bytes[i + 2] - b'0') as u16 * 8
                + (bytes[i + 3] - b'0') as u16;
            if number <= 255 {
                result.push(number as u8);
                i += 4;
                continue;
            }
        }
        result.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&result).into_owned()
}
fn mounts(text: &str) -> Vec<Mount> {
    let mut rows: Vec<_> = text
        .lines()
        .filter_map(|line| {
            let (left, right) = line.split_once(" - ")?;
            let left: Vec<_> = left.split_whitespace().collect();
            let right: Vec<_> = right.split_whitespace().collect();
            if left.len() < 6 || right.len() < 3 {
                return None;
            }
            Some(Mount {
                id: left[0].parse().ok()?,
                device: left[2].into(),
                path: decode(left[4]),
                kind: right[0].into(),
                source: decode(right[1]),
            })
        })
        .collect();
    rows.sort_by_key(|m| m.path.len());
    rows
}

fn names(path: &Path) -> Vec<String> {
    fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect()
}
fn backing(sys: &Path, name: &str, disks: &[String]) -> BTreeSet<String> {
    let mut pending = vec![name.to_owned()];
    let mut visited = HashSet::new();
    let mut result = BTreeSet::new();
    while let Some(name) = pending.pop() {
        if !visited.insert(name.clone()) {
            continue;
        }
        for disk in disks {
            if &name == disk
                || sys
                    .join("block")
                    .join(disk)
                    .join(&name)
                    .join("partition")
                    .exists()
            {
                result.insert(disk.clone());
            }
        }
        pending.extend(names(&sys.join("class/block").join(&name).join("slaves")));
    }
    result
}
fn block_name(sys: &Path, mount: &Mount) -> Option<String> {
    fs::canonicalize(sys.join("dev/block").join(&mount.device))
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .or_else(|| {
            let path = Path::new(&mount.source);
            if !path.starts_with("/dev") {
                return None;
            }
            let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
            path.file_name().map(|n| n.to_string_lossy().into_owned())
        })
}

pub fn read_usage(path: &str) -> Option<Usage> {
    let path_c = CString::new(path).ok()?;
    let mut buffer = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: the path is NUL-terminated and buffer points to writable storage.
    if unsafe { libc::statvfs(path_c.as_ptr(), buffer.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: statvfs initialized the structure on success.
    let value = unsafe { buffer.assume_init() };
    let fragment = if value.f_frsize == 0 {
        value.f_bsize
    } else {
        value.f_frsize
    };
    let (total, used, available) =
        occupancy(value.f_blocks, value.f_bfree, value.f_bavail, fragment)?;
    let mut info = std::mem::MaybeUninit::<libc::statx>::uninit();
    // SAFETY: valid C path and writable output. Request the mountinfo-compatible ID, not MNT_ID_UNIQUE.
    let mount_id = if unsafe {
        libc::statx(
            libc::AT_FDCWD,
            path_c.as_ptr(),
            0,
            libc::STATX_MNT_ID,
            info.as_mut_ptr(),
        )
    } == 0
    {
        // SAFETY: statx initialized the structure on success.
        let info = unsafe { info.assume_init() };
        (info.stx_mask & libc::STATX_MNT_ID != 0).then_some(info.stx_mnt_id)
    } else {
        None
    };
    Some(Usage {
        mount_id,
        fsid: value.f_fsid,
        dev: fs::metadata(path).ok()?.dev(),
        total,
        used,
        available,
    })
}
fn occupancy(blocks: u64, free: u64, available: u64, fragment: u64) -> Option<(u64, u64, u64)> {
    if blocks == 0 || fragment == 0 || free > blocks || available > free {
        return None;
    }
    Some((
        blocks.checked_mul(fragment)?,
        (blocks - free).checked_mul(fragment)?,
        available.checked_mul(fragment)?,
    ))
}

pub fn collect(
    sys: &Path,
    text: &str,
    disks: &[String],
    mut read: impl FnMut(&str) -> Option<Usage>,
) -> HashMap<String, Vec<Volume>> {
    let mut result: HashMap<_, _> = disks.iter().map(|d| (d.clone(), vec![])).collect();
    let mut volumes: HashMap<(String, u64, String), (BTreeSet<String>, Volume)> = HashMap::new();
    for mount in mounts(text) {
        let Some(name) = block_name(sys, &mount) else {
            continue;
        };
        let mut owners = backing(sys, &name, disks);
        let mut btrfs_id = None;
        if mount.kind == "btrfs" {
            for filesystem in names(&sys.join("fs/btrfs")) {
                let devices = names(&sys.join("fs/btrfs").join(&filesystem).join("devices"));
                if devices.contains(&name) {
                    btrfs_id = Some(filesystem);
                    for device in devices {
                        owners.extend(backing(sys, &device, disks));
                    }
                }
            }
        }
        if owners.is_empty() {
            continue;
        }
        let Some(usage) = read(&mount.path) else {
            continue;
        };
        // A stacked/overmounted path must not attribute another filesystem's statistics to this disk.
        // Btrfs st_dev and statvfs fsid vary by subvolume; mount IDs do not have that ambiguity.
        if usage.mount_id.map_or_else(
            || format!("{}:{}", libc::major(usage.dev), libc::minor(usage.dev)) != mount.device,
            |id| id != mount.id,
        ) {
            continue;
        }
        let key = (
            mount.kind.clone(),
            if mount.kind == "btrfs" { 0 } else { usage.fsid },
            if mount.kind == "btrfs" {
                btrfs_id.unwrap_or(name)
            } else if usage.fsid == 0 {
                name
            } else {
                String::new()
            },
        );
        let (_, volume) = volumes
            .entry(key)
            .and_modify(|(devices, _)| devices.extend(owners.clone()))
            .or_insert_with(|| {
                (
                    owners,
                    Volume {
                        source: mount.source,
                        kind: mount.kind,
                        mounts: vec![],
                        total: usage.total,
                        used: usage.used,
                        available: usage.available,
                        shared: false,
                    },
                )
            });
        if !volume.mounts.contains(&mount.path) {
            volume.mounts.push(mount.path);
        }
    }
    for (owners, mut volume) in volumes.into_values() {
        volume.shared = owners.len() > 1;
        for owner in owners {
            result.get_mut(&owner).unwrap().push(volume.clone());
        }
    }
    for rows in result.values_mut() {
        rows.sort_by(|a, b| a.mounts[0].cmp(&b.mounts[0]));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn partition(sys: &Path, disk: &str, name: &str) {
        let path = sys.join("block").join(disk).join(name);
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("partition"), "1").unwrap();
    }
    #[test]
    fn mounted_partitions_deduplicate_subvolumes_and_bind_mounts_without_counting_reserved_as_used()
    {
        let dir = tempfile::tempdir().unwrap();
        let sys = dir.path();
        partition(sys, "sda", "sda1");
        partition(sys, "sda", "sda2");
        let text = "1 0 8:1 / / rw - btrfs /dev/sda1 rw\n2 1 8:1 /home /home rw - btrfs /dev/sda1 rw\n3 0 8:2 / /data\\040files rw - ext4 /dev/sda2 rw\n4 0 0:1 / /virtual rw - tmpfs tmpfs rw\n";
        let disks = vec!["sda".into(), "sdb".into()];
        let rows = collect(sys, text, &disks, |path| {
            Some(Usage {
                mount_id: None,
                fsid: if path == "/data files" {
                    2
                } else if path == "/home" {
                    999
                } else {
                    1
                },
                dev: libc::makedev(8, if path == "/data files" { 2 } else { 1 }),
                total: 100,
                used: 30,
                available: 60,
            })
        });
        assert_eq!(rows["sda"].len(), 2);
        assert_eq!(rows["sda"][0].mounts, vec!["/", "/home"]);
        assert!(rows["sdb"].is_empty());
        assert_eq!(occupancy(100, 70, 60, 4096), Some((409600, 122880, 245760)));
        assert!(occupancy(u64::MAX, 0, 0, 4096).is_none());
        assert!(occupancy(100, 120, 0, 4096).is_none());
    }
    #[test]
    fn mapped_multi_device_pools_are_shared_and_unreadable_or_overmounted_filesystems_stay_unknown()
    {
        let dir = tempfile::tempdir().unwrap();
        let sys = dir.path();
        partition(sys, "sda", "sda1");
        partition(sys, "sdb", "sdb1");
        let slaves = sys.join("class/block/dm-0/slaves");
        fs::create_dir_all(&slaves).unwrap();
        fs::write(slaves.join("sda1"), "").unwrap();
        fs::write(slaves.join("sdb1"), "").unwrap();
        let text = "1 0 253:0 / /pool rw - ext4 /dev/dm-0 rw\n";
        let disks = vec!["sda".into(), "sdb".into()];
        let usage = Usage {
            mount_id: None,
            fsid: 7,
            dev: libc::makedev(253, 0),
            total: 100,
            used: 80,
            available: 20,
        };
        let rows = collect(sys, text, &disks, |_| Some(usage));
        assert!(rows["sda"][0].shared);
        assert_eq!(rows["sdb"][0].used, 80);
        assert!(collect(sys, text, &disks, |_| None)["sda"].is_empty());
        assert!(
            collect(sys, text, &disks, |_| Some(Usage {
                mount_id: Some(2),
                ..usage
            }))["sda"]
                .is_empty()
        );
        assert!(
            collect(sys, text, &disks, |_| Some(Usage {
                dev: libc::makedev(8, 1),
                ..usage
            }))["sda"]
                .is_empty()
        );
    }
    #[test]
    fn btrfs_multi_device_members_and_real_filesystem_statistics() {
        let dir = tempfile::tempdir().unwrap();
        let sys = dir.path();
        partition(sys, "sda", "sda1");
        partition(sys, "sdb", "sdb1");
        let path = sys.join("fs/btrfs/uuid/devices");
        fs::create_dir_all(path.join("sda1")).unwrap();
        fs::create_dir_all(path.join("sdb1")).unwrap();
        let rows = collect(
            sys,
            "1 0 0:42 / /pool rw - btrfs /dev/sda1 rw\n2 0 0:42 /sub /sub rw - btrfs /dev/sdb1 rw\n",
            &["sda".into(), "sdb".into()],
            |path| {
                Some(Usage {
                    mount_id: Some(if path == "/pool" { 1 } else { 2 }),
                    fsid: if path == "/pool" { 8 } else { 999 },
                    dev: libc::makedev(0, 43),
                    total: 100,
                    used: 50,
                    available: 50,
                })
            },
        );
        assert!(rows["sdb"][0].shared);
        assert_eq!(rows["sda"].len(), 1);
        assert_eq!(rows["sda"][0].mounts.len(), 2);
        let real = read_usage(dir.path().to_str().unwrap()).unwrap();
        assert!(real.total >= real.used);
        assert!(real.available <= real.total);
        assert!(read_usage("/task-manager-path-does-not-exist").is_none());
    }
}
