"""Per-user desktop installation, explicitly initiated from the application."""
import json
import os
from pathlib import Path
import shutil
import sys
import tempfile

from PySide6.QtWidgets import QMessageBox

APP_ID = "net.linux.TaskManager"


def data_home() -> Path:
    configured = os.environ.get("XDG_DATA_HOME", "")
    return Path(configured) if configured and Path(configured).is_absolute() else Path.home() / ".local/share"


def desktop_quote(path: Path) -> str:
    # Desktop Entry Exec has its own quoting rules, not shell quoting.
    value = str(path).replace("%", "%%")
    for char in ("\\", '"', "`", "$"):
        value = value.replace(char, "\\" + char)
    # Backslashes are parsed by both the desktop-entry and Exec parsers.
    return '"' + value.replace("\\", "\\\\") + '"'


def install_files(executable: Path, icon: Path, root: Path | None = None,
                  bin_dir: Path | None = None) -> Path:
    root = root or data_home()
    bin_dir = bin_dir or Path.home() / ".local/bin"
    destination = root / "linux-task-manager"
    binary = destination / "Task-Manager"
    destination.mkdir(parents=True, exist_ok=True)
    applications = root / "applications"
    applications.mkdir(parents=True, exist_ok=True)
    bin_dir.mkdir(parents=True, exist_ok=True)
    link = bin_dir / "task-manager"
    if (link.exists() or link.is_symlink()) and (not link.is_symlink() or link.resolve() != binary.resolve()):
        raise FileExistsError(f"{link} ist bereits durch ein anderes Programm belegt.")
    with tempfile.NamedTemporaryFile(dir=destination, delete=False, prefix=".install-") as temporary:
        temp_path = Path(temporary.name)
    try:
        shutil.copyfile(executable, temp_path)
        temp_path.chmod(0o755)
        temp_path.replace(binary)
    finally:
        temp_path.unlink(missing_ok=True)
    installed_icon = destination / "task-manager.svg"
    shutil.copyfile(icon, installed_icon)
    desktop = applications / f"{APP_ID}.desktop"
    text = ("[Desktop Entry]\nType=Application\nVersion=1.0\nName=Task-Manager\n"
            "GenericName=Systemmonitor\nComment=Prozesse und Leistung unter Linux überwachen\n"
            f"Exec={desktop_quote(binary)}\nIcon={installed_icon}\n"
            "Terminal=false\nCategories=System;Monitor;\n"
            f"StartupWMClass={APP_ID}\nKeywords=Prozesse;CPU;RAM;Task;Monitor;\n")
    desktop.write_text(text, encoding="utf-8")
    desktop.chmod(0o644)
    if not link.is_symlink():
        link.symlink_to(binary)
    (destination / "installation.json").write_text(json.dumps({
        "binary": str(binary), "desktop": str(desktop), "link": str(link),
    }), encoding="utf-8")
    return binary


def install_dialog(parent, icon: Path):
    if not getattr(sys, "frozen", False):
        QMessageBox.information(parent, "Portable Version erforderlich",
            "Die grafische Installation ist in der fertigen Linux-Datei verfügbar.\n"
            "Erstelle sie mit scripts/build.py oder verwende das Download-Paket.")
        return
    result = QMessageBox.question(parent, "Task-Manager installieren",
        "Task-Manager für deinen Benutzer installieren?\n\n"
        "Die App wird in dein App-Menü aufgenommen. Es sind keine Administratorrechte nötig.\n"
        f"Speicherort: {data_home() / 'linux-task-manager'}\n\n"
        "Der Terminalstarter wird unter ~/.local/bin/task-manager angelegt.",
        QMessageBox.StandardButton.Yes | QMessageBox.StandardButton.No,
        QMessageBox.StandardButton.Yes)
    if result != QMessageBox.StandardButton.Yes:
        return
    try:
        install_files(Path(sys.executable), icon)
    except OSError as error:
        QMessageBox.critical(parent, "Installation fehlgeschlagen", str(error))
        return
    QMessageBox.information(parent, "Installation abgeschlossen",
        "Du kannst Task-Manager jetzt über dein App-Menü starten.\n\n"
        "Im Terminal: ~/.local/bin/task-manager\n"
        "Wenn ~/.local/bin in deinem PATH steht, genügt task-manager.\n\n"
        "Das entpackte Download-Paket kannst du anschließend löschen.")


def uninstall_dialog(parent):
    destination = data_home() / "linux-task-manager"
    if not (destination / "installation.json").is_file():
        QMessageBox.information(parent, "Task-Manager", "Es wurde keine Installation für diesen Benutzer gefunden.")
        return
    if QMessageBox.question(parent, "Task-Manager deinstallieren",
        "App-Menü-Eintrag und installierte Programmdateien entfernen?",
        QMessageBox.StandardButton.Yes | QMessageBox.StandardButton.No,
        QMessageBox.StandardButton.No) != QMessageBox.StandardButton.Yes:
        return
    try:
        binary = destination / "Task-Manager"
        link = Path.home() / ".local/bin/task-manager"
        if link.is_symlink() and link.resolve() == binary.resolve():
            link.unlink()
        (data_home() / "applications" / f"{APP_ID}.desktop").unlink(missing_ok=True)
        for name in ("Task-Manager", "task-manager.svg", "installation.json"):
            (destination / name).unlink(missing_ok=True)
        if not any(destination.iterdir()):
            destination.rmdir()
    except OSError as error:
        QMessageBox.critical(parent, "Deinstallation fehlgeschlagen", str(error))
        return
    QMessageBox.information(parent, "Deinstallation abgeschlossen", "Die installierte App wurde entfernt.")
