use std::env;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const APP_ID: &str = "net.linux.TaskManager";
const ICON: &str = include_str!("../assets/task-manager.svg");
const MARKER: &str = "linux-task-manager-rust\n";

#[derive(Clone)]
pub struct InstallPaths {
    pub data: PathBuf,
    pub bin: PathBuf,
}

impl InstallPaths {
    pub fn for_user() -> io::Result<Self> {
        let home = env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .ok_or_else(|| io::Error::other(crate::tr!("Home directory unavailable.")))?;
        let data = env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home.join(".local/share"));
        Ok(Self {
            data,
            bin: home.join(".local/bin"),
        })
    }
    pub fn directory(&self) -> PathBuf {
        self.data.join("linux-task-manager")
    }
    pub fn binary(&self) -> PathBuf {
        self.directory().join("Task-Manager")
    }
    fn desktop(&self) -> PathBuf {
        self.data
            .join("applications")
            .join(format!("{APP_ID}.desktop"))
    }
    fn link(&self) -> PathBuf {
        self.bin.join("task-manager")
    }
}

fn desktop_value(path: &Path) -> io::Result<String> {
    let value = path
        .to_str()
        .ok_or_else(|| io::Error::other(crate::tr!("The installation path must support UTF-8.")))?;
    if value.contains(['\n', '\r']) {
        return Err(io::Error::other(crate::tr!(
            "The installation path contains a line break."
        )));
    }
    Ok(value.to_owned())
}

fn desktop_exec(path: &Path) -> io::Result<String> {
    let value = desktop_value(path)?.replace('%', "%%");
    let mut escaped = String::from("\"");
    for character in value.chars() {
        match character {
            '\\' => escaped.push_str("\\\\\\\\"),
            '"' | '`' | '$' => {
                escaped.push_str("\\\\");
                escaped.push(character);
            }
            _ => escaped.push(character),
        }
    }
    escaped.push('"');
    Ok(escaped)
}

pub fn install(source: &Path, paths: &InstallPaths) -> io::Result<PathBuf> {
    let directory = paths.directory();
    let binary = paths.binary();
    let link = paths.link();
    let desktop = paths.desktop();
    let exec = desktop_exec(&binary)?;
    if fs::symlink_metadata(&link).is_ok() && fs::read_link(&link).ok().as_ref() != Some(&binary) {
        return Err(io::Error::other(crate::tf!(
            "{0} is occupied by another program.",
            link.display()
        )));
    }
    if binary.exists()
        && !directory.join(".installed-by-task-manager").is_file()
        && !directory.join("installation.json").is_file()
    {
        return Err(io::Error::other(crate::tr!(
            "The destination already contains an unrelated file named Task-Manager."
        )));
    }
    fs::create_dir_all(&directory)?;
    fs::create_dir_all(desktop.parent().expect("desktop parent"))?;
    fs::create_dir_all(&paths.bin)?;
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = directory.join(format!(".install-{}-{suffix}", std::process::id()));
    let result = (|| {
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        io::copy(&mut File::open(source)?, &mut output)?;
        output.set_permissions(fs::Permissions::from_mode(0o755))?;
        output.sync_all()?;
        fs::rename(&temporary, &binary)?;
        let icon = directory.join("task-manager.svg");
        fs::write(&icon, ICON)?;
        let icon_value = desktop_value(&icon)?.replace('\\', "\\\\");
        let generic = crate::i18n::desktop_entry_text("GenericName", "System monitor");
        let comment = crate::i18n::desktop_entry_text(
            "Comment",
            "Monitor processes and performance on Linux",
        );
        let keywords =
            crate::i18n::desktop_entry_text("Keywords", "Processes;CPU;RAM;Task;Monitor;Rust;");
        fs::write(
            &desktop,
            format!(
                "[Desktop Entry]\nType=Application\nVersion=1.0\nName=Task-Manager\n\
            {generic}{comment}\
            Exec={exec}\nIcon={icon_value}\nTerminal=false\nCategories=System;Monitor;\n\
            StartupWMClass={APP_ID}\nX-KDE-Wayland-Interfaces=org_kde_plasma_window_management\n{keywords}"
            ),
        )?;
        fs::set_permissions(&desktop, fs::Permissions::from_mode(0o644))?;
        if fs::symlink_metadata(&link).is_err() {
            symlink(&binary, &link)?;
        }
        fs::write(directory.join(".installed-by-task-manager"), MARKER)?;
        Ok(binary.clone())
    })();
    let _ = fs::remove_file(temporary);
    result
}

pub fn uninstall(paths: &InstallPaths) -> io::Result<()> {
    let directory = paths.directory();
    if fs::read_to_string(directory.join(".installed-by-task-manager"))
        .as_deref()
        .ok()
        != Some(MARKER)
    {
        return Err(io::Error::other(crate::tr!(
            "No Rust installation found for this user."
        )));
    }
    let link = paths.link();
    if fs::read_link(&link).ok().as_ref() == Some(&paths.binary()) {
        fs::remove_file(link)?;
    }
    for path in [
        paths.desktop(),
        paths.binary(),
        directory.join("task-manager.svg"),
        directory.join("installation.json"),
        directory.join(".installed-by-task-manager"),
    ] {
        match fs::remove_file(path) {
            Ok(()) => (),
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(e) => return Err(e),
        }
    }
    if fs::read_dir(&directory)?.next().is_none() {
        fs::remove_dir(directory)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn install_upgrade_and_uninstall_in_directory_with_spaces() {
        let temp = tempfile::Builder::new()
            .prefix("task manager ")
            .tempdir()
            .unwrap();
        let paths = InstallPaths {
            data: temp.path().join("data"),
            bin: temp.path().join("bin"),
        };
        let source = temp.path().join("source");
        fs::write(&source, b"first executable").unwrap();
        let binary = install(&source, &paths).unwrap();
        assert_eq!(fs::read(&binary).unwrap(), b"first executable");
        assert_eq!(
            fs::metadata(&binary).unwrap().permissions().mode() & 0o777,
            0o755
        );
        assert_eq!(fs::read_link(paths.link()).unwrap(), binary);
        assert!(
            fs::read_to_string(paths.desktop())
                .unwrap()
                .contains("Terminal=false")
        );
        if Command::new("desktop-file-validate")
            .arg("--version")
            .output()
            .is_ok()
        {
            assert!(
                Command::new("desktop-file-validate")
                    .arg(paths.desktop())
                    .status()
                    .unwrap()
                    .success()
            );
        }
        install(&binary, &paths).unwrap();
        fs::write(directory_file(&paths, "user-note.txt"), b"keep me").unwrap();
        uninstall(&paths).unwrap();
        assert!(!binary.exists() && !paths.link().exists() && !paths.desktop().exists());
        assert!(directory_file(&paths, "user-note.txt").exists());
    }

    fn directory_file(paths: &InstallPaths, name: &str) -> PathBuf {
        paths.directory().join(name)
    }

    #[test]
    fn refuses_unrelated_launcher_and_quotes_exec() {
        let temp = tempfile::tempdir().unwrap();
        let paths = InstallPaths {
            data: temp.path().join("data"),
            bin: temp.path().join("bin"),
        };
        fs::create_dir_all(&paths.bin).unwrap();
        fs::write(paths.link(), b"unrelated").unwrap();
        let source = temp.path().join("source");
        fs::write(&source, b"binary").unwrap();
        assert!(install(&source, &paths).is_err());
        assert_eq!(fs::read(paths.link()).unwrap(), b"unrelated");
        assert_eq!(
            desktop_exec(Path::new("/tmp/a b%/$task")).unwrap(),
            "\"/tmp/a b%%/\\\\$task\""
        );
    }
}
